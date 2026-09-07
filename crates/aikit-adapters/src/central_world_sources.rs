//! Root→project contextualisation (W10 V5): a project context inside a
//! Central world binds the SAME pasu entity refs the root materialises —
//! never a second subject — and the binding is governed by Central's
//! authored world relations through `central.world.effective-sources`
//! (ancestry propagation with per-hop provenance, overrides, declared
//! exclusions).
//!
//! Semantics, in one line each:
//!
//! * A project that declares no world relations of its own inherits the
//!   root lineage (`control:root`) by convention — one world, one human;
//!   authored relations only refine (override a revision, exclude a
//!   declared source), they never mint a second subject.
//! * An entity whose governing source is `excluded` in the project's
//!   effective relations is withheld from that context's index and the
//!   withholding is disclosed. Nothing is deleted anywhere.
//! * A discovered (project-wiki) object that re-declares an entity
//!   subject under a different ref is refused as a stand-in: the
//!   materialised entity keeps the subject.
//! * When the world-relations carrier itself is unavailable the binding
//!   degrades to uncontextualised: entities pass through unannotated and
//!   the unavailability is disclosed (fail-open, descope law).

use crate::runner::CommandRunner;
use aikit_core::{AikitError, Result, WikiObject};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub const BINDING_PRODUCER_REF: &str = "aikit/central-world-binding/v1";
pub const BINDING_EXTENSION: &str = "aikit.world-binding/v1";
pub const ROOT_WORLD_REF: &str = "control:root";

/// The effective-source reading Central returned for one world.
#[derive(Debug, Clone, Default)]
pub struct WorldBinding {
    pub world_ref: String,
    /// True when the project declared no relations and the root lineage was
    /// applied by convention; disclosed, never silent.
    pub inherited_root_lineage: bool,
    pub sources: Vec<EffectiveSource>,
}

#[derive(Debug, Clone)]
pub struct EffectiveSource {
    pub source_ref: String,
    /// `available` | `excluded` (Central's EffectiveSourceState).
    pub state: String,
    pub effective_revision: String,
    /// World refs from the requesting world up to the source's world.
    pub propagation_path: Vec<String>,
}

/// Ask Central for a world's effective source relations. The input follows
/// the Action contract: `scope` (`root`|`project`), optional `project`,
/// required `world_ref`.
pub fn read_world_binding<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    scope: &str,
    project: Option<&str>,
    world_ref: &str,
) -> Result<WorldBinding> {
    let mut input = json!({"scope": scope, "world_ref": world_ref});
    if let Some(project) = project {
        input["project"] = json!(project);
    }
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        central_root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "central.world.effective-sources".into(),
        input.to_string(),
    ];
    let output = runner
        .run(&argv)?
        .require(&argv, "central.world_sources_unavailable")?;
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|e| AikitError::new("central.world_sources_invalid", e.to_string()))?;
    if envelope["ok"] != true {
        return Err(AikitError::new(
            "central.world_sources_unavailable",
            format!(
                "central.world.effective-sources did not succeed: {}",
                envelope["error"]["message"].as_str().unwrap_or("unknown")
            ),
        ));
    }
    let data = &envelope["data"];
    let mut binding = WorldBinding {
        world_ref: data["world_ref"]
            .as_str()
            .unwrap_or(world_ref)
            .to_owned(),
        inherited_root_lineage: false,
        sources: Vec::new(),
    };
    if let Some(entries) = data["sources"].as_array() {
        for entry in entries {
            binding.sources.push(EffectiveSource {
                source_ref: entry["ref"].as_str().unwrap_or_default().to_owned(),
                state: entry["state"].as_str().unwrap_or("available").to_owned(),
                effective_revision: entry["effective_revision"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                propagation_path: entry["propagation_path"]
                    .as_array()
                    .map(|path| {
                        path.iter()
                            .filter_map(|world| world.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    Ok(binding)
}

/// The project context for a Central-relative project name: the declared
/// `project_id` from its ProjectCentral manifest when readable, else the
/// `project:<name>` convention.
pub fn project_world_ref(central_root: &Path, project: &str) -> String {
    let manifest = central_root.join("Work").join(project).join("ProjectCentral/project.json");
    if let Ok(text) = fs::read_to_string(&manifest) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(project_id) = value["project_id"].as_str() {
                if !project_id.trim().is_empty() {
                    return project_id.trim().to_owned();
                }
            }
        }
    }
    format!("project:{project}")
}

/// Read a project's effective binding, falling back to the root lineage
/// when the project declares no world of its own (Central answers
/// `MissingWorld` for a world ref with no authored record).
pub fn read_project_binding<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    project: &str,
    absences: &mut Vec<String>,
) -> Option<WorldBinding> {
    let world_ref = project_world_ref(central_root, project);
    match read_world_binding(runner, executable, central_root, "project", Some(project), &world_ref) {
        Ok(binding) => Some(binding),
        Err(project_error) => {
            match read_world_binding(runner, executable, central_root, "root", None, ROOT_WORLD_REF)
            {
                Ok(mut binding) => {
                    binding.inherited_root_lineage = true;
                    absences.push(format!(
                        "Project {project} declares no world relations; the root lineage applies ({})",
                        project_error.message()
                    ));
                    Some(binding)
                }
                Err(root_error) => {
                    absences.push(format!(
                        "World relations unavailable; binding is uncontextualised: {}",
                        root_error.message()
                    ));
                    None
                }
            }
        }
    }
}

/// True when a declared world source governs an entity source ref: equal,
/// or the declaration names an ancestor path of the entity's source
/// (segment-boundary prefix, so `Control/user/identity` governs
/// `.../identity/formation.md` but not `.../identity-archive/x.md`).
fn governs(declared: &str, entity_source: &str) -> bool {
    if declared == entity_source {
        return true;
    }
    entity_source
        .strip_prefix(declared)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some()
}

/// Apply a project context's world binding to a discovered object set:
/// annotate each bound entity with the binding provenance (same refs —
/// never a second subject), withhold entities whose governing sources are
/// all excluded, and refuse stand-in objects that re-declare an entity
/// subject under a different ref.
pub fn bind_project_context(
    objects: &mut Vec<WikiObject>,
    binding: &WorldBinding,
    absences: &mut Vec<String>,
) {
    if binding.sources.is_empty() {
        return;
    }
    // The entity subjects that exist: any stand-in re-declaring one under a
    // different ref is refused (a Project never mints a second human).
    // Subjects are claimed by the materialised entities themselves (they
    // carry the entity producer in their provenance); a discovered object
    // without that producer has no subject claim.
    let mut subject_refs: BTreeMap<String, String> = BTreeMap::new();
    for object in objects.iter() {
        if !is_materialised_entity(object) {
            continue;
        }
        if let Some(subject) = entity_subject(object) {
            subject_refs
                .entry(subject)
                .or_insert_with(|| object.ref_id().as_str().to_owned());
        }
    }
    let withheld_subjects = |object: &WikiObject| -> Vec<String> {
        let mut withheld = Vec::new();
        for declared in &binding.sources {
            if declared.state != "excluded" {
                continue;
            }
            if object_source_refs(object)
                .iter()
                .any(|source| governs(&declared.source_ref, source))
            {
                withheld.push(declared.source_ref.clone());
            }
        }
        withheld
    };

    let mut kept = Vec::new();
    let mut stand_ins = Vec::new();
    for object in objects.drain(..) {
        if let Some(subject) = entity_subject(&object) {
            if let Some(entity_ref) = subject_refs.get(&subject) {
                if entity_ref != object.ref_id().as_str() {
                    absences.push(format!(
                        "Object {} re-declares entity subject {subject}; kept the materialised entity {}",
                        object.ref_id().as_str(),
                        entity_ref
                    ));
                    stand_ins.push(object);
                    continue;
                }
            }
        }
        let excluded = withheld_subjects(&object);
        if !excluded.is_empty() {
            absences.push(format!(
                "Entity {} withheld from this context: source {} excluded by {} world relations",
                object.ref_id().as_str(),
                excluded.join(", "),
                binding.world_ref
            ));
            stand_ins.push(object);
            continue;
        }
        kept.push(object);
    }
    for object in &mut kept {
        annotate_entity(object, binding);
    }
    *objects = kept;
    // Withheld/stand-in objects leave the context; they are disclosed above
    // and dropped (nothing outside this context is touched).
    drop(stand_ins);
}

/// The pasu subject an object carries, if any (`aikit.pasu/v1` extension).
fn entity_subject(object: &WikiObject) -> Option<String> {
    let extensions = match object {
        WikiObject::Node(node) => Some(&node.extensions),
        _ => None,
    }?;
    extensions
        .get(super::central_entities::PASU_EXTENSION)
        .and_then(|value| value.get("subject_ref"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

/// True for objects the entity materialisation produced (their provenance
/// carries the entity producer ref) — the only objects that may claim a
/// pasu subject.
fn is_materialised_entity(object: &WikiObject) -> bool {
    let WikiObject::Node(node) = object else {
        return false;
    };
    node.provenance.iter().any(|entry| {
        entry
            .producer_ref
            .as_ref()
            .map(|producer| producer.as_str() == super::central_entities::ENTITY_PRODUCER_REF)
            .unwrap_or(false)
    })
}

/// Every source ref an object declares (its own and its provenance).
fn object_source_refs(object: &WikiObject) -> Vec<String> {
    match object {
        WikiObject::Node(node) => {
            let mut refs: Vec<String> = node
                .source_refs
                .iter()
                .map(|source| source.as_str().to_owned())
                .collect();
            refs.extend(
                node.provenance
                    .iter()
                    .map(|entry| entry.source_ref.as_str().to_owned()),
            );
            refs
        }
        _ => Vec::new(),
    }
}

/// Record the binding on an entity: a derived extension naming the world,
/// the convention (when the root lineage was applied by inheritance) and
/// the per-source effective revision + propagation path. The entity's own
/// ref, provenance and sourced relations are untouched.
fn annotate_entity(object: &mut WikiObject, binding: &WorldBinding) {
    let WikiObject::Node(node) = object else {
        return;
    };
    let is_entity = node.extensions.contains_key(super::central_entities::PASU_EXTENSION)
        || node.node_type == "pasu";
    if !is_entity {
        return;
    }
    let sources = node
        .source_refs
        .iter()
        .map(|source| source.as_str().to_owned())
        .chain(
            node.provenance
                .iter()
                .map(|entry| entry.source_ref.as_str().to_owned()),
        )
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();
    for declared in &binding.sources {
        if sources
            .iter()
            .any(|source| governs(&declared.source_ref, source))
        {
            bindings.push(json!({
                "source_ref": declared.source_ref,
                "state": declared.state,
                "effective_revision": declared.effective_revision,
                "propagation_path": declared.propagation_path,
            }));
        }
    }
    if bindings.is_empty() {
        return;
    }
    node.extensions.insert(
        BINDING_EXTENSION.to_owned(),
        json!({
            "producer_ref": BINDING_PRODUCER_REF,
            "world_ref": binding.world_ref,
            "inherited_root_lineage": binding.inherited_root_lineage,
            "bindings": bindings,
        }),
    );
}
