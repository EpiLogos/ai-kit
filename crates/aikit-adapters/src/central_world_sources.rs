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
//!   explicit declaration absence may select the documented root lineage —
//!   including a Work member with no ProjectCentral manifest at all, whose
//!   declaration is structurally non-existent (there is no project record to
//!   read), not merely unreadable.

use crate::runner::{CommandRunner, SystemRunner};
use aikit_core::{AikitError, Result, WikiObject};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};

pub const BINDING_PRODUCER_REF: &str = "aikit/central-world-binding/v1";
pub const BINDING_EXTENSION: &str = "aikit.world-binding/v1";
pub const ROOT_WORLD_REF: &str = "control:root";
pub const WORLD_DECLARATION_ABSENT: &str = "central.world_declaration_absent";

// A composite observation shares one live allowance. The runner owns its
// separate finite retirement allowance; this is not a bound on JSON heap use.
const WORLD_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const WORLD_PROBE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct WorldBinding {
    pub world_ref: String,
    pub inherited_root_lineage: bool,
    pub sources: Vec<EffectiveSource>,
}

#[derive(Debug, Clone)]
pub struct EffectiveSource {
    pub source_ref: String,
    pub state: String,
    pub effective_revision: String,
    pub propagation_path: Vec<String>,
    /// The actual owner relation, including authority, treatment, each
    /// provenance hop and future fields. Pure local bindings have no receipt.
    pub native_relation: Option<Value>,
}

struct WorldProbe {
    started: Instant,
    timeout: Duration,
    remaining_bytes: usize,
    last_observation: Option<(Value, crate::runner::Output)>,
}

impl WorldProbe {
    fn new<R: CommandRunner>(runner: &R) -> Self {
        Self {
            started: Instant::now(),
            timeout: runner
                .configured_timeout()
                .unwrap_or(WORLD_PROBE_TIMEOUT)
                .min(WORLD_PROBE_TIMEOUT),
            remaining_bytes: WORLD_PROBE_BYTES,
            last_observation: None,
        }
    }

    // Only the last actual bounded request is retained until its returned
    // data is qualified. This is execution evidence, never another World reading.
    fn qualify_error(&self, error: AikitError) -> AikitError {
        match &self.last_observation {
            Some((envelope, output)) => error
                .with_native_result(envelope.clone())
                .with_native_capture(
                    Some(output.status),
                    output.stdout.as_bytes().to_vec(),
                    output.stderr.as_bytes().to_vec(),
                ),
            None => error,
        }
    }

    fn require_live_allowance(&self) -> Result<()> {
        if self.started.elapsed() > self.timeout {
            return Err(AikitError::new(
                "central.world_probe_timeout",
                "World observation allowance expired before acknowledgement",
            ));
        }
        Ok(())
    }

    fn request<R: CommandRunner>(
        &mut self,
        runner: &R,
        executable: &Path,
        central_root: &Path,
        action: &str,
        input: Value,
    ) -> Result<Value> {
        let timeout = self
            .timeout
            .checked_sub(self.started.elapsed())
            .ok_or_else(|| {
                AikitError::new(
                    "central.world_probe_timeout",
                    "World observation live allowance expired",
                )
                .with("execution_started", "false")
                .with("native_action", action)
            })?;
        if timeout.is_zero() || self.remaining_bytes < 2 {
            return Err(AikitError::new(
                "central.world_probe_budget",
                "World observation has no remaining capture allowance",
            )
            .with("execution_started", "false")
            .with("native_action", action));
        }
        // The existing transport takes text argv. Refuse an unrepresentable
        // physical coordinate rather than selecting a lossy replacement name.
        let coordinate = |path: &Path, name: &str| -> Result<String> {
            path.to_str().map(str::to_owned).ok_or_else(|| {
                AikitError::new(
                    "central.world_transport_unsupported",
                    "Native World text transport requires an exact UTF-8 coordinate",
                )
                .with("coordinate", name)
                .with("execution_started", "false")
            })
        };
        let argv = vec![
            coordinate(executable, "executable")?,
            "--json".into(),
            "--root".into(),
            coordinate(central_root, "root")?,
            "action".into(),
            "run".into(),
            action.into(),
            input.to_string(),
        ];
        let output = runner
            .run_with_limits(&argv, timeout, self.remaining_bytes, true)
            .map_err(|error| {
                super::central_file_map::private_transport_failure(error.code(), &error)
                    .with("native_action", action)
            })?;
        let observed = |error: AikitError| {
            error
                .with("native_action", action)
                .with("status", output.status.to_string())
                .with("stdout_text_bytes", output.stdout.len().to_string())
                .with("stderr_text_bytes", output.stderr.len().to_string())
                .with_native_capture(
                    Some(output.status),
                    output.stdout.as_bytes().to_vec(),
                    output.stderr.as_bytes().to_vec(),
                )
        };
        let captured = output
            .stdout
            .len()
            .checked_add(output.stderr.len())
            .ok_or_else(|| {
                observed(AikitError::new(
                    "central.world_probe_budget",
                    "World capture length overflowed",
                ))
            })?;
        self.remaining_bytes = self.remaining_bytes.checked_sub(captured).ok_or_else(|| {
            observed(AikitError::new(
                "central.world_probe_budget",
                "Runner exceeded World capture allowance",
            ))
        })?;
        if self.started.elapsed() > self.timeout {
            return Err(observed(AikitError::new(
                "central.world_probe_timeout",
                "World observation exceeded its live allowance",
            )));
        }
        let envelope: Value = serde_json::from_str(&output.stdout).map_err(|error| {
            observed(
                AikitError::new(
                    "central.world_sources_invalid",
                    "Invalid native World JSON response",
                )
                .with_private_native_cause(&AikitError::new(
                    "central.world_sources_invalid",
                    error.to_string(),
                )),
            )
        })?;
        match envelope["ok"].as_bool() {
            Some(false) => {
                let native_error = envelope
                    .get("error")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| {
                        observed(
                            AikitError::new(
                                "central.world_sources_invalid",
                                "Native World failure omitted its error envelope",
                            )
                            .with_native_result(envelope.clone()),
                        )
                    })?;
                let absent = action == "central.world.effective-sources"
                    && native_error["code"].as_str() == Some(WORLD_DECLARATION_ABSENT)
                    && native_error["details"]["state"].as_str() == Some("absent")
                    && native_error["details"]["world_ref"] == input["world_ref"]
                    && input["world_ref"].as_str().is_some();
                let mut failure = AikitError::new(
                    if absent {
                        WORLD_DECLARATION_ABSENT
                    } else {
                        "central.world_sources_unavailable"
                    },
                    "Native World request failed",
                )
                .with(
                    "native_code",
                    super::central_file_map::native_failure_code(&native_error["code"]),
                )
                .with_native_result(envelope.clone());
                if let Some(present) =
                    native_error["details"]["requested_declaration_present"].as_bool()
                {
                    failure = failure.with("requested_declaration_present", present.to_string());
                }
                Err(observed(failure))
            }
            Some(true) if output.ok() => {
                let data = envelope.get("data").cloned().ok_or_else(|| {
                    observed(
                        AikitError::new(
                            "central.world_sources_invalid",
                            "Native World success omitted data",
                        )
                        .with_native_result(envelope.clone()),
                    )
                })?;
                self.last_observation = Some((envelope.clone(), output.clone()));
                Ok(data)
            }
            _ => Err(observed(
                AikitError::new(
                    "central.world_sources_invalid",
                    "Native World receipt has no consistent success/failure state",
                )
                .with_native_result(envelope.clone()),
            )),
        }
    }
}

fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            AikitError::new(
                "central.world_sources_invalid",
                format!("Missing native {key}"),
            )
        })
}

fn read_world_binding_in<R: CommandRunner>(
    probe: &mut WorldProbe,
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
    let data = probe.request(
        runner,
        executable,
        central_root,
        "central.world.effective-sources",
        input,
    )?;
    (|| {
        if data["world_ref"].as_str() != Some(world_ref) {
            return Err(AikitError::new(
                "central.world_sources_invalid",
                "Native World reading changed the requested identity",
            ));
        }
        let entries = data["sources"].as_array().ok_or_else(|| {
            AikitError::new(
                "central.world_sources_invalid",
                "Missing native source array",
            )
        })?;
        let mut binding = WorldBinding {
            world_ref: world_ref.into(),
            inherited_root_lineage: false,
            sources: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for entry in entries {
            let source = required_text(entry, "ref")?;
            let state = required_text(entry, "state")?;
            if !matches!(state, "available" | "excluded") || !seen.insert(source) {
                return Err(AikitError::new(
                    "central.world_sources_invalid",
                    "Unsupported source state or duplicate effective source identity",
                ));
            }
            let revision = required_text(entry, "effective_revision")?;
            required_text(entry, "effective_source_world")?;
            for key in ["authority", "source_treatment", "effective_treatment"] {
                if entry[key].as_str().is_none() {
                    return Err(AikitError::new(
                        "central.world_sources_invalid",
                        format!("Missing native {key}"),
                    ));
                }
            }
            let path = entry["propagation_path"].as_array().ok_or_else(|| {
                AikitError::new(
                    "central.world_sources_invalid",
                    "Missing source propagation path",
                )
            })?;
            let propagation_path = path
                .iter()
                .map(|hop| {
                    hop.as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            AikitError::new(
                                "central.world_sources_invalid",
                                "Invalid propagation hop",
                            )
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            let provenance = entry["provenance"].as_array().ok_or_else(|| {
                AikitError::new(
                    "central.world_sources_invalid",
                    "Missing native provenance array",
                )
            })?;
            for hop in provenance {
                required_text(hop, "world")?;
                for key in ["revision", "authority", "treatment"] {
                    if hop[key].as_str().is_none() {
                        return Err(AikitError::new(
                            "central.world_sources_invalid",
                            format!("Missing native provenance {key}"),
                        ));
                    }
                }
            }
            binding.sources.push(EffectiveSource {
                source_ref: source.into(),
                state: state.into(),
                effective_revision: revision.into(),
                propagation_path,
                native_relation: Some(entry.clone()),
            });
        }
        probe.require_live_allowance()?;
        Ok(binding)
    })()
    .map_err(|error: AikitError| probe.qualify_error(error))
}

/// A bounded, strict native effective-source read. No manifest or prose fallback.
pub fn read_world_binding<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    scope: &str,
    project: Option<&str>,
    world_ref: &str,
) -> Result<WorldBinding> {
    read_world_binding_in(
        &mut WorldProbe::new(runner),
        runner,
        executable,
        central_root,
        scope,
        project,
        world_ref,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectFacetIdentity {
    Present(String),
    ManifestAbsent,
}

struct ProjectFacet {
    identity: ProjectFacetIdentity,
    receipt: Value,
}

fn read_project_facet<R: CommandRunner>(
    probe: &mut WorldProbe,
    runner: &R,
    executable: &Path,
    central_root: &Path,
    project: &str,
) -> Result<ProjectFacet> {
    let data = probe.request(
        runner,
        executable,
        central_root,
        "central.world.here",
        json!({"project": project}),
    )?;
    if data["schema"].as_str() != Some("central.world-here/v1") {
        return Err(probe.qualify_error(AikitError::new(
            "central.world_sources_invalid",
            "Unsupported native World here schema",
        )));
    }
    let facet = data.get("project_world").ok_or_else(|| {
        probe.qualify_error(AikitError::new(
            "central.world_sources_invalid",
            "Native World here omitted selected Project facet",
        ))
    })?;
    let identity = if facet["name"].as_str() != Some(project) {
        None
    } else {
        match facet["state"].as_str() {
            Some("present") => Some(ProjectFacetIdentity::Present(
                required_text(facet, "ref")
                    .map_err(|error| probe.qualify_error(error))?
                    .into(),
            )),
            Some("absent")
                if facet["absence_kind"].as_str() == Some("projectcentral-manifest-absent")
                    && facet["work_member_present"] == true =>
            {
                Some(ProjectFacetIdentity::ManifestAbsent)
            }
            _ => None,
        }
    }
    .ok_or_else(|| {
        probe.qualify_error(
            AikitError::new(
                "central.world_sources_unavailable",
                "Native selected Project facet is absent, changed or unavailable",
            )
            .with(
                "project_absence_kind",
                match facet["absence_kind"].as_str() {
                    Some("work-member-absent") => "work-member-absent",
                    Some("projectcentral-manifest-absent") => "projectcentral-manifest-absent",
                    _ => "unrecognized",
                },
            )
            .with_native_result(facet.clone()),
        )
    })?;
    probe.require_live_allowance()?;
    Ok(ProjectFacet {
        identity,
        receipt: facet.clone(),
    })
}

/// Obtain the actual native Project World reference. This returns an error
/// for a manifest-less member; it never mints an identity from a directory name.
/// Rust callers must now handle Result<String> rather than a fabricated String.
pub fn project_world_ref(central_root: &Path, project: &str) -> Result<String> {
    let runner = SystemRunner::new();
    let executable = super::central_file_map::executable();
    let facet = read_project_facet(
        &mut WorldProbe::new(&runner),
        &runner,
        &executable,
        central_root,
        project,
    )?;
    match facet.identity {
        ProjectFacetIdentity::Present(reference) => Ok(reference),
        ProjectFacetIdentity::ManifestAbsent => Err(AikitError::new(
            "central.project_world_absent",
            "Existing Work member has no native Project World facet",
        )
        .with_native_result(facet.receipt.clone())),
    }
}

fn read_project_binding_result<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    project: &str,
) -> Result<(WorldBinding, Option<String>)> {
    let mut probe = WorldProbe::new(runner);
    let before = read_project_facet(&mut probe, runner, executable, central_root, project)?;
    let (mut binding, inheritance) = match &before.identity {
        ProjectFacetIdentity::Present(reference) => match read_world_binding_in(
            &mut probe,
            runner,
            executable,
            central_root,
            "project",
            Some(project),
            reference,
        ) {
            Ok(binding) => (binding, None),
            Err(error) if error.code() == WORLD_DECLARATION_ABSENT => {
                let root = read_world_binding_in(
                    &mut probe,
                    runner,
                    executable,
                    central_root,
                    "root",
                    None,
                    ROOT_WORLD_REF,
                )?;
                (root, Some(format!("Project {project} declares no World relations; root lineage applies: {error}")))
            }
            Err(error) => return Err(error),
        },
        ProjectFacetIdentity::ManifestAbsent => {
            let root = read_world_binding_in(
                &mut probe,
                runner,
                executable,
                central_root,
                "root",
                None,
                ROOT_WORLD_REF,
            )?;
            (root, Some(format!("Existing Work/{project} has no ProjectCentral manifest; root lineage applies (native projectcentral-manifest-absent facet)")))
        }
    };
    let current = read_project_facet(&mut probe, runner, executable, central_root, project)
        .map_err(|error| {
            let prior = AikitError::new(
                "central.world_binding_changed",
                "Actual prior native Project facet",
            )
            .with_native_result(before.receipt.clone())
            .with_private_native_cause(&error);
            error.with_private_native_cause(&prior)
        })?;
    if current.identity != before.identity {
        return Err(AikitError::new(
            "central.world_binding_changed",
            "Native Project World identity changed during binding observation",
        )
        .with_native_result(current.receipt.clone())
        .with_private_native_cause(
            &AikitError::new(
                "central.world_binding_changed",
                "Prior native Project facet",
            )
            .with_native_result(before.receipt.clone()),
        ));
    }
    binding.inherited_root_lineage = inheritance.is_some();
    probe.require_live_allowance()?;
    Ok((binding, inheritance))
}

/// Read the actual native Project facet and current relations. An unavailable
/// owner remains unavailable; only typed target absence permits root lineage.
pub fn read_project_binding<R: CommandRunner>(
    runner: &R,
    executable: &Path,
    central_root: &Path,
    project: &str,
    absences: &mut Vec<String>,
) -> Option<WorldBinding> {
    match read_project_binding_result(runner, executable, central_root, project) {
        Ok((binding, inheritance)) => {
            if let Some(disclosure) = inheritance {
                absences.push(disclosure);
            }
            Some(binding)
        }
        Err(error) => {
            absences.push(format!("Project {project} World binding unavailable; no root lineage is assumed: {}: {error}", error.code()));
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
            let mut observed = json!({
                "source_ref": declared.source_ref,
                "state": declared.state,
                "effective_revision": declared.effective_revision,
                "propagation_path": declared.propagation_path,
            });
            if let Some(relation) = &declared.native_relation {
                observed["native_relation"] = relation.clone();
            }
            bindings.push(observed);
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
                    native_relation: None,
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
}
