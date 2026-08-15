//! Production V2 application-service adapter over the proven palette backend.
//!
//! The CLI's application `Service` already implements [`PaletteBackend`]. This
//! adapter therefore lets the V2 reducer/runtime consume that same service object
//! without shelling `aikit`, cloning resolver rules into the TUI, or waiting for
//! every V1 Capsule presentation to be removed. The V2 Quick path consumes the
//! backend's ResourceRef-native shallow navigation index; the old capsule matcher
//! remains available only to the V1 compatibility presentation.

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::id::{CapsuleId, EventId};
use aikit_core::resource::{
    ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass, ResourceIndex,
    ResourceRef, ResourceSearchIndex,
};
use aikit_core::{
    AikitError, FamiliarityContext, FamiliarityObservation, FamiliarityUse, Result,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};
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

    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        let mut index = self.backend.navigation_index();
        if let Some(familiarity) = self.backend.familiarity()? {
            index.apply_familiarity(
                &familiarity,
                &familiarity_context(self.backend.context()),
                now_ms(),
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
        }
        Ok(index)
    }

    fn learned_accessibility(
        &self,
        resource: &ResourceRef,
    ) -> Result<Option<aikit_core::AccessibilityAssessment>> {
        Ok(self
            .backend
            .familiarity()?
            .map(|store| {
                store.assess_destination(
                    resource,
                    &familiarity_context(self.backend.context()),
                    now_ms(),
                    DEFAULT_FAMILIARITY_HALF_LIFE_MS,
                )
            })
            .filter(|assessment| !assessment.is_empty()))
    }

    fn record_destination_use(&mut self, destination: ResourceRef) -> Result<()> {
        let observation = FamiliarityObservation::destination(
            EventId::generate().as_str().to_string(),
            destination,
            familiarity_context(self.backend.context()),
            now_ms(),
        )
        .from_surface(
            ResourceRef::parse("surface/aikit/tui")
                .expect("static V2 TUI surface ResourceRef must be valid"),
        );
        self.backend.record_familiarity(observation)
    }

    fn record_action_use(&mut self, action: &ContextualActionDescriptor) -> Result<()> {
        let observation = FamiliarityObservation::destination(
            EventId::generate().as_str().to_string(),
            action.subject.clone(),
            familiarity_context(self.backend.context()),
            now_ms(),
        )
        .via_action(action.action.clone())
        .from_surface(
            ResourceRef::parse("surface/aikit/tui")
                .expect("static V2 TUI surface ResourceRef must be valid"),
        );
        self.backend.record_familiarity(observation)
    }
}

impl TuiApplicationService for PaletteApplicationService<'_> {
    fn search(&self, query: &str) -> Result<ResourceListReadModel> {
        let index = self.navigation_index()?;
        let resources = index
            .search(query, 256)
            .into_iter()
            .map(|hit| ResourceListItem {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: summary_with_navigation_evidence(hit.summary, &hit.navigation_evidence),
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

        let index = self.navigation_index()?;
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
            "ranking": hit.ranking,
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
        let target_effects = if projected.effects.is_empty() {
            "no target effects".to_string()
        } else {
            projected
                .effects
                .iter()
                .map(|effect| effect.describe())
                .collect::<Vec<_>>()
                .join(", ")
        };
        Ok(CompositionPreview {
            revision: format!("{}:{}", projected.view.catalog_revision, projected.view.hash),
            scope,
            staged: staged.clone(),
            summary: format!(
                "{} staged change{} -> {} active capability{}; target effects: {}",
                staged.len(),
                plural(staged.len()),
                projected.view.active.len(),
                plural(projected.view.active.len()),
                target_effects,
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
        let learned = self.learned_accessibility(resource)?;
        if let Ok(capsule) = CapsuleId::parse(resource.as_str()) {
            let view = self.backend.view();
            if let Some(entry) = view.catalog_index.get(&capsule) {
                return Ok(json!({
                    "resource": resource.as_str(),
                    "name": entry.name,
                    "description": entry.description,
                    "kind": entry.kind.as_str(),
                    "active": view.is_active(&capsule),
                    "declaredEnabled": view.is_declared_enabled(&capsule),
                    "unavailable": view.unavailable.get(&capsule).map(|reason| format!("{reason:?}")),
                    "related": view.related_to(&capsule).into_iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                    "learnedAccessibility": learned,
                    "resolutionHash": view.hash.to_string(),
                }));
            }
        }

        let index = self.navigation_index()?;
        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            AikitError::new(
                "tui.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let hit = index
            .search(resource.as_str(), 1)
            .into_iter()
            .find(|hit| &hit.resource == resource);
        let contextual_actions = index
            .actions_for(resource)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let explanation = record.explanation();

        Ok(json!({
            "resource": resource.as_str(),
            "name": record.descriptor.name,
            "description": record.descriptor.description,
            "kind": record.descriptor.kind.as_str(),
            "owner": explanation.owner,
            "sources": explanation.sources,
            "providers": explanation.providers,
            "eligibility": explanation.eligibility,
            "authoredPreference": explanation.preference,
            "annotations": record.descriptor.annotations,
            "ranking": hit.as_ref().map(|hit| &hit.ranking),
            "navigationEvidence": hit.as_ref().map(|hit| &hit.navigation_evidence),
            "contextualActions": contextual_actions,
            "learnedAccessibility": learned,
            "resolutionHash": self.backend.view().hash.to_string(),
        }))
    }

    fn history(&self, resource: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>> {
        let wanted_capsule = resource.and_then(|wanted| CapsuleId::parse(wanted.as_str()).ok());
        let mut entries = self
            .backend
            .recent()
            .into_iter()
            .enumerate()
            .filter(|(_, intent)| match resource {
                None => true,
                Some(_) => wanted_capsule
                    .as_ref()
                    .is_some_and(|id| &intent.capsule == id),
            })
            .map(|(index, intent)| {
                let summary = intent
                    .redacted_argv()
                    .ok()
                    .filter(|argv| !argv.is_empty())
                    .map(|argv| format!("run · {} · {}", intent.capsule, argv.join(" ")))
                    .unwrap_or_else(|| format!("run · {}", intent.capsule));
                HistoryEntry {
                    id: format!("recent-{index}"),
                    summary,
                }
            })
            .collect::<Vec<_>>();

        if let Some(store) = self.backend.familiarity()? {
            let mut observations = store.snapshot().observations;
            observations.sort_by(|left, right| {
                right
                    .observed_at_ms
                    .cmp(&left.observed_at_ms)
                    .then_with(|| right.observation_id.cmp(&left.observation_id))
            });
            entries.extend(
                observations
                    .into_iter()
                    .filter(|observation| {
                        resource.is_none_or(|wanted| observation.destination == *wanted)
                    })
                    .map(|observation| {
                        let route = match &observation.use_kind {
                            FamiliarityUse::Destination => "destination".to_string(),
                            FamiliarityUse::Route { route, steps } => {
                                format!("route {route} · {} step{}", steps.len(), plural(steps.len()))
                            }
                        };
                        let action = observation
                            .source_action
                            .as_ref()
                            .map(|action| format!(" · action {action}"))
                            .unwrap_or_default();
                        let surface = observation
                            .source_surface
                            .as_ref()
                            .map(|surface| format!(" · surface {surface}"))
                            .unwrap_or_default();
                        HistoryEntry {
                            id: observation.observation_id,
                            summary: format!(
                                "use · {} · {route}{action}{surface}",
                                observation.destination
                            ),
                        }
                    }),
            );
        }

        Ok(entries)
    }

    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel> {
        let index = self.navigation_index()?;
        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            AikitError::new(
                "tui.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let explanation = record.explanation();
        let contextual_actions = index
            .actions_for(resource)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let resolver_related = CapsuleId::parse(resource.as_str())
            .ok()
            .filter(|capsule| self.backend.view().catalog_index.contains_key(capsule))
            .map(|capsule| {
                self.backend
                    .view()
                    .related_to(&capsule)
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(RelationReadModel {
            subject: resource.clone(),
            value: json!({
                "owner": explanation.owner,
                "sources": explanation.sources,
                "providers": explanation.providers,
                "contextualActions": contextual_actions,
                "related": resolver_related.clone(),
                "resolverRelated": resolver_related,
            }),
        })
    }

    fn observe_resource_use(&mut self, resource: &ResourceRef) -> Result<()> {
        self.record_destination_use(resource.clone())
    }

    fn contextual_actions(&self, resource: &ResourceRef) -> Result<Vec<ContextualActionDescriptor>> {
        let index = self.navigation_index()?;
        Ok(index.actions_for(resource).into_iter().cloned().collect())
    }

    fn invoke_action(&mut self, action: &ContextualActionDescriptor) -> Result<ActionOutcome> {
        let outcome = match action.action.as_str() {
            "action/project/open" => ActionOutcome::Opened {
                subject: action.subject.clone(),
                summary: format!("opened {}", action.subject),
            },
            "action/capability/explain" => {
                let explanation = self.explain(&action.subject)?;
                ActionOutcome::Explained {
                    subject: action.subject.clone(),
                    summary: to_string_pretty(&explanation).map_err(json_error)?,
                }
            }
            "action/capability/toggle" => {
                let capsule = capsule_id(&action.subject)?;
                let intent = if is_on(self.backend.view(), &capsule) {
                    ActivationIntent::Disable
                } else {
                    ActivationIntent::Enable
                };
                ActionOutcome::Staged {
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
                }
            }
            other => {
                return Err(AikitError::new(
                    "tui.action_not_implemented",
                    format!("canonical Action {other} has no TUI application operation"),
                ))
            }
        };
        self.record_action_use(action)?;
        Ok(outcome)
    }
}

fn familiarity_context(context: &aikit_core::ContextDescriptor) -> FamiliarityContext {
    FamiliarityContext {
        project: context.project_id.as_ref().and_then(|project| {
            ResourceRef::parse(&format!("project/{project}")).ok()
        }),
        actor: None,
        agency: None,
        focus: context.task.clone(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

fn summary_with_navigation_evidence(summary: String, evidence: &[NavigationEvidence]) -> String {
    if evidence.is_empty() {
        return summary;
    }
    let labels = evidence
        .iter()
        .map(|item| {
            let class = match item.class {
                NavigationEvidenceClass::CurrentContext => "current context",
                NavigationEvidenceClass::ExplicitPin => "explicit pin",
                NavigationEvidenceClass::Recent => "recent",
                NavigationEvidenceClass::LearnedUsage => "learned usage",
                NavigationEvidenceClass::ChangedProject => "changed project",
            };
            item.detail
                .as_deref()
                .map(|detail| format!("{class}: {detail}"))
                .unwrap_or_else(|| class.to_string())
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{summary} · evidence: {labels}")
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
