//! Typed Project reflection over the existing ProjectMap federation.
//!
//! This hardens Knowledge Navigation. It does not create another Wiki, universal
//! graph, or description ontology. Semantic meaning, native descriptions, exact
//! code, verification, and history retain their own authority and are related only
//! by explicit stable ProjectMap bindings.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::project_map::{
    PROJECT_MAP_VERSION, ProjectLens, ProjectMap, ProjectMapEndpoint, ProjectMapStep,
};
use crate::resource::{ResourceRef, SourceAuthority, SourceRef};

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
            authoritative: candidate.declared_authority == Some(SourceAuthority::Authored),
        });
    }

    if candidate.adopted_source.is_some() {
        return LocalSourceClassification::Classified(LocalSourceRoleEvidence {
            role: LocalSourceRole::OrdinarySource,
            evidence: vec!["adopted-source".into()],
            authoritative: candidate.declared_authority == Some(SourceAuthority::Authored),
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
    pub endpoint: ProjectMapEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionRelationView {
    pub endpoint: ProjectMapEndpoint,
    #[serde(default)]
    pub route: Vec<ProjectMapStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReflectionReadModel {
    pub version: String,
    pub project_map_version: String,
    pub subject: Option<ProjectMapEndpoint>,
    #[serde(default)]
    pub meaning: Vec<ReflectionRelationView>,
    #[serde(default)]
    pub descriptions: Vec<ReflectionRelationView>,
    #[serde(default)]
    pub code: Vec<ReflectionRelationView>,
    #[serde(default)]
    pub verification: Vec<ReflectionRelationView>,
    #[serde(default)]
    pub other: Vec<ReflectionRelationView>,
    pub truncated: bool,
}

/// Build a bounded human/Agent reflection neighbourhood from the already-real
/// ProjectMap. Provider-native graphs remain outside this read model; each item
/// is reached only through explicit ProjectMap bindings.
pub fn project_reflection(
    map: &ProjectMap,
    subject: &ResourceRef,
    max_hops: usize,
    limit: usize,
) -> ProjectReflectionReadModel {
    let subject_endpoint = map.endpoint(subject).cloned();
    let mut reachable = Vec::new();

    if subject_endpoint.is_some() && max_hops > 0 && limit > 0 {
        for endpoint in map.endpoints() {
            if &endpoint.resource == subject {
                continue;
            }
            if let Some(route) = map.route(subject, &endpoint.resource, max_hops) {
                reachable.push(ReflectionRelationView {
                    endpoint: endpoint.clone(),
                    route,
                });
            }
        }
        reachable.sort_by(|left, right| {
            (left.route.len(), &left.endpoint.resource)
                .cmp(&(right.route.len(), &right.endpoint.resource))
        });
    }

    let truncated = reachable.len() > limit;
    reachable.truncate(limit);

    let mut meaning = Vec::new();
    let mut descriptions = Vec::new();
    let mut code = Vec::new();
    let mut verification = Vec::new();
    let mut other = Vec::new();

    for item in reachable {
        match item.endpoint.lens {
            ProjectLens::SemanticWiki | ProjectLens::Canon => meaning.push(item),
            ProjectLens::SourcePool => descriptions.push(item),
            ProjectLens::Code | ProjectLens::Git => code.push(item),
            ProjectLens::Verification => verification.push(item),
            _ => other.push(item),
        }
    }

    ProjectReflectionReadModel {
        version: PROJECT_REFLECTION_VERSION.into(),
        project_map_version: PROJECT_MAP_VERSION.into(),
        subject: subject_endpoint,
        meaning,
        descriptions,
        code,
        verification,
        other,
        truncated,
    }
}

/// A target-owned strong reflection assertion. The coordinate is deliberately
/// opaque to AIKit: Epi/QL can use Mx/Mx′ or another formal coordinate while an
/// ordinary Project can use its own stable subject name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionMapping {
    pub coordinate: String,
    pub semantic: ResourceRef,
    pub implementation: ResourceRef,
    pub relation: String,
    #[serde(default)]
    pub description: Option<ResourceRef>,
    #[serde(default)]
    pub description_relation: Option<String>,
    #[serde(default)]
    pub expected_implementation_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReflectionRelationFace {
    Direct,
    Conjugate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstitutiveReflectionRelation {
    pub ref_id: String,
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    pub face: ReflectionRelationFace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionLaw {
    pub id: String,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub source_revision: Option<String>,
    pub unique_implementation: bool,
    #[serde(default)]
    pub mappings: Vec<ReflectionMapping>,
    #[serde(default)]
    pub constitutive_relations: Vec<ConstitutiveReflectionRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    pub law: String,
    pub source_revision: Option<String>,
    pub passed: bool,
    #[serde(default)]
    pub issues: Vec<ReflectionIssue>,
}

impl ReflectionVerification {
    pub fn is_conformant(&self) -> bool {
        self.passed
    }
}

/// Verify a target-owned reflection law against explicit ProjectMap relations.
/// The function compares refs, relation names and optional target-owned revisions;
/// it never parses or invents the target's coordinate semantics.
pub fn verify_reflection_law(map: &ProjectMap, law: &ReflectionLaw) -> ReflectionVerification {
    let mut issues = Vec::new();

    for mapping in &law.mappings {
        let pair: Vec<_> = map
            .bindings()
            .iter()
            .filter(|binding| {
                binding.from == mapping.semantic && binding.to == mapping.implementation
            })
            .collect();

        if pair.is_empty() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Missing,
                relation_ref: mapping.coordinate.clone(),
                detail: format!(
                    "missing reflected binding {} -> {}",
                    mapping.semantic, mapping.implementation
                ),
            });
        } else if !pair.iter().any(|binding| binding.relation == mapping.relation) {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::WrongRelation,
                relation_ref: mapping.coordinate.clone(),
                detail: format!("expected relation {}", mapping.relation),
            });
        }

        if law.unique_implementation {
            let implementations: BTreeSet<_> = map
                .bindings()
                .iter()
                .filter(|binding| {
                    binding.from == mapping.semantic && binding.relation == mapping.relation
                })
                .map(|binding| binding.to.clone())
                .collect();
            if implementations.len() > 1 {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::Duplicate,
                    relation_ref: mapping.coordinate.clone(),
                    detail: format!(
                        "{} implementation targets found for a unique reflection",
                        implementations.len()
                    ),
                });
            }
        }

        match (
            map.endpoint(&mapping.implementation),
            &mapping.expected_implementation_revision,
        ) {
            (None, _) => issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Missing,
                relation_ref: mapping.coordinate.clone(),
                detail: format!("implementation endpoint {} is absent", mapping.implementation),
            }),
            (Some(endpoint), Some(expected))
                if endpoint.revision.as_deref() != Some(expected.as_str()) =>
            {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::Stale,
                    relation_ref: mapping.coordinate.clone(),
                    detail: format!(
                        "implementation revision {:?} does not match expected {}",
                        endpoint.revision, expected
                    ),
                });
            }
            _ => {}
        }

        if let (Some(description), Some(relation)) =
            (&mapping.description, &mapping.description_relation)
        {
            let described = map.bindings().iter().any(|binding| {
                binding.from == *description
                    && binding.to == mapping.implementation
                    && binding.relation == *relation
            });
            if !described {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::Missing,
                    relation_ref: mapping.coordinate.clone(),
                    detail: format!(
                        "description {} does not {} implementation {}",
                        description, relation, mapping.implementation
                    ),
                });
            }
        }
    }

    let expected_constitutive: BTreeSet<_> = law
        .constitutive_relations
        .iter()
        .map(|relation| {
            (
                relation.from.clone(),
                relation.to.clone(),
                relation.relation.clone(),
            )
        })
        .collect();

    for relation in &law.constitutive_relations {
        if relation.face == ReflectionRelationFace::Conjugate && relation.from == relation.to {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::ConstitutiveFlattening,
                relation_ref: relation.ref_id.clone(),
                detail: "conjugate relation collapses both faces onto one resource identity".into(),
            });
        }

        let pair: Vec<_> = map
            .bindings()
            .iter()
            .filter(|binding| binding.from == relation.from && binding.to == relation.to)
            .collect();
        if pair.is_empty() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Missing,
                relation_ref: relation.ref_id.clone(),
                detail: "constitutive relation is absent from ProjectMap".into(),
            });
        } else if !pair
            .iter()
            .any(|binding| binding.relation == relation.relation)
        {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::WrongRelation,
                relation_ref: relation.ref_id.clone(),
                detail: format!("expected relation {}", relation.relation),
            });
        }
    }

    for binding in map.bindings() {
        if binding.relation.starts_with("constitutive:")
            && !expected_constitutive.contains(&(
                binding.from.clone(),
                binding.to.clone(),
                binding.relation.clone(),
            ))
        {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::Stale,
                relation_ref: binding.relation.clone(),
                detail: "ProjectMap carries a constitutive binding absent from the current target-owned law".into(),
            });
        }
    }

    ReflectionVerification {
        law: law.id.clone(),
        source_revision: law.source_revision.clone(),
        passed: issues.is_empty(),
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_map::ProjectMapBinding;
    use crate::resource::ResourceKind;

    fn endpoint(id: &str, lens: ProjectLens) -> ProjectMapEndpoint {
        ProjectMapEndpoint {
            resource: ResourceRef::parse(id).unwrap(),
            kind: ResourceKind::KnowledgeNode,
            lens,
            authority: SourceAuthority::Authored,
            provider: None,
            revision: None,
            label: None,
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
    fn constitutive_conjugate_flattening_is_rejected_without_changing_project_map() {
        let same = ResourceRef::parse("resource:same").unwrap();
        let law = ReflectionLaw {
            id: "law:test".into(),
            source: None,
            source_revision: None,
            unique_implementation: false,
            mappings: vec![],
            constitutive_relations: vec![ConstitutiveReflectionRelation {
                ref_id: "rel:conjugate".into(),
                from: same.clone(),
                to: same,
                relation: "constitutive:conjugate".into(),
                face: ReflectionRelationFace::Conjugate,
            }],
        };
        let mut map = ProjectMap::new();
        map.add_endpoint(endpoint("resource:same", ProjectLens::SemanticWiki))
            .unwrap();
        let verification = verify_reflection_law(&map, &law);
        assert!(!verification.is_conformant());
        assert!(verification
            .issues
            .iter()
            .any(|issue| issue.kind == ReflectionIssueKind::ConstitutiveFlattening));
        assert!(map.bindings().is_empty());
    }

    #[test]
    fn wrong_relation_is_reported_against_existing_project_map_binding() {
        let semantic = ResourceRef::parse("semantic:one").unwrap();
        let code = ResourceRef::parse("code:one").unwrap();
        let mut map = ProjectMap::new();
        map.add_endpoint(endpoint("semantic:one", ProjectLens::SemanticWiki))
            .unwrap();
        map.add_endpoint(endpoint("code:one", ProjectLens::Code))
            .unwrap();
        map.bind(ProjectMapBinding {
            from: semantic.clone(),
            to: code.clone(),
            relation: "mentions".into(),
            reversible: true,
            authority: SourceAuthority::Authored,
            provider: None,
            provenance: vec![],
        })
        .unwrap();
        let law = ReflectionLaw {
            id: "law:one".into(),
            source: None,
            source_revision: None,
            unique_implementation: true,
            mappings: vec![ReflectionMapping {
                coordinate: "subject-one".into(),
                semantic,
                implementation: code,
                relation: "implemented-by".into(),
                description: None,
                description_relation: None,
                expected_implementation_revision: None,
            }],
            constitutive_relations: vec![],
        };
        let verification = verify_reflection_law(&map, &law);
        assert!(verification
            .issues
            .iter()
            .any(|issue| issue.kind == ReflectionIssueKind::WrongRelation));
    }
}
