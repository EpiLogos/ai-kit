//! Praxis classification (Skill / Method / Methodology) and situated
//! metadata for Skills.
//!
//! A Method is not a second resource. It is a Skill whose description starts
//! with [`METHOD_DESCRIPTION_PREFIX`]. The optional situated relations below are
//! metadata about that same Skill identity; they never create a Method identity,
//! source lifecycle, store, or activation path.

use serde::{Deserialize, Serialize};

use crate::capsule::Kind;
use crate::id::CapsuleId;
use crate::resolve::{ResolvedView, UnavailableReason};
use crate::resource::{
    ResolveExpression, ResourceIndex, ResourceKind, ResourceRef, SourceRef, SourceRevision,
};
use crate::{AikitError, Result};

pub const METHOD_VERSION: &str = "aikit.skill-method-metadata/v1";

/// The detection convention for situated operational patterns: a Method is
/// just a Skill whose description carries a `METHOD:` prefix. Nothing else
/// about the capsule changes — the skill stays authored, versioned, trusted
/// and resolved exactly like any other. The prefix only makes the Method
/// discoverable as such.
pub const METHOD_DESCRIPTION_PREFIX: &str = "METHOD:";

/// The sibling convention for field-level praxis: a Methodology is a Skill
/// whose description carries a `METHODOLOGY:` prefix. It orients among ways of
/// acting — field vocabulary, determining relations, the Methods that apply and
/// when, attention strategy and Return paths. Like `METHOD:` it is only a
/// classification of the ordinary Skill identity: no resource kind, store,
/// trust or projection lifecycle follows from it.
pub const METHODOLOGY_DESCRIPTION_PREFIX: &str = "METHODOLOGY:";

/// The situated payload declared after the prefix, if the description carries
/// one. Detection is prefix-only: an empty payload is still a declared Method
/// (the full description remains the skill's description).
///
/// `METHODOLOGY:` never detects as a Method: the prefixes differ at the colon.
pub fn method_payload(description: &str) -> Option<&str> {
    let trimmed = description.trim_start();
    let rest = trimmed.strip_prefix(METHOD_DESCRIPTION_PREFIX)?;
    Some(rest.trim())
}

/// The field-level payload after a `METHODOLOGY:` prefix, if declared.
pub fn methodology_payload(description: &str) -> Option<&str> {
    let trimmed = description.trim_start();
    let rest = trimmed.strip_prefix(METHODOLOGY_DESCRIPTION_PREFIX)?;
    Some(rest.trim())
}

/// The exact condition `method run` needs and did not find: the truth a
/// refusal must name, with the route that supplies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MethodRunBarrier {
    /// Stable machine-readable refusal code.
    pub code: &'static str,
    /// The missing condition, in the resolver's own terms.
    pub condition: String,
    /// The command or authoring act that supplies the missing condition.
    pub recovery: String,
}

/// Diagnose whether `method run` can execute this Method in this context and,
/// when it cannot, name the exact missing condition — never the catch-all.
///
/// The first true failure is the one named: resolution's own withholding
/// (trust, standing, policy, retirement), then the enablement seam, then the
/// one condition the native runner adds. That runner is the same one
/// `aikit run` uses — never a second transport — and it executes `[script]`
/// payloads: script-kind capsules, the executable-praxis pattern the
/// registered practices follow. A skill-kind Method is authored faculty, not
/// an executable body; its route is the agent (`aikit act invoke`), so an
/// enabled, active, trusted skill Method refuses with `method.no_executable_body`,
/// not with a claim that no scope enables it — the scope did.
///
/// The id must be in the view's catalogue; callers refuse unknown refs before
/// consulting the barrier.
pub fn run_barrier(view: &ResolvedView, id: &CapsuleId) -> Option<MethodRunBarrier> {
    let entry = view.catalog_index.get(id)?;
    if let Some(reason) = view.unavailable_reason(id) {
        let recovery = match reason {
            UnavailableReason::TrustRequired => {
                "review the revision: `aikit trust record <ref>`".to_string()
            }
            _ => "the reason names its own ground; resolve it there".to_string(),
        };
        return Some(MethodRunBarrier {
            code: "method.withheld",
            condition: format!("is withheld by resolution in this context: {}", reason.describe()),
            recovery,
        });
    }
    if !view.is_active(id) {
        let condition = if view.is_declared_disabled(id) {
            "is explicitly disabled in this context".to_string()
        } else {
            "is catalogued, but no scope enables it in this context".to_string()
        };
        return Some(MethodRunBarrier {
            code: "method.not_enabled",
            condition,
            recovery: format!(
                "enable it in the scope you mean: `aikit enable {id} --scope project --apply` (or `--scope global`)"
            ),
        });
    }
    if entry.kind != Kind::Script {
        return Some(MethodRunBarrier {
            code: "method.no_executable_body",
            condition: format!(
                "is enabled and active in this context, but a {} Method carries no deterministic \
                 executable body — `method run` drives the same native runner `aikit run` uses, \
                 which only a `[script]` payload provides",
                entry.kind.as_str()
            ),
            recovery: format!(
                "invoke the Skill through the agent: `aikit act invoke {id}`; or give the Method \
                 executable support: a script-kind capsule whose description carries the `METHOD:` \
                 prefix, the executable-praxis pattern the registered practices use"
            ),
        });
    }
    None
}


/// The common classification of one Skill identity.
///
/// ```text
/// Skill        what reusable faculty can be exercised?            (unprefixed)
/// Method       how do faculties, sources and operations compose   (METHOD:)
///              for this class of act?
/// Methodology  what field am I in, which Methods apply when, and  (METHODOLOGY:)
///              how does Return propagate through it?
/// ```
///
/// The form is read from the ordinary Skill description and nothing else. It
/// grants no authority, orders nothing and never changes the Skill's identity,
/// source, trust, activation or projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PraxisForm {
    Skill,
    Method,
    Methodology,
}

impl PraxisForm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Method => "method",
            Self::Methodology => "methodology",
        }
    }

    /// The disclosure position this form answers in the Agent's sixfold
    /// reading: `#1` what can I do, `#2` how do I act, `#3` how do I orient.
    pub fn position(self) -> u8 {
        match self {
            Self::Skill => 1,
            Self::Method => 2,
            Self::Methodology => 3,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "skill" => Some(Self::Skill),
            "method" => Some(Self::Method),
            "methodology" => Some(Self::Methodology),
            _ => None,
        }
    }
}

/// Classify a Skill description. Unprefixed descriptions are ordinary Skill
/// praxis; a mid-description mention of either prefix classifies nothing.
pub fn praxis_form(description: &str) -> PraxisForm {
    if methodology_payload(description).is_some() {
        PraxisForm::Methodology
    } else if method_payload(description).is_some() {
        PraxisForm::Method
    } else {
        PraxisForm::Skill
    }
}

/// The payload after whichever prefix classified the description, or the
/// whole trimmed description for ordinary Skill praxis.
pub fn praxis_payload(description: &str) -> &str {
    methodology_payload(description)
        .or_else(|| method_payload(description))
        .unwrap_or_else(|| description.trim())
}

/// Immutable receipt identifying the scoped adaptation of an unchanged Skill.
///
/// Runtime authoring remains the existing `SkillUsageOverlayPatch` mechanism in
/// Profile/scope resolution. Situated-use metadata only points at the resulting
/// exact digest; it does not introduce another overlay store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageOverlayRef {
    pub skill: ResourceRef,
    pub scope: String,
    pub digest: String,
    #[serde(default)]
    pub source: Option<SourceRef>,
}

impl UsageOverlayRef {
    pub fn validate(&self) -> Result<()> {
        if self.scope.trim().is_empty() {
            return Err(AikitError::new(
                "method.overlay_scope_empty",
                "UsageOverlay receipt scope must be non-empty",
            ));
        }
        if self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AikitError::new(
                "method.overlay_digest_invalid",
                "UsageOverlay receipt must carry an exact lowercase 64-character content digest",
            )
            .with("skill", self.skill.to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SituatedSkillRef {
    pub skill: ResourceRef,
    #[serde(default)]
    pub usage_overlay: Option<UsageOverlayRef>,
}

/// Optional situated-use metadata attached to the Skill named by `id`.
///
/// `id` is the existing Skill resource identity. `source`/`revision` identify
/// the evidence that supplied these relations, not a Method source object.
/// Every related member remains independently owned and no body is copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPraxisMetadata {
    pub id: ResourceRef,
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub focus: Vec<ResourceRef>,
    #[serde(default)]
    pub project_domain: Vec<ResourceRef>,
    #[serde(default)]
    pub skills: Vec<SituatedSkillRef>,
    #[serde(default)]
    pub actions: Vec<ResourceRef>,
    #[serde(default)]
    pub capabilities: Vec<ResourceRef>,
    #[serde(default)]
    pub context_sources: Vec<ResourceRef>,
    #[serde(default)]
    pub verification: Vec<ResourceRef>,
    /// Source-authored semantic movement this Method expects to be useful. This is
    /// an intention/pattern only: actual Invocation/Activity may return a different
    /// observed ResolvePath, which remains observed evidence rather than Method truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_resolve: Option<ResolveExpression>,
    #[serde(default)]
    pub expected_return_forms: Vec<String>,
}

impl SkillPraxisMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AikitError::new(
                "method.name_empty",
                "Method name must be non-empty",
            ));
        }
        for skill in &self.skills {
            if let Some(overlay) = &skill.usage_overlay {
                overlay.validate()?;
                if overlay.skill != skill.skill {
                    return Err(AikitError::new(
                        "method.overlay_skill_mismatch",
                        "UsageOverlay receipt must refer to the Skill it adapts",
                    )
                    .with("skill", skill.skill.to_string())
                    .with("overlay_skill", overlay.skill.to_string()));
                }
            }
        }
        if self
            .expected_return_forms
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err(AikitError::new(
                "method.return_form_empty",
                "Method expected return forms must be non-empty when declared",
            ));
        }
        Ok(())
    }
}

/// Compatibility names for callers of the superseded #108 API. These are type
/// aliases only: neither name creates a resource identity or source format.
pub type Method = SkillPraxisMetadata;
pub type MethodSkillRef = SituatedSkillRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPraxisResolvedRef {
    pub reference: ResourceRef,
    pub expected_kind: ResourceKind,
    #[serde(default)]
    pub actual_kind: Option<ResourceKind>,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPraxisMetadataResolution {
    pub method: ResourceRef,
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub focus: Vec<SkillPraxisResolvedRef>,
    pub project_domain: Vec<SkillPraxisResolvedRef>,
    pub skills: Vec<SkillPraxisResolvedRef>,
    pub actions: Vec<SkillPraxisResolvedRef>,
    pub capabilities: Vec<SkillPraxisResolvedRef>,
    pub context_sources: Vec<SkillPraxisResolvedRef>,
    pub verification: Vec<SkillPraxisResolvedRef>,
    pub overlays: Vec<UsageOverlayRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_resolve: Option<ResolveExpression>,
    pub expected_return_forms: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl SkillPraxisMetadataResolution {
    pub fn is_complete(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// Resolve a Method-classified Skill and its situated-use metadata against the
/// same V2 resource field used by ContextResolution. This is observational: it
/// never enables, disables, trusts, orders, or mutates referenced resources.
pub fn resolve_skill_praxis_metadata(
    method: &SkillPraxisMetadata,
    resources: &dyn ResourceIndex,
) -> Result<SkillPraxisMetadataResolution> {
    method.validate()?;
    let mut warnings = Vec::new();

    match resources.resource(&method.id) {
        None => warnings.push(format!(
            "Method-classified Skill {} is absent from the resource field",
            method.id
        )),
        Some(record) if record.descriptor.kind != ResourceKind::Capability => {
            warnings.push(format!(
                "Method-classified resource {} has kind {}, expected capability",
                method.id,
                record.descriptor.kind.as_str()
            ))
        }
        Some(record) if method_payload(&record.descriptor.description).is_none() => {
            warnings.push(format!(
            "Skill {} is not Method-classified because its description lacks the METHOD: prefix",
            method.id
        ))
        }
        Some(_) => {}
    }

    let focus = resolve_many(
        &method.focus,
        ResourceKind::KnowledgeNode,
        resources,
        &mut warnings,
        false,
    );
    let project_domain = resolve_many(
        &method.project_domain,
        ResourceKind::Project,
        resources,
        &mut warnings,
        false,
    );
    // Native Skills are currently V2 Capability resources; Skill identity/source
    // remains the capsule/source system rather than a duplicate ResourceKind.
    let skills = resolve_many(
        &method
            .skills
            .iter()
            .map(|value| value.skill.clone())
            .collect::<Vec<_>>(),
        ResourceKind::Capability,
        resources,
        &mut warnings,
        true,
    );
    let actions = resolve_many(
        &method.actions,
        ResourceKind::Action,
        resources,
        &mut warnings,
        true,
    );
    let capabilities = resolve_many(
        &method.capabilities,
        ResourceKind::Capability,
        resources,
        &mut warnings,
        true,
    );
    let context_sources = resolve_many(
        &method.context_sources,
        ResourceKind::ContextSource,
        resources,
        &mut warnings,
        true,
    );
    // Verification is a relation, not a hard lens/type. Existing Verification
    // resources may be represented by Action/Capability/KnowledgeSource refs, so
    // preserve the actual kind while requiring existence only.
    let verification = resolve_any(&method.verification, resources, &mut warnings);

    Ok(SkillPraxisMetadataResolution {
        method: method.id.clone(),
        source: method.source.clone(),
        revision: method.revision.clone(),
        focus,
        project_domain,
        skills,
        actions,
        capabilities,
        context_sources,
        verification,
        overlays: method
            .skills
            .iter()
            .filter_map(|value| value.usage_overlay.clone())
            .collect(),
        expected_resolve: method.expected_resolve.clone(),
        expected_return_forms: method.expected_return_forms.clone(),
        warnings,
    })
}

fn resolve_many(
    refs: &[ResourceRef],
    expected: ResourceKind,
    resources: &dyn ResourceIndex,
    warnings: &mut Vec<String>,
    strict_kind: bool,
) -> Vec<SkillPraxisResolvedRef> {
    refs.iter()
        .map(|reference| match resources.resource(reference) {
            None => {
                warnings.push(format!(
                    "Method reference {reference} is absent (expected {})",
                    expected.as_str()
                ));
                SkillPraxisResolvedRef {
                    reference: reference.clone(),
                    expected_kind: expected,
                    actual_kind: None,
                    resolved: false,
                }
            }
            Some(record) if strict_kind && record.descriptor.kind != expected => {
                warnings.push(format!(
                    "Method reference {reference} has kind {}, expected {}",
                    record.descriptor.kind.as_str(),
                    expected.as_str()
                ));
                SkillPraxisResolvedRef {
                    reference: reference.clone(),
                    expected_kind: expected,
                    actual_kind: Some(record.descriptor.kind),
                    resolved: false,
                }
            }
            Some(record) => SkillPraxisResolvedRef {
                reference: reference.clone(),
                expected_kind: expected,
                actual_kind: Some(record.descriptor.kind),
                resolved: true,
            },
        })
        .collect()
}

fn resolve_any(
    refs: &[ResourceRef],
    resources: &dyn ResourceIndex,
    warnings: &mut Vec<String>,
) -> Vec<SkillPraxisResolvedRef> {
    refs.iter()
        .map(|reference| match resources.resource(reference) {
            Some(record) => SkillPraxisResolvedRef {
                reference: reference.clone(),
                expected_kind: record.descriptor.kind,
                actual_kind: Some(record.descriptor.kind),
                resolved: true,
            },
            None => {
                warnings.push(format!(
                    "Method verification reference {reference} is absent"
                ));
                SkillPraxisResolvedRef {
                    reference: reference.clone(),
                    expected_kind: ResourceKind::Capability,
                    actual_kind: None,
                    resolved: false,
                }
            }
        })
        .collect()
}

pub type MethodResolvedRef = SkillPraxisResolvedRef;
pub type MethodResolution = SkillPraxisMetadataResolution;

/// Compatibility entry point for the superseded #108 API.
pub fn resolve_method(method: &Method, resources: &dyn ResourceIndex) -> Result<MethodResolution> {
    resolve_skill_praxis_metadata(method, resources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{MemoryResourceIndex, ResourceDescriptor, ResourceRecord};

    #[test]
    fn method_detection_is_prefix_only_and_never_asserts_semantics() {
        assert_eq!(
            method_payload("METHOD: inhabit a project wiki from Control state"),
            Some("inhabit a project wiki from Control state")
        );
        // Leading whitespace before the prefix does not hide a Method.
        assert_eq!(
            method_payload("  METHOD: situate the work"),
            Some("situate the work")
        );
        // An empty payload is still a declared Method.
        assert_eq!(method_payload("METHOD:"), Some(""));
        // Without the prefix there is no Method, and a mid-description
        // mention does not detect.
        assert_eq!(method_payload("Review docs before merging."), None);
        assert_eq!(method_payload("Use a METHOD: prefix here"), None);
    }

    #[test]
    fn praxis_form_classifies_one_skill_identity_three_ways() {
        assert_eq!(praxis_form("Recover sources first."), PraxisForm::Skill);
        assert_eq!(
            praxis_form("METHOD: repair from evidence"),
            PraxisForm::Method
        );
        assert_eq!(
            praxis_form("  METHODOLOGY: orient the documentation field"),
            PraxisForm::Methodology
        );
        // The Methodology prefix is not a Method, and neither prefix detects
        // mid-description.
        assert_eq!(method_payload("METHODOLOGY: orient"), None);
        assert_eq!(methodology_payload("METHOD: act"), None);
        assert_eq!(
            praxis_form("Explains the METHODOLOGY: prefix."),
            PraxisForm::Skill
        );
        assert_eq!(praxis_payload("METHODOLOGY: orient"), "orient");
        assert_eq!(praxis_payload("METHOD: act"), "act");
        assert_eq!(praxis_payload(" plain "), "plain");
        assert_eq!(PraxisForm::Methodology.position(), 3);
        assert_eq!(PraxisForm::parse("Method"), Some(PraxisForm::Method));
        assert_eq!(PraxisForm::parse("workflow"), None);
    }

    fn record(id: &str, kind: ResourceKind) -> ResourceRecord {
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            id,
            id,
        ))
    }

    #[test]
    fn method_composes_refs_without_copying_or_conferring_authority() {
        let mut resources = MemoryResourceIndex::default();
        let mut classified = record("skill:project-change", ResourceKind::Capability);
        classified.descriptor.description = "METHOD: Project change".into();
        resources.insert(classified);
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        resources.insert(record("action:verify", ResourceKind::Action));
        resources.insert(record(
            "context:project-ground",
            ResourceKind::ContextSource,
        ));
        resources.insert(record("project:demo", ResourceKind::Project));

        let method = Method {
            id: ResourceRef::parse("skill:project-change").unwrap(),
            source: SourceRef::parse("source:method:project-change").unwrap(),
            revision: None,
            name: "Project change".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![ResourceRef::parse("project:demo").unwrap()],
            skills: vec![MethodSkillRef {
                skill: ResourceRef::parse("cap:wayfinder").unwrap(),
                usage_overlay: Some(UsageOverlayRef {
                    skill: ResourceRef::parse("cap:wayfinder").unwrap(),
                    scope: "project".into(),
                    digest: "a".repeat(64),
                    source: None,
                }),
            }],
            actions: vec![ResourceRef::parse("action:verify").unwrap()],
            capabilities: vec![],
            context_sources: vec![ResourceRef::parse("context:project-ground").unwrap()],
            verification: vec![ResourceRef::parse("action:verify").unwrap()],
            expected_resolve: Some(
                crate::resource::parse_resolve_expression(
                    "@0 context:project-ground x @5 action:verify",
                )
                .unwrap(),
            ),
            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],
        };

        let resolved = resolve_method(&method, &resources).unwrap();
        assert!(resolved.is_complete());
        assert_eq!(resolved.skills.len(), 1);
        assert_eq!(resolved.overlays.len(), 1);
        assert_eq!(resolved.expected_return_forms.len(), 2);
        assert_eq!(
            resolved
                .expected_resolve
                .as_ref()
                .map(ResolveExpression::render),
            Some("@0 context:project-ground x @5 action:verify".into())
        );
        assert_eq!(resolved.method, method.id);
    }

    #[test]
    fn missing_or_wrong_refs_are_explainable_not_promoted() {
        let mut resources = MemoryResourceIndex::default();
        let mut classified = record("skill:broken", ResourceKind::Capability);
        classified.descriptor.description = "METHOD: Broken".into();
        resources.insert(classified);
        resources.insert(record("action:not-a-skill", ResourceKind::Action));
        let method = Method {
            id: ResourceRef::parse("skill:broken").unwrap(),
            source: SourceRef::parse("source:method:broken").unwrap(),
            revision: None,
            name: "Broken".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![MethodSkillRef {
                skill: ResourceRef::parse("action:not-a-skill").unwrap(),
                usage_overlay: None,
            }],
            actions: vec![ResourceRef::parse("action:missing").unwrap()],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![],
            expected_resolve: None,
            expected_return_forms: vec![],
        };
        let resolved = resolve_method(&method, &resources).unwrap();
        assert!(!resolved.is_complete());
        assert_eq!(resolved.warnings.len(), 2);
    }

    #[test]
    fn situated_metadata_cannot_mint_a_method_identity_beside_an_ordinary_skill() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("skill:orient", ResourceKind::Capability));
        let method = Method {
            id: ResourceRef::parse("skill:orient").unwrap(),
            source: SourceRef::parse("source:metadata:orient").unwrap(),
            revision: None,
            name: "Orient".into(),
            description: String::new(),
            focus: vec![],
            project_domain: vec![],
            skills: vec![],
            actions: vec![],
            capabilities: vec![],
            context_sources: vec![],
            verification: vec![],
            expected_resolve: None,
            expected_return_forms: vec![],
        };

        let resolved = resolve_method(&method, &resources).unwrap();
        assert!(!resolved.is_complete());
        assert_eq!(resolved.method, method.id);
        assert!(resolved.warnings[0].contains("lacks the METHOD: prefix"));
    }

    // -----------------------------------------------------------------------
    // `run_barrier` — the enable→run seam answers with the truth.
    //
    // The verifier's defect: a declared, trusted, enabled, active Method
    // refused as "no scope enables it in this context" because the runner's
    // gate was `ResolvedView::can_run` — a palette predicate ("is this kind
    // runnable while inactive") that is false for every skill-kind Method,
    // whatever the scopes say. The ladder names the real condition instead.
    // -----------------------------------------------------------------------

    mod barrier {
        use super::*;
        use crate::catalog::MemoryCatalog;
        use crate::context::ContextDescriptor;
        use crate::id::RegistrySource;
        use crate::policy::ManagedPolicy;
        use crate::resolve::{resolve, ResolveRequest};
        use crate::scope::{LayerOrigin, ScopeKind, ScopeLayer};
        use crate::trust::{MemoryTrust, TrustState};

        fn method_capsule(id: &str, kind: &str) -> crate::capsule::Capsule {
            let leaf = id.rsplit('/').next().unwrap();
            let support = match kind {
                "script" => "\n[script]\nentry = \"payload/run.sh\"\n",
                _ => "\n[skill]\n",
            };
            let src = format!(
                r#"
schema = 1
id = "{id}"
kind = "{kind}"
name = "{leaf}"
description = "METHOD: a bounded practice for the test field."
{support}"#
            );
            let mut capsule = crate::capsule::Capsule::from_toml_str(&src).unwrap();
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
                    context: ContextDescriptor::for_project("/work/seam"),
                    layers: if enable.is_empty() {
                        Vec::new()
                    } else {
                        vec![ScopeLayer::new(
                            ScopeKind::Project,
                            LayerOrigin::new("project profile.toml"),
                            patch,
                        )]
                    },
                    policy: ManagedPolicy::default(),
                },
            )
            .unwrap()
        }

        fn trusted(id: &str) -> MemoryTrust {
            let mut trust = MemoryTrust::default();
            trust.set(
                RegistrySource::new("personal"),
                CapsuleId::parse(id).unwrap(),
                crate::id::Revision::from_raw("r1"),
                TrustState::Trusted,
            );
            trust
        }

        #[test]
        fn an_enabled_active_skill_method_names_the_missing_executable_body() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(method_capsule("skill/practice/day-close", "skill"));
            let view = view_with(&catalog, &trusted("skill/practice/day-close"),
                &["skill/practice/day-close"]);

            // The seam the verifier walked: declared, enabled, active — and
            // still not runnable, because the capsule carries no `[script]`
            // body. The refusal must say exactly that.
            assert!(view.is_declared_enabled(&CapsuleId::parse("skill/practice/day-close").unwrap()));
            assert!(view.is_active(&CapsuleId::parse("skill/practice/day-close").unwrap()));
            assert!(!view.can_run(&CapsuleId::parse("skill/practice/day-close").unwrap()));

            let barrier = run_barrier(
                &view,
                &CapsuleId::parse("skill/practice/day-close").unwrap(),
            )
            .expect("a skill Method without executable support refuses");
            assert_eq!(barrier.code, "method.no_executable_body");
            let condition = barrier.condition.as_str();
            let recovery = barrier.recovery.as_str();
            assert!(
                condition.contains("enabled and active"),
                "the condition credits the enablement that landed: {condition}"
            );
            assert!(
                condition.contains("[script]"),
                "the condition names the missing body: {condition}"
            );
            assert!(
                !condition.contains("no scope enables it"),
                "the mislabel is the defect: {condition}"
            );
            assert!(
                recovery.contains("aikit act invoke"),
                "the route names the agent invocation: {recovery}"
            );
        }

        #[test]
        fn an_unenabled_method_names_the_enablement_seam() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(method_capsule("skill/practice/day-close", "skill"));
            let view = view_with(&catalog, &trusted("skill/practice/day-close"), &[]);

            let barrier = run_barrier(
                &view,
                &CapsuleId::parse("skill/practice/day-close").unwrap(),
            )
            .expect("a Method no scope enables refuses");
            assert_eq!(barrier.code, "method.not_enabled");
            let condition = barrier.condition.as_str();
            let recovery = barrier.recovery.as_str();
            assert!(
                condition.contains("no scope enables it in this context"),
                "here the enablement answer is the true one: {condition}"
            );
            assert!(
                recovery.contains("aikit enable"),
                "the route names the enabling command: {recovery}"
            );
        }

        #[test]
        fn an_unreviewed_method_names_the_review_route() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(method_capsule("skill/practice/day-close", "skill"));
            let view = view_with(
                &catalog,
                &MemoryTrust::default(),
                &["skill/practice/day-close"],
            );

            let barrier = run_barrier(
                &view,
                &CapsuleId::parse("skill/practice/day-close").unwrap(),
            )
            .expect("an unreviewed skill Method refuses");
            assert_eq!(barrier.code, "method.withheld");
            let condition = barrier.condition.as_str();
            let recovery = barrier.recovery.as_str();
            assert!(
                condition.contains("not been reviewed"),
                "resolution's own withholding travels: {condition}"
            );
            assert!(
                recovery.contains("aikit trust record"),
                "the route names the review command: {recovery}"
            );
        }

        #[test]
        fn a_script_method_with_support_runs() {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(method_capsule("script/practice/compose", "script"));
            let view = view_with(
                &catalog,
                &MemoryTrust::default(),
                &["script/practice/compose"],
            );

            assert_eq!(
                run_barrier(&view, &CapsuleId::parse("script/practice/compose").unwrap()),
                None,
                "the registered-practice shape is exactly what method run executes"
            );
        }
    }
}
