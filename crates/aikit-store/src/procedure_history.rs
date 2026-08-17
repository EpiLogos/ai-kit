//! Read-only Procedure History projection.
//!
//! A persisted `procedure.json` proves that a plan was recorded; it does **not**
//! by itself prove that the mutation reached the world. The existing Procedure
//! runner records current satisfaction separately under `.satisfied/<digest>`.
//! This reader preserves that distinction and never reconstructs an apply event
//! from directory presence or an undo journal.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use aikit_core::procedure::{Procedure, ProcedureKind};
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{
    AikitError, HistoryEvidence, HistoryKind, HistoryRecoverability, Result,
    EXPLAIN_HISTORY_VERSION,
};

use crate::procedure::{GIT_FILE, JOURNAL_FILE, PROCEDURE_FILE};
use crate::{AikitHome, ProcedureRunner};

pub fn procedure_history_evidence(home: &AikitHome) -> Result<Vec<HistoryEvidence>> {
    let root = home.state().join("procedures");
    if !root.exists() {
        return Ok(Vec::new());
    }

    let runner = ProcedureRunner::new(home);
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

        let bytes = fs::read(&path).map_err(|error| {
            AikitError::new(
                "history.procedure_read_failed",
                format!("could not read {}: {error}", path.display()),
            )
        })?;
        let parsed: Procedure = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "history.procedure_invalid",
                format!("{} is not valid Procedure metadata: {error}", path.display()),
            )
        })?;
        if entry.file_name().to_string_lossy() != parsed.id.as_str() {
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
        entries.push(procedure_evidence(home, &dir, &procedure)?);
    }

    entries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(entries)
}

fn procedure_evidence(
    home: &AikitHome,
    dir: &std::path::Path,
    procedure: &Procedure,
) -> Result<HistoryEvidence> {
    let subject = ResourceRef::parse(&format!("procedure/{}", procedure.id))?;
    let mut canonical_refs = BTreeSet::from([subject.clone()]);
    collect_kind_refs(&procedure.kind, &mut canonical_refs);

    let satisfied = home
        .state()
        .join("procedures")
        .join(".satisfied")
        .join(procedure.digest.as_str())
        .is_file();
    let journal_present = dir.join(JOURNAL_FILE).is_file();
    let git_record_present = dir.join(GIT_FILE).is_file();

    let mut details = BTreeMap::new();
    details.insert("kind".into(), procedure.kind.as_str().into());
    details.insert("planDigest".into(), procedure.digest.to_string());
    details.insert("isolation".into(), procedure.isolation.as_str().into());
    details.insert("editCount".into(), procedure.plan.edits.len().to_string());
    details.insert("currentSatisfactionRecorded".into(), satisfied.to_string());
    details.insert("undoJournalPresent".into(), journal_present.to_string());
    details.insert("gitCommitRecordPresent".into(), git_record_present.to_string());
    if !satisfied {
        details.insert(
            "satisfactionAbsence".into(),
            "recorded plan may be unapplied, failed, or subsequently undone; History does not infer which"
                .into(),
        );
    }
    let touched = procedure.plan.touched_paths();
    if !touched.is_empty() {
        details.insert(
            "touchedPaths".into(),
            touched
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    let state = if satisfied {
        "current satisfaction is recorded"
    } else {
        "no current satisfaction marker"
    };
    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: procedure.id.to_string(),
        kind: HistoryKind::Procedure,
        subject,
        authorities: vec![SourceAuthority::Generated],
        occurred_at_unix_ms: None,
        summary: format!(
            "Procedure {} · {} · {state}",
            procedure.id,
            procedure.kind.as_str()
        ),
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
        | ProcedureKind::MuxInstall { .. }
        | ProcedureKind::DoctorFix { .. }
        | ProcedureKind::IntegrationSetup { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::procedure::{MutationIsolation, Plan, ProcedureKind};

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
