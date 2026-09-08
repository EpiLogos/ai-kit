//! W5 project recency: a root-only, composed SessionStart horizon.
//!
//! Suppression here is projection, never deletion. Project specifications stay
//! registered and explicit listing remains exhaustive.

use std::time::Duration;

use aikit_core::profile::ConfigTable;
use aikit_store::{Index, ProjectActivityEvidence, Timestamp};
use serde::Serialize;

use crate::projects::ProjectSpec;

const DEFAULT_COLD_AFTER_DAYS: u64 = 30;
const DEFAULT_MAX_PROJECTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRecencyConfig {
    pub cold_after: Duration,
    pub max_projects: usize,
}

impl Default for ProjectRecencyConfig {
    fn default() -> Self {
        Self {
            cold_after: Duration::from_secs(DEFAULT_COLD_AFTER_DAYS * 86_400),
            max_projects: DEFAULT_MAX_PROJECTS,
        }
    }
}

impl ProjectRecencyConfig {
    pub fn from_config(config: Option<&ConfigTable>) -> Self {
        let defaults = Self::default();
        let cold_after_days = config
            .and_then(|values| values.get("cold_after_days"))
            .and_then(toml::Value::as_integer)
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_COLD_AFTER_DAYS);
        let max_projects = config
            .and_then(|values| values.get("max_projects"))
            .and_then(toml::Value::as_integer)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(defaults.max_projects);
        Self {
            cold_after: Duration::from_secs(cold_after_days.saturating_mul(86_400)),
            max_projects,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecencyState {
    Active,
    Cold,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecency {
    pub project: String,
    pub root: String,
    pub state: RecencyState,
    pub last_active: Option<ProjectActivityEvidence>,
}

pub fn describe(row: &ProjectRecency) -> serde_json::Value {
    serde_json::json!({
        "project": row.project,
        "root": row.root,
        "state": row.state,
        "last_active": row.last_active.as_ref().map(crate::activity_evidence::describe),
    })
}

/// Classify every directory-backed registration. Nothing is filtered here.
pub fn classify_all(
    index: &Index,
    specs: &[ProjectSpec],
    now: Timestamp,
    config: ProjectRecencyConfig,
) -> aikit_core::Result<Vec<ProjectRecency>> {
    let mut rows = Vec::new();
    for spec in specs {
        for root in &spec.directories {
            let last_active = index.project_last_activity(root)?;
            let state = match &last_active {
                Some(evidence) if evidence.occurred_at.age(now) <= config.cold_after => {
                    RecencyState::Active
                }
                Some(_) => RecencyState::Cold,
                None => RecencyState::Unknown,
            };
            rows.push(ProjectRecency {
                project: spec.id.clone(),
                root: root.display().to_string(),
                state,
                last_active,
            });
        }
    }
    rows.sort_by(|left, right| {
        right
            .last_active
            .as_ref()
            .map(|e| e.occurred_at)
            .cmp(&left.last_active.as_ref().map(|e| e.occurred_at))
            .then_with(|| left.project.cmp(&right.project))
            .then_with(|| left.root.cmp(&right.root))
    });
    Ok(rows)
}

/// Render only the active, bounded automatic horizon and disclose suppression.
pub fn render_session_start(rows: &[ProjectRecency], max_projects: usize) -> String {
    let active: Vec<_> = rows
        .iter()
        .filter(|row| row.state == RecencyState::Active)
        .take(max_projects)
        .collect();
    let cold = rows
        .iter()
        .filter(|row| row.state == RecencyState::Cold)
        .count();
    let unknown = rows
        .iter()
        .filter(|row| row.state == RecencyState::Unknown)
        .count();
    let bounded = rows
        .iter()
        .filter(|row| row.state == RecencyState::Active)
        .count()
        .saturating_sub(active.len());
    let mut lines = vec!["[continuity/project-recency] root project horizon".to_string()];
    lines.extend(active.into_iter().map(|row| {
        let at = row
            .last_active
            .as_ref()
            .map(|e| e.occurred_at.to_string())
            .unwrap_or_default();
        format!("- {} — {} (last active {at})", row.project, row.root)
    }));
    lines.push(format!(
        "suppressed from automatic horizon: cold={cold}, unknown={unknown}, budget={bounded}; explicit project listing remains exhaustive"
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::ContextId;
    use aikit_store::home::AikitHome;
    use std::path::PathBuf;

    fn spec(id: &str, root: PathBuf) -> ProjectSpec {
        ProjectSpec {
            schema: 1,
            id: id.into(),
            directories: vec![root],
            repositories: vec![],
            inherit_default_skill_sets: true,
            skill_sets: vec![],
        }
    }

    #[test]
    fn cold_and_unknown_are_suppressed_only_from_the_automatic_horizon() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("home"));
        let index = Index::open(&home.database()).unwrap();
        let now = Timestamp::from_nanos(40 * 86_400 * 1_000_000_000);
        let hot_root = temp.path().join("hot");
        let cold_root = temp.path().join("cold");
        let unknown_root = temp.path().join("unknown");
        index
            .record_project_activity(&ProjectActivityEvidence::new(
                &hot_root,
                Timestamp::from_nanos(39 * 86_400 * 1_000_000_000),
                ContextId::generate(),
                Some("Edit".into()),
                None,
            ))
            .unwrap();
        index
            .record_project_activity(&ProjectActivityEvidence::new(
                &cold_root,
                Timestamp::from_nanos(1),
                ContextId::generate(),
                Some("Edit".into()),
                None,
            ))
            .unwrap();
        let rows = classify_all(
            &index,
            &[
                spec("hot", hot_root.clone()),
                spec("cold", cold_root),
                spec("unknown", unknown_root),
            ],
            now,
            ProjectRecencyConfig::default(),
        )
        .unwrap();
        assert_eq!(
            rows.len(),
            3,
            "the exhaustive read model keeps every registration"
        );
        let rendered = render_session_start(&rows, 8);
        assert!(rendered.contains("hot"));
        assert!(!rendered.contains(&format!(
            "cold — {}",
            rows.iter().find(|r| r.project == "cold").unwrap().root
        )));
        assert!(rendered.contains("cold=1, unknown=1"));
    }
}
