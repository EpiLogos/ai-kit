//! Canonical V2 application-service adapter over AIKit's one resolved backend.
//!
//! This service is the semantic bridge used by the final TUI surface. It consumes
//! one ResourceRef-native navigation field and one resolved Context/application
//! backend. Package-backed Capabilities may still translate to Capsule operations
//! at the package compatibility boundary, but generic V2 Resources are never
//! detected by attempting to parse their identity as a Capsule.

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::application_context::application_context_resolution;
use aikit_core::composition_mutation::{changed_ground, CompositionBasis};
use aikit_core::id::{CapsuleId, EventId};
use aikit_core::resource::{
    action_semantic_profile, parse_or_search_expression, resolve_action_candidates,
    resolve_expression, resolve_path_identity, resolve_subjects, ContextualActionDescriptor,
    NavigationEvidence, NavigationEvidenceClass, ResolveExpression, ResolvePath, ResolvePathStep,
    ResourceDescriptor, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSearchIndex,
};
use aikit_core::{
    explain_history_actions_for, install_explain_history_actions, AikitError, FamiliarityContext,
    FamiliarityObservation, FamiliarityUse, ForgetScope, KnowledgeAddress, KnowledgeContextPack,
    KnowledgeProviderStatus, KnowledgeReading, KnowledgeRelationView, KnowledgeRoute,
    KnowledgeSources, OperativePathEvidence, RelationDirection, RelationEdge, RelationNode,
    RelationOrigin, RelationQuery, Result, RouteStepEvidence, SourceAuthority,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};
use aikit_store::KnowledgeHistoryOperation;
use serde_json::{json, to_string_pretty, to_value, Value};

use crate::application::{
    ActionInvocationReceipt, ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview,
    HistoryEntry, RelationReadModel, ResolvedActionReadModel, ResolvedSearchReadModel,
    ResourceListItem, ResourceListReadModel, StagedChanges, TuiApplicationService,
};
use crate::backend::{FactoryWorkEntry, PaletteBackend, Toggle};
use crate::session_space_service::install_session_space_navigation_resources;
use crate::staging::is_on;
use crate::workspace_navigation::{
    install_start_factory_work_action, install_workspace_destination_navigation_resources,
    workspace_section_for_destination, START_FACTORY_WORK_ACTION_REF,
    WORKSPACE_DESTINATION_ACTION_REF,
};

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

    pub fn factory_work_entry(&self) -> FactoryWorkEntry {
        self.backend.factory_work_entry()
    }

    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        Self::navigation_index_from(self.backend)
    }

    fn navigation_index_from(backend: &dyn PaletteBackend) -> Result<ResourceSearchIndex> {
        let mut index = crate::project_world_service::resource_index_with_records(
            backend,
            backend.context_resource_records()?,
        )?;
        let session_spaces = backend.session_space_navigation()?;
        install_session_space_navigation_resources(&mut index, &session_spaces);
        install_workspace_destination_navigation_resources(&mut index)?;
        if backend.factory_work_entry().is_ready() {
            install_start_factory_work_action(&mut index)?;
        }
        install_explain_history_actions(&mut index)?;
        if let Some(familiarity) = backend.familiarity()? {
            index.apply_familiarity(
                &familiarity,
                &familiarity_context(backend.context()),
                now_ms(),
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
        }
        Ok(index)
    }

    /// Resolve Search once against the canonical heterogeneous Resource field.
    /// Deep Knowledge providers may contribute canonical refs for subject terms
    /// before the expression is evaluated; they do not receive punctuation as a
    /// fake prose query and they do not mint a second path identity.
    pub fn resolve_search(&self, query: &str) -> Result<ResolvedSearchReadModel> {
        Self::resolve_search_from(self.backend, query)
    }

    /// Shared read-only resolver for headless and interactive consumers.
    pub fn resolve_search_from(
        backend: &dyn PaletteBackend,
        query: &str,
    ) -> Result<ResolvedSearchReadModel> {
        let mut index = Self::navigation_index_from(backend)?;
        let expression = parse_or_search_expression(query)?;

        for subject in resolve_subjects(&expression) {
            if subject.trim().is_empty() {
                continue;
            }
            // A subject is already a subject: it reaches Knowledge as a typed
            // expression, never as raw text to be lexed a second time.
            if let Some(knowledge) =
                backend.knowledge_resolve(&ResolveExpression::ordinary_search(subject), 256)?
            {
                for hit in knowledge.hits {
                    if ResourceIndex::resource(&index, &hit.resource).is_some() {
                        continue;
                    }
                    let mut descriptor = ResourceDescriptor::new(
                        hit.resource.clone(),
                        hit.kind,
                        hit.label,
                        hit.snippet,
                    );
                    descriptor
                        .annotations
                        .insert("knowledge.provider".into(), hit.provider.to_string());
                    descriptor
                        .annotations
                        .insert("knowledge.authority".into(), format!("{:?}", hit.authority));
                    index.insert_resource(ResourceRecord::new(descriptor), Vec::new());
                }
            }
        }

        if let Some(familiarity) = backend.familiarity()? {
            index.apply_resolve_path_familiarity(
                &familiarity,
                &resolve_path_identity(&expression),
                &familiarity_context(backend.context()),
                now_ms(),
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
        }
        let path = resolve_expression(&expression, &index, 256);
        // Empty human Search is the existing zero-query navigation state, not an
        // explicit `@` aperture. Preserve its evidence-only presentation while
        // the typed expression/path remains inspectable. Explicit `@` is non-empty
        // input and therefore discloses the full addressable field.
        let zero_query_hits = query.trim().is_empty().then(|| index.search("", 256));
        let resources = path
            .candidates
            .iter()
            .filter(|candidate| {
                zero_query_hits
                    .as_ref()
                    .is_none_or(|hits| hits.iter().any(|hit| hit.resource == candidate.resource))
            })
            .filter_map(|candidate| {
                let record = ResourceIndex::resource(&index, &candidate.resource)?;
                let evidence = index
                    .search(candidate.resource.as_str(), 256)
                    .into_iter()
                    .find(|hit| hit.resource == candidate.resource)
                    .map(|hit| hit.navigation_evidence)
                    .unwrap_or_default();
                Some(ResourceListItem {
                    resource: candidate.resource.clone(),
                    kind: candidate.kind,
                    label: record.descriptor.name.clone(),
                    summary: summary_with_navigation_evidence(
                        record.descriptor.description.clone(),
                        &evidence,
                    ),
                })
            })
            .collect::<Vec<_>>();
        let revision = format!(
            "aikit.resolve-search/v1:{}:{}:{}:{}",
            backend.view().catalog_revision,
            backend.view().hash,
            query,
            path.identity
        );

        Ok(ResolvedSearchReadModel {
            expression,
            path,
            resources: ResourceListReadModel {
                revision,
                resources,
            },
        })
    }

    /// Discover and qualify one canonical Action for the explicit selected subject.
    /// This is side-effect free: even an `=` expression stops at qualification.
    /// ContextResolution, not punctuation, decides whether invocation is available.
    pub fn resolve_action_for_subject(
        &self,
        query: &str,
        subject: &ResourceRef,
    ) -> Result<ResolvedActionReadModel> {
        let resolved = self.resolve_search(query)?;
        let index = self.navigation_index()?;
        let scope_layers = self.backend.scope_layers().unwrap_or(&[]);
        let context = application_context_resolution(
            self.backend.context(),
            self.backend.view(),
            scope_layers,
            &index,
            aikit_core::RequestedActors::default(),
        )?;
        let candidates = resolve_action_candidates(&resolved.path, &index, &context);

        for candidate in candidates {
            let Some(action) = index
                .actions_for(subject)
                .into_iter()
                .find(|contextual| contextual.action == *candidate.action.resource())
                .cloned()
            else {
                continue;
            };
            let semantic_profile = action_semantic_profile(
                &candidate,
                &resolved.path,
                subject,
                self.backend.context().task.as_deref(),
                &index,
            )?;
            return Ok(ResolvedActionReadModel {
                expression: resolved.expression,
                path: resolved.path,
                candidate,
                semantic_profile,
                action,
            });
        }

        Err(AikitError::new(
            "application.resolve_action_no_contextual_candidate",
            format!(
                "Resolve expression `{query}` did not produce a canonical Action applicable to {subject}"
            ),
        ))
    }

    /// Cross the already-existing native invocation boundary with a previously
    /// qualified Action. The receipt returns the exact observed ResolvePath and
    /// #29 records path accessibility beside ordinary destination familiarity.
    pub fn invoke_resolved_action(
        &mut self,
        resolved: &ResolvedActionReadModel,
    ) -> Result<ActionInvocationReceipt> {
        if !resolved.candidate.available_in_context {
            return Err(AikitError::new(
                "application.resolve_action_unavailable",
                format!(
                    "Action {} is not available in the current ContextResolution",
                    resolved.action.action
                ),
            ));
        }
        let outcome = <Self as TuiApplicationService>::invoke_action(self, &resolved.action)?;
        self.record_resolve_path_use(&resolved.path, resolved)?;
        Ok(ActionInvocationReceipt {
            action: resolved.action.action.clone(),
            subject: resolved.action.subject.clone(),
            observed_path: resolved.path.clone(),
            outcome,
        })
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

    fn record_resolve_path_use(
        &mut self,
        path: &ResolvePath,
        resolved: &ResolvedActionReadModel,
    ) -> Result<()> {
        let surface = ResourceRef::parse("surface/aikit/tui")
            .expect("static V2 TUI surface ResourceRef must be valid");
        let mut relation_ops = path
            .steps
            .iter()
            .filter_map(|step| match step {
                ResolvePathStep::Relation { op } => Some(*op),
                _ => None,
            })
            .collect::<Vec<_>>();
        relation_ops.sort();
        relation_ops.dedup();
        let mut horizons = path
            .steps
            .iter()
            .filter_map(|step| match step {
                ResolvePathStep::Address { horizon, .. } => *horizon,
                _ => None,
            })
            .collect::<Vec<_>>();
        horizons.sort();
        horizons.dedup();

        let steps = vec![
            RouteStepEvidence {
                resource: resolved.action.action.clone(),
                provider: None,
                lens: None,
                revision: None,
            },
            RouteStepEvidence {
                resource: resolved.action.subject.clone(),
                provider: None,
                lens: None,
                revision: None,
            },
        ];
        let observation = FamiliarityObservation::resolve_path(
            EventId::generate().as_str().to_string(),
            None,
            resolved.action.subject.clone(),
            steps,
            OperativePathEvidence {
                path_identity: path.identity.clone(),
                expression: path.expression.clone(),
                relation_ops,
                horizons,
                method: resolved.semantic_profile.method_relations.first().cloned(),
                action: Some(resolved.action.action.clone()),
                surface: Some(surface.clone()),
                activity: None,
                return_ref: None,
            },
            familiarity_context(self.backend.context()),
            now_ms(),
        )?
        .via_action(resolved.action.action.clone())
        .from_surface(surface);
        self.backend.record_familiarity(observation)
    }

    /// Shared body for [`TuiApplicationService::relations`] and
    /// [`TuiApplicationService::relations_at_depth`]. `relations` calls this
    /// with the historical fixed depth of `2`; `relations_at_depth` (the
    /// Graph presentation's `+`/`-` control) passes through the requested,
    /// already-bounded depth. Only the Knowledge-address path actually reads
    /// `depth` — the resolver fallback's "often used with" edges are an
    /// intrinsic one-hop set with no deeper resolver traversal to request,
    /// so its `RelationQuery` still records the requested depth (for Inspector
    /// honesty about what was asked) without pretending to have walked it.
    fn relations_at_depth_impl(
        &self,
        resource: &ResourceRef,
        depth: u8,
    ) -> Result<RelationReadModel> {
        if let Some(address) = self.backend.knowledge_address(resource)? {
            if let Some(view) = self
                .backend
                .knowledge_relations(&address, depth, 256, 512)?
            {
                // The typed view is authoritative; `value` is retained only for
                // Inspector/JSON parity with what the provider actually returned.
                let value = to_value(&view).map_err(json_error)?;
                return Ok(RelationReadModel {
                    subject: resource.clone(),
                    view,
                    value,
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

        // No Knowledge address: build a genuine, honestly-attributed
        // KnowledgeRelationView from the package resolver's "often used with"
        // catalog edges, rather than emitting untyped "related" strings. The
        // resolver *derived* this pairing from a manifest declaration, so it is
        // never Authored, and its lens is named for what it is.
        let mut view = KnowledgeRelationView::focus_only(
            RelationQuery {
                focus: resource.clone(),
                depth,
                max_nodes: aikit_core::DEFAULT_RELATION_NODE_BUDGET,
                max_edges: aikit_core::DEFAULT_RELATION_EDGE_BUDGET,
                filters: Vec::new(),
            },
            RelationNode::new(
                resource.clone(),
                record.descriptor.kind,
                record.descriptor.name.clone(),
            ),
        )?;
        if let Some(capsule) = self.package_capability_id(resource)? {
            for related in self.backend.view().related_to(&capsule) {
                let related_ref = match ResourceRef::parse(related.to_string()) {
                    Ok(related_ref) => related_ref,
                    Err(_) => {
                        view.warnings.push(format!(
                            "resolver related id {related} could not be represented as a Resource"
                        ));
                        continue;
                    }
                };
                if !view.push_node(RelationNode::new(
                    related_ref.clone(),
                    ResourceKind::Capability,
                    related_ref.to_string(),
                )) {
                    // Node budget exhausted; view.truncated is already set, and the
                    // edge cannot be pushed without its endpoint present.
                    continue;
                }
                let origin = RelationOrigin::new(SourceAuthority::Derived).in_lens("resolver");
                view.push_edge(RelationEdge::new(
                    resource.clone(),
                    related_ref,
                    "related-skill",
                    RelationDirection::Bidirectional,
                    origin,
                ))?;
            }
        }

        let resolver_related = view
            .edges
            .iter()
            .map(|edge| edge.to.to_string())
            .collect::<Vec<_>>();
        let value = json!({
            "owner": explanation.owner,
            "sources": explanation.sources,
            "providers": explanation.providers,
            "contextualActions": contextual_actions,
            "related": resolver_related.clone(),
            "resolverRelated": resolver_related,
        });

        Ok(RelationReadModel {
            subject: resource.clone(),
            view,
            value,
        })
    }
}

impl TuiApplicationService for ApplicationService<'_> {
    fn search(&self, query: &str) -> Result<ResourceListReadModel> {
        Ok(self.resolve_search(query)?.resources)
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
                            FamiliarityUse::ResolvePath {
                                knowledge_route,
                                steps,
                                operative,
                            } => {
                                let route = knowledge_route
                                    .as_ref()
                                    .map(|route| format!(" · route {route}"))
                                    .unwrap_or_default();
                                format!(
                                    "resolve {}{route} · {} step{}",
                                    operative.path_identity,
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
        self.relations_at_depth_impl(resource, 2)
    }

    fn relations_at_depth(&self, resource: &ResourceRef, depth: u8) -> Result<RelationReadModel> {
        self.relations_at_depth_impl(resource, depth)
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
            WORKSPACE_DESTINATION_ACTION_REF => {
                let section =
                    workspace_section_for_destination(&action.subject).ok_or_else(|| {
                        AikitError::new(
                            "application.unknown_workspace_destination",
                            format!(
                                "{} is not a known Workspace destination Surface",
                                action.subject
                            ),
                        )
                    })?;
                ActionOutcome::NavigatedTo {
                    section,
                    summary: format!("opened {}", action.subject),
                }
            }
            START_FACTORY_WORK_ACTION_REF => {
                let started = self.backend.start_factory_work()?;
                ActionOutcome::FactoryWorkStarted {
                    summary: started.summary,
                    receipt: started.receipt,
                }
            }
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

/// Regression coverage for the typed-relation-presentation boundary: `relations()`
/// must surface a real [`KnowledgeRelationView`] on both the Knowledge-address path
/// and the resolver fallback path, never only an untyped `"related"` string list.
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use aikit_core::capsule::Capsule;
    use aikit_core::catalog::MemoryCatalog;
    use aikit_core::context::{ContextDescriptor, Isolation};
    use aikit_core::id::{CapsuleId, ContextId, GenerationId, RegistrySource, Revision, SessionId};
    use aikit_core::platform::{Platform, TargetId};
    use aikit_core::policy::ManagedPolicy;
    use aikit_core::resolve::{resolve, ResolveRequest, ResolvedView};
    use aikit_core::resource::{SourceRef, SourceRevision};
    use aikit_core::scope::ScopeKind;
    use aikit_core::search::SearchDoc;
    use aikit_core::trust::{MemoryTrust, TrustState};
    use aikit_core::{
        FamiliarityContext, KnowledgeOperations, NativeSourcePoolProvider, SemanticWikiIndex,
        SemanticWikiProvider, SourceBinding, SourceMaterial, SourcePoolProvider, SourceVisibility,
    };

    use crate::backend::{JobOutput, Projected, PromotionDraft, RunIntent, Toggle};

    use super::*;

    /// A minimal, honest [`PaletteBackend`]: the Knowledge and package/resolver
    /// surfaces `relations()` actually reads are backed by real fixtures (a real
    /// resolved catalogue, a real Knowledge fixture); every operation this test
    /// never exercises is left `unimplemented!()` rather than faked.
    struct FakeBackend {
        context: ContextDescriptor,
        view: ResolvedView,
        capsules: BTreeMap<CapsuleId, Capsule>,
        knowledge: Option<(ResourceRef, KnowledgeAddress, KnowledgeRelationView)>,
    }

    impl PaletteBackend for FakeBackend {
        fn context(&self) -> &ContextDescriptor {
            &self.context
        }

        fn view(&self) -> &ResolvedView {
            &self.view
        }

        fn documents(&self) -> Vec<SearchDoc> {
            Vec::new()
        }

        fn knowledge_address(&self, resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
            Ok(self
                .knowledge
                .as_ref()
                .filter(|(subject, _, _)| subject == resource)
                .map(|(_, address, _)| address.clone()))
        }

        fn knowledge_relations(
            &self,
            _address: &KnowledgeAddress,
            _depth: u8,
            _max_nodes: usize,
            _max_edges: usize,
        ) -> Result<Option<KnowledgeRelationView>> {
            Ok(self.knowledge.as_ref().map(|(_, _, view)| view.clone()))
        }

        fn capsule(&self, id: &CapsuleId) -> Option<&Capsule> {
            self.capsules.get(id)
        }

        fn preview(&self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<Projected> {
            unimplemented!("relation tests never preview a composition")
        }

        fn apply(&mut self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<GenerationId> {
            unimplemented!("relation tests never apply a composition")
        }

        fn start(&mut self, _intent: &RunIntent) -> Result<JobOutput> {
            unimplemented!("relation tests never start a run")
        }

        fn recent(&self) -> Vec<RunIntent> {
            Vec::new()
        }

        fn promotion_drafts(&self) -> Vec<PromotionDraft> {
            Vec::new()
        }

        fn promote(&mut self, _draft: &PromotionDraft) -> Result<CapsuleId> {
            unimplemented!("relation tests never promote a draft")
        }
    }

    fn test_context() -> ContextDescriptor {
        ContextDescriptor {
            context_id: ContextId::parse("ctx_TESTCONTEXT000000000000").unwrap(),
            session_id: Some(SessionId::parse("ses_TESTSESSION000000000000").unwrap()),
            project_id: None,
            project_root: None,
            task: None,
            isolation: Isolation::Shared,
            platform: Platform::Linux,
            targets: vec![TargetId::shell()],
            mux: None,
            host: "test-host".into(),
        }
    }

    /// A real skill capsule, resolved through the real catalogue/trust/resolve
    /// pipeline, declaring `related_skills` the way an author actually would.
    fn skill_capsule(id: &str, related_skills: &[&str]) -> Capsule {
        let related_toml = related_skills
            .iter()
            .map(|related| format!("\"{related}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let leaf = id.rsplit('/').next().unwrap();
        let src = format!(
            r#"schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "Test skill {leaf}."
related_skills = [{related_toml}]

[skill]
root = "payload"
"#
        );
        let mut capsule = Capsule::from_toml_str(&src)
            .unwrap_or_else(|e| panic!("fixture manifest for {id} should parse: {e}"));
        capsule.revision = Some(Revision::from_raw(format!("rev-{id}")));
        capsule.source = Some(RegistrySource::personal());
        capsule.root = Some(PathBuf::from(format!("/registry/{}", id.replace('/', "-"))));
        capsule
    }

    fn resolve_fixture(capsules: Vec<Capsule>) -> (BTreeMap<CapsuleId, Capsule>, ResolvedView) {
        let mut catalog = MemoryCatalog::default();
        for capsule in &capsules {
            catalog.insert(capsule.clone());
        }
        let mut trust = MemoryTrust::default();
        for capsule in &capsules {
            trust.set(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
                TrustState::Reviewed,
            );
        }
        let view = resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: test_context(),
                layers: vec![],
                policy: ManagedPolicy::default(),
            },
        )
        .expect("an empty layer stack always resolves");
        let by_id = capsules.into_iter().map(|c| (c.id.clone(), c)).collect();
        (by_id, view)
    }

    /// Regression test for the exact defect this workstream fixes: a real Wiki
    /// neighbourhood must reach the `RelationReadModel` with its typed nodes,
    /// edges and origins intact, not flattened into an untyped `"related"` list
    /// (which, for this fixture, would previously have been empty).
    #[test]
    fn knowledge_address_relations_survive_typed_into_the_read_model() {
        let objects = aikit_core::parse_wiki_objects(
            r#"{"objects":[
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
               "provenance":[],"title":"Root","parent_space_refs":[],"child_space_refs":[],
               "node_refs":["wiki:node:auth"]},
              {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:auth","revision":1,
               "provenance":[{"source_ref":"source:spec"}],"type":"Concept","title":"Authentication",
               "space_refs":["wiki:space:root"],"source_refs":["source:spec"]}
            ]}"#,
        )
        .unwrap();
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let material = vec![SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse("source:spec").unwrap(),
                revision: SourceRevision::parse("sha256:spec").unwrap(),
                title: "Auth spec".into(),
                tags: vec!["auth".into()],
                visibility: SourceVisibility::Team,
                owners: Vec::new(),
                media_type: "text/markdown".into(),
                locator: None,
                metadata: BTreeMap::new(),
            },
            body: "Authentication rotates session tokens.".into(),
        }];
        let mut sources = NativeSourcePoolProvider::new();
        sources.rebuild(&material).unwrap();
        let subject = ResourceRef::parse("wiki:node:auth").unwrap();
        let address = KnowledgeAddress::Wiki(subject.clone());
        let app = aikit_core::KnowledgeApplication::new(FamiliarityContext {
            project: Some(ResourceRef::parse("project:demo").unwrap()),
            actor: None,
            agency: None,
            focus: None,
        })
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);
        let view = KnowledgeOperations::relations(&app, &address, 2, 256, 512).unwrap();
        assert!(
            view.nodes
                .iter()
                .any(|node| node.resource.as_str() == "source:spec"),
            "the fixture's own Knowledge application must produce a real neighbourhood"
        );

        let mut backend = FakeBackend {
            context: test_context(),
            view: resolve_fixture(Vec::new()).1,
            capsules: BTreeMap::new(),
            knowledge: Some((subject.clone(), address, view.clone())),
        };
        let service = ApplicationService::new(&mut backend);
        let relation = service.relations(&subject).unwrap();

        assert_eq!(relation.subject, subject);
        assert_eq!(
            relation.view, view,
            "the typed view must pass through unchanged"
        );
        assert!(
            relation
                .view
                .nodes
                .iter()
                .any(|node| node.resource.as_str() == "source:spec"),
            "before this fix, list/tree/graph could see none of this: only an \
             untyped `related` key (absent from KnowledgeRelationView's own \
             serialisation) was ever scraped"
        );
        assert!(!relation.view.edges.is_empty());
        assert_eq!(
            relation.value["nodes"].as_array().map(Vec::len),
            Some(view.nodes.len()),
            "the untyped value must stay in parity with the typed view for Inspector detail"
        );
    }

    /// The resolver fallback (no Knowledge address) must build a genuine,
    /// honestly-attributed [`KnowledgeRelationView`] from the package resolver's
    /// "often used with" edges — never a parallel untyped relation ontology.
    #[test]
    fn resolver_fallback_builds_a_valid_derived_relation_view() {
        let subject_id = "skill/alpha";
        let related_id = "skill/beta";
        let alpha = skill_capsule(subject_id, &[related_id]);
        let beta = skill_capsule(related_id, &[]);
        let (capsules, view) = resolve_fixture(vec![alpha, beta]);

        let mut backend = FakeBackend {
            context: test_context(),
            view,
            capsules,
            knowledge: None,
        };
        let service = ApplicationService::new(&mut backend);
        let subject = ResourceRef::parse(subject_id).unwrap();
        let relation = service.relations(&subject).unwrap();

        assert_eq!(relation.view.query.focus, subject);
        assert!(relation
            .view
            .nodes
            .iter()
            .any(|node| node.resource.as_str() == related_id));
        let edge = relation
            .view
            .edges
            .iter()
            .find(|edge| edge.to.as_str() == related_id)
            .expect("resolver fallback must expose the related capsule as a typed edge");
        assert_eq!(edge.origin.authority, SourceAuthority::Derived);
        assert_eq!(
            edge.origin.lens.as_deref(),
            Some("resolver"),
            "the fallback must be honest about where the edge came from"
        );
        assert!(
            relation
                .view
                .nodes
                .iter()
                .any(|node| node.resource == edge.from),
            "push_edge already enforces this, but the view must never carry a \
             dangling endpoint"
        );
        assert!(relation
            .view
            .nodes
            .iter()
            .any(|node| node.resource == edge.to));
    }
}
