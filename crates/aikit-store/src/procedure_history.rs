//! Read-only Procedure History projection.
//!
//! A persisted `procedure.json` proves that a plan was recorded; it does **not**
//! by itself prove that the mutation reached the world. The existing Procedure
//! runner records current satisfaction separately under `.satisfied/<digest>`.
//! This reader preserves that distinction and never reconstructs an apply event
//! from directory presence or an undo journal.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use aikit_core::procedure::{Procedure, ProcedureKind};
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{
    AikitError, HistoryEvidence, HistoryKind, HistoryRecoverability, Result,
    EXPLAIN_HISTORY_VERSION,
};

use crate::derived_cache::{self, FileSignature};
use crate::procedure::{GIT_FILE, JOURNAL_FILE, PLAN_FILE, PROCEDURE_FILE};
use crate::{AikitHome, ProcedureRunner};

/// Where what a procedure's record says is kept: derived data under the home's cache, never an authority.
const PROCEDURE_FACTS_FILE: &str = "history/procedure-facts.v1.json";
const PROCEDURE_FACTS_VERSION: u32 = 1;

/// What the history reading takes from a Procedure's record. A record carries its whole plan, an
/// installed home keeps a hundred and seventy of them (88 MB), a retained Procedure is immutable,
/// and `aikit history` read every one three times over — parsed, parsed again and checked against
/// its plan — so the reading is derived from a record once and kept beside the signatures of the
/// two files it was validated from. A record that was rewritten derives again, and is validated
/// again: what is kept was validated on the files it names. Whether the plan was applied, undone or
/// journalled is read live, from small files, every time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ProcedureFacts {
    record: FileSignature,
    plan: FileSignature,
    id: String,
    kind: String,
    digest: String,
    isolation: String,
    edit_count: usize,
    touched_paths: Vec<String>,
    /// The Procedure's own reference and every capsule it names, as `ResourceRef` strings.
    refs: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct ProcedureFactsFile {
    version: u32,
    procedures: BTreeMap<String, ProcedureFacts>,
}

impl ProcedureFacts {
    fn of(procedure: &Procedure, record: FileSignature, plan: FileSignature) -> Result<Self> {
        let subject = ResourceRef::parse(&format!("procedure/{}", procedure.id))?;
        let mut refs = BTreeSet::from([subject]);
        collect_kind_refs(&procedure.kind, &mut refs);
        Ok(Self {
            record,
            plan,
            id: procedure.id.to_string(),
            kind: procedure.kind.as_str().into(),
            digest: procedure.digest.to_string(),
            isolation: procedure.isolation.as_str().into(),
            edit_count: procedure.plan.edits.len(),
            touched_paths: procedure
                .plan
                .touched_paths()
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            refs: refs.iter().map(ToString::to_string).collect(),
        })
    }
}

pub fn procedure_history_evidence(home: &AikitHome) -> Result<Vec<HistoryEvidence>> {
    let root = home.state().join("procedures");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let runner = ProcedureRunner::new(home);
    let facts_path = derived_cache::path_in(home, PROCEDURE_FACTS_FILE);
    let mut kept = derived_cache::load::<ProcedureFactsFile>(&facts_path)
        .filter(|kept| kept.version == PROCEDURE_FACTS_VERSION)
        .unwrap_or_else(|| ProcedureFactsFile {
            version: PROCEDURE_FACTS_VERSION,
            ..ProcedureFactsFile::default()
        });
    let mut seen = BTreeSet::new();
    let mut changed = false;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| {
        AikitError::new(
            "history.procedure_list_failed",
            format!("could not list {}: {error}", root.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "history.procedure_list_failed",
                format!("could not read {}: {error}", root.display()),
            )
        })?;
        let dir = entry.path();
        if !dir.is_dir() || entry.file_name() == ".satisfied" {
            continue;
        }
        let path = dir.join(PROCEDURE_FILE);
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let record = FileSignature::of(&path);
        let plan = FileSignature::of(&dir.join(PLAN_FILE));
        seen.insert(name.clone());
        if let Some(facts) = kept
            .procedures
            .get(&name)
            .filter(|facts| facts.id == name && facts.record == record && facts.plan == plan)
        {
            entries.push(procedure_evidence(home, &dir, facts)?);
            continue;
        }

        let bytes = fs::read(&path).map_err(|error| {
            AikitError::new(
                "history.procedure_read_failed",
                format!("could not read {}: {error}", path.display()),
            )
        })?;
        let parsed: Procedure = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "history.procedure_invalid",
                format!(
                    "{} is not valid Procedure metadata: {error}",
                    path.display()
                ),
            )
        })?;
        if name != parsed.id.as_str() {
            return Err(AikitError::new(
                "history.procedure_invalid",
                format!(
                    "{} contains Procedure {} under the wrong identity directory",
                    path.display(),
                    parsed.id
                ),
            ));
        }
        // Reuse the Procedure authority's own structural validation rather than
        // creating a second validator in History.
        let procedure = runner.load(&parsed.id)?;
        let facts = ProcedureFacts::of(&procedure, record, plan)?;
        entries.push(procedure_evidence(home, &dir, &facts)?);
        kept.procedures.insert(name, facts);
        changed = true;
    }
    let before = kept.procedures.len();
    kept.procedures.retain(|name, _| seen.contains(name));
    if changed || kept.procedures.len() != before {
        derived_cache::save(&facts_path, &kept);
    }

    entries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(entries)
}

fn procedure_evidence(
    home: &AikitHome,
    dir: &Path,
    facts: &ProcedureFacts,
) -> Result<HistoryEvidence> {
    let subject = ResourceRef::parse(&format!("procedure/{}", facts.id))?;
    let canonical_refs = facts
        .refs
        .iter()
        .map(String::as_str)
        .map(ResourceRef::parse)
        .collect::<Result<BTreeSet<_>>>()?;

    let satisfied = home
        .state()
        .join("procedures")
        .join(".satisfied")
        .join(&facts.digest)
        .is_file();
    let journal_present = dir.join(JOURNAL_FILE).is_file();
    let git_record_present = dir.join(GIT_FILE).is_file();

    let mut details = BTreeMap::new();
    details.insert("kind".into(), facts.kind.clone());
    details.insert("planDigest".into(), facts.digest.clone());
    details.insert("isolation".into(), facts.isolation.clone());
    details.insert("editCount".into(), facts.edit_count.to_string());
    details.insert("currentSatisfactionRecorded".into(), satisfied.to_string());
    details.insert("undoJournalPresent".into(), journal_present.to_string());
    details.insert(
        "gitCommitRecordPresent".into(),
        git_record_present.to_string(),
    );
    if !satisfied {
        details.insert(
            "satisfactionAbsence".into(),
            "recorded plan may be unapplied, failed, or subsequently undone; History does not infer which"
                .into(),
        );
    }
    if !facts.touched_paths.is_empty() {
        details.insert("touchedPaths".into(), facts.touched_paths.join(", "));
    }

    let state = if satisfied {
        "current satisfaction is recorded"
    } else {
        "no current satisfaction marker"
    };
    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: facts.id.clone(),
        kind: HistoryKind::Procedure,
        subject,
        authorities: vec![SourceAuthority::Generated],
        occurred_at_unix_ms: None,
        summary: format!("Procedure {} · {} · {state}", facts.id, facts.kind),
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance: Vec::new(),
        // Procedure undo remains owned by ProcedureRunner. History currently
        // advertises the evidence but deliberately exposes no generic restore
        // action that could bypass its drift/preflight checks.
        recoverability: HistoryRecoverability::InspectOnly,
        details,
    })
}

fn collect_kind_refs(kind: &ProcedureKind, refs: &mut BTreeSet<ResourceRef>) {
    match kind {
        ProcedureKind::Adopt { capsules, .. } => {
            refs.extend(
                capsules
                    .iter()
                    .filter_map(|capsule| ResourceRef::parse(&capsule.to_string()).ok()),
            );
        }
        ProcedureKind::Supersede { winner, losers } => {
            if let Ok(reference) = ResourceRef::parse(&winner.to_string()) {
                refs.insert(reference);
            }
            refs.extend(
                losers
                    .iter()
                    .filter_map(|capsule| ResourceRef::parse(&capsule.to_string()).ok()),
            );
        }
        ProcedureKind::DependencyInstall { tool } | ProcedureKind::Custom { capsule: tool } => {
            if let Ok(reference) = ResourceRef::parse(&tool.to_string()) {
                refs.insert(reference);
            }
        }
        ProcedureKind::ProfileFork { base, fork } => {
            if let Ok(reference) = ResourceRef::parse(&base.to_string()) {
                refs.insert(reference);
            }
            if let Ok(reference) = ResourceRef::parse(&fork.to_string()) {
                refs.insert(reference);
            }
        }
        ProcedureKind::SkillSet { set, .. } => {
            if let Ok(reference) = ResourceRef::parse(set) {
                refs.insert(reference);
            }
        }
        ProcedureKind::Import { .. }
        | ProcedureKind::Collate { .. }
        | ProcedureKind::Promote { .. }
        | ProcedureKind::ClientInstall { .. }
        | ProcedureKind::ToolsProjection { .. }
        | ProcedureKind::MuxInstall { .. }
        | ProcedureKind::DoctorFix { .. }
        | ProcedureKind::IntegrationSetup { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::procedure::{MutationIsolation, Plan, ProcedureKind};

    fn saved(runner: &ProcedureRunner, kind: ProcedureKind, edit_note: &str) -> Procedure {
        let mut plan = Plan::new();
        plan.notes.push(edit_note.into());
        let procedure = Procedure::new(kind, plan, MutationIsolation::Direct).unwrap();
        runner.save(&procedure).unwrap();
        procedure
    }

    fn kinds() -> Vec<ProcedureKind> {
        vec![
            ProcedureKind::DoctorFix {
                checks: vec!["history-test".into()],
            },
            ProcedureKind::DependencyInstall {
                tool: aikit_core::CapsuleId::parse("script/test/nt").unwrap(),
            },
            ProcedureKind::Supersede {
                winner: aikit_core::CapsuleId::parse("skill/rust/review").unwrap(),
                losers: vec![aikit_core::CapsuleId::parse("skill/rust/old").unwrap()],
            },
        ]
    }

    fn facts_file(home: &AikitHome) -> std::path::PathBuf {
        derived_cache::path_in(home, PROCEDURE_FACTS_FILE)
    }

    #[test]
    fn a_procedure_record_is_validated_once_and_the_reading_is_the_same_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let runner = ProcedureRunner::new(&home);
        let saved: Vec<Procedure> = kinds()
            .into_iter()
            .enumerate()
            .map(|(index, kind)| saved(&runner, kind, &format!("note {index}")))
            .collect();

        let first = procedure_history_evidence(&home).unwrap();
        assert_eq!(first.len(), 3);
        assert!(facts_file(&home).is_file(), "what a record said is kept");
        assert_eq!(procedure_history_evidence(&home).unwrap(), first);
        fs::remove_file(facts_file(&home)).unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap(), first);

        // What is read live is read live: a satisfaction marker written after the record was kept shows.
        let marked = &saved[0];
        let satisfied = home.state().join("procedures/.satisfied");
        fs::create_dir_all(&satisfied).unwrap();
        fs::write(satisfied.join(marked.digest.as_str()), "").unwrap();
        let after = procedure_history_evidence(&home).unwrap();
        let row = after
            .iter()
            .find(|row| row.id == marked.id.to_string())
            .unwrap();
        assert_eq!(
            row.details
                .get("currentSatisfactionRecorded")
                .map(String::as_str),
            Some("true")
        );
        let before_row = first
            .iter()
            .find(|row| row.id == marked.id.to_string())
            .unwrap();
        assert_eq!(
            before_row
                .details
                .get("currentSatisfactionRecorded")
                .map(String::as_str),
            Some("false"),
            "the same record read before the marker said so"
        );
    }

    #[test]
    fn a_kept_record_is_not_read_again_and_a_changed_one_is_validated_again() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let runner = ProcedureRunner::new(&home);
        let procedure = saved(&runner, kinds().remove(0), "kept");
        let first = procedure_history_evidence(&home).unwrap();
        let record = home
            .state()
            .join("procedures")
            .join(procedure.id.as_str())
            .join(PROCEDURE_FILE);

        // A same-length, same-time stand-in that is not JSON at all is not noticed: the reading came from what the real record said.
        let original = fs::read(&record).unwrap();
        let modified = fs::metadata(&record).unwrap().modified().unwrap();
        fs::write(&record, vec![b'x'; original.len()]).unwrap();
        fs::File::options()
            .write(true)
            .open(&record)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap(), first);

        // A record that is no longer the file the facts came from is validated again - here, refused.
        fs::write(&record, vec![b'x'; original.len() + 1]).unwrap();
        let refused = procedure_history_evidence(&home).unwrap_err();
        assert_eq!(refused.code(), "history.procedure_invalid", "{refused}");
    }

    #[test]
    fn an_unusable_facts_cache_costs_the_reading_nothing_and_a_removed_record_is_let_go() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let runner = ProcedureRunner::new(&home);
        let one = saved(&runner, kinds().remove(0), "one");
        saved(&runner, kinds().remove(1), "two");
        let expected = procedure_history_evidence(&home).unwrap();
        let cache = facts_file(&home);

        fs::write(&cache, "{ not json").unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap(), expected);
        fs::write(&cache, r#"{"version":99,"procedures":{}}"#).unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap(), expected);

        // Nowhere to keep it: a file where its directory should be.
        fs::remove_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(cache.parent().unwrap(), "in the way").unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap(), expected);
        fs::remove_file(cache.parent().unwrap()).unwrap();

        procedure_history_evidence(&home).unwrap();
        let kept = |home: &AikitHome| -> usize {
            let text = fs::read_to_string(facts_file(home)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["procedures"].as_object().unwrap().len()
        };
        assert_eq!(kept(&home), 2);
        fs::remove_dir_all(home.state().join("procedures").join(one.id.as_str())).unwrap();
        assert_eq!(procedure_history_evidence(&home).unwrap().len(), 1);
        assert_eq!(kept(&home), 1);
    }

    #[test]
    fn a_saved_plan_without_satisfaction_is_not_reported_as_applied() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let runner = ProcedureRunner::new(&home);
        let procedure = Procedure::new(
            ProcedureKind::DoctorFix {
                checks: vec!["history-test".into()],
            },
            Plan::new(),
            MutationIsolation::Direct,
        )
        .unwrap();
        runner.save(&procedure).unwrap();

        let history = procedure_history_evidence(&home).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].kind, HistoryKind::Procedure);
        assert_eq!(history[0].authorities, vec![SourceAuthority::Generated]);
        assert_eq!(
            history[0].details.get("currentSatisfactionRecorded"),
            Some(&"false".to_string())
        );
        assert!(history[0]
            .details
            .get("satisfactionAbsence")
            .is_some_and(|text| text.contains("unapplied, failed, or subsequently undone")));
    }
}
