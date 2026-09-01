#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    source = target.read_text()
    if source.count(old) < count:
        raise SystemExit(f"missing A6 patch anchor in {path}: {old[:140]!r}")
    target.write_text(source.replace(old, new, count))


# ---------------------------------------------------------------------------
# Core Action semantic qualification over the existing ActionRef/ResolvePath.
# ---------------------------------------------------------------------------
op = "crates/aikit-core/src/resource/operative.rs"
patch(
    op,
    '''use super::{ResourceIndex, ResourceKind, ResourceRecord, ResourceRef};\n''',
    '''use super::{\n    OwnerRef, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, ResourceSource,\n};\n''',
)
patch(
    op,
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ResolvedActionCandidate {\n    pub action: ActionRef,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub horizon: Option<AddressHorizon>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub relation: Option<RelationOp>,\n    /// ContextResolution remains the authority for whether this Action is\n    /// actually available for invocation in the current world.\n    pub available_in_context: bool,\n}\n''',
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ResolvedActionCandidate {\n    pub action: ActionRef,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub horizon: Option<AddressHorizon>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub relation: Option<RelationOp>,\n    /// ContextResolution remains the authority for whether this Action is\n    /// actually available for invocation in the current world.\n    pub available_in_context: bool,\n}\n\n/// Contextual semantic qualification of one real ActionRef. The profile is\n/// derived from current ResolvePath + Resource/subject evidence; it is not a new\n/// Action registry and it confers no execution authority.\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ActionSemanticProfile {\n    pub action_ref: ActionRef,\n    #[serde(default)]\n    pub relation_affinities: BTreeSet<RelationOp>,\n    #[serde(default)]\n    pub horizon_affinities: BTreeSet<AddressHorizon>,\n    #[serde(default)]\n    pub subject_ref_kinds: BTreeSet<ResourceKind>,\n    #[serde(default)]\n    pub method_relations: Vec<ResourceRef>,\n    #[serde(default)]\n    pub focus_relations: Vec<String>,\n    #[serde(default)]\n    pub expected_return_forms: Vec<String>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub native_owner: Option<OwnerRef>,\n    #[serde(default)]\n    pub provenance: Vec<ResourceSource>,\n}\n''',
)
patch(
    op,
    '''pub fn resolve_action_candidates(\n    path: &ResolvePath,\n    resources: &dyn ResourceIndex,\n    context: &ContextResolution,\n) -> Vec<ResolvedActionCandidate> {\n''',
    '''pub fn resolve_action_candidates(\n    path: &ResolvePath,\n    resources: &dyn ResourceIndex,\n    context: &ContextResolution,\n) -> Vec<ResolvedActionCandidate> {\n''',
)
# Insert qualification after the candidate resolver.
anchor = '''        .collect()\n}\n\n/// Build the thin sixfold disclosure object from real currently-addressable refs.\n'''
insert = '''        .collect()\n}\n\n/// Describe why one syntax-resolved Action is meaningful for the selected subject.\n/// Relation affinities are the operators that actually participated in this\n/// qualification; horizon affinities come from the general resource classifier.\n/// Method and Focus relations are likewise current path/context evidence rather\n/// than invented declarations. Expected return forms are native-owner annotations.\npub fn action_semantic_profile(\n    candidate: &ResolvedActionCandidate,\n    path: &ResolvePath,\n    subject: &ResourceRef,\n    focus: Option<&str>,\n    resources: &dyn ResourceIndex,\n) -> Result<ActionSemanticProfile> {\n    let action_record = resources.resource(candidate.action.resource()).ok_or_else(|| {\n        AikitError::new(\n            "resolve.action_profile_missing",\n            format!("Action {} disappeared during semantic qualification", candidate.action.resource()),\n        )\n    })?;\n    let subject_record = resources.resource(subject).ok_or_else(|| {\n        AikitError::new(\n            "resolve.action_subject_missing",\n            format!("Action subject {subject} is absent from the Resource field"),\n        )\n    })?;\n\n    let relation_affinities = path\n        .steps\n        .iter()\n        .filter_map(|step| match step {\n            ResolvePathStep::Relation { op } => Some(*op),\n            _ => None,\n        })\n        .collect::<BTreeSet<_>>();\n    let horizon_affinities = horizons_for_resource(action_record);\n    let method_relations = path\n        .candidates\n        .iter()\n        .filter(|resolved| resolved.kind == ResourceKind::Method)\n        .map(|resolved| resolved.resource.clone())\n        .collect::<BTreeSet<_>>()\n        .into_iter()\n        .collect();\n    let focus_relations = focus\n        .filter(|value| !value.trim().is_empty())\n        .map(|value| vec![value.to_string()])\n        .unwrap_or_default();\n    let expected_return_forms = action_record\n        .descriptor\n        .annotations\n        .get("action.expected-return-forms")\n        .map(|value| {\n            value\n                .split(',')\n                .map(str::trim)\n                .filter(|value| !value.is_empty())\n                .map(ToString::to_string)\n                .collect()\n        })\n        .unwrap_or_default();\n\n    Ok(ActionSemanticProfile {\n        action_ref: candidate.action.clone(),\n        relation_affinities,\n        horizon_affinities,\n        subject_ref_kinds: BTreeSet::from([subject_record.descriptor.kind]),\n        method_relations,\n        focus_relations,\n        expected_return_forms,\n        native_owner: action_record.descriptor.owner.clone(),\n        provenance: action_record.descriptor.sources.clone(),\n    })\n}\n\n/// Build the thin sixfold disclosure object from real currently-addressable refs.\n'''
patch(op, anchor, insert)

mod_rs = "crates/aikit-core/src/resource/mod.rs"
patch(
    mod_rs,
    '''    horizons_for_resource, parse_or_search_expression, parse_resolve_expression,\n    resolve_action_candidates, resolve_expression, resolve_path_identity, resolve_search,\n    six_horizon_disclosure, ActionRef, AddressHorizon, RelationOp, ResolveCandidate,\n    ResolveExpression, ResolvePath, ResolvePathStep, ResolvedActionCandidate,\n    OPERATIVE_SYNTAX_VERSION,\n''',
    '''    action_semantic_profile, horizons_for_resource, parse_or_search_expression,\n    parse_resolve_expression, resolve_action_candidates, resolve_expression, resolve_path_identity,\n    resolve_search, six_horizon_disclosure, ActionRef, ActionSemanticProfile, AddressHorizon,\n    RelationOp, ResolveCandidate, ResolveExpression, ResolvePath, ResolvePathStep,\n    ResolvedActionCandidate, OPERATIVE_SYNTAX_VERSION,\n''',
)

# ResourceSearchIndex can recover every contextual subject relation for one
# canonical Action without manufacturing per-subject identities.
search = "crates/aikit-core/src/resource/search.rs"
patch(
    search,
    '''    pub fn actions_for(&self, subject: &ResourceRef) -> Vec<&ContextualActionDescriptor> {\n        self.actions\n            .iter()\n            .filter_map(|((candidate, _), action)| (candidate == subject).then_some(action))\n            .collect()\n    }\n\n    pub fn len(&self) -> usize {\n''',
    '''    pub fn actions_for(&self, subject: &ResourceRef) -> Vec<&ContextualActionDescriptor> {\n        self.actions\n            .iter()\n            .filter_map(|((candidate, _), action)| (candidate == subject).then_some(action))\n            .collect()\n    }\n\n    pub fn subjects_for_action(&self, action: &ResourceRef) -> Vec<&ContextualActionDescriptor> {\n        self.actions\n            .values()\n            .filter(|contextual| &contextual.action == action)\n            .collect()\n    }\n\n    pub fn len(&self) -> usize {\n''',
)

# ---------------------------------------------------------------------------
# Native Action records carry real application ownership/source availability.
# ContextResolution can therefore qualify them as available instead of syntax
# silently granting authority merely because an ActionRef exists.
# ---------------------------------------------------------------------------
backend = "crates/aikit-tui/src/backend.rs"
patch(
    backend,
    '''    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,\n    ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,\n};\n''',
    '''    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,\n    OwnerRef, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,\n    ResourceSource, SourceAuthority, SourceRef, SourceState,\n};\n''',
)
patch(
    backend,
    '''fn session_space_store(home: Option<&AikitHome>) -> Result<SessionSpaceApplicationStore> {\n''',
    '''fn native_application_action_record(\n    id: ResourceRef,\n    name: &str,\n    description: &str,\n    expected_return_forms: &str,\n) -> ResourceRecord {\n    let mut descriptor = ResourceDescriptor::new(id, ResourceKind::Action, name, description);\n    descriptor.owner = Some(\n        OwnerRef::parse("aikit/application-service")\n            .expect("static native Action owner reference must be valid"),\n    );\n    descriptor.sources.push(ResourceSource {\n        source: SourceRef::parse("source/aikit/application-service")\n            .expect("static native Action source reference must be valid"),\n        authority: Some(SourceAuthority::Authored),\n        revision: None,\n        locator: None,\n        state: SourceState::Available,\n    });\n    descriptor.annotations.insert(\n        "action.expected-return-forms".into(),\n        expected_return_forms.into(),\n    );\n    ResourceRecord::new(descriptor)\n}\n\nfn session_space_store(home: Option<&AikitHome>) -> Result<SessionSpaceApplicationStore> {\n''',
)
patch(
    backend,
    '''        index.insert_resource(\n            ResourceRecord::new(ResourceDescriptor::new(\n                open_project.clone(),\n                ResourceKind::Action,\n                "Open project",\n                "enter the selected Project workspace",\n            )),\n            Vec::new(),\n        );\n        index.insert_resource(\n            ResourceRecord::new(ResourceDescriptor::new(\n                explain_capability.clone(),\n                ResourceKind::Action,\n                "Explain capability",\n                "show why this Capability has its current resolved state",\n            )),\n            Vec::new(),\n        );\n        index.insert_resource(\n            ResourceRecord::new(ResourceDescriptor::new(\n                toggle_capability.clone(),\n                ResourceKind::Action,\n                "Toggle capability",\n                "stage an enable/disable change at the selected mutation scope",\n            )),\n            Vec::new(),\n        );\n''',
    '''        index.insert_resource(\n            native_application_action_record(\n                open_project.clone(),\n                "Open project",\n                "enter the selected Project workspace",\n                "opened",\n            ),\n            Vec::new(),\n        );\n        index.insert_resource(\n            native_application_action_record(\n                explain_capability.clone(),\n                "Explain capability",\n                "show why this Capability has its current resolved state",\n                "explanation",\n            ),\n            Vec::new(),\n        );\n        index.insert_resource(\n            native_application_action_record(\n                toggle_capability.clone(),\n                "Toggle capability",\n                "stage an enable/disable change at the selected mutation scope",\n                "staged-change",\n            ),\n            Vec::new(),\n        );\n''',
)

explain_actions = "crates/aikit-core/src/explain_history_actions.rs"
patch(
    explain_actions,
    '''    ActionStageability, ContextualActionDescriptor, ResourceDescriptor, ResourceIndex,\n    ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,\n};\n''',
    '''    ActionStageability, ContextualActionDescriptor, OwnerRef, ResourceDescriptor, ResourceIndex,\n    ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex, ResourceSource,\n    SourceAuthority, SourceRef, SourceState,\n};\n''',
)
patch(
    explain_actions,
    '''pub fn explain_history_action_resources() -> Result<[ResourceRecord; 2]> {\n    Ok([\n        ResourceRecord::new(ResourceDescriptor::new(\n            ResourceRef::parse(EXPLAIN_ACTION_REF)?,\n            ResourceKind::Action,\n            "Explain",\n            "Explain why the selected Resource is present, unavailable, degraded, staged, projected or learned-easy from owner-held evidence.",\n        )),\n        ResourceRecord::new(ResourceDescriptor::new(\n            ResourceRef::parse(HISTORY_ACTION_REF)?,\n            ResourceKind::Action,\n            "History",\n            "Read evidence-bearing recent, familiar, changed and recoverable history for the selected Resource without creating a second history authority.",\n        )),\n    ])\n}\n''',
    '''fn native_explain_history_action_record(\n    id: ResourceRef,\n    name: &str,\n    description: &str,\n    expected_return_forms: &str,\n) -> Result<ResourceRecord> {\n    let mut descriptor = ResourceDescriptor::new(id, ResourceKind::Action, name, description);\n    descriptor.owner = Some(OwnerRef::parse("aikit/explain-history")?);\n    descriptor.sources.push(ResourceSource {\n        source: SourceRef::parse("source/aikit-core/explain-history-actions")?,\n        authority: Some(SourceAuthority::Authored),\n        revision: None,\n        locator: None,\n        state: SourceState::Available,\n    });\n    descriptor.annotations.insert(\n        "action.expected-return-forms".into(),\n        expected_return_forms.into(),\n    );\n    Ok(ResourceRecord::new(descriptor))\n}\n\npub fn explain_history_action_resources() -> Result<[ResourceRecord; 2]> {\n    Ok([\n        native_explain_history_action_record(\n            ResourceRef::parse(EXPLAIN_ACTION_REF)?,\n            "Explain",\n            "Explain why the selected Resource is present, unavailable, degraded, staged, projected or learned-easy from owner-held evidence.",\n            "explanation",\n        )?,\n        native_explain_history_action_record(\n            ResourceRef::parse(HISTORY_ACTION_REF)?,\n            "History",\n            "Read evidence-bearing recent, familiar, changed and recoverable history for the selected Resource without creating a second history authority.",\n            "history-evidence",\n        )?,\n    ])\n}\n''',
)

# ---------------------------------------------------------------------------
# Application seam: Resolve -> Action qualification -> existing invocation.
# The resolved receipt carries the actual path and records that path in #29.
# ---------------------------------------------------------------------------
application = "crates/aikit-tui/src/application.rs"
patch(
    application,
    '''    search_contextual_actions, ContextualActionDescriptor, ResolveExpression, ResolvePath,\n    ResourceKind, ResourceRef,\n};\n''',
    '''    search_contextual_actions, ActionSemanticProfile, ContextualActionDescriptor,\n    ResolveExpression, ResolvePath, ResolvedActionCandidate, ResourceKind, ResourceRef,\n};\n''',
)
patch(
    application,
    '''pub struct ResolvedSearchReadModel {\n    pub expression: ResolveExpression,\n    pub path: ResolvePath,\n    pub resources: ResourceListReadModel,\n}\n\nimpl ResourceListReadModel {\n''',
    '''pub struct ResolvedSearchReadModel {\n    pub expression: ResolveExpression,\n    pub path: ResolvePath,\n    pub resources: ResourceListReadModel,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ResolvedActionReadModel {\n    pub expression: ResolveExpression,\n    pub path: ResolvePath,\n    pub candidate: ResolvedActionCandidate,\n    pub semantic_profile: ActionSemanticProfile,\n    pub action: ContextualActionDescriptor,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct ActionInvocationReceipt {\n    pub action: ResourceRef,\n    pub subject: ResourceRef,\n    pub observed_path: ResolvePath,\n    pub outcome: ActionOutcome,\n}\n\nimpl ResourceListReadModel {\n''',
)

service = "crates/aikit-tui/src/application_service.rs"
patch(
    service,
    '''use aikit_core::composition_mutation::{changed_ground, CompositionBasis};\n''',
    '''use aikit_core::application_context::application_context_resolution;\nuse aikit_core::composition_mutation::{changed_ground, CompositionBasis};\n''',
)
patch(
    service,
    '''    parse_or_search_expression, resolve_expression, resolve_path_identity,\n    ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass, ResolveExpression,\n    ResourceDescriptor, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,\n    ResourceSearchIndex,\n};\n''',
    '''    action_semantic_profile, parse_or_search_expression, resolve_action_candidates,\n    resolve_expression, resolve_path_identity, ContextualActionDescriptor, NavigationEvidence,\n    NavigationEvidenceClass, ResolveExpression, ResolvePath, ResolvePathStep, ResourceDescriptor,\n    ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,\n};\n''',
)
patch(
    service,
    '''    FamiliarityObservation, FamiliarityUse, ForgetScope, KnowledgeAddress, KnowledgeContextPack,\n    KnowledgeProviderStatus, KnowledgeReading, KnowledgeRoute, KnowledgeSources, Result,\n    DEFAULT_FAMILIARITY_HALF_LIFE_MS, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,\n};\n''',
    '''    FamiliarityObservation, FamiliarityUse, ForgetScope, KnowledgeAddress, KnowledgeContextPack,\n    KnowledgeProviderStatus, KnowledgeReading, KnowledgeRoute, KnowledgeSources,\n    OperativePathEvidence, Result, RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS,\n    EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,\n};\n''',
)
patch(
    service,
    '''    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,\n    RelationReadModel, ResolvedSearchReadModel, ResourceListItem, ResourceListReadModel,\n    StagedChanges, TuiApplicationService,\n};\n''',
    '''    ActionInvocationReceipt, ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview,\n    HistoryEntry, RelationReadModel, ResolvedActionReadModel, ResolvedSearchReadModel,\n    ResourceListItem, ResourceListReadModel, StagedChanges, TuiApplicationService,\n};\n''',
)
patch(
    service,
    '''    /// Resolve the deliberately retained package compatibility identity for a\n''',
    '''    /// Discover and qualify one canonical Action for the explicit selected subject.\n    /// This is side-effect free: even an `=` expression stops at qualification.\n    /// ContextResolution, not punctuation, decides whether invocation is available.\n    pub fn resolve_action_for_subject(\n        &self,\n        query: &str,\n        subject: &ResourceRef,\n    ) -> Result<ResolvedActionReadModel> {\n        let resolved = self.resolve_search(query)?;\n        let index = self.navigation_index()?;\n        let scope_layers = self.backend.scope_layers().unwrap_or(&[]);\n        let context = application_context_resolution(\n            self.backend.context(),\n            self.backend.view(),\n            scope_layers,\n            &index,\n        )?;\n        let candidates = resolve_action_candidates(&resolved.path, &index, &context);\n\n        for candidate in candidates {\n            let Some(action) = index\n                .actions_for(subject)\n                .into_iter()\n                .find(|contextual| contextual.action == *candidate.action.resource())\n                .cloned()\n            else {\n                continue;\n            };\n            let semantic_profile = action_semantic_profile(\n                &candidate,\n                &resolved.path,\n                subject,\n                self.backend.context().task.as_deref(),\n                &index,\n            )?;\n            return Ok(ResolvedActionReadModel {\n                expression: resolved.expression,\n                path: resolved.path,\n                candidate,\n                semantic_profile,\n                action,\n            });\n        }\n\n        Err(AikitError::new(\n            "application.resolve_action_no_contextual_candidate",\n            format!(\n                "Resolve expression `{query}` did not produce a canonical Action applicable to {subject}"\n            ),\n        ))\n    }\n\n    /// Cross the already-existing native invocation boundary with a previously\n    /// qualified Action. The receipt returns the exact observed ResolvePath and\n    /// #29 records path accessibility beside ordinary destination familiarity.\n    pub fn invoke_resolved_action(\n        &mut self,\n        resolved: &ResolvedActionReadModel,\n    ) -> Result<ActionInvocationReceipt> {\n        if !resolved.candidate.available_in_context {\n            return Err(AikitError::new(\n                "application.resolve_action_unavailable",\n                format!(\n                    "Action {} is not available in the current ContextResolution",\n                    resolved.action.action\n                ),\n            ));\n        }\n        let outcome = <Self as TuiApplicationService>::invoke_action(self, &resolved.action)?;\n        self.record_resolve_path_use(&resolved.path, resolved)?;\n        Ok(ActionInvocationReceipt {\n            action: resolved.action.action.clone(),\n            subject: resolved.action.subject.clone(),\n            observed_path: resolved.path.clone(),\n            outcome,\n        })\n    }\n\n    /// Resolve the deliberately retained package compatibility identity for a\n''',
)
patch(
    service,
    '''    fn record_action_use(&mut self, action: &ContextualActionDescriptor) -> Result<()> {\n        let observation = FamiliarityObservation::destination(\n            EventId::generate().as_str().to_string(),\n            action.subject.clone(),\n            familiarity_context(self.backend.context()),\n            now_ms(),\n        )\n        .via_action(action.action.clone())\n        .from_surface(\n            ResourceRef::parse("surface/aikit/tui")\n                .expect("static V2 TUI surface ResourceRef must be valid"),\n        );\n        self.backend.record_familiarity(observation)\n    }\n}\n''',
    '''    fn record_action_use(&mut self, action: &ContextualActionDescriptor) -> Result<()> {\n        let observation = FamiliarityObservation::destination(\n            EventId::generate().as_str().to_string(),\n            action.subject.clone(),\n            familiarity_context(self.backend.context()),\n            now_ms(),\n        )\n        .via_action(action.action.clone())\n        .from_surface(\n            ResourceRef::parse("surface/aikit/tui")\n                .expect("static V2 TUI surface ResourceRef must be valid"),\n        );\n        self.backend.record_familiarity(observation)\n    }\n\n    fn record_resolve_path_use(\n        &mut self,\n        path: &ResolvePath,\n        resolved: &ResolvedActionReadModel,\n    ) -> Result<()> {\n        let surface = ResourceRef::parse("surface/aikit/tui")\n            .expect("static V2 TUI surface ResourceRef must be valid");\n        let mut relation_ops = path\n            .steps\n            .iter()\n            .filter_map(|step| match step {\n                ResolvePathStep::Relation { op } => Some(*op),\n                _ => None,\n            })\n            .collect::<Vec<_>>();\n        relation_ops.sort();\n        relation_ops.dedup();\n        let mut horizons = path\n            .steps\n            .iter()\n            .filter_map(|step| match step {\n                ResolvePathStep::Address { horizon, .. } => *horizon,\n                _ => None,\n            })\n            .collect::<Vec<_>>();\n        horizons.sort();\n        horizons.dedup();\n\n        let steps = vec![\n            RouteStepEvidence {\n                resource: resolved.action.action.clone(),\n                provider: None,\n                lens: None,\n                revision: None,\n            },\n            RouteStepEvidence {\n                resource: resolved.action.subject.clone(),\n                provider: None,\n                lens: None,\n                revision: None,\n            },\n        ];\n        let observation = FamiliarityObservation::resolve_path(\n            EventId::generate().as_str().to_string(),\n            None,\n            resolved.action.subject.clone(),\n            steps,\n            OperativePathEvidence {\n                path_identity: path.identity.clone(),\n                expression: path.expression.clone(),\n                relation_ops,\n                horizons,\n                method: resolved.semantic_profile.method_relations.first().cloned(),\n                action: Some(resolved.action.action.clone()),\n                surface: Some(surface.clone()),\n                activity: None,\n                return_ref: None,\n            },\n            familiarity_context(self.backend.context()),\n            now_ms(),\n        )?\n        .via_action(resolved.action.action.clone())\n        .from_surface(surface);\n        self.backend.record_familiarity(observation)\n    }\n}\n''',
)

# ---------------------------------------------------------------------------
# Test backend uses the same #29 store so the application acceptance can observe
# both destination and ResolvePath evidence produced by the real invocation seam.
# ---------------------------------------------------------------------------
common = "crates/aikit-tui/tests/common/mod.rs"
patch(
    common,
    '''use aikit_core::{AikitError, Result};\n''',
    '''use aikit_core::{AikitError, FamiliarityObservation, FamiliarityStore, Result};\n''',
)
patch(
    common,
    '''    pub promoted: Vec<CapsuleId>,\n}\n''',
    '''    pub promoted: Vec<CapsuleId>,\n    pub familiarity: FamiliarityStore,\n}\n''',
)
patch(
    common,
    '''            promoted: Vec::new(),\n        };\n''',
    '''            promoted: Vec::new(),\n            familiarity: FamiliarityStore::new(),\n        };\n''',
)
patch(
    common,
    '''    fn documents(&self) -> Vec<SearchDoc> {\n''',
    '''    fn familiarity(&self) -> Result<Option<FamiliarityStore>> {\n        Ok(Some(self.familiarity.clone()))\n    }\n\n    fn record_familiarity(&mut self, observation: FamiliarityObservation) -> Result<()> {\n        self.familiarity.record(observation)\n    }\n\n    fn documents(&self) -> Vec<SearchDoc> {\n''',
)

acceptance = "crates/aikit-tui/tests/application_service_backend_v2.rs"
patch(
    acceptance,
    '''use aikit_core::TargetId;\n''',
    '''use aikit_core::{FamiliarityUse, ResourceRef, TargetId};\n''',
)
patch(
    acceptance,
    '''#[test]\nfn production_application_never_calls_restart_only_effect_live() {\n''',
    '''#[test]\nfn operative_action_is_qualified_then_invoked_through_the_existing_application_boundary() {\n    let dir = tempfile::tempdir().unwrap();\n    let mut backend = Fixture::new(dir.path(), vec![script("script/ops/deploy")]);\n    let subject = ResourceRef::parse("script/ops/deploy").unwrap();\n\n    let resolved = {\n        let service = ApplicationService::new(&mut backend);\n        service\n            .resolve_action_for_subject("+ @5 action/capability/explain", &subject)\n            .unwrap()\n    };\n\n    assert_eq!(resolved.action.action.as_str(), "action/capability/explain");\n    assert_eq!(resolved.action.subject, subject);\n    assert!(resolved.candidate.available_in_context);\n    assert!(resolved\n        .semantic_profile\n        .relation_affinities\n        .contains(&aikit_core::resource::RelationOp::Affirm));\n    assert!(resolved\n        .semantic_profile\n        .horizon_affinities\n        .contains(&aikit_core::resource::AddressHorizon::H5));\n    assert!(resolved\n        .semantic_profile\n        .subject_ref_kinds\n        .contains(&aikit_core::resource::ResourceKind::Capability));\n    assert_eq!(\n        resolved.semantic_profile.expected_return_forms,\n        vec!["explanation"]\n    );\n    assert!(resolved.semantic_profile.native_owner.is_some());\n    assert!(!resolved.semantic_profile.provenance.is_empty());\n\n    let receipt = {\n        let mut service = ApplicationService::new(&mut backend);\n        service.invoke_resolved_action(&resolved).unwrap()\n    };\n    assert_eq!(receipt.action.as_str(), "action/capability/explain");\n    assert_eq!(receipt.subject, subject);\n    assert_eq!(receipt.observed_path.identity, resolved.path.identity);\n    assert!(receipt.outcome.summary().contains("script/ops/deploy"));\n\n    let snapshot = backend.familiarity.snapshot();\n    assert!(snapshot.observations.iter().any(|observation| {\n        matches!(\n            &observation.use_kind,\n            FamiliarityUse::ResolvePath { operative, .. }\n                if operative.path_identity == resolved.path.identity\n                    && operative.action.as_ref() == Some(&resolved.action.action)\n        )\n    }));\n}\n\n#[test]\nfn express_operator_qualifies_action_without_crossing_the_invocation_boundary() {\n    let dir = tempfile::tempdir().unwrap();\n    let mut backend = Fixture::new(dir.path(), vec![script("script/ops/deploy")]);\n    let subject = ResourceRef::parse("script/ops/deploy").unwrap();\n    let before = backend.familiarity.len();\n\n    let resolved = {\n        let service = ApplicationService::new(&mut backend);\n        service\n            .resolve_action_for_subject(\n                "@5 action/capability/explain = @5 action/capability/explain",\n                &subject,\n            )\n            .unwrap()\n    };\n\n    assert_eq!(resolved.action.action.as_str(), "action/capability/explain");\n    assert!(resolved\n        .semantic_profile\n        .relation_affinities\n        .contains(&aikit_core::resource::RelationOp::Express));\n    assert_eq!(\n        backend.familiarity.len(),\n        before,\n        "Resolve/`=` must remain side-effect free until native invocation"\n    );\n}\n\n#[test]\nfn production_application_never_calls_restart_only_effect_live() {\n''',
)
