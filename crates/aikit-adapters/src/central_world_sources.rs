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
//! * An unavailable World-relations carrier is not an unconstrained context.
//!   Consumers withhold Central material instead of widening disclosure. Only
//!   explicit declaration absence may select the documented root lineage.

use crate::runner::CommandRunner;
use aikit_core::{AikitError, Result, WikiObject};
use serde_json::{json, Value};
use std::{collections::BTreeMap, collections::BTreeSet, fs, path::Path};

pub const BINDING_PRODUCER_REF: &str = "aikit/central-world-binding/v1";
pub const BINDING_EXTENSION: &str = "aikit.world-binding/v1";
pub const ROOT_WORLD_REF: &str = "control:root";

/// A world ref that has no authored record at all: the declaration is
/// *absent*. This is the only case where the root lineage applies by
/// convention. Distinct from "unreadable", which must never widen.
pub const WORLD_DECLARATION_ABSENT: &str = "central.world_declaration_absent";

/// The message Central uses for the absent case (`missing World <ref>`,
/// ctrl/src/world.rs:583). Kept as a fallback only: Central now names absence
/// in the error code, which is what a consumer should read.
const MISSING_WORLD_MARKER: &str = "missing World ";

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
    // This call *asks a question* Central answers with a structured envelope:
    // the `central.world_declaration_absent` answer — the one case where the
    // root lineage applies by convention — arrives as an `ok:false` envelope
    // with a non-zero exit (ctrl maps `invalid_input` to exit 2,
    // ctrl/src/cli.rs `exit_code`). `CommandRunner::run` returns `Ok` for a
    // command that ran and failed, so the envelope is read first and only a
    // command that could not run at all (or produced no envelope) is an
    // unavailability. Demanding exit 0 before reading would turn Central's
    // explicit "no authored record" into a source-level failure and withhold
    // the inherited graph from every project that declares no world.
    let output = runner.run(&argv)?;
    let envelope: Value = serde_json::from_str(&output.stdout).map_err(|e| {
        if output.ok() {
            AikitError::new("central.world_sources_invalid", e.to_string())
        } else {
            AikitError::new(
                "central.world_sources_unavailable",
                format!(
                    "`{}` exited with status {}: {e}",
                    argv.join(" "),
                    output.status
                ),
            )
        }
    })?;
    if envelope["ok"] != true {
        let code = envelope["error"]["code"].as_str().unwrap_or_default();
        let message = envelope["error"]["message"].as_str().unwrap_or("unknown");
        // Prefer the code: Central names absence explicitly. The marker check
        // stays for a Central that has not yet been rebuilt with it, and the
        // two must agree — a code that says absent on some other message would
        // widen what a turn receives on a failure that is not absence.
        let absent = code == WORLD_DECLARATION_ABSENT
            || (code.ends_with("invalid_input") && message.contains(MISSING_WORLD_MARKER));
        return Err(AikitError::new(
            if absent {
                WORLD_DECLARATION_ABSENT
            } else {
                "central.world_sources_unavailable"
            },
            format!("central.world.effective-sources did not succeed: {message}"),
        ));
    }
    let data = &envelope["data"];
    if data["world_ref"].as_str() != Some(world_ref) {
        return Err(AikitError::new(
            "central.world_sources_invalid",
            "Native World source reading changed or omitted the requested World identity",
        ));
    }
    let entries = data["sources"].as_array().ok_or_else(|| {
        AikitError::new(
            "central.world_sources_invalid",
            "Native World source reading omitted its source array",
        )
    })?;
    let mut binding = WorldBinding {
        world_ref: world_ref.into(),
        inherited_root_lineage: false,
        sources: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    for entry in entries {
        let source = entry["ref"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                AikitError::new("central.world_sources_invalid", "Missing source identity")
            })?;
        let state = entry["state"]
            .as_str()
            .filter(|s| matches!(*s, "available" | "excluded"))
            .ok_or_else(|| {
                AikitError::new(
                    "central.world_sources_invalid",
                    "Missing or unsupported native source state",
                )
            })?;
        let revision = entry["effective_revision"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                AikitError::new("central.world_sources_invalid", "Missing source revision")
            })?;
        if !seen.insert(source) {
            return Err(AikitError::new(
                "central.world_sources_invalid",
                "Duplicate effective source identity",
            ));
        }
        let path = entry["propagation_path"].as_array().ok_or_else(|| {
            AikitError::new(
                "central.world_sources_invalid",
                "Missing source propagation path",
            )
        })?;
        let propagation_path = path
            .iter()
            .map(|p| {
                p.as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        AikitError::new("central.world_sources_invalid", "Invalid propagation hop")
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        binding.sources.push(EffectiveSource {
            source_ref: source.into(),
            state: state.into(),
            effective_revision: revision.into(),
            propagation_path,
        });
    }
    Ok(binding)
}

/// The project context for a Central-relative project name: the declared
/// `project_id` from its ProjectCentral manifest when readable, else the
/// `project:<name>` convention.
pub fn project_world_ref(central_root: &Path, project: &str) -> String {
    let manifest = central_root
        .join("Work")
        .join(project)
        .join("ProjectCentral/project.json");
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

/// Read a project's effective binding, inheriting the root lineage **only**
/// when the project genuinely declares no world of its own (Central answers
/// `missing World <ref>` for a world ref with no authored record).
///
/// The two failure modes are kept apart deliberately. "No Project-specific
/// declaration" is convention: one world, one human, so the root lineage
/// applies and is disclosed as inherited. "The declaration could not be read
/// or validated" is a source-level failure, and the answer to it is *no
/// binding* — an unreadable exclusion must never broaden what a turn receives.
pub fn read_project_binding<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    project: &str,
    absences: &mut Vec<String>,
) -> Option<WorldBinding> {
    let world_ref = project_world_ref(central_root, project);
    match read_world_binding(
        runner,
        executable,
        central_root,
        "project",
        Some(project),
        &world_ref,
    ) {
        Ok(binding) => Some(binding),
        Err(error) if error.code() == WORLD_DECLARATION_ABSENT => {
            match read_world_binding(
                runner,
                executable,
                central_root,
                "root",
                None,
                ROOT_WORLD_REF,
            ) {
                Ok(mut binding) => {
                    binding.inherited_root_lineage = true;
                    absences.push(format!(
                        "Project {project} declares no world relations; the root lineage applies ({})",
                        error.message()
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
        Err(project_error) => {
            absences.push(format!(
                "Project {project} world relations could not be read or validated; binding is uncontextualised and no root lineage is assumed: {}",
                project_error.message()
            ));
            None
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
    // Coherence: withholding a node must not leave references to it behind.
    // An edge naming a withheld endpoint, or a space anchored on a withheld
    // entity, would otherwise survive in the permitted view as a pointer to
    // material the policy withheld. Membership is repaired in place; the
    // authored set is untouched at source.
    let removed: BTreeSet<String> = stand_ins
        .iter()
        .map(|object| object.ref_id().as_str().to_owned())
        .collect();
    let mut coherent = Vec::with_capacity(kept.len());
    for mut object in kept {
        let dangling = match &object {
            WikiObject::Edge(edge) => {
                if removed.contains(edge.from_ref.as_str()) {
                    Some(format!(
                        "Edge {} withheld with its endpoint {}",
                        edge.ref_id.as_str(),
                        edge.from_ref.as_str()
                    ))
                } else if removed.contains(edge.to_ref.as_str()) {
                    Some(format!(
                        "Edge {} withheld with its endpoint {}",
                        edge.ref_id.as_str(),
                        edge.to_ref.as_str()
                    ))
                } else {
                    None
                }
            }
            WikiObject::Space(space) => space
                .anchor_ref
                .as_ref()
                .filter(|anchor| removed.contains(anchor.as_str()))
                .map(|anchor| {
                    format!(
                        "Space {} withheld with its anchor {}",
                        space.ref_id.as_str(),
                        anchor.as_str()
                    )
                }),
            _ => None,
        };
        if let Some(reason) = dangling {
            absences.push(reason);
            stand_ins.push(object);
            continue;
        }
        if let WikiObject::Space(space) = &mut object {
            let before = space.node_refs.len();
            space
                .node_refs
                .retain(|member| !removed.contains(member.as_str()));
            if space.node_refs.len() != before {
                absences.push(format!(
                    "Space {} dropped {} withheld member(s)",
                    space.ref_id.as_str(),
                    before - space.node_refs.len()
                ));
            }
        }
        coherent.push(object);
    }
    *objects = coherent;
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

/// Every source ref an object declares (its own and its provenance). Edges and
/// spaces carry provenance as well, so a declared exclusion governing one of
/// them withholds it exactly as directly as it withholds a node.
fn object_source_refs(object: &WikiObject) -> Vec<String> {
    let mut refs: Vec<String> = Vec::new();
    let provenance = match object {
        WikiObject::Node(node) => {
            refs.extend(
                node.source_refs
                    .iter()
                    .map(|source| source.as_str().to_owned()),
            );
            &node.provenance
        }
        WikiObject::Edge(edge) => &edge.provenance,
        WikiObject::Space(space) => &space.provenance,
        _ => return refs,
    };
    refs.extend(
        provenance
            .iter()
            .map(|entry| entry.source_ref.as_str().to_owned()),
    );
    refs
}

/// Record the binding on an entity: a derived extension naming the world,
/// the convention (when the root lineage was applied by inheritance) and
/// the per-source effective revision + propagation path. The entity's own
/// ref, provenance and sourced relations are untouched.
fn annotate_entity(object: &mut WikiObject, binding: &WorldBinding) {
    let WikiObject::Node(node) = object else {
        return;
    };
    let is_entity = node
        .extensions
        .contains_key(super::central_entities::PASU_EXTENSION)
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::knowledge_wiki::{
        WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef, WikiSpace,
        OKF_WIKI_PROFILE,
    };
    use aikit_core::resource::{ResourceRef, SourceRef};

    use crate::central_entities::{ENTITY_PRODUCER_REF, PASU_EXTENSION};
    use crate::runner::{CommandRunner, Output};

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn s(raw: &str) -> SourceRef {
        SourceRef::parse(raw).unwrap()
    }

    /// A materialised pasu entity: the only object kind that claims a subject.
    fn entity(ref_raw: &str, subject: &str, sources: &[&str]) -> WikiObject {
        let mut extensions = BTreeMap::new();
        extensions.insert(PASU_EXTENSION.to_owned(), json!({ "subject_ref": subject }));
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r(ref_raw),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: s(sources
                    .first()
                    .copied()
                    .unwrap_or("central:source:control:root:x")),
                source_revision: None,
                producer_ref: Some(r(ENTITY_PRODUCER_REF)),
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "pasu".into(),
            title: Some(subject.to_owned()),
            space_refs: Vec::new(),
            source_refs: sources.iter().map(|raw| s(raw)).collect(),
            local_space_ref: None,
            extensions,
        })
    }

    /// An ordinary authored wiki node: no subject claim, no entity producer.
    fn node(ref_raw: &str, sources: &[&str]) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r(ref_raw),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some(ref_raw.to_owned()),
            space_refs: Vec::new(),
            source_refs: sources.iter().map(|raw| s(raw)).collect(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    fn edge(ref_raw: &str, from: &str, to: &str) -> WikiObject {
        WikiObject::Edge(WikiEdge {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r(ref_raw),
            revision: 1,
            provenance: Vec::new(),
            from_ref: r(from),
            to_ref: r(to),
            relation: "relates-to".into(),
            origin: WikiEdgeOrigin::Compiled,
            origin_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    fn space(ref_raw: &str, anchor: Option<&str>, members: &[&str]) -> WikiObject {
        WikiObject::Space(WikiSpace {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r(ref_raw),
            revision: 1,
            provenance: Vec::new(),
            title: Some(ref_raw.to_owned()),
            parent_space_refs: Vec::new(),
            child_space_refs: Vec::new(),
            node_refs: members.iter().map(|raw| r(raw)).collect(),
            anchor_ref: anchor.map(r),
            extensions: BTreeMap::new(),
        })
    }

    fn binding(world_ref: &str, sources: &[(&str, &str)]) -> WorldBinding {
        WorldBinding {
            world_ref: world_ref.into(),
            inherited_root_lineage: false,
            sources: sources
                .iter()
                .map(|(source_ref, state)| EffectiveSource {
                    source_ref: (*source_ref).into(),
                    state: (*state).into(),
                    effective_revision: "rev-1".into(),
                    propagation_path: vec!["control:root".into(), world_ref.into()],
                })
                .collect(),
        }
    }

    fn refs(objects: &[WikiObject]) -> Vec<String> {
        objects
            .iter()
            .map(|object| object.ref_id().as_str().to_owned())
            .collect()
    }

    // --- the withholding rule (absolute: withhold, never broaden) ---

    #[test]
    fn a_declared_exclusion_withholds_the_entity_its_edges_and_its_space() {
        let mut objects = vec![
            entity(
                "wiki:node:identity",
                "central:pasu:identity",
                &["central:source:control:root:Control/user/identity"],
            ),
            node(
                "wiki:node:ordinary",
                &["central:source:control:root:Control/agents"],
            ),
            edge("wiki:edge:one", "wiki:node:identity", "wiki:node:ordinary"),
            // Anchored on the withheld entity: the space leaves with it.
            space(
                "central:wiki:root",
                Some("wiki:node:identity"),
                &["wiki:node:identity", "wiki:node:ordinary"],
            ),
            // Anchored on a kept entity with the withheld one as a mere
            // member: the space survives with its membership repaired.
            space(
                "central:wiki:project:epilogos/demo",
                Some("wiki:node:ordinary"),
                &["wiki:node:identity", "wiki:node:ordinary"],
            ),
        ];
        let world = binding(
            "epilogos/demo",
            &[
                ("central:source:control:root:Control/user", "excluded"),
                ("central:source:control:root:Control/agents", "available"),
            ],
        );
        let mut absences = Vec::new();
        bind_project_context(&mut objects, &world, &mut absences);

        // The excluded entity, the edge naming it as an endpoint, and the
        // space anchored on it all leave the context together.
        assert_eq!(
            refs(&objects),
            vec![
                "wiki:node:ordinary".to_owned(),
                "central:wiki:project:epilogos/demo".to_owned(),
            ],
            "{absences:?}"
        );
        // The surviving space's membership was repaired in place.
        match objects.last() {
            Some(WikiObject::Space(space)) => {
                assert_eq!(space.node_refs, vec![r("wiki:node:ordinary")])
            }
            other => panic!("expected the repaired space, got {other:?}"),
        }
        // Every withholding is disclosed, nothing is silent.
        assert_eq!(absences.len(), 4, "{absences:?}");
        assert!(absences.iter().all(|absence| absence.contains("withheld")));
    }

    #[test]
    fn an_available_binding_annotates_kept_entities_and_touches_nothing_else() {
        let mut objects = vec![
            entity(
                "wiki:node:identity",
                "central:pasu:identity",
                &["central:source:control:root:Control/user/identity"],
            ),
            node(
                "wiki:node:ordinary",
                &["central:source:control:root:Control/agents"],
            ),
        ];
        let world = binding(
            "epilogos/demo",
            &[("central:source:control:root:Control/user", "available")],
        );
        let mut absences = Vec::new();
        bind_project_context(&mut objects, &world, &mut absences);
        assert!(absences.is_empty(), "{absences:?}");
        assert_eq!(objects.len(), 2);
        match &objects[0] {
            WikiObject::Node(node) => {
                let extension = &node.extensions[BINDING_EXTENSION];
                assert_eq!(extension["world_ref"], "epilogos/demo");
                assert_eq!(extension["bindings"][0]["state"], "available");
                assert_eq!(extension["bindings"][0]["effective_revision"], "rev-1");
            }
            other => panic!("expected the annotated entity, got {other:?}"),
        }
        // A non-entity keeps no binding extension: the binding governs
        // entities, it does not rewrite authored wiki objects.
        match &objects[1] {
            WikiObject::Node(node) => assert!(!node.extensions.contains_key(BINDING_EXTENSION)),
            other => panic!("expected the untouched node, got {other:?}"),
        }
    }

    #[test]
    fn a_stand_in_redeclaration_of_an_entity_subject_is_refused() {
        let mut objects = vec![
            entity(
                "wiki:node:identity",
                "central:pasu:identity",
                &["central:source:control:root:Control/user/identity"],
            ),
            // A discovered object re-declaring the same subject under another
            // ref: the materialised entity keeps the subject.
            node("wiki:node:stand-in", &["central:source:project:other:x"]),
        ];
        // Make the stand-in claim the subject directly.
        if let WikiObject::Node(node) = &mut objects[1] {
            node.extensions.insert(
                PASU_EXTENSION.to_owned(),
                json!({ "subject_ref": "central:pasu:identity" }),
            );
        }
        let world = binding(
            "epilogos/demo",
            &[("central:source:control:root:Control", "available")],
        );
        let mut absences = Vec::new();
        bind_project_context(&mut objects, &world, &mut absences);
        assert_eq!(refs(&objects), vec!["wiki:node:identity".to_owned()]);
        assert!(
            absences
                .iter()
                .any(|absence| absence.contains("re-declares entity subject")),
            "{absences:?}"
        );
    }

    // --- the binding read (absence inherits, failure never widens) ---

    /// Answers each `central.world.effective-sources` call from a canned
    /// envelope keyed by the requested scope.
    struct EnvelopeRunner {
        project: String,
        root: String,
    }

    impl CommandRunner for EnvelopeRunner {
        fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
            let input: Value = serde_json::from_str(argv.last().expect("argv has the input"))
                .expect("the last argument is the Action input");
            let stdout = if input["scope"] == "project" {
                &self.project
            } else {
                &self.root
            };
            Ok(Output::success(stdout.clone()))
        }
    }

    fn ok_envelope(world_ref: &str, sources: Value) -> String {
        json!({
            "ok": true,
            "data": { "world_ref": world_ref, "sources": sources }
        })
        .to_string()
    }

    fn error_envelope(code: &str, message: &str) -> String {
        json!({
            "ok": false,
            "error": { "code": code, "message": message }
        })
        .to_string()
    }

    fn root_sources() -> Value {
        json!([{
            "ref": "central:source:control:root:Control",
            "state": "available",
            "effective_revision": "root-rev-1",
            "propagation_path": ["control:root"]
        }])
    }

    #[test]
    fn an_absent_project_declaration_inherits_the_root_lineage_and_discloses_it() {
        let runner = EnvelopeRunner {
            project: error_envelope(WORLD_DECLARATION_ABSENT, "missing World project:bare"),
            root: ok_envelope(ROOT_WORLD_REF, root_sources()),
        };
        let mut absences = Vec::new();
        let binding = read_project_binding(
            &runner,
            Path::new("ctrl"),
            Path::new("/central"),
            "bare",
            &mut absences,
        )
        .expect("absence selects the documented root lineage");
        assert!(binding.inherited_root_lineage);
        assert_eq!(binding.world_ref, ROOT_WORLD_REF);
        assert_eq!(binding.sources.len(), 1);
        assert_eq!(binding.sources[0].state, "available");
        assert_eq!(binding.sources[0].propagation_path, vec!["control:root"]);
        assert!(
            absences
                .iter()
                .any(|absence| absence.contains("declares no world relations")),
            "{absences:?}"
        );
    }

    #[test]
    fn an_unreadable_project_declaration_never_widens_to_the_root_lineage() {
        let runner = EnvelopeRunner {
            project: error_envelope(
                "central.world_sources_unavailable",
                "the world relations file is corrupt",
            ),
            root: ok_envelope(ROOT_WORLD_REF, root_sources()),
        };
        let mut absences = Vec::new();
        let binding = read_project_binding(
            &runner,
            Path::new("ctrl"),
            Path::new("/central"),
            "demo",
            &mut absences,
        );
        assert!(
            binding.is_none(),
            "a source-level failure yields no binding"
        );
        assert!(
            absences
                .iter()
                .any(|absence| absence.contains("no root lineage is assumed")),
            "{absences:?}"
        );
    }

    #[test]
    fn a_readable_project_declaration_is_answered_as_declared() {
        let runner = EnvelopeRunner {
            // No ProjectCentral manifest in the fixture root, so the effective
            // world ref is the `project:<name>` convention — the envelope must
            // echo exactly the world that was asked about.
            project: ok_envelope(
                "project:demo",
                json!([{
                    "ref": "central:source:control:root:Control/user",
                    "state": "excluded",
                    "effective_revision": "proj-rev-1",
                    "propagation_path": ["control:root", "project:demo"]
                }]),
            ),
            root: ok_envelope(ROOT_WORLD_REF, root_sources()),
        };
        let mut absences = Vec::new();
        let binding = read_project_binding(
            &runner,
            Path::new("ctrl"),
            Path::new("/central"),
            "demo",
            &mut absences,
        )
        .expect("a readable declaration is a binding");
        assert!(!binding.inherited_root_lineage);
        assert_eq!(binding.world_ref, "project:demo");
        assert_eq!(binding.sources[0].state, "excluded");
        assert!(absences.is_empty(), "{absences:?}");
    }

    // --- the world ref convention (manifest id, else the Work name) ---

    #[test]
    fn the_world_ref_uses_the_manifest_project_id_and_falls_back_to_the_work_name() {
        let temp = tempfile::tempdir().unwrap();
        let manifest = temp.path().join("Work/demo/ProjectCentral/project.json");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &manifest,
            r#"{"schema":"central.project/v1","project_id":"epilogos/demo"}"#,
        )
        .unwrap();
        assert_eq!(
            project_world_ref(temp.path(), "demo"),
            "epilogos/demo",
            "a readable manifest supplies the declared project id"
        );
        assert_eq!(
            project_world_ref(temp.path(), "bare"),
            "project:bare",
            "a member with no manifest scopes by its Work name"
        );
        std::fs::write(&manifest, "not json at all").unwrap();
        assert_eq!(
            project_world_ref(temp.path(), "demo"),
            "project:demo",
            "an unreadable manifest never invents an identity"
        );
    }
}
