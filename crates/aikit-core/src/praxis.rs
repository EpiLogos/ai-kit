//! Operational binding of selected Methods to an already-resolved AIKit Context.
//!
//! ContextResolution remains the owner of what is available/operative. A Method
//! is selected around a Focus only after that resolution exists; selection never
//! grants trust, capability, Action authority, or SkillSet precedence.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context_resolution::ContextResolution;
use crate::explain_history::{
    EvidenceProvenance, ExplainEvidence, ExplainFact, HistoryEvidence, HistoryKind,
    HistoryRecoverability, EXPLAIN_HISTORY_VERSION,
};
use crate::method::{resolve_method, Method, MethodResolution};
use crate::resource::{ResourceIndex, ResourceRef, SourceAuthority};

pub const PRAXIS_RESOLUTION_VERSION: &str = "aikit.praxis-resolution/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedMethod {
    pub method: ResourceRef,
    pub resolution: MethodResolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PraxisResolution {
    pub version: String,
    /// Exact version of the operational ContextResolution under which this Method
    /// selection was made. The full ContextResolution remains the owner receipt.
    pub context_resolution_version: String,
    #[serde(default)]
    pub focus: Vec<ResourceRef>,
    #[serde(default)]
    pub methods: Vec<SelectedMethod>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Resolve explicitly selected Methods under an existing ContextResolution.
///
/// `available_methods` are source-loaded Method bodies keyed by stable MethodRef.
/// The V2 ResourceIndex is used only to resolve their referenced resources. No
/// Method member is enabled by this function; normal Profile/scope/ContextResolution
/// and Action authority continue to decide operativity.
pub fn resolve_praxis(
    context: &ContextResolution,
    resources: &dyn ResourceIndex,
    available_methods: &[Method],
    selected: &[ResourceRef],
    focus: &[ResourceRef],
) -> PraxisResolution {
    let mut methods = Vec::new();
    let mut warnings = Vec::new();

    for reference in selected {
        let Some(method) = available_methods
            .iter()
            .find(|method| &method.id == reference)
        else {
            warnings.push(format!(
                "selected Method {reference} is absent from the source-loaded Method field"
            ));
            continue;
        };
        match resolve_method(method, resources) {
            Ok(resolution) => {
                warnings.extend(
                    resolution
                        .warnings
                        .iter()
                        .map(|warning| format!("Method {reference}: {warning}")),
                );
                methods.push(SelectedMethod {
                    method: reference.clone(),
                    resolution,
                });
            }
            Err(error) => warnings.push(format!(
                "Method {reference} is invalid under this ContextResolution: {}",
                error.message()
            )),
        }
    }

    PraxisResolution {
        version: PRAXIS_RESOLUTION_VERSION.into(),
        context_resolution_version: context.version.clone(),
        focus: focus.to_vec(),
        methods,
        warnings,
    }
}

/// Explain the source/resolution condition of each selected Method without
/// promoting Method membership into activation or authority.
pub fn explain_praxis(praxis: &PraxisResolution) -> Vec<ExplainEvidence> {
    praxis
        .methods
        .iter()
        .map(|selected| {
            let resolution = &selected.resolution;
            let source_ref = ResourceRef::parse(resolution.source.as_str()).ok();
            let mut facts = vec![
                ExplainFact {
                    relation: "method-source".into(),
                    authority: None,
                    summary: format!("Method source is {}", resolution.source),
                    canonical_refs: source_ref.clone().into_iter().collect(),
                    provenance: vec![EvidenceProvenance {
                        source: source_ref,
                        revision: resolution.revision.as_ref().map(ToString::to_string),
                        ..EvidenceProvenance::default()
                    }],
                },
                ExplainFact {
                    relation: "context-resolution".into(),
                    authority: Some(SourceAuthority::Derived),
                    summary: format!(
                        "selected under ContextResolution {}",
                        praxis.context_resolution_version
                    ),
                    canonical_refs: praxis.focus.clone(),
                    provenance: Vec::new(),
                },
            ];

            for reference in resolved_refs(resolution) {
                facts.push(ExplainFact {
                    relation: "method-member".into(),
                    authority: Some(SourceAuthority::Derived),
                    summary: format!("Method relates resource {reference}"),
                    canonical_refs: vec![reference],
                    provenance: Vec::new(),
                });
            }
            for overlay in &resolution.overlays {
                facts.push(ExplainFact {
                    relation: "usage-overlay".into(),
                    authority: Some(SourceAuthority::Derived),
                    summary: format!(
                        "Skill {} adapted at {} scope with digest {}",
                        overlay.skill, overlay.scope, overlay.digest
                    ),
                    canonical_refs: vec![overlay.skill.clone()],
                    provenance: overlay
                        .source
                        .as_ref()
                        .and_then(|source| ResourceRef::parse(source.as_str()).ok())
                        .map(|source| EvidenceProvenance {
                            source: Some(source),
                            revision: Some(overlay.digest.clone()),
                            ..EvidenceProvenance::default()
                        })
                        .into_iter()
                        .collect(),
                });
            }

            ExplainEvidence {
                schema: EXPLAIN_HISTORY_VERSION.into(),
                subject: selected.method.clone(),
                facts,
            }
        })
        .collect()
}

/// Emit inspectable History evidence for the praxis input condition. This is not
/// a Run or fitness judgement. Callers append operation/Factory/Actuation return
/// evidence from those owners rather than asking AIKit to synthesize it.
pub fn praxis_history_evidence(praxis: &PraxisResolution) -> Vec<HistoryEvidence> {
    praxis
        .methods
        .iter()
        .map(|selected| {
            let resolution = &selected.resolution;
            let mut canonical_refs = BTreeSet::from([selected.method.clone()]);
            canonical_refs.extend(praxis.focus.iter().cloned());
            canonical_refs.extend(resolved_refs(resolution));
            canonical_refs.extend(
                resolution
                    .overlays
                    .iter()
                    .map(|overlay| overlay.skill.clone()),
            );

            let source = ResourceRef::parse(resolution.source.as_str()).ok();
            if let Some(source) = &source {
                canonical_refs.insert(source.clone());
            }

            let mut details = BTreeMap::new();
            details.insert(
                "contextResolutionVersion".into(),
                praxis.context_resolution_version.clone(),
            );
            details.insert("praxisResolutionVersion".into(), praxis.version.clone());
            details.insert("source".into(), resolution.source.to_string());
            details.insert(
                "sourceRevision".into(),
                resolution
                    .revision
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unversioned".into()),
            );
            details.insert(
                "usageOverlays".into(),
                resolution
                    .overlays
                    .iter()
                    .map(|overlay| {
                        format!("{}@{}#{}", overlay.skill, overlay.scope, overlay.digest)
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            );
            details.insert(
                "expectedReturns".into(),
                resolution.expected_return_forms.join(","),
            );
            details.insert("warnings".into(), resolution.warnings.len().to_string());

            HistoryEvidence {
                schema: EXPLAIN_HISTORY_VERSION.into(),
                id: format!(
                    "praxis:{}:{}",
                    selected.method, praxis.context_resolution_version
                ),
                // `Recent` is intentionally used as the generic evidence class;
                // Method does not need a parallel History ontology merely to be
                // attributable in the shared read model.
                kind: HistoryKind::Recent,
                subject: selected.method.clone(),
                authorities: vec![SourceAuthority::Derived],
                occurred_at_unix_ms: None,
                summary: format!(
                    "Method {} selected under {} with {} member ref{} and {} overlay{}",
                    selected.method,
                    praxis.context_resolution_version,
                    resolved_refs(resolution).len(),
                    if resolved_refs(resolution).len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    resolution.overlays.len(),
                    if resolution.overlays.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ),
                canonical_refs: canonical_refs.into_iter().collect(),
                provenance: vec![EvidenceProvenance {
                    source,
                    revision: resolution.revision.as_ref().map(ToString::to_string),
                    ..EvidenceProvenance::default()
                }],
                recoverability: HistoryRecoverability::InspectOnly,
                details,
            }
        })
        .collect()
}

fn resolved_refs(resolution: &MethodResolution) -> Vec<ResourceRef> {
    resolution
        .focus
        .iter()
        .chain(&resolution.project_domain)
        .chain(&resolution.skills)
        .chain(&resolution.actions)
        .chain(&resolution.capabilities)
        .chain(&resolution.context_sources)
        .chain(&resolution.verification)
        .map(|resolved| resolved.reference.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::method::{MethodSkillRef, UsageOverlayRef};
    use crate::project::{
        ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
    };
    use crate::resolve::{resolve, ResolveRequest};
    use crate::resource::{
        MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord, SourceRef,
    };
    use crate::trust::AlwaysTrusted;
    use crate::{ContextDescriptor, ManagedPolicy, MemoryCatalog};

    fn record(id: &str, kind: ResourceKind) -> ResourceRecord {
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            id,
            id,
        ))
    }

    fn context(resources: &MemoryResourceIndex) -> ContextResolution {
        let catalog = MemoryCatalog::default();
        let trust = AlwaysTrusted;
        let request = ResolveRequest {
            context: ContextDescriptor::for_project("/tmp/test"),
            layers: vec![],
            policy: ManagedPolicy::default(),
        };
        let deterministic = resolve(&catalog, &trust, &request).unwrap();
        compose_context_resolution(
            &deterministic,
            ProjectBinding::new(
                ProjectRef::parse("project:test").unwrap(),
                ProjectConstituentRef::parse("constituent:test").unwrap(),
                ProjectBindingLocator::LocalDirectory {
                    path: "/tmp/test".into(),
                },
            ),
            &[],
            resources,
            RequestedActors::default(),
        )
    }

    #[test]
    fn method_selection_is_downstream_of_context_resolution_not_a_precedence_engine() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        let context = context(&resources);
        let method = Method {
            id: ResourceRef::parse("method:orient").unwrap(),
            source: SourceRef::parse("source:method:orient").unwrap(),
            revision: None,
            name: "Orient".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![MethodSkillRef {
                skill: ResourceRef::parse("cap:wayfinder").unwrap(),
                usage_overlay: None,
            }],
            actions: vec![],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![],
            expected_return_forms: vec!["evidence".into()],
        };
        let resolved = resolve_praxis(
            &context,
            &resources,
            &[method],
            &[ResourceRef::parse("method:orient").unwrap()],
            &[],
        );
        assert_eq!(resolved.context_resolution_version, context.version);
        assert_eq!(resolved.methods.len(), 1);
        assert!(resolved.warnings.is_empty());
        assert_eq!(context.capabilities.len(), 1);
    }

    #[test]
    fn explain_and_history_preserve_method_source_overlay_and_resolution_condition() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        resources.insert(record("context:ground", ResourceKind::ContextSource));
        let context = context(&resources);
        let overlay = UsageOverlayRef {
            skill: ResourceRef::parse("cap:wayfinder").unwrap(),
            scope: "project".into(),
            digest: "a".repeat(64),
            source: Some(SourceRef::parse("source:overlay:project").unwrap()),
        };
        let method = Method {
            id: ResourceRef::parse("method:orient").unwrap(),
            source: SourceRef::parse("source:method:orient").unwrap(),
            revision: None,
            name: "Orient".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![MethodSkillRef {
                skill: ResourceRef::parse("cap:wayfinder").unwrap(),
                usage_overlay: Some(overlay),
            }],
            actions: vec![],
            capabilities: vec![],
            context_sources: vec![ResourceRef::parse("context:ground").unwrap()],
            verification: vec![],
            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],
        };
        let praxis = resolve_praxis(
            &context,
            &resources,
            &[method],
            &[ResourceRef::parse("method:orient").unwrap()],
            &[ResourceRef::parse("focus:project-orientation").unwrap()],
        );
        let explained = explain_praxis(&praxis);
        assert_eq!(explained.len(), 1);
        assert!(
            explained[0]
                .facts
                .iter()
                .any(|fact| fact.relation == "usage-overlay"
                    && fact.summary.contains(&"a".repeat(64)))
        );
        let history = praxis_history_evidence(&praxis);
        assert_eq!(history.len(), 1);
        assert!(history[0]
            .canonical_refs
            .iter()
            .any(|reference| reference.as_str() == "context:ground"));
        assert_eq!(
            history[0].details.get("contextResolutionVersion"),
            Some(&context.version)
        );
    }
}
