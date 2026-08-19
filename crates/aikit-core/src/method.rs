//! Native Method semantics: situated composition without copying the things composed.
//!
//! A Method is deliberately narrower than a Profile and richer than a SkillSet.
//! It records how independently owned praxis/resources relate around a Focus. It
//! does not activate capabilities, confer trust, mutate Skill source, or own
//! Action authority.

use serde::{Deserialize, Serialize};

use crate::resource::{ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, SourceRef, SourceRevision};
use crate::{AikitError, Result};

pub const METHOD_VERSION: &str = "aikit.method/v1";

/// Immutable receipt identifying the scoped adaptation of an unchanged Skill.
///
/// Runtime authoring remains the existing `SkillUsageOverlayPatch` mechanism in
/// Profile/scope resolution. A Method only points at the resulting exact digest;
/// it does not introduce another overlay store.
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
pub struct MethodSkillRef {
    pub skill: ResourceRef,
    #[serde(default)]
    pub usage_overlay: Option<UsageOverlayRef>,
}

/// Source-owned situated praxis composition.
///
/// `source`/`revision` identify where the Method itself came from. Every member
/// remains a stable reference to independently owned source; no member body is
/// copied into this resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Method {
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
    pub skills: Vec<MethodSkillRef>,
    #[serde(default)]
    pub actions: Vec<ResourceRef>,
    #[serde(default)]
    pub capabilities: Vec<ResourceRef>,
    #[serde(default)]
    pub context_sources: Vec<ResourceRef>,
    #[serde(default)]
    pub verification: Vec<ResourceRef>,
    #[serde(default)]
    pub expected_return_forms: Vec<String>,
}

impl Method {
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

    /// A V2 resource record for indexing/search/resolution. The Method body stays
    /// in its source; annotations only expose compact routing/provenance facts.
    pub fn resource_record(&self) -> ResourceRecord {
        let mut descriptor = crate::resource::ResourceDescriptor::new(
            self.id.clone(),
            ResourceKind::Method,
            self.name.clone(),
            self.description.clone(),
        );
        descriptor
            .annotations
            .insert("method.version".into(), METHOD_VERSION.into());
        descriptor
            .annotations
            .insert("method.source".into(), self.source.to_string());
        if let Some(revision) = &self.revision {
            descriptor
                .annotations
                .insert("method.revision".into(), revision.to_string());
        }
        ResourceRecord::new(descriptor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodResolvedRef {
    pub reference: ResourceRef,
    pub expected_kind: ResourceKind,
    #[serde(default)]
    pub actual_kind: Option<ResourceKind>,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodResolution {
    pub method: ResourceRef,
    pub source: SourceRef,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub focus: Vec<MethodResolvedRef>,
    pub project_domain: Vec<MethodResolvedRef>,
    pub skills: Vec<MethodResolvedRef>,
    pub actions: Vec<MethodResolvedRef>,
    pub capabilities: Vec<MethodResolvedRef>,
    pub context_sources: Vec<MethodResolvedRef>,
    pub verification: Vec<MethodResolvedRef>,
    pub overlays: Vec<UsageOverlayRef>,
    pub expected_return_forms: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl MethodResolution {
    pub fn is_complete(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// Resolve Method references against the same V2 resource field used by
/// ContextResolution. This is deliberately observational: it never enables,
/// disables, trusts, orders, or mutates referenced resources.
pub fn resolve_method(method: &Method, resources: &dyn ResourceIndex) -> Result<MethodResolution> {
    method.validate()?;
    let mut warnings = Vec::new();

    let focus = resolve_many(&method.focus, ResourceKind::KnowledgeNode, resources, &mut warnings, false);
    let project_domain = resolve_many(&method.project_domain, ResourceKind::Project, resources, &mut warnings, false);
    // Native Skills are currently V2 Capability resources; Skill identity/source
    // remains the capsule/source system rather than a duplicate ResourceKind.
    let skills = resolve_many(
        &method.skills.iter().map(|value| value.skill.clone()).collect::<Vec<_>>(),
        ResourceKind::Capability,
        resources,
        &mut warnings,
        true,
    );
    let actions = resolve_many(&method.actions, ResourceKind::Action, resources, &mut warnings, true);
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

    Ok(MethodResolution {
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
) -> Vec<MethodResolvedRef> {
    refs.iter()
        .map(|reference| match resources.resource(reference) {
            None => {
                warnings.push(format!(
                    "Method reference {reference} is absent (expected {})",
                    expected.as_str()
                ));
                MethodResolvedRef {
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
                MethodResolvedRef {
                    reference: reference.clone(),
                    expected_kind: expected,
                    actual_kind: Some(record.descriptor.kind),
                    resolved: false,
                }
            }
            Some(record) => MethodResolvedRef {
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
) -> Vec<MethodResolvedRef> {
    refs.iter()
        .map(|reference| match resources.resource(reference) {
            Some(record) => MethodResolvedRef {
                reference: reference.clone(),
                expected_kind: record.descriptor.kind,
                actual_kind: Some(record.descriptor.kind),
                resolved: true,
            },
            None => {
                warnings.push(format!("Method verification reference {reference} is absent"));
                MethodResolvedRef {
                    reference: reference.clone(),
                    expected_kind: ResourceKind::Capability,
                    actual_kind: None,
                    resolved: false,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{MemoryResourceIndex, ResourceDescriptor};

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
        resources.insert(record("cap:wayfinder", ResourceKind::Capability));
        resources.insert(record("action:verify", ResourceKind::Action));
        resources.insert(record("context:project-ground", ResourceKind::ContextSource));
        resources.insert(record("project:demo", ResourceKind::Project));

        let method = Method {
            id: ResourceRef::parse("method:project-change").unwrap(),
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
            expected_return_forms: vec!["evidence".into(), "returned-difference".into()],
        };

        let resolved = resolve_method(&method, &resources).unwrap();
        assert!(resolved.is_complete());
        assert_eq!(resolved.skills.len(), 1);
        assert_eq!(resolved.overlays.len(), 1);
        assert_eq!(resolved.expected_return_forms.len(), 2);
        assert_eq!(method.resource_record().descriptor.kind, ResourceKind::Method);
    }

    #[test]
    fn missing_or_wrong_refs_are_explainable_not_promoted() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("action:not-a-skill", ResourceKind::Action));
        let method = Method {
            id: ResourceRef::parse("method:broken").unwrap(),
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
            expected_return_forms: vec![],
        };
        let resolved = resolve_method(&method, &resources).unwrap();
        assert!(!resolved.is_complete());
        assert_eq!(resolved.warnings.len(), 2);
    }
}
