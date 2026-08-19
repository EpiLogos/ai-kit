//! Typed Project reflection over the existing ProjectMap federation.
//!
//! This hardens Knowledge Navigation. It does not create another Wiki, universal
//! graph, or description ontology. Semantic meaning, native descriptions, exact
//! code, verification, and history retain their own authority and are related only
//! by explicit stable ProjectMap bindings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::project_map::{ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint, ProjectMapStep};
use crate::resource::{ProviderRef, ResourceRef, SourceAuthority, SourceRef};

pub const PROJECT_REFLECTION_VERSION: &str = "aikit.project-reflection/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalSourceRole {
    HumanProjectGround,
    AgentGovernance,
    AgentMaintainedWiki,
    LocalStructuralDescription,
    OrdinarySource,
    DerivedDocumentation,
    CodeIndexObservation,
    TemporalWorkingMaterial,
    Praxis,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceRoleEvidence {
    pub role: LocalSourceRole,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub authoritative: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceCandidate {
    pub path: String,
    #[serde(default)]
    pub declared_role: Option<LocalSourceRole>,
    #[serde(default)]
    pub declared_authority: Option<SourceAuthority>,
    #[serde(default)]
    pub adopted_source: Option<SourceRef>,
    #[serde(default)]
    pub content_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalSourceClassification {
    Classified(LocalSourceRoleEvidence),
    Ambiguous(Vec<LocalSourceRoleEvidence>),
    Unresolved,
}

pub fn classify_local_source(candidate: &LocalSourceCandidate) -> LocalSourceClassification {
    if let Some(role) = candidate.declared_role {
        return LocalSourceClassification::Classified(LocalSourceRoleEvidence {
            role,
            evidence: vec!["declared-role".into()],
            authoritative: candidate
                .declared_authority
                .is_some_and(|authority| authority == SourceAuthority::Authoritative),
        });
    }

    if candidate.adopted_source.is_some() {
        return LocalSourceClassification::Classified(LocalSourceRoleEvidence {
            role: LocalSourceRole::OrdinarySource,
            evidence: vec!["adopted-source".into()],
            authoritative: candidate
                .declared_authority
                .is_some_and(|authority| authority == SourceAuthority::Authoritative),
        });
    }

    let lower_path = candidate.path.to_ascii_lowercase();
    let mut hints = Vec::new();
    if lower_path.ends_with("agents.md") || lower_path.ends_with("claude.md") {
        hints.push(LocalSourceRoleEvidence {
            role: LocalSourceRole::AgentGovernance,
            evidence: vec!["conventional-filename-hint".into()],
            authoritative: false,
        });
    }
    if lower_path.contains("/wiki/") || lower_path.ends_with("wiki.md") {
        hints.push(LocalSourceRoleEvidence {
            role: LocalSourceRole::AgentMaintainedWiki,
            evidence: vec!["path-hint".into()],
            authoritative: false,
        });
    }
    if lower_path.contains("/skills/") || lower_path.contains("/methods/") {
        hints.push(LocalSourceRoleEvidence {
            role: LocalSourceRole::Praxis,
            evidence: vec!["path-hint".into()],
            authoritative: false,
        });
    }
    if lower_path.contains("/now/") || lower_path.contains("/day/") {
        hints.push(LocalSourceRoleEvidence {
            role: LocalSourceRole::TemporalWorkingMaterial,
            evidence: vec!["path-hint".into()],
            authoritative: false,
        });
    }
    if lower_path.contains("generated") || lower_path.contains("derived") {
        hints.push(LocalSourceRoleEvidence {
            role: LocalSourceRole::DerivedDocumentation,
            evidence: vec!["path-hint".into()],
            authoritative: false,
        });
    }

    for hint in &candidate.content_hints {
        let lower = hint.to_ascii_lowercase();
        if lower.contains("human-authored project ground") {
            hints.push(LocalSourceRoleEvidence {
                role: LocalSourceRole::HumanProjectGround,
                evidence: vec!["content-hint".into()],
                authoritative: false,
            });
        }
        if lower.contains("structural source") || lower.contains("coordinate map") {
            hints.push(LocalSourceRoleEvidence {
                role: LocalSourceRole::LocalStructuralDescription,
                evidence: vec!["content-hint".into()],
                authoritative: false,
            });
        }
    }

    hints.sort_by_key(|hint| hint.role);
    hints.dedup_by_key(|hint| hint.role);
    match hints.len() {
        0 => LocalSourceClassification::Unresolved,
        1 => LocalSourceClassification::Classified(hints.remove(0)),
        _ => LocalSourceClassification::Ambiguous(hints),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionResourceView {
    pub resource: ResourceRef,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub authority: Option<SourceAuthority>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionMapping {
    pub from: ProjectMapEndpoint,
    pub to: ProjectMapEndpoint,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReflectionReadModel {
    pub version: String,
    pub project_map_version: String,
    pub lenses: Vec<ProjectLens>,
    pub resources: Vec<ReflectionResourceView>,
    pub mappings: Vec<ReflectionMapping>,
    pub unresolved: Vec<String>,
}

pub fn project_reflection(map: &ProjectMap) -> ProjectReflectionReadModel {
    let mut lenses = BTreeSet::new();
    let mut resources: BTreeMap<ResourceRef, ReflectionResourceView> = BTreeMap::new();
    let mut mappings = Vec::new();
    let mut unresolved = Vec::new();

    for binding in &map.bindings {
        for endpoint in [&binding.from, &binding.to] {
            lenses.insert(endpoint.lens);
            match &endpoint.resource {
                Some(resource) => {
                    resources.entry(resource.clone()).or_insert_with(|| ReflectionResourceView {
                        resource: resource.clone(),
                        provider: endpoint.provider.clone(),
                        source: endpoint.source.clone(),
                        authority: endpoint.authority,
                    });
                }
                None => unresolved.push(format!("{} endpoint has no stable resource ref", endpoint.lens.as_str())),
            }
        }
        mappings.push(ReflectionMapping {
            from: binding.from.clone(),
            to: binding.to.clone(),
            relation: binding.relation.clone(),
        });
    }

    ProjectReflectionReadModel {
        version: PROJECT_REFLECTION_VERSION.into(),
        project_map_version: map.version.clone(),
        lenses: lenses.into_iter().collect(),
        resources: resources.into_values().collect(),
        mappings,
        unresolved,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReflectionRelationFace {
    Direct,
    Conjugate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstitutiveReflectionRelation {
    pub ref_id: String,
    pub from: ProjectMapEndpoint,
    pub to: ProjectMapEndpoint,
    pub relation: String,
    pub face: ReflectionRelationFace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionLaw {
    pub target: String,
    pub source_revision: String,
    pub relations: Vec<ConstitutiveReflectionRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReflectionIssueKind {
    Missing,
    WrongRelation,
    Duplicate,
    Stale,
    ConstitutiveFlattening,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionIssue {
    pub kind: ReflectionIssueKind,
    pub relation_ref: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionVerification {
    pub target: String,
    pub source_revision: String,
    pub issues: Vec<ReflectionIssue>,
}

impl ReflectionVerification {
    pub fn is_conformant(&self) -> bool {
        self.issues.is_empty()
    }
}

pub fn verify_reflection_law(map: &ProjectMap, law: &ReflectionLaw) -> ReflectionVerification {
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();

    for expected in &law.relations {
        let matches: Vec<&ProjectMapBinding> = map
            .bindings
            .iter()
            .filter(|binding| binding.from == expected.from && binding.to == expected.to)
            .collect();
        if matches.is_empty() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Missing,
                relation_ref: expected.ref_id.clone(),
                detail: "constitutive relation absent from ProjectMap".into(),
            });
            continue;
        }
        if matches.len() > 1 {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Duplicate,
                relation_ref: expected.ref_id.clone(),
                detail: format!("{} duplicate bindings found", matches.len()),
            });
        }
        let correct = matches.iter().any(|binding| binding.relation == expected.relation);
        if !correct {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::WrongRelation,
                relation_ref: expected.ref_id.clone(),
                detail: format!("expected relation {}", expected.relation),
            });
        }
        seen.insert((expected.from.clone(), expected.to.clone(), expected.relation.clone()));
    }

    for binding in &map.bindings {
        if binding.relation.starts_with("constitutive:")
            && !seen.contains(&(binding.from.clone(), binding.to.clone(), binding.relation.clone()))
        {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Stale,
                relation_ref: binding.relation.clone(),
                detail: "ProjectMap carries a constitutive binding absent from the current target-owned law".into(),
            });
        }
    }

    let mut adjacency: BTreeMap<ProjectMapEndpoint, VecDeque<ProjectMapEndpoint>> = BTreeMap::new();
    for relation in &law.relations {
        adjacency
            .entry(relation.from.clone())
            .or_default()
            .push_back(relation.to.clone());
    }
    for relation in &law.relations {
        if relation.face != ReflectionRelationFace::Conjugate {
            continue;
        }
        if relation.from.lens == relation.to.lens && relation.from.resource == relation.to.resource {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::ConstitutiveFlattening,
                relation_ref: relation.ref_id.clone(),
                detail: "conjugate relation collapses both faces onto the same ProjectMap identity".into(),
            });
        }
    }

    ReflectionVerification {
        target: law.target.clone(),
        source_revision: law.source_revision.clone(),
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_map::{ProjectMap, ProjectMapEndpoint};

    fn endpoint(lens: ProjectLens, id: &str) -> ProjectMapEndpoint {
        ProjectMapEndpoint {
            lens,
            resource: Some(ResourceRef::parse(id).unwrap()),
            provider: None,
            source: None,
            authority: None,
        }
    }

    #[test]
    fn conventional_agent_instruction_filename_is_only_a_hint() {
        let source = LocalSourceCandidate {
            path: "/project/AGENTS.md".into(),
            declared_role: None,
            declared_authority: None,
            adopted_source: None,
            content_hints: vec![],
        };
        match classify_local_source(&source) {
            LocalSourceClassification::Classified(evidence) => {
                assert_eq!(evidence.role, LocalSourceRole::AgentGovernance);
                assert!(!evidence.authoritative);
            }
            other => panic!("unexpected classification {other:?}"),
        }
    }

    #[test]
    fn constitutive_conjugate_flattening_is_rejected() {
        let same = endpoint(ProjectLens::SemanticWiki, "resource:same");
        let law = ReflectionLaw {
            target: "epi".into(),
            source_revision: "abc123".into(),
            relations: vec![ConstitutiveReflectionRelation {
                ref_id: "rel:conjugate".into(),
                from: same.clone(),
                to: same,
                relation: "constitutive:conjugate".into(),
                face: ReflectionRelationFace::Conjugate,
            }],
        };
        let verification = verify_reflection_law(&ProjectMap {
            version: "aikit.project-map/v1".into(),
            bindings: law
                .relations
                .iter()
                .map(|relation| ProjectMapBinding {
                    from: relation.from.clone(),
                    to: relation.to.clone(),
                    relation: relation.relation.clone(),
                })
                .collect(),
        }, &law);
        assert!(!verification.is_conformant());
        assert!(verification
            .issues
            .iter()
            .any(|issue| issue.kind == ReflectionIssueKind::ConstitutiveFlattening));
    }
}
