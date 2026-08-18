//! Canonical V2 application-service adapter over AIKit's one resolved backend.
//!
//! This service is the semantic bridge used by the final TUI surface. It consumes
//! one ResourceRef-native navigation field and one resolved Context/application
//! backend. Package-backed Capabilities may still translate to Capsule operations
//! at the package compatibility boundary, but generic V2 Resources are never
//! detected by attempting to parse their identity as a Capsule.

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::composition_mutation::{changed_ground, CompositionBasis};
use aikit_core::id::{CapsuleId, EventId};
use aikit_core::resource::{
    ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass, ResourceIndex,
    ResourceKind, ResourceRef, ResourceSearchIndex,
};
use aikit_core::{
    explain_history_actions_for, install_explain_history_actions, AikitError, FamiliarityContext,
    FamiliarityObservation, FamiliarityUse, ForgetScope, KnowledgeAddress, KnowledgeContextPack,
    KnowledgeProviderStatus, KnowledgeReading, KnowledgeRoute, KnowledgeSources, Result,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};
use aikit_store::KnowledgeHistoryOperation;
use serde_json::{json, to_string_pretty, to_value, Value};

use crate::application::{
    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,
    RelationReadModel, ResourceListItem, ResourceListReadModel, StagedChanges,
    TuiApplicationService,
};
use crate::backend::{PaletteBackend, Toggle};
use crate::staging::is_on;

/// One V2 application service over the already-resolved backend.
///
/// The backend is still named `PaletteBackend` while #59 removes its remaining
/// compatibility callers; this service does not inherit Palette semantics.
pub struct ApplicationService<'a> {
    backend: &'a mut dyn PaletteBackend,
}

impl<'a> ApplicationService<'a> {
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
        install_explain_history_actions(&mut index)?;
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

    /// Resolve the deliberately retained package compatibility identity for a
    /// Resource only when that Resource is canonically a Capability and the live
    /// package catalog actually owns the same id.
    fn package_capability_id(&self, resource: &ResourceRef) -> Result<Option<CapsuleId>> {
        let index = self.navigation_index()?;
        let Some(record) = ResourceIndex::resource(&index, resource) else {
            return Ok(None);
        };
        if record.descriptor.kind != ResourceKind::Capability {
            return Ok(None);
        }
        let Ok(capsule) = CapsuleId::parse(resource.as_str()) else {
            return Ok(None);
        };
        Ok(self.backend.capsule(&capsule).is_some().then_some(capsule))
    }

    fn require_package_capability(&self, resource: &ResourceRef) -> Result<CapsuleId> {
        self.package_capability_id(resource)?.ok_or_else(|| {
            AikitError::new(
                "application.resource_not_package_capability",
                format!(
                    "{resource} is not a package-backed Capability; generic V2 Resources have no Capsule fallback"
                ),
            )
        })
    }

    fn package_toggles(&self, staged: &StagedChanges) -> Result<Vec<Toggle>> {
        staged
            .resources()
            .map(|resource| {
                let capsule = self.require_package_capability(resource)?;
                let enable = staged.get(resource) == Some(ActivationIntent::Enable);
                Ok(Toggle::new(capsule, enable))
            })
            .collect()
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

impl TuiApplicationService for ApplicationService<'_> {
    fn search(&self, query: &str) -> Result<ResourceListReadModel> {
        let index = self.navigation_index()?;
        let mut resources = index
            .search(query, 256)
            .into_iter()
            .map(|hit| ResourceListItem {
                resource: hit.resource,
                kind: hit.kind,
                label: hit.label,
                summary: summary_with_navigation_evidence(hit.summary, &hit.navigation_evidence),
            })
            .collect::<Vec<_>>();
        if let Some(knowledge) = self.backend.knowledge_search(query, 256)? {
            for hit in knowledge.hits {
                if resources.iter().any(|item| item.resource == hit.resource) {
                    continue;
                }
                let provider = hit.provider.to_string();
                resources.push(ResourceListItem {
                    resource: hit.resource,
                    kind: hit.kind,
                    label: hit.label,
                    summary: format!("{} · {provider} · {:?}", hit.snippet, hit.authority),
                });
            }
        }
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
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(reading) = self.backend.knowledge_read(&address)? {
                return Ok(json!({
                    "resource": resource.as_str(),
                    "knowledgeAddress": address,
                    "reading": reading,
                    "context": to_value(self.backend.context()).map_err(json_error)?,
                    "catalogRevision": self.backend.view().catalog_revision,
                    "resolutionHash": self.backend.view().hash.to_string(),
                }));
            }
        }
        let index = self.navigation_index()?;
        let hit = index
            .search(resource.as_str(), 256)
            .into_iter()
            .find(|hit| &hit.resource == resource)
            .ok_or_else(|| {
                AikitError::new(
                    "application.resource_not_in_navigation_index",
                    format!("{resource} is not in the V2 navigation index"),
                )
            })?;

        let package_state = self.package_capability_id(resource)?.map(|capsule| {
            let view = self.backend.view();
            json!({
                "active": view.is_active(&capsule),
                "declaredEnabled": view.is_declared_enabled(&capsule),
                "available": !view.unavailable.contains_key(&capsule),
                "runnable": view.can_run(&capsule),
            })
        });

        Ok(json!({
            "resource": resource.as_str(),
            "kind": hit.kind.as_str(),
            "label": hit.label,
            "summary": hit.summary,
            "context": to_value(self.backend.context()).map_err(json_error)?,
            "ranking": hit.ranking,
            "navigationEvidence": hit.navigation_evidence,
            "packageCapabilityState": package_state,
            "catalogRevision": self.backend.view().catalog_revision,
            "resolutionHash": self.backend.view().hash.to_string(),
        }))
    }

    fn preview_composition(
        &self,
        scope: aikit_core::scope::ScopeKind,
        staged: &StagedChanges,
    ) -> Result<CompositionPreview> {
        let toggles = self.package_toggles(staged)?;
        let before = self.backend.view();
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
        let ground = changed_ground(before, &projected.view);
        Ok(CompositionPreview {
            revision: composition_revision(before, &projected.view),
            scope,
            staged: staged.clone(),
            summary: format!(
                "{} staged change{} -> {} active capability{}; changed ground: +{} -{} capability{}, +{} -{} warning{}; target effects: {}",
                staged.len(),
                plural(staged.len()),
                projected.view.active.len(),
                plural(projected.view.active.len()),
                ground.capabilities_added.len(),
                ground.capabilities_removed.len(),
                plural(ground.capabilities_added.len() + ground.capabilities_removed.len()),
                ground.warnings_added.len(),
                ground.warnings_removed.len(),
                plural(ground.warnings_added.len() + ground.warnings_removed.len()),
                target_effects,
            ),
        })
    }

    fn apply_composition(&mut self, preview: &CompositionPreview) -> Result<ApplyReceipt> {
        let toggles = self.package_toggles(&preview.staged)?;
        let current = self.backend.view().clone();
        let projected = self.backend.preview(preview.scope, &toggles)?;
        let current_revision = composition_revision(&current, &projected.view);
        if current_revision != preview.revision {
            return Err(AikitError::new(
                "composition.preview_stale",
                "the accepted composition preview no longer matches the live resolution basis",
            )
            .with("expected_revision", preview.revision.clone())
            .with("current_revision", current_revision));
        }
        let ground = changed_ground(&current, &projected.view);
        let generation = self.backend.apply(preview.scope, &toggles)?;
        Ok(ApplyReceipt {
            revision: generation.to_string(),
            summary: format!(
                "applied generation {generation}; changed ground: +{} -{} capability{}, +{} -{} warning{}",
                ground.capabilities_added.len(),
                ground.capabilities_removed.len(),
                plural(ground.capabilities_added.len() + ground.capabilities_removed.len()),
                ground.warnings_added.len(),
                ground.warnings_removed.len(),
                plural(ground.warnings_added.len() + ground.warnings_removed.len()),
            ),
        })
    }

    fn explain(&self, resource: &ResourceRef) -> Result<Value> {
        let learned = self.learned_accessibility(resource)?;
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(explanation) = self.backend.knowledge_explain(&address)? {
                return Ok(json!({
                    "resource": resource.as_str(),
                    "knowledgeAddress": address,
                    "knowledge": explanation,
                    "learnedAccessibility": learned,
                    "catalogRevision": self.backend.view().catalog_revision,
                    "resolutionHash": self.backend.view().hash.to_string(),
                }));
            }
        }
        let index = self.navigation_index()?;
        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            AikitError::new(
                "application.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let hit = index
            .search(resource.as_str(), 256)
            .into_iter()
            .find(|hit| &hit.resource == resource);
        let contextual_actions = index
            .actions_for(resource)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let explanation = record.explanation();
        let package_state = self
            .package_capability_id(resource)?
            .map(|capsule| {
                let view = self.backend.view();
                json!({
                    "active": view.is_active(&capsule),
                    "declaredEnabled": view.is_declared_enabled(&capsule),
                    "unavailable": view.unavailable.get(&capsule).map(|reason| format!("{reason:?}")),
                    "runnable": view.can_run(&capsule),
                    "related": view.related_to(&capsule).into_iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                })
            });

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
            "packageCapabilityState": package_state,
            "catalogRevision": self.backend.view().catalog_revision,
            "resolutionHash": self.backend.view().hash.to_string(),
        }))
    }

    fn history(&self, resource: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>> {
        let wanted_capsule = resource
            .map(|wanted| self.package_capability_id(wanted))
            .transpose()?
            .flatten();
        let mut entries = self
            .backend
            .knowledge_history(resource)?
            .into_iter()
            .map(|receipt| {
                let summary = match receipt.operation {
                    KnowledgeHistoryOperation::Route => receipt
                        .route
                        .as_ref()
                        .map(|route| {
                            format!(
                                "knowledge route · {} · {} step{}",
                                route.route,
                                route.steps.len(),
                                plural(route.steps.len())
                            )
                        })
                        .unwrap_or_else(|| "knowledge route receipt".into()),
                    KnowledgeHistoryOperation::Frame => receipt
                        .frame
                        .as_ref()
                        .map(|frame| {
                            format!(
                                "knowledge frame · {} reading{} · {} route{} · {} absence{}",
                                frame.readings.len(),
                                plural(frame.readings.len()),
                                frame.routes.len(),
                                plural(frame.routes.len()),
                                frame.absences.len(),
                                plural(frame.absences.len())
                            )
                        })
                        .unwrap_or_else(|| "knowledge frame receipt".into()),
                };
                HistoryEntry {
                    id: receipt.receipt_id,
                    summary,
                }
            })
            .collect::<Vec<_>>();
        entries.extend(
            self.backend
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
                .collect::<Vec<_>>(),
        );

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
                                format!(
                                    "route {route} · {} step{}",
                                    steps.len(),
                                    plural(steps.len())
                                )
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
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(view) = self.backend.knowledge_relations(&address, 2, 256, 512)? {
                return Ok(RelationReadModel {
                    subject: resource.clone(),
                    value: to_value(view).map_err(json_error)?,
                });
            }
        }
        let index = self.navigation_index()?;
        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            AikitError::new(
                "application.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let explanation = record.explanation();
        let contextual_actions = index
            .actions_for(resource)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let resolver_related = self
            .package_capability_id(resource)?
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

    fn knowledge_read(&self, address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
        self.backend.knowledge_read(address)
    }

    fn knowledge_route(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeRoute>> {
        self.backend.knowledge_route(query, addresses)
    }

    fn knowledge_frame(
        &mut self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeContextPack>> {
        self.backend.knowledge_frame(query, addresses)
    }

    fn knowledge_sources(&self, address: &KnowledgeAddress) -> Result<Option<KnowledgeSources>> {
        self.backend.knowledge_sources(address)
    }

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        self.backend.knowledge_status()
    }

    fn knowledge_forget(&mut self, scope: ForgetScope) -> Result<bool> {
        self.backend.knowledge_forget(scope)
    }

    fn observe_resource_use(&mut self, resource: &ResourceRef) -> Result<()> {
        self.record_destination_use(resource.clone())
    }

    fn contextual_actions(
        &self,
        resource: &ResourceRef,
    ) -> Result<Vec<ContextualActionDescriptor>> {
        let index = self.navigation_index()?;
        let mut actions = index
            .actions_for(resource)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if self.backend.knowledge_address(resource)?.is_some() {
            for action in explain_history_actions_for(resource)? {
                if !actions
                    .iter()
                    .any(|existing| existing.action == action.action)
                {
                    actions.push(action);
                }
            }
        }
        Ok(actions)
    }

    fn invoke_action(&mut self, action: &ContextualActionDescriptor) -> Result<ActionOutcome> {
        let outcome = match action.action.as_str() {
            EXPLAIN_ACTION_REF => {
                let evidence = crate::explain_history_service::ExplainHistoryApplicationService::explain_evidence(
                    self,
                    &action.subject,
                )?;
                ActionOutcome::Explained {
                    subject: action.subject.clone(),
                    summary: to_string_pretty(&evidence).map_err(json_error)?,
                }
            }
            HISTORY_ACTION_REF => {
                let history = crate::explain_history_service::ExplainHistoryApplicationService::history_evidence(
                    self,
                    Some(&action.subject),
                )?;
                ActionOutcome::History {
                    subject: action.subject.clone(),
                    summary: format!(
                        "history · {} evidence entr{} for {}",
                        history.entries.len(),
                        if history.entries.len() == 1 {
                            "y"
                        } else {
                            "ies"
                        },
                        action.subject
                    ),
                }
            }
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
                let capsule = self.require_package_capability(&action.subject)?;
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
                    "application.action_not_implemented",
                    format!("canonical Action {other} has no application operation"),
                ))
            }
        };
        self.record_action_use(action)?;
        Ok(outcome)
    }
}

fn familiarity_context(context: &aikit_core::ContextDescriptor) -> FamiliarityContext {
    FamiliarityContext {
        project: context
            .project_id
            .as_ref()
            .and_then(|project| ResourceRef::parse(&format!("project/{project}")).ok()),
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

fn composition_revision(
    before: &aikit_core::ResolvedView,
    after: &aikit_core::ResolvedView,
) -> String {
    let before = CompositionBasis::from_view(before);
    let after = CompositionBasis::from_view(after);
    format!(
        "{}:{}=>{}:{}",
        before.catalog_revision,
        before.resolution_hash,
        after.catalog_revision,
        after.resolution_hash
    )
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

fn json_error(error: serde_json::Error) -> AikitError {
    AikitError::new(
        "application.read_model_encode_failed",
        format!("could not encode application read model: {error}"),
    )
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
