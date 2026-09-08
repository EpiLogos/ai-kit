//! Method classification and situated metadata for Skills.
//!
//! A Method is not a second resource. It is a Skill whose description starts
//! with [`METHOD_DESCRIPTION_PREFIX`]. The optional situated relations below are
//! metadata about that same Skill identity; they never create a Method identity,
//! source lifecycle, store, or activation path.

use serde::{Deserialize, Serialize};

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

/// The situated payload declared after the prefix, if the description carries
/// one. Detection is prefix-only: an empty payload is still a declared Method
/// (the full description remains the skill's description).
pub fn method_payload(description: &str) -> Option<&str> {
    let trimmed = description.trim_start();
    let rest = trimmed.strip_prefix(METHOD_DESCRIPTION_PREFIX)?;
    Some(rest.trim())
}

/// Immutable receipt identifying the scoped adaptation of an unchanged Skill.
///
/// Runtime authoring remains the existing `SkillUsageOverlayPatch` mechanism in
/// Profile/scope resolution. Situated-use metadata only points at the resulting
/// exact digest; it does not introduce another overlay store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub fn resolve_method(
    method: &Method,
    resources: &dyn ResourceIndex,
) -> Result<MethodResolution> {
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
}
