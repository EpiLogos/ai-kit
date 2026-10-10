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
use crate::id::CapsuleId;
use crate::method::{praxis_form, resolve_method, Method, MethodResolution, PraxisForm};
use crate::resolve::ResolvedView;
use crate::resource::{ResourceIndex, ResourceRef, SourceAuthority};
use crate::trust::TrustState;

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

// ---------------------------------------------------------------------------
// The encounter-task praxis receipt
//
// Factory workflow units carry required, identity-bearing praxisRefs. They ride
// the NOW preparation (`PreparedFactoryUnit.praxis_refs`) and the dispatched
// session's opening packet, but nothing downstream resolved them. This receipt
// is the resolution foundation: each ref is read against the resolved AIKit
// catalogue at the boundary where the task's NOW context is prepared.
//
// A sibling of `aikit.praxis-resolution/v1`, deliberately not a reuse of it:
// that contract binds explicitly *selected*, source-loaded Methods under a
// ContextResolution and reports gaps as warnings. This receipt answers the
// different question a *required* ref poses — does it name a real, classified,
// standing-proven Skill here — and fails closed: any unresolvable ref refuses
// the claim and is named exactly, never absorbed into a warning. Like Method
// selection, the receipt asserts resolution standing only; it never grants
// trust, activation, capability or authority.
// ---------------------------------------------------------------------------

pub const ENCOUNTER_TASK_PRAXIS_SCHEMA: &str = "aikit.encounter-task-praxis/v1";

/// The involvement-ladder standing of one catalogued praxisRef in the
/// resolution context the receipt was read under. `available` passes its own
/// trust/policy gates here; `unavailable` is withheld with a named reason;
/// `unproven` is catalogued but neither trusted nor active — its standing is
/// not established here, so the receipt declines to claim it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PraxisStanding {
    Available,
    Unavailable,
    Unproven,
}

/// One resolved praxisRef: the classification and standing of the catalogued
/// Skill it names, read from the Skill's ordinary identity (`method.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPraxisRefStanding {
    /// The praxisRef exactly as the workflow unit names it.
    pub reference: String,
    /// Classification read from the catalogued description prefix.
    pub form: PraxisForm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub standing: PraxisStanding,
}

/// Machine-readable refusal codes. The exact ref, the missing condition and
/// the route that supplies it travel together, in the repo's fail-closed
/// refusal style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PraxisRefusalCode {
    /// Names no catalogued Skill: absent from the resolved catalogue, or not
    /// a Skill identity at all.
    RefAbsent,
    /// Catalogued but withheld by resolution in this context.
    RefWithheld,
    /// Catalogued, not withheld, but neither trusted nor active here:
    /// standing unproven.
    RefStandingUnproven,
}

/// One unresolvable praxisRef, named exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PraxisRefRefusal {
    pub reference: String,
    pub code: PraxisRefusalCode,
    /// The missing condition, in the resolver's own terms.
    pub condition: String,
    /// The command or authoring act that supplies the missing condition.
    pub recovery: String,
}

/// The praxis resolution of one workflow unit's required refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitPraxisResolution {
    pub workflow_unit_ref: String,
    pub resolved: Vec<ResolvedPraxisRefStanding>,
    pub refusals: Vec<PraxisRefRefusal>,
}

/// The praxis claim of the reading. `resolved` only when every required ref
/// is catalogued, classified and standing-proven in this context; any refusal
/// fails the whole claim closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PraxisClaim {
    Resolved,
    Refused,
}

/// The typed receipt emitted into the encounter-task NOW preparation reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncounterTaskPraxisReceipt {
    pub schema: String,
    /// The resolution generation the standing was read under: the exact
    /// resolved view whose gates this receipt reports.
    pub resolution_hash: String,
    pub claim: PraxisClaim,
    pub units: Vec<UnitPraxisResolution>,
    /// What the receipt does and does not assert.
    pub standing: String,
}

/// Resolve one praxisRef against the resolved catalogue: does it name a real
/// catalogued Skill, what form does its description classify it as, and does
/// it carry standing (trust/availability) in this resolution context?
pub fn resolve_praxis_ref(
    view: &ResolvedView,
    reference: &str,
) -> Result<ResolvedPraxisRefStanding, PraxisRefRefusal> {
    let refusal = |code, condition: String, recovery: String| PraxisRefRefusal {
        reference: reference.to_string(),
        code,
        condition,
        recovery,
    };
    let Ok(id) = CapsuleId::parse(reference) else {
        return Err(refusal(
            PraxisRefusalCode::RefAbsent,
            format!(
                "praxisRef {reference} is not a Skill identity (expected `kind/...`), so it names no catalogued Skill"
            ),
            "correct the praxisRef, or author and register the Skill it means: \
             `aikit search` and `aikit praxis list` name what this context catalogues"
                .into(),
        ));
    };
    let Some(entry) = view.catalog_index.get(&id) else {
        return Err(refusal(
            PraxisRefusalCode::RefAbsent,
            format!("praxisRef {reference} is absent from the resolved catalogue"),
            "author or register the Skill the praxisRef names, or correct the ref: \
             `aikit search` and `aikit praxis list` name what this context catalogues"
                .into(),
        ));
    };
    let form = praxis_form(&entry.description);
    let revision = entry.revision.as_ref().map(ToString::to_string);
    if let Some(reason) = view.unavailable_reason(&id) {
        return Err(refusal(
            PraxisRefusalCode::RefWithheld,
            format!(
                "praxisRef {reference} is withheld by resolution in this context: {}",
                reason.describe()
            ),
            "review the revision: `aikit trust record <ref>`; the withholding reason names its own ground"
                .into(),
        ));
    }
    let standing = if view.is_active(&id) || entry.trust == TrustState::Trusted {
        PraxisStanding::Available
    } else {
        return Err(refusal(
            PraxisRefusalCode::RefStandingUnproven,
            format!(
                "praxisRef {reference} is catalogued in this context but its standing is unproven: neither trusted nor active"
            ),
            format!(
                "review the revision: `aikit trust record {reference}`; or enable it in the scope you mean: `aikit enable {reference} --scope project --apply`"
            ),
        ));
    };
    Ok(ResolvedPraxisRefStanding {
        reference: reference.to_string(),
        form,
        revision,
        standing,
    })
}

/// Resolve every praxisRef the dispatched encounter task's workflow units
/// require, per unit, against the resolved view. Fail-closed: the claim is
/// [`PraxisClaim::Resolved`] only when every required ref is catalogued,
/// classified and standing-proven here; any refusal names the exact ref and
/// refuses the whole claim. Units carrying no praxisRefs resolve nothing.
pub fn resolve_encounter_task_praxis<'a>(
    view: &ResolvedView,
    units: impl IntoIterator<Item = (&'a str, &'a [String])>,
) -> EncounterTaskPraxisReceipt {
    let units: Vec<UnitPraxisResolution> = units
        .into_iter()
        .filter(|(_, praxis_refs)| !praxis_refs.is_empty())
        .map(|(workflow_unit_ref, praxis_refs)| {
            let mut resolved = Vec::new();
            let mut refusals = Vec::new();
            for reference in praxis_refs {
                match resolve_praxis_ref(view, reference) {
                    Ok(standing) => resolved.push(standing),
                    Err(refusal) => refusals.push(refusal),
                }
            }
            UnitPraxisResolution {
                workflow_unit_ref: workflow_unit_ref.to_string(),
                resolved,
                refusals,
            }
        })
        .collect();
    let claim = if !units.is_empty() && units.iter().all(|unit| unit.refusals.is_empty()) {
        PraxisClaim::Resolved
    } else {
        PraxisClaim::Refused
    };
    EncounterTaskPraxisReceipt {
        schema: ENCOUNTER_TASK_PRAXIS_SCHEMA.into(),
        resolution_hash: view.hash.to_string(),
        claim,
        units,
        standing:
            "resolution standing only; a required praxisRef's resolution never grants trust, \
             activation, capability or authority"
                .into(),
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
                        source: source_ref.clone(),
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

            if let Some(expected) = &resolution.expected_resolve {
                facts.push(ExplainFact {
                    relation: "expected-resolve".into(),
                    authority: Some(SourceAuthority::Authored),
                    summary: format!("Method expects Resolve pattern {}", expected.render()),
                    canonical_refs: Vec::new(),
                    provenance: vec![EvidenceProvenance {
                        source: source_ref.clone(),
                        revision: resolution.revision.as_ref().map(ToString::to_string),
                        ..EvidenceProvenance::default()
                    }],
                });
            }

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
                "expectedResolve".into(),
                resolution
                    .expected_resolve
                    .as_ref()
                    .map(|expected| expected.render())
                    .unwrap_or_default(),
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
        let mut method_record = record("skill:orient", ResourceKind::Capability);
        method_record.descriptor.description = "METHOD: Orient".into();
        resources.insert(method_record);
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        let context = context(&resources);
        let capabilities_before = context.capabilities.clone();
        let method = Method {
            id: ResourceRef::parse("skill:orient").unwrap(),
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
            expected_resolve: None,
            expected_return_forms: vec!["evidence".into()],
        };
        let resolved = resolve_praxis(
            &context,
            &resources,
            &[method],
            &[ResourceRef::parse("skill:orient").unwrap()],
            &[],
        );
        assert_eq!(resolved.context_resolution_version, context.version);
        assert_eq!(resolved.methods.len(), 1);
        assert!(resolved.warnings.is_empty());
        assert_eq!(context.capabilities, capabilities_before);
        assert_eq!(context.capabilities.len(), 2);
    }

    #[test]
    fn explain_and_history_preserve_method_source_overlay_and_resolution_condition() {
        let mut resources = MemoryResourceIndex::default();
        let mut method_record = record("skill:orient", ResourceKind::Capability);
        method_record.descriptor.description = "METHOD: Orient".into();
        resources.insert(method_record);
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
            id: ResourceRef::parse("skill:orient").unwrap(),
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
            expected_resolve: Some(
                crate::resource::parse_resolve_expression("@0 context:ground x @5 cap:wayfinder")
                    .unwrap(),
            ),
            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],
        };
        let praxis = resolve_praxis(
            &context,
            &resources,
            &[method],
            &[ResourceRef::parse("skill:orient").unwrap()],
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
        assert!(explained[0].facts.iter().any(|fact| {
            fact.relation == "expected-resolve"
                && fact.authority == Some(SourceAuthority::Authored)
                && fact
                    .summary
                    .contains("@0 context:ground x @5 cap:wayfinder")
        }));
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
        assert_eq!(
            history[0].details.get("expectedResolve"),
            Some(&"@0 context:ground x @5 cap:wayfinder".into())
        );
    }

    // -----------------------------------------------------------------------
    // The encounter-task praxis receipt — required refs resolved at the
    // encounter-task boundary, fail-closed, standing only.
    // -----------------------------------------------------------------------

    mod receipt {
        use super::*;
        use crate::capsule::Capsule;
        use crate::id::RegistrySource;
        use crate::trust::MemoryTrust;

        fn capsule(id: &str, description: &str) -> Capsule {
            let leaf = id.rsplit('/').next().unwrap();
            let src = format!(
                r#"
schema = 1
id = "{id}"
kind = "skill"
name = "{leaf}"
description = "{description}"

[skill]"#
            );
            let mut capsule = Capsule::from_toml_str(&src).unwrap();
            capsule.revision = Some(crate::id::Revision::from_raw("r1"));
            capsule.source = Some(RegistrySource::new("personal"));
            capsule
        }

        fn view_with(
            catalog: &MemoryCatalog,
            trust: &MemoryTrust,
            enable: &[&str],
        ) -> ResolvedView {
            let mut patch = crate::profile::PoolPatch::default();
            for id in enable {
                patch.enable.push(CapsuleId::parse(id).unwrap());
            }
            resolve(
                catalog,
                trust,
                &ResolveRequest {
                    context: ContextDescriptor::for_project("/work/receipt"),
                    layers: if enable.is_empty() {
                        Vec::new()
                    } else {
                        vec![crate::scope::ScopeLayer::new(
                            crate::scope::ScopeKind::Project,
                            crate::scope::LayerOrigin::new("project profile.toml"),
                            patch,
                        )]
                    },
                    policy: ManagedPolicy::default(),
                },
            )
            .unwrap()
        }

        fn trusted(ids: &[&str]) -> MemoryTrust {
            let mut trust = MemoryTrust::default();
            for id in ids {
                trust.set(
                    RegistrySource::new("personal"),
                    CapsuleId::parse(id).unwrap(),
                    crate::id::Revision::from_raw("r1"),
                    TrustState::Trusted,
                );
            }
            trust
        }

        fn reviewed_only(id: &str) -> MemoryTrust {
            let mut trust = MemoryTrust::default();
            trust.set(
                RegistrySource::new("personal"),
                CapsuleId::parse(id).unwrap(),
                crate::id::Revision::from_raw("r1"),
                TrustState::Reviewed,
            );
            trust
        }

        fn fixture() -> (MemoryCatalog, MemoryTrust) {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(capsule(
                "skill/practice/day-close",
                "METHOD: close the day from the field",
            ));
            catalog.insert(capsule(
                "skill/field/orienting",
                "METHODOLOGY: orient the documentation field",
            ));
            catalog.insert(capsule("skill/practice/plain", "A plain reusable skill."));
            // All three reviewed and promoted, so the receipt tests exercise
            // classification and standing, not the review gate.
            let trust = trusted(&[
                "skill/practice/day-close",
                "skill/field/orienting",
                "skill/practice/plain",
            ]);
            (catalog, trust)
        }

        fn unit_refs<'a>(unit: &'a str, refs: &'a [String]) -> (&'a str, &'a [String]) {
            (unit, refs)
        }

        #[test]
        fn a_known_catalogued_skill_resolves_with_form_revision_and_standing() {
            let (catalog, trust) = fixture();
            let view = view_with(&catalog, &trust, &[]);
            let refs = vec!["skill/practice/plain".to_string()];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/one", &refs)]);
            assert_eq!(receipt.schema, ENCOUNTER_TASK_PRAXIS_SCHEMA);
            assert_eq!(receipt.resolution_hash, view.hash.to_string());
            assert_eq!(receipt.claim, PraxisClaim::Resolved);
            assert_eq!(receipt.units.len(), 1);
            assert_eq!(receipt.units[0].workflow_unit_ref, "unit/one");
            let standing = &receipt.units[0].resolved[0];
            assert_eq!(standing.reference, "skill/practice/plain");
            assert_eq!(standing.form, PraxisForm::Skill);
            assert_eq!(standing.revision.as_deref(), Some("r1"));
            assert_eq!(standing.standing, PraxisStanding::Available);
            assert!(receipt.units[0].refusals.is_empty());
            // The wire shape keeps the claim and standing legible.
            let json = serde_json::to_value(&receipt).unwrap();
            assert_eq!(json["claim"], "resolved");
            assert_eq!(json["units"][0]["resolved"][0]["standing"], "available");
            assert_eq!(json["units"][0]["resolved"][0]["form"], "skill");
        }

        #[test]
        fn an_unknown_ref_is_named_exactly_and_refuses_the_claim() {
            let (catalog, trust) = fixture();
            let view = view_with(&catalog, &trust, &[]);
            let refs = vec![
                "skill/practice/plain".to_string(),
                "skill/none/such".to_string(),
            ];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/one", &refs)]);
            assert_eq!(receipt.claim, PraxisClaim::Refused);
            assert_eq!(receipt.units[0].resolved.len(), 1);
            let refusal = &receipt.units[0].refusals[0];
            assert_eq!(refusal.code, PraxisRefusalCode::RefAbsent);
            assert!(refusal.condition.contains("skill/none/such"));
            assert!(refusal
                .condition
                .contains("absent from the resolved catalogue"));
            assert!(!refusal.recovery.is_empty());
            let json = serde_json::to_value(&receipt).unwrap();
            assert_eq!(json["claim"], "refused");
            assert_eq!(json["units"][0]["refusals"][0]["code"], "ref-absent");
        }

        #[test]
        fn a_malformed_ref_names_no_catalogued_skill() {
            let (catalog, trust) = fixture();
            let view = view_with(&catalog, &trust, &[]);
            let refs = vec!["orienting-practice".to_string()];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/one", &refs)]);
            assert_eq!(receipt.claim, PraxisClaim::Refused);
            assert_eq!(
                receipt.units[0].refusals[0].code,
                PraxisRefusalCode::RefAbsent
            );
            assert!(receipt.units[0].refusals[0]
                .condition
                .contains("not a Skill identity"));
        }

        #[test]
        fn classification_reads_the_description_prefix_not_a_separate_identity() {
            let (catalog, trust) = fixture();
            let view = view_with(&catalog, &trust, &[]);
            let refs = vec![
                "skill/practice/day-close".to_string(),
                "skill/field/orienting".to_string(),
                "skill/practice/plain".to_string(),
            ];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/forms", &refs)]);
            assert_eq!(receipt.claim, PraxisClaim::Resolved);
            let forms: Vec<(String, PraxisForm)> = receipt.units[0]
                .resolved
                .iter()
                .map(|standing| (standing.reference.clone(), standing.form))
                .collect();
            assert_eq!(
                forms,
                vec![
                    ("skill/practice/day-close".into(), PraxisForm::Method),
                    ("skill/field/orienting".into(), PraxisForm::Methodology),
                    ("skill/practice/plain".into(), PraxisForm::Skill),
                ]
            );
        }

        #[test]
        fn an_unproven_standing_fails_closed_with_its_routes() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(capsule("skill/practice/plain", "A plain reusable skill."));
            // Reviewed but not trusted, and not active: standing unproven.
            let view = view_with(&catalog, &reviewed_only("skill/practice/plain"), &[]);
            let refs = vec!["skill/practice/plain".to_string()];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/one", &refs)]);
            assert_eq!(receipt.claim, PraxisClaim::Refused);
            let refusal = &receipt.units[0].refusals[0];
            assert_eq!(refusal.code, PraxisRefusalCode::RefStandingUnproven);
            assert!(refusal.condition.contains("standing is unproven"));
            assert!(refusal.recovery.contains("aikit trust record"));
            assert!(refusal.recovery.contains("aikit enable"));
        }

        #[test]
        fn a_withheld_ref_refuses_with_the_resolution_reason() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(capsule("skill/practice/plain", "A plain reusable skill."));
            // Declared enabled but never reviewed: resolution withholds it.
            let view = view_with(&catalog, &MemoryTrust::default(), &["skill/practice/plain"]);
            let refs = vec!["skill/practice/plain".to_string()];
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/one", &refs)]);
            assert_eq!(receipt.claim, PraxisClaim::Refused);
            let refusal = &receipt.units[0].refusals[0];
            assert_eq!(refusal.code, PraxisRefusalCode::RefWithheld);
            assert!(refusal.condition.contains("withheld by resolution"));
            assert!(refusal.recovery.contains("aikit trust record"));
        }

        #[test]
        fn units_without_praxis_refs_resolve_nothing_and_the_standing_line_never_grants_trust() {
            let (catalog, trust) = fixture();
            let view = view_with(&catalog, &trust, &[]);
            let receipt = resolve_encounter_task_praxis(&view, [unit_refs("unit/empty", &[])]);
            assert!(receipt.units.is_empty());
            assert!(receipt
                .standing
                .contains("never grants trust, activation, capability or authority"));
        }
    }
}
