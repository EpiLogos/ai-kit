//! Development entry: the prepared operative context of the work a body is
//! actually carrying.
//!
//! A task is a Run. A body inhabiting a Position carries current work that
//! Factory names exactly (`factory development current-work`), and the World
//! join already resolves it — Position → Factory state → current work (Run,
//! WorkflowUnit, custody) → the work's source-qualified child NOW. The
//! WorkflowUnit carries the authored meaning of the act: its developmental
//! concern, the difference it must make, the praxis and capabilities it
//! names, the verification it owes and where its Return goes.
//!
//! The entry is that work, prepared. It runs exactly when Refocus delivers —
//! fresh occupancy, compaction, a current-work transition, sustained work —
//! and calls the existing NOW preparation (`now-context prepare`) with the
//! Run as its basis: the child NOW's exact sources, the Factory Run and unit,
//! and precisely the unit's capability rows from the Project matrix. Nothing
//! is inferred from what the person typed; a body with no current work gets
//! no entry, and a conversation stays a conversation.
//!
//! What the body receives is references with revisions, not bodies; "emitted"
//! here is emission on the harness context channel, never observed use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::id::CapsuleId;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::now_context::{PreparedNowContext, RedisNowConfig, RedisNowStore};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::inhabitation::{pick, Joined};

pub const CAPABILITY: &str = "hook/aikit/development-entry";
pub const ENTRY_SCHEMA: &str = "aikit.development-entry/v2";
const PREPARE_SCHEMA: &str = "aikit.now-preparation-request/v1";
/// Rendered entry budget. The entry names; it does not carry bodies.
const MAX_RENDERED_CHARS: usize = 6_000;

/// The capsule's configuration (`[config."hook/aikit/development-entry"]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EntryConfig {
    /// An `aikit.redis-now-config/v1` file: where the participant's prepared
    /// view lives. Required — the entry is a prepared NOW view.
    #[serde(default)]
    pub redis_config: Option<PathBuf>,
}

impl EntryConfig {
    pub fn from_table(table: &toml::value::Table) -> Result<Self> {
        toml::Value::Table(table.clone())
            .try_into()
            .map_err(|error| fail("development_entry.config", error.to_string()))
    }
}

fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message.into())
}

/// The work a body is carrying, exactly as the World join resolved it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkBinding {
    pub central_root: PathBuf,
    pub project: String,
    pub project_root: PathBuf,
    pub factory_state: PathBuf,
    pub position_ref: String,
    pub occupant_generation: String,
    pub work_ref: Option<String>,
    pub run_ref: String,
    pub workflow_unit_ref: String,
    pub child_now_ref: String,
    pub work_digest: Option<String>,
    /// The `factory development workflow-unit` reading.
    pub workflow_unit: Value,
}

impl WorkBinding {
    /// `None` unless the join resolved exactly one current work with its
    /// Run, WorkflowUnit and source-qualified child NOW. Ambiguous or absent
    /// work has no entry: Factory refuses to guess, and so does this.
    pub fn from_joined(joined: &Joined) -> Option<Self> {
        let reading = &joined.reading;
        let identity = &reading.identity;
        if identity.current_work_outcome.as_deref() != Some("one") {
            return None;
        }
        let refs = &identity.current_work_refs;
        Some(Self {
            central_root: PathBuf::from(joined.trail.central_root.as_ref()?),
            project: joined.trail.project_name.clone()?,
            project_root: joined.trail.project_root.clone()?,
            factory_state: PathBuf::from(joined.trail.factory_state.as_ref()?),
            position_ref: reading.position_ref.clone()?,
            occupant_generation: reading.occupant_generation.clone()?,
            work_ref: refs.get("work_ref").cloned(),
            run_ref: refs.get("run_ref")?.clone(),
            workflow_unit_ref: refs.get("workflow_unit_ref")?.clone(),
            child_now_ref: identity.child_now_ref.clone()?,
            work_digest: identity.current_work_digest.clone(),
            workflow_unit: joined.trail.workflow_unit.clone()?,
        })
    }

    pub fn participant_ref(&self) -> String {
        format!("participant/{}", self.position_ref)
    }
}

/// The `factory.workflow-unit-reading/v1` field for a snake-case name: the
/// reading spells it camelCase (`developmentalConcern`); the Return is nested
/// (`requiredReturn.contract` / `.address`); agent requirements nest under
/// `agentRequirements`.
fn unit_value<'a>(unit: &'a Value, key: &str) -> &'a Value {
    let camel = {
        let mut out = String::new();
        let mut upper = false;
        for c in key.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                out.extend(c.to_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    };
    let nested = match key {
        "required_return_contract" => &unit["requiredReturn"]["contract"],
        "required_return_address" => &unit["requiredReturn"]["address"],
        "agent_refs" => &unit["agentRequirements"]["agentRefs"],
        "agent_set_refs" => &unit["agentRequirements"]["agentSetRefs"],
        _ => &Value::Null,
    };
    [&unit[camel.as_str()], &unit[key], nested]
        .into_iter()
        .find(|value| !value.is_null())
        .unwrap_or(&Value::Null)
}

fn unit_field(unit: &Value, key: &str) -> Option<String> {
    unit_value(unit, key)
        .as_str()
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
}

fn unit_list(unit: &Value, key: &str) -> Vec<String> {
    unit_value(unit, key)
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .or_else(|| pick(item, &["ref", "value"]))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The authored concern of the work: the unit's own developmental concern and
/// the difference it must make — never the text of a chat turn.
pub fn work_concern(binding: &WorkBinding) -> Option<String> {
    let concern = unit_field(&binding.workflow_unit, "developmental_concern")?;
    Some(
        match unit_field(&binding.workflow_unit, "required_difference") {
            Some(difference) => format!("{concern} — required difference: {difference}"),
            None => concern,
        },
    )
}

/// The Project matrix carriers, telos-first, from the checkout the body works
/// in and then the primary. `None` when the Project carries none.
fn matrix_carriers(binding: &WorkBinding, cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    let checkout = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf);
    [checkout, Some(binding.project_root.clone())]
        .into_iter()
        .flatten()
        .flat_map(|root| {
            let pc = root.join("ProjectCentral");
            [
                pc.join("user").join("telos"),
                pc.join("user"),
                pc.join("telos"),
            ]
        })
        .map(|base| {
            (
                base.join("capability-matrix.json"),
                base.join("capability-matrix.csv"),
            )
        })
        .find(|(manifest, csv)| manifest.is_file() && csv.is_file())
}

/// Capability ids a matrix CSV declares, read only to split the unit's named
/// capabilities into those this Project's matrix carries and those it does
/// not (a unit may name suite or sibling capabilities).
fn matrix_ids(csv: &Path) -> Vec<String> {
    std::fs::read_to_string(csv)
        .map(|text| {
            text.lines()
                .skip(1)
                .filter_map(|line| line.split(',').next())
                .filter(|id| id.starts_with("cap."))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The `aikit.now-preparation-request/v1` for this work. Pure: every input is
/// a resolved owner ref; the preparation itself re-reads and revalidates them.
pub fn preparation_request(
    binding: &WorkBinding,
    cwd: &Path,
    agent_session: &str,
    redis: &RedisNowConfig,
    expected_version: u64,
) -> Result<(Value, Vec<String>)> {
    let concern = work_concern(binding).ok_or_else(|| {
        fail(
            "development_entry.unit_unreadable",
            format!(
                "WorkflowUnit {} carries no developmental concern",
                binding.workflow_unit_ref
            ),
        )
    })?;
    let mut notes = Vec::new();
    let named_capabilities = unit_list(&binding.workflow_unit, "capability_refs");
    let matrix = match matrix_carriers(binding, cwd) {
        Some((manifest, csv)) => {
            let declared = matrix_ids(&csv);
            let (carried, foreign): (Vec<String>, Vec<String>) = named_capabilities
                .iter()
                .cloned()
                .partition(|id| declared.contains(id));
            if !foreign.is_empty() {
                notes.push(format!(
                    "capabilities the unit names that no {} matrix row carries (Factory capability refs, read as named): {}",
                    binding.project,
                    foreign.join(", ")
                ));
            }
            (!carried.is_empty()).then(|| {
                json!({
                    "manifest": manifest, "csv": csv, "view_id": null,
                    "capability_refs": carried, "full_scope": false,
                    "agent_visibility": "payload", "external_egress": "allowed",
                })
            })
        }
        None => {
            if !named_capabilities.is_empty() {
                notes.push(format!(
                    "{} carries no ql-capability-matrix/1 project carriers; the unit's capabilities ({}) are named, not read",
                    binding.project,
                    named_capabilities.join(", ")
                ));
            }
            None
        }
    };
    let request = json!({
        "schema": PREPARE_SCHEMA,
        "redis": redis,
        "project_ref": format!("project/{}", binding.project),
        "now_ref": binding.child_now_ref,
        "participant_ref": binding.participant_ref(),
        "agent_session": agent_session,
        "concern": concern,
        "disclosure_revision": format!("{ENTRY_SCHEMA}:{}", binding.occupant_generation),
        "practice_refs": unit_list(&binding.workflow_unit, "praxis_refs"),
        "central": {
            "root": binding.central_root,
            "project": binding.project,
            "source_refs": [],
        },
        "factory": {
            "state": binding.factory_state,
            "run_ref": binding.run_ref,
            "workflow_unit_refs": [binding.workflow_unit_ref],
        },
        "matrix": matrix,
        "wiki_queries": [],
        "candidate_items": [],
        "continuation": json!({
            "schema": ENTRY_SCHEMA,
            "work_digest": binding.work_digest,
            "workflow_unit_ref": binding.workflow_unit_ref,
        }).to_string(),
        "expected_version": expected_version,
        "external_provider": false,
        "allow_redis_env_import": false,
        "selection": {"mode": "all"},
    });
    Ok((request, notes))
}

fn bounded(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// What the body reads: its work, the unit's own obligations, then the exact
/// prepared material. Obligations and degradations come before the lists —
/// the entry is bounded and those lines must never be what is cut.
pub fn render(
    binding: &WorkBinding,
    view: &PreparedNowContext,
    notes: &[String],
    practice_files: &BTreeMap<String, PathBuf>,
) -> String {
    let unit = &binding.workflow_unit;
    let mut lines = vec![format!(
        "[Development entry] {ENTRY_SCHEMA} — the work this Position carries, prepared from its Run. References, not bodies; nothing here is permission or proof."
    )];
    lines.push(format!(
        "work: {} · Run {} · WorkflowUnit {} · Position {} · child NOW {}",
        binding.work_ref.as_deref().unwrap_or("-"),
        binding.run_ref,
        binding.workflow_unit_ref,
        binding.position_ref,
        binding.child_now_ref
    ));
    if let Some(concern) = unit_field(unit, "developmental_concern") {
        lines.push(format!("concern (authored): {}", bounded(&concern, 400)));
    }
    if let Some(difference) = unit_field(unit, "required_difference") {
        lines.push(format!(
            "required difference: {}",
            bounded(&difference, 300)
        ));
    }
    let verification = unit_list(unit, "required_verification");
    if !verification.is_empty() {
        lines.push(format!("verification owed: {}", verification.join("; ")));
    }
    if let (Some(contract), Some(address)) = (
        unit_field(unit, "required_return_contract"),
        unit_field(unit, "required_return_address"),
    ) {
        lines.push(format!("Return: {contract} → {address}"));
    }
    let effects = unit_list(unit, "permitted_effects");
    if !effects.is_empty() {
        lines.push(format!("permitted effects: {}", effects.join(", ")));
    }
    if let Some(stop) = unit_field(unit, "stop_conditions") {
        lines.push(format!("stop when: {}", bounded(&stop, 200)));
    }
    if let Some(escalate) = unit_field(unit, "escalation_conditions") {
        lines.push(format!("escalate when: {}", bounded(&escalate, 200)));
    }
    for note in notes {
        lines.push(format!("note: {note}"));
    }
    let practice = unit_list(unit, "praxis_refs");
    if practice.is_empty() {
        lines.push("practice: the unit names none".into());
    } else {
        lines.push("practice the unit calls for (read before acting):".into());
        for reference in &practice {
            lines.push(match practice_files.get(reference) {
                Some(file) => format!("  - {reference} (read: {})", file.display()),
                None => format!("  - {reference} (not in this context's catalogue)"),
            });
        }
    }
    let (capabilities, sources): (Vec<_>, Vec<_>) = view.items.iter().partition(|item| {
        item.source_ref.as_str().starts_with("matrix:") || item.title.starts_with("cap.")
    });
    if !capabilities.is_empty() {
        lines.push("capabilities (the unit's own, from the Project matrix):".into());
        for item in capabilities {
            lines.push(format!(
                "  - {} — {}",
                item.title,
                bounded(&item.excerpt.replace('\n', " "), 220)
            ));
        }
    }
    if !sources.is_empty() {
        lines.push("sources (exact, from the work's NOW; revision at preparation):".into());
        for item in sources {
            lines.push(format!(
                "  - {} @ {} — {}",
                item.source_ref,
                bounded(&item.source_revision, 24),
                bounded(&item.title, 120)
            ));
        }
    }
    lines.push(format!(
        "prepared NOW view v{} for {} — `aikit now-context inspect --participant-ref {}` reads it",
        view.version, view.participant_ref, view.participant_ref
    ));
    bounded(&lines.join("\n"), MAX_RENDERED_CHARS)
}

/// Resolve each praxis ref the unit names to the SKILL.md the body opens.
pub fn practice_files(
    unit: &Value,
    capsule_roots: &BTreeMap<CapsuleId, PathBuf>,
) -> BTreeMap<String, PathBuf> {
    unit_list(unit, "praxis_refs")
        .into_iter()
        .filter_map(|reference| {
            let id = CapsuleId::parse(&reference).ok()?;
            let file = capsule_roots.get(&id)?.join("payload/SKILL.md");
            file.is_file().then_some((reference, file))
        })
        .collect()
}

/// The entry for the work a Refocus is delivering for. A warm view prepared
/// for the same work and session is re-rendered without re-preparing; any
/// other case prepares through the existing NOW preparation, which reads the
/// owners afresh and publishes by compare-and-swap.
pub fn deliver(
    binding: &WorkBinding,
    cwd: &Path,
    client: &str,
    session: &str,
    config: &EntryConfig,
    capsule_roots: &BTreeMap<CapsuleId, PathBuf>,
) -> Result<String> {
    let path = config.redis_config.as_ref().ok_or_else(|| {
        fail(
            "development_entry.redis_unconfigured",
            "the development entry is a prepared NOW view and needs redis_config",
        )
    })?;
    let bytes = std::fs::read(path).map_err(|e| {
        fail(
            "development_entry.redis_config",
            format!("{}: {e}", path.display()),
        )
    })?;
    let redis: RedisNowConfig = serde_json::from_slice(&bytes)
        .map_err(|e| fail("development_entry.redis_config", e.to_string()))?;
    if redis.credential_ref.is_some() {
        return Err(fail(
            "development_entry.redis_config",
            "credentialed Redis NOW is not read from a hook process",
        ));
    }
    let store = RedisNowStore::new(redis.clone())?;
    let participant = ResourceRef::parse(binding.participant_ref())?;
    let agent_session = format!("agent-session/{client}:{session}");
    let files = practice_files(&binding.workflow_unit, capsule_roots);

    if let Ok(Some(view)) = store.read_prepared(&participant, false, None) {
        let same_work = view
            .continuation
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|c| {
                c["work_digest"].as_str() == binding.work_digest.as_deref()
                    && c["workflow_unit_ref"].as_str() == Some(binding.workflow_unit_ref.as_str())
            });
        if same_work && view.agent_session.as_str() == agent_session {
            let mut text = render(binding, &view, &[], &files);
            text.push_str("\n(re-delivered: the work and its prepared view are unchanged)");
            return Ok(text);
        }
    }
    let expected = store.current_version(&participant, None)?;
    let (request, notes) = preparation_request(binding, cwd, &agent_session, &redis, expected)?;
    crate::jev_now::prepare_value(cwd, request)?;
    let view = store
        .read_prepared(&participant, false, None)?
        .ok_or_else(|| {
            fail(
                "development_entry.unpublished",
                "the prepared view was not readable after publication",
            )
        })?;
    Ok(render(binding, &view, &notes, &files))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(unit: Value) -> WorkBinding {
        WorkBinding {
            central_root: "/c".into(),
            project: "O-I".into(),
            project_root: "/c/Work/O-I".into(),
            factory_state: "/c/Work/O-I/.factory/development-state.json".into(),
            position_ref: "central:position:project:O-I:factory-guardian".into(),
            occupant_generation: "gen-7".into(),
            work_ref: Some("work:01".into()),
            run_ref: "run:01".into(),
            workflow_unit_ref: "workflow-unit:01".into(),
            child_now_ref: "central:now:project:O-I:abc".into(),
            work_digest: Some("digest-1".into()),
            workflow_unit: unit,
        }
    }

    fn unit() -> Value {
        json!({
            "workflow_unit_ref": "workflow-unit:01",
            "developmental_concern": "Make the desktop agency mint refuse cross-Project reuse",
            "required_difference": "a failed mint never runs as another Project's Agent",
            "required_verification": ["kernel agency tests", "a fresh desktop chat in a second Project"],
            "required_return_contract": "factory.return/v1",
            "required_return_address": "central:now:project:O-I:abc",
            "praxis_refs": ["skill/aikit/verification"],
            "capability_refs": ["cap.oi.desktop-agency", "cap.factory.run-map"],
            "permitted_effects": ["edit desktop/cradle/kernel"],
            "stop_conditions": "the mint owner refuses for a reason outside this work",
            "escalation_conditions": "an owner contract would have to change",
        })
    }

    #[test]
    fn the_concern_is_the_units_authored_concern_never_a_chat_turn() {
        assert_eq!(
            work_concern(&binding(unit())).unwrap(),
            "Make the desktop agency mint refuse cross-Project reuse — required difference: a failed mint never runs as another Project's Agent"
        );
        assert!(
            work_concern(&binding(json!({}))).is_none(),
            "a unit without a concern has no entry"
        );
    }

    #[test]
    fn the_request_is_built_from_the_run_and_its_child_now() {
        let redis: RedisNowConfig = serde_json::from_value(json!({
            "schema":"aikit.redis-now-config/v1","address":"127.0.0.1:6381","key_prefix":"aikit-now"
        }))
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let (request, notes) = preparation_request(
            &binding(unit()),
            temp.path(),
            "agent-session/claude:s1",
            &redis,
            4,
        )
        .unwrap();
        assert_eq!(request["now_ref"], "central:now:project:O-I:abc");
        assert_eq!(request["factory"]["run_ref"], "run:01");
        assert_eq!(
            request["factory"]["workflow_unit_refs"],
            json!(["workflow-unit:01"])
        );
        assert_eq!(
            request["participant_ref"],
            "participant/central:position:project:O-I:factory-guardian"
        );
        assert_eq!(request["expected_version"], 4);
        assert_eq!(request["selection"]["mode"], "all");
        assert_eq!(
            request["practice_refs"],
            json!(["skill/aikit/verification"])
        );
        assert!(
            request["matrix"].is_null(),
            "no carriers in this fixture Project"
        );
        assert!(notes[0].contains("cap.oi.desktop-agency"), "{notes:?}");
    }

    #[test]
    fn obligations_lead_and_practice_is_the_units_own() {
        let view: PreparedNowContext = serde_json::from_value(json!({
            "schema":"aikit.prepared-now-context/v1","project_ref":"project/O-I","now_ref":"central:now:project:O-I:abc",
            "participant_ref":"participant/p","agent_session":"agent-session/claude:s1","version":2,
            "basis":{"disclosure_revision":"d"},"basis_digest":"x","concern":"c","prepared_at_unix_ms":0,
            "items":[{"source_ref":"central:source:project:O-I:docs/A.md","source_revision":"r1","title":"docs/A.md","excerpt":"a",
                      "agent_visibility":"payload","external_egress":"allowed"}]
        }))
        .unwrap();
        let text = render(
            &binding(unit()),
            &view,
            &["a note".into()],
            &BTreeMap::new(),
        );
        let difference = text.find("required difference").unwrap();
        let sources = text.find("sources (exact").unwrap();
        assert!(difference < sources, "obligations precede the lists");
        assert!(text.contains("verification owed: kernel agency tests"));
        assert!(text.contains("Return: factory.return/v1 → central:now:project:O-I:abc"));
        assert!(text.contains("skill/aikit/verification (not in this context's catalogue)"));
        assert!(text.contains("central:source:project:O-I:docs/A.md @ r1"));
        assert!(text.contains("prepared NOW view v2"));
    }

    /// The exact `factory.workflow-unit-reading/v1` shape Factory answers on
    /// this machine (O-I, `expression-development-two-voice`), trimmed.
    fn real_unit() -> Value {
        json!({
            "contract": "factory.workflow-unit-reading/v1",
            "workflowUnitRef": "workflow-unit:35EEXBYNZ9M8Q7G9575445D7Z2",
            "developmentalConcern": "Review what Anima performed and sent, compare it with the intent, and return it",
            "requiredDifference": "An attributed review of Anima's act and message, returned to Factory",
            "requiredReturn": {"contract": "Return the review: intent against performance and the evidence refs",
                               "address": "central:source:project:O-I:ProjectCentral/now"},
            "requiredVerification": ["the review names the message it answers and the file it checked"],
            "agentRequirements": {"agentRefs": ["agent/aletheia"], "agentSetRefs": ["agent-set/aletheia"], "agencyRefs": []},
            "praxisRefs": ["skill/ql/aletheia-expressive-return", "skill/ql/aletheia-stack-traverse"],
            "capabilityRefs": ["capability/expression-observe", "capability/source-read"],
            "permittedEffects": ["answer Anima through one AIKit encounter send"],
            "stopConditions": "Stop when there is no performed evidence to disclose",
            "escalationConditions": "Escalate Recognition decisions to the owner; never promote on its own"
        })
    }

    #[test]
    fn the_real_factory_unit_reading_is_read_as_factory_spells_it() {
        let unit = real_unit();
        let b = binding(unit.clone());
        assert_eq!(
            work_concern(&b).unwrap(),
            "Review what Anima performed and sent, compare it with the intent, and return it — required difference: An attributed review of Anima's act and message, returned to Factory"
        );
        assert_eq!(
            unit_list(&unit, "praxis_refs"),
            vec![
                "skill/ql/aletheia-expressive-return",
                "skill/ql/aletheia-stack-traverse"
            ]
        );
        assert_eq!(
            unit_field(&unit, "required_return_address").as_deref(),
            Some("central:source:project:O-I:ProjectCentral/now")
        );
        assert_eq!(unit_list(&unit, "agent_refs"), vec!["agent/aletheia"]);
        let view: PreparedNowContext = serde_json::from_value(json!({
            "schema":"aikit.prepared-now-context/v1","project_ref":"project/O-I","now_ref":"central:now:project:O-I:abc",
            "participant_ref":"participant/p","agent_session":"agent-session/pi:s","version":1,
            "basis":{"disclosure_revision":"d"},"basis_digest":"x","concern":"c","prepared_at_unix_ms":0
        })).unwrap();
        let text = render(&b, &view, &[], &BTreeMap::new());
        assert!(
            text.contains("verification owed: the review names the message it answers"),
            "{text}"
        );
        assert!(text.contains("Return: Return the review: intent against performance and the evidence refs → central:source:project:O-I:ProjectCentral/now"));
        assert!(text.contains("stop when: Stop when there is no performed evidence to disclose"));
        assert!(text.contains("skill/ql/aletheia-expressive-return"));
    }
}
