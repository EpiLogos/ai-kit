//! Production V2 application-service adapter over the proven palette backend.
//!
//! The CLI's application `Service` already implements [`PaletteBackend`]. This
//! adapter therefore lets the V2 reducer/runtime consume that same service object
//! without shelling `aikit`, cloning resolver rules into the TUI, or waiting for
//! every V1 Capsule presentation to be removed. #41 can widen search from the
//! compatibility capsule catalog to all V2 Resources without changing the service
//! boundary established here.

use std::cell::RefCell;

use aikit_core::id::CapsuleId;
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::search::parse_query;
use aikit_core::{AikitError, Result};
use serde_json::{json, to_value, Value};

use crate::application::{
    ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry, RelationReadModel,
    ResourceListItem, ResourceListReadModel, StagedChanges, TuiApplicationService,
};
use crate::backend::{PaletteBackend, Toggle};
use crate::search::Matcher;

pub struct PaletteApplicationService<'a> {
    backend: &'a mut dyn PaletteBackend,
    matcher: RefCell<Matcher>,
}

impl<'a> PaletteApplicationService<'a> {
    pub fn new(backend: &'a mut dyn PaletteBackend) -> Self {
        Self {
            backend,
            matcher: RefCell::new(Matcher::new()),
        }
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
        let parsed = parse_query(query);
        let docs = self.backend.documents();
        let rows = self.matcher.borrow_mut().rank(&parsed, &docs);
        let resources = rows
            .into_iter()
            .map(|row| {
                Ok(ResourceListItem {
                    resource: resource_ref(&row.doc.id)?,
                    kind: ResourceKind::Capability,
                    label: row.doc.name,
                    summary: row.doc.description,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ResourceListReadModel {
            revision: format!(
                "{}:{}:{}",
                self.backend.view().catalog_revision,
                self.backend.view().hash,
                query
            ),
            resources,
        })
    }

    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value> {
        let capsule = capsule_id(resource)?;
        let view = self.backend.view();
        Ok(json!({
            "resource": resource.as_str(),
            "context": to_value(self.backend.context()).map_err(json_error)?,
            "active": view.is_active(&capsule),
            "declaredEnabled": view.is_declared_enabled(&capsule),
            "available": !view.unavailable.contains_key(&capsule),
            "runnable": view.can_run(&capsule),
            "catalogRevision": view.catalog_revision,
            "resolutionHash": view.hash.to_string(),
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
            "unavailable": view.unavailable.get(&capsule).map(|reason| reason.describe()),
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
            .map(|(index, intent)| HistoryEntry {
                id: format!("recent-{index}"),
                summary: if intent.args.is_empty() {
                    intent.capsule.to_string()
                } else {
                    format!("{} {}", intent.capsule, intent.args.join(" "))
                },
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
}

fn capsule_id(resource: &ResourceRef) -> Result<CapsuleId> {
    CapsuleId::parse(resource.as_str()).map_err(|error| {
        AikitError::new(
            "tui.resource_not_capsule_compatible",
            format!("{resource} is not representable by the V1 capsule adapter: {error}"),
        )
    })
}

fn resource_ref(capsule: &CapsuleId) -> Result<ResourceRef> {
    ResourceRef::parse(&capsule.to_string()).map_err(|error| {
        AikitError::new(
            "tui.invalid_resource_ref",
            format!("could not expose capsule {capsule} as a ResourceRef: {error}"),
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
