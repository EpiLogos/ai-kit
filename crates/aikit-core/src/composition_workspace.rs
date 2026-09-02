//! UI-neutral Project/Profile/SkillSet composition workspace read model.
//!
//! This module does not own ProjectSpec persistence or SkillSet membership files.
//! It makes their distinct relation semantics explicit for every consumer:
//!
//! - Project -> SkillSet selection is additive, may be authored or inherited, and
//!   is effective when present in the resolved Project selection union.
//! - SkillSet -> Capability membership/projection remains represented by
//!   `ProfileCompositionReadModel` and never acquires activation/trust authority.
//! - neither relation has semantic precedence/reordering.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition_mutation::ProfileCompositionReadModel;
use crate::project_world::ProjectWorldReadModel;

pub const COMPOSITION_WORKSPACE_VERSION: &str = "aikit.composition-workspace/v2";

/// Provenance of one Project -> SkillSet selection in the resolved Project world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkillSetRelationReadModel {
    pub skill_set: String,
    pub authored: bool,
    pub inherited: bool,
    pub effective: bool,
    pub available: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Typed Project -> SkillSet selection intent. There is deliberately no Reorder:
/// Project selections compose by stable set-union and SkillSet projection itself
/// is union-only. Presentation order is not application authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectSkillSetSelectionIntent {
    Add,
    Remove,
}

/// Write-free staged authored Project selection changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedProjectSkillSetSelections {
    changes: BTreeMap<String, ProjectSkillSetSelectionIntent>,
}

impl StagedProjectSkillSetSelections {
    pub fn stage(&mut self, skill_set: impl Into<String>, intent: ProjectSkillSetSelectionIntent) {
        self.changes.insert(skill_set.into(), intent);
    }

    pub fn unstage(&mut self, skill_set: &str) -> Option<ProjectSkillSetSelectionIntent> {
        self.changes.remove(skill_set)
    }

    pub fn get(&self, skill_set: &str) -> Option<ProjectSkillSetSelectionIntent> {
        self.changes.get(skill_set).copied()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Apply staged intent to an authored copy only. Inherited/default relations
    /// are never rewritten as a side-effect of staging a Project-local selection.
    pub fn authored_after(&self, authored_before: &[String]) -> Vec<String> {
        let mut after: BTreeSet<String> = authored_before.iter().cloned().collect();
        for (skill_set, intent) in &self.changes {
            match intent {
                ProjectSkillSetSelectionIntent::Add => {
                    after.insert(skill_set.clone());
                }
                ProjectSkillSetSelectionIntent::Remove => {
                    after.remove(skill_set);
                }
            }
        }
        after.into_iter().collect()
    }
}

/// Resolve Project -> SkillSet relation states without inventing precedence.
///
/// `effective` must be the canonical Project resolution result (including parent
/// matches/default inheritance) supplied by the Project owner. `available` is the
/// set of SkillSet identities the current provider/store can actually inspect.
pub fn project_skill_set_relations(
    authored: &[String],
    inherited: &[String],
    effective: &[String],
    available: &[String],
) -> Vec<ProjectSkillSetRelationReadModel> {
    let authored: BTreeSet<_> = authored.iter().cloned().collect();
    let inherited: BTreeSet<_> = inherited.iter().cloned().collect();
    let effective: BTreeSet<_> = effective.iter().cloned().collect();
    let available: BTreeSet<_> = available.iter().cloned().collect();
    let identities: BTreeSet<_> = authored
        .iter()
        .chain(inherited.iter())
        .chain(effective.iter())
        .cloned()
        .collect();

    identities
        .into_iter()
        .map(|skill_set| {
            let is_authored = authored.contains(&skill_set);
            let is_inherited = inherited.contains(&skill_set);
            let is_effective = effective.contains(&skill_set);
            let is_available = available.contains(&skill_set);
            let mut provenance = Vec::new();
            if is_authored {
                provenance.push("project-authored".to_string());
            }
            if is_inherited {
                provenance.push("inherited/default".to_string());
            }
            if is_effective {
                provenance.push("project-resolution".to_string());
            }
            if !is_available {
                provenance.push("provider-unavailable".to_string());
            }
            ProjectSkillSetRelationReadModel {
                skill_set,
                authored: is_authored,
                inherited: is_inherited,
                effective: is_effective,
                available: is_available,
                provenance,
            }
        })
        .collect()
}

/// One application read model for the composition workspace. It deliberately
/// embeds the existing Project-world read model instead of copying Project,
/// ContextSource, actor/runtime, model/harness, projection or Generation truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectCompositionWorkspaceReadModel {
    pub version: String,
    pub project_world: ProjectWorldReadModel,
    pub profile: ProfileCompositionReadModel,
    pub project_skill_sets: Vec<ProjectSkillSetRelationReadModel>,
    #[serde(default)]
    pub pending_project_skill_sets: StagedProjectSkillSetSelections,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl ProjectCompositionWorkspaceReadModel {
    pub fn new(
        project_world: ProjectWorldReadModel,
        profile: ProfileCompositionReadModel,
        project_skill_sets: Vec<ProjectSkillSetRelationReadModel>,
    ) -> Self {
        let mut warnings = project_world.warnings.clone();
        warnings.extend(profile.warnings.clone());
        for relation in &project_skill_sets {
            if relation.effective && !relation.available {
                warnings.push(format!(
                    "SkillSet `{}` remains selected but is currently unavailable",
                    relation.skill_set
                ));
            }
        }
        warnings.sort();
        warnings.dedup();
        Self {
            version: COMPOSITION_WORKSPACE_VERSION.to_string(),
            project_world,
            profile,
            project_skill_sets,
            pending_project_skill_sets: StagedProjectSkillSetSelections::default(),
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_skillset_relations_distinguish_authored_inherited_effective_and_unavailable() {
        let relations = project_skill_set_relations(
            &["operator".into()],
            &["developer".into()],
            &["operator".into(), "developer".into(), "missing".into()],
            &["operator".into(), "developer".into()],
        );

        let operator = relations
            .iter()
            .find(|r| r.skill_set == "operator")
            .unwrap();
        assert!(operator.authored);
        assert!(!operator.inherited);
        assert!(operator.effective);
        assert!(operator.available);

        let developer = relations
            .iter()
            .find(|r| r.skill_set == "developer")
            .unwrap();
        assert!(!developer.authored);
        assert!(developer.inherited);
        assert!(developer.effective);

        let missing = relations.iter().find(|r| r.skill_set == "missing").unwrap();
        assert!(missing.effective);
        assert!(!missing.available);
        assert!(missing
            .provenance
            .iter()
            .any(|p| p == "provider-unavailable"));
    }

    #[test]
    fn staging_project_skillset_selection_never_mutates_inherited_relations() {
        let authored = vec!["operator".to_string()];
        let inherited = vec!["developer".to_string()];
        let snapshot = authored.clone();
        let mut staged = StagedProjectSkillSetSelections::default();
        staged.stage("operator", ProjectSkillSetSelectionIntent::Remove);
        staged.stage("research", ProjectSkillSetSelectionIntent::Add);

        let after = staged.authored_after(&authored);

        assert_eq!(authored, snapshot);
        assert_eq!(inherited, vec!["developer".to_string()]);
        assert_eq!(after, vec!["research".to_string()]);
    }
}
