//! Production V2 application-service adapter over the proven palette backend.
//!
//! The CLI's application `Service` already implements [`PaletteBackend`]. This
//! adapter therefore lets the V2 reducer/runtime consume that same service object
//! without shelling `aikit`, cloning resolver rules into the TUI, or waiting for
//! every V1 Capsule presentation to be removed. The V2 Quick path consumes the
//! backend's ResourceRef-native shallow navigation index; the old capsule matcher
//! remains available only to the V1 compatibility presentation.

use aikit_core::id::CapsuleId;
use aikit_core::resource::{ContextualActionDescriptor, ResourceRef};
use aikit_core::{AikitError, Result};
use serde_json::{json, to_string_pretty, to_value, Value};

use crate::application::{
    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,
    RelationReadModel, ResourceListItem, ResourceListReadModel, StagedChanges,
    TuiApplicationService,
};
use crate::backend::{PaletteBackend, Toggle};
use crate::staging::is_on;

pub struct PaletteApplicationService<'a> {
    backend: &'a mut dyn PaletteBackend,
}

impl<'a> PaletteApplicationService<'a> {
    pub fn new(backend: &'a mut dyn PaletteBackend) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &dyn PaletteBackend {
        self.backend
    }

    pub fn backend_mut(&mut self) -> &mut dyn PaletteBackend {
        self.backend
    }
}

impl TuiApplicationService for PaletteApplicationService<'_> {
    fn search(&self, query: &str) -> Result<ResourceListReadModel> {
        let index = self.backend.navigation_index();
        let resources = index
            .search(query, 256)
            .into_iter()
            .map(|hit| ResourceListItem {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: hit.summary,
            })
            .collect();
        Ok(ResourceListReadModel {
            revision: format!(
                "aikit.resource-search/v2:{}:{}:{}",
                self.backend.view().catalog_revision,
                self.backend.view().hash,
                query
            ),
            resources,
        })
    }

    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value> {
        if let Ok(capsule) = CapsuleId::parse(resource.as_str()) {
            let view = self.backend.view();
            return Ok(json!({
                "resource": resource.as_str(),
                "context": to_value(self.backend.context()).map_err(json_error)?,
                "active": view.is_active(&capsule),
                "declaredEnabled": view.is_declared_enabled(&capsule),
                "available": !view.unavailable.contains_key(&capsule),
                "runnable": view.can_run(&capsule),
                "catalogRevision": view.catalog_revision,
                "resolutionHash": view.hash.to_string(),
            }));
        }

        let index = self.backend.navigation_index();
        let hit = index
            .search(resource.as_str(), 256)
            .into_iter()
            .find(|hit| &hit.resource == resource)
            .ok_or_else(|| {
                AikitError::new(
                    "tui.resource_not_in_navigation_index",
                    format!("{resource} is not in the V2 navigation index"),
                )
            })?;
        Ok(json!({
            "resource": resource.as_str(),
            "kind": hit.kind.as_str(),
            "label": hit.label,
            "summary": hit.summary,
            "context": to_value(self.backend.context()).map_err(json_error)?,
            "navigationEvidence": hit.navigation_evidence,
        }))
    }

    fn preview_composition(
        &self,
        scope: aikit_core::scope::ScopeKind,
        staged: &StagedChanges,
    ) -> Result<CompositionPreview> {
        let toggles = toggles(staged)?;
        let projected = self.backend.preview(scope, &toggles)?;
        Ok(CompositionPreview {
            revision: format!("{}:{}", projected.view.catalog_revision, projected.view.hash),
            scope,
            staged: staged.clone(),
            summary: format!(
                "{} staged change{} -> {} active capability{}; {} client effect{}",
                staged.len(),
                plural(staged.len()),
                projected.view.active.len(),
                plural(projected.view.active.len()),
                projected.effects.len(),
                plural(projected.effects.len()),
            ),
        })
    }

    fn apply_composition(&mut self, preview: &CompositionPreview) -> Result<ApplyReceipt> {
        let toggles = toggles(&preview.staged)?;
        let generation = self.backend.apply(preview.scope, &toggles)?;
        Ok(ApplyReceipt {
            revision: generation.to_string(),
            summary: format!("applied generation {generation}"),
        })
    }

    fn explain(&self, resource: &ResourceRef) -> Result<Value> {
        let capsule = capsule_id(resource)?;
        let view = self.backend.view();
        let entry = view.catalog_index.get(&capsule).ok_or_else(|| {
            AikitError::new(
                "tui.resource_not_in_catalog",
                format!("{resource} is not in the resolved catalog"),
            )
        })?;
        Ok(json!({
            "resource": resource.as_str(),
            "name": entry.name,
            "description": entry.description,
            "kind": entry.kind.as_str(),
            "active": view.is_active(&capsule),
            "declaredEnabled": view.is_declared_enabled(&capsule),
            "unavailable": view.unavailable.get(&capsule).map(|reason| format!("{reason:?}")),
            "related": view.related_to(&capsule).into_iter().map(|id| id.to_string()).collect::<Vec<_>>(),
            "resolutionHash": view.hash.to_string(),
        }))
    }

    fn history(&self, resource: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>> {
        let wanted = resource.map(capsule_id).transpose()?;
        Ok(self
            .backend
            .recent()
            .into_iter()
            .enumerate()
            .filter(|(_, intent)| wanted.as_ref().is_none_or(|id| &intent.capsule == id))
            .map(|(index, intent)| {
                let summary = intent
                    .redacted_argv()
                    .ok()
                    .filter(|argv| !argv.is_empty())
                    .map(|argv| format!("{} · {}", intent.capsule, argv.join(" ")))
                    .unwrap_or_else(|| intent.capsule.to_string());
                HistoryEntry {
                    id: format!("recent-{index}"),
                    summary,
                }
            })
            .collect())
    }

    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel> {
        let capsule = capsule_id(resource)?;
        let view = self.backend.view();
        if !view.catalog_index.contains_key(&capsule) {
            return Err(AikitError::new(
                "tui.resource_not_in_catalog",
                format!("{resource} is not in the resolved catalog"),
            ));
        }
        Ok(RelationReadModel {
            subject: resource.clone(),
            value: json!({
                "related": view
                    .related_to(&capsule)
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            }),
        })
    }

    fn contextual_actions(&self, resource: &ResourceRef) -> Result<Vec<ContextualActionDescriptor>> {
        let index = self.backend.navigation_index();
        Ok(index.actions_for(resource).into_iter().cloned().collect())
    }

    fn invoke_action(&mut self, action: &ContextualActionDescriptor) -> Result<ActionOutcome> {
        match action.action.as_str() {
            "action/project/open" => Ok(ActionOutcome::Opened {
                subject: action.subject.clone(),
                summary: format!("opened {}", action.subject),
            }),
            "action/capability/explain" => {
                let explanation = self.explain(&action.subject)?;
                Ok(ActionOutcome::Explained {
                    subject: action.subject.clone(),
                    summary: to_string_pretty(&explanation).map_err(json_error)?,
                })
            }
            "action/capability/toggle" => {
                let capsule = capsule_id(&action.subject)?;
                let intent = if is_on(self.backend.view(), &capsule) {
                    ActivationIntent::Disable
                } else {
                    ActivationIntent::Enable
                };
                Ok(ActionOutcome::Staged {
                    resource: action.subject.clone(),
                    intent,
                    summary: format!(
                        "staged {} for {}",
                        match intent {
                            ActivationIntent::Enable => "enable",
                            ActivationIntent::Disable => "disable",
                        },
                        action.subject
                    ),
                })
            }
            other => Err(AikitError::new(
                "tui.action_not_implemented",
                format!("canonical Action {other} has no TUI application operation"),
            )),
        }
    }
}

fn capsule_id(resource: &ResourceRef) -> Result<CapsuleId> {
    CapsuleId::parse(resource.as_str()).map_err(|error| {
        AikitError::new(
            "tui.resource_not_capsule_compatible",
            format!("{resource} is not representable by the V1 capsule adapter: {error:?}"),
        )
    })
}

fn toggles(staged: &StagedChanges) -> Result<Vec<Toggle>> {
    staged
        .resources()
        .map(|resource| {
            let capsule = capsule_id(resource)?;
            let enable = staged.get(resource) == Some(ActivationIntent::Enable);
            Ok(Toggle::new(capsule, enable))
        })
        .collect()
}

fn json_error(error: serde_json::Error) -> AikitError {
    AikitError::new(
        "tui.read_model_encode_failed",
        format!("could not encode TUI read model: {error}"),
    )
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}