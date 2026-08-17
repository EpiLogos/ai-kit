//! Store-owned apply seam for staged SkillSet membership relations.
//!
//! Core owns the semantic mutation. This module owns only the durable external
//! mutation required to make that intent real. It reuses the existing SkillSet
//! Procedure planners and runner so CLI, TUI and agent callers can share one
//! preview/apply authority instead of writing `members` or `set.toml` themselves.

use serde::{Deserialize, Serialize};

use aikit_core::composition_mutation::SkillSetRelationMutation;
use aikit_core::id::CapsuleId;
use aikit_core::skillset::SkillSet;
use aikit_core::{AikitError, ProcedureId, Result};

use crate::home::AikitHome;
use crate::procedure::{ProcedureDiff, ProcedureRunner};
use crate::skillsets;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillSetRelationProcedurePreview {
    pub mutation: SkillSetRelationMutation,
    pub procedure: ProcedureId,
    pub diff: ProcedureDiff,
    /// Membership identity observed when preview was produced. This closes the
    /// review/apply interval independently of the effective-resolution basis.
    pub membership_basis: Vec<CapsuleId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSetRelationProcedureReceipt {
    pub mutation: SkillSetRelationMutation,
    pub procedure: ProcedureId,
    pub applied_edits: usize,
    pub already_satisfied: bool,
    pub resulting_members: Vec<CapsuleId>,
    pub undo: String,
}

/// Plan and diff one SkillSet membership mutation without writing anything.
pub fn preview_skillset_relation_mutation(
    home: &AikitHome,
    mutation: SkillSetRelationMutation,
) -> Result<SkillSetRelationProcedurePreview> {
    let (set_name, capability, add) = mutation_parts(&mutation);
    let current = skillsets::load(home, set_name)?;
    if !current.provenance.is_writable() {
        return Err(AikitError::new(
            "composition.skillset_read_only",
            format!("SkillSet `{set_name}` is observed and cannot be mutated in place"),
        ));
    }
    let procedure = if add {
        skillsets::plan_add(home, set_name, std::slice::from_ref(capability))?
    } else {
        skillsets::plan_remove(home, set_name, std::slice::from_ref(capability))?
    };
    let runner = ProcedureRunner::new(home);
    let diff = runner.diff(&procedure)?;
    Ok(SkillSetRelationProcedurePreview {
        mutation,
        procedure: procedure.id,
        diff,
        membership_basis: member_ids(&current),
    })
}

/// Apply an accepted membership preview through the canonical Procedure runner.
///
/// The SkillSet is re-read first; if membership changed since preview, the
/// accepted mutation is stale and must be previewed again. Procedure execution
/// remains reversible and records its own immutable plan/digest/undo journal.
pub fn apply_skillset_relation_mutation(
    home: &AikitHome,
    preview: SkillSetRelationProcedurePreview,
) -> Result<SkillSetRelationProcedureReceipt> {
    let (set_name, capability, add) = mutation_parts(&preview.mutation);
    let current = skillsets::load(home, set_name)?;
    if member_ids(&current) != preview.membership_basis {
        return Err(AikitError::new(
            "composition.preview_stale",
            "SkillSet membership changed after the accepted preview was produced",
        )
        .with("skill_set", set_name.to_string()));
    }

    // Re-plan from the still-matching source state and insist on the same
    // immutable Procedure identity the caller reviewed.
    let procedure = if add {
        skillsets::plan_add(home, set_name, std::slice::from_ref(capability))?
    } else {
        skillsets::plan_remove(home, set_name, std::slice::from_ref(capability))?
    };
    if procedure.id != preview.procedure {
        return Err(AikitError::new(
            "composition.preview_stale",
            "SkillSet Procedure identity changed after preview",
        )
        .with("skill_set", set_name.to_string())
        .with("expected_procedure", preview.procedure.to_string())
        .with("current_procedure", procedure.id.to_string()));
    }

    let outcome = ProcedureRunner::new(home).run(&procedure)?;
    let resulting = skillsets::load(home, set_name)?;
    Ok(SkillSetRelationProcedureReceipt {
        mutation: preview.mutation,
        procedure: procedure.id.clone(),
        applied_edits: outcome.applied,
        already_satisfied: outcome.already_satisfied,
        resulting_members: member_ids(&resulting),
        undo: format!("aikit procedure undo {}", procedure.id),
    })
}

fn mutation_parts(mutation: &SkillSetRelationMutation) -> (&str, &CapsuleId, bool) {
    match mutation {
        SkillSetRelationMutation::Add {
            skill_set,
            capability,
        } => (skill_set, capability, true),
        SkillSetRelationMutation::Remove {
            skill_set,
            capability,
        } => (skill_set, capability, false),
    }
}

fn member_ids(set: &SkillSet) -> Vec<CapsuleId> {
    set.members.keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::skillset::SetMembership;
    use tempfile::tempdir;

    fn id(raw: &str) -> CapsuleId {
        CapsuleId::parse(raw).unwrap()
    }

    #[test]
    fn preview_is_write_free_and_apply_uses_a_reversible_procedure() {
        let dir = tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("aikit"));
        skillsets::create(&home, "operator", &[id("skill/a")], &[]).unwrap();
        let mutation = SkillSetRelationMutation::Add {
            skill_set: "operator".into(),
            capability: id("skill/b"),
        };

        let preview = preview_skillset_relation_mutation(&home, mutation).unwrap();
        let before = skillsets::load(&home, "operator").unwrap();
        assert!(!before.members.contains_key(&id("skill/b")));
        assert!(!preview.diff.is_empty());

        let receipt = apply_skillset_relation_mutation(&home, preview).unwrap();
        let after = skillsets::load(&home, "operator").unwrap();
        assert_eq!(
            after.members.get(&id("skill/b")),
            Some(&SetMembership::Explicit)
        );
        assert_eq!(receipt.applied_edits, 1);
        assert!(receipt.undo.contains(receipt.procedure.as_str()));
    }

    #[test]
    fn accepted_preview_is_rejected_after_membership_drift() {
        let dir = tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("aikit"));
        skillsets::create(&home, "operator", &[id("skill/a")], &[]).unwrap();
        let preview = preview_skillset_relation_mutation(
            &home,
            SkillSetRelationMutation::Add {
                skill_set: "operator".into(),
                capability: id("skill/b"),
            },
        )
        .unwrap();

        skillsets::add(&home, "operator", &[id("skill/c")]).unwrap();
        let error = apply_skillset_relation_mutation(&home, preview).unwrap_err();
        assert_eq!(error.code(), "composition.preview_stale");
        assert!(!skillsets::load(&home, "operator")
            .unwrap()
            .members
            .contains_key(&id("skill/b")));
    }
}