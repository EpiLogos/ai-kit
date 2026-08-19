//! Project reflection over the existing ProjectMap federation.
//!
//! This module hardens Knowledge Navigation; it does not create another graph or
//! wiki. Meaning, local structural description, code, verification and history
//! remain differently authoritative resources joined only by explicit ProjectMap
//! bindings. The read model is deliberately bounded and presentation-friendly.

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

impl LocalSourceRole {
    /// Existing ProjectLens is sufficient: source role is metadata about a source,
    /// not a reason to grow another hard lens identity.
    pub fn project_lens(self) -> ProjectLens {
        match self {
            Self::HumanProjectGround => ProjectLens::Canon,
            Self::AgentMaintainedWiki => ProjectLens::SemanticWiki,
            Self::CodeIndexObservation => ProjectLens::Code,
            Self::AgentGovernance
            | Self::LocalStructuralDescription
            | Self::OrdinarySource
            | Self::DerivedDocumentation
            | Self::TemporalWorkingMaterial
            | Self::Praxis
            | Self::Unresolved => ProjectLens::SourcePool,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalSourceRoleEvidence {
    Declared,
    AdoptedRelation,
    ProvenanceAndContent,
    Generated,
    FilenameHintOnly,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceCandidate {
    pub source: SourceRef,
    pub path: String,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub declared_role: Option<LocalSourceRole>,
    #[serde(default)]
    pub adopted_role: Option<LocalSourceRole>,
    #[serde(default)]
    pub generated: bool,
    #[serde(default)]
    pub body_excerpt: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceClassification {
    pub source: SourceRef,
    pub role: LocalSourceRole,
    pub evidence: LocalSourceRoleEvidence,
    pub project_lens: ProjectLens,
    #[serde(default)]
    pub candidates: Vec<LocalSourceRole>,
    pub reason: String,
}

/// Classify by explicit source relation/provenance first, content second, and
/// filename only as an unresolved hint. A conventional filename never acquires
/// human authorship or governance authority by itself.
pub fn classify_local_source(candidate: &LocalSourceCandidate) -> LocalSourceClassification {
    if candidate.generated || candidate.authority == SourceAuthority::Generated {
        return classified(
            candidate,
            LocalSourceRole::DerivedDocumentation,
            LocalSourceRoleEvidence::Generated,
            "source is explicitly generated; projection/index output cannot self-promote into authored source",
        );
    }
    if let Some(role) = candidate.adopted_role {
        return classified(
            candidate,
            role,
            LocalSourceRoleEvidence::AdoptedRelation,
            "source role comes from an explicit adopted/retained relation",
        );
    }
    if let Some(role) = candidate.declared_role {
        return classified(
            candidate,
            role,
            LocalSourceRoleEvidence::Declared,
            "source role is explicitly declared by its owning source contract",
        );
    }

    let metadata = candidate
        .metadata
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let body = candidate
        .body_excerpt
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let combined = format!("{metadata} {body}");

    if candidate.authority == SourceAuthority::Derived
        && (combined.contains("code-index")
            || combined.contains("gitnexus")
            || combined.contains("derived-code"))
    {
        return classified(
            candidate,
            LocalSourceRole::CodeIndexObservation,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "derived provenance plus code-index provider evidence identifies an observation, not semantic authority",
        );
    }
    if combined.contains("agent-maintained")
        && (combined.contains("semantic wiki") || combined.contains("okf"))
    {
        return classified(
            candidate,
            LocalSourceRole::AgentMaintainedWiki,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content/provenance explicitly identifies Agent-maintained semantic knowledge",
        );
    }
    if candidate.authority == SourceAuthority::Authored
        && (combined.contains("how agents should")
            || combined.contains("agent governance")
            || combined.contains("collaboration law"))
    {
        return classified(
            candidate,
            LocalSourceRole::AgentGovernance,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "authored provenance plus governance semantics identifies standing Agent guidance",
        );
    }
    if candidate.authority == SourceAuthority::Authored
        && (combined.contains("project purpose")
            || combined.contains("why this project exists")
            || combined.contains("authored project ground"))
    {
        return classified(
            candidate,
            LocalSourceRole::HumanProjectGround,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "authored provenance plus explicit project-purpose semantics identifies human Project ground",
        );
    }
    if combined.contains("local structural description")
        || (combined.contains("module")
            && (combined.contains("owns")
                || combined.contains("interface")
                || combined.contains("contract"))
            && (combined.contains("describes") || combined.contains("applies to")))
    {
        return classified(
            candidate,
            LocalSourceRole::LocalStructuralDescription,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content describes the local structure/ownership/interface of a code region",
        );
    }
    if combined.contains("working material") && combined.contains("day") {
        return classified(
            candidate,
            LocalSourceRole::TemporalWorkingMaterial,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content identifies moving temporal working material rather than stable reference source",
        );
    }
    if combined.contains("method")
        && (combined.contains("skill") || combined.contains("contextsource"))
    {
        return classified(
            candidate,
            LocalSourceRole::Praxis,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content describes reusable/contextual praxis composition",
        );
    }

    let path = candidate.path.to_lowercase();
    let mut hints = Vec::new();
    if path.ends_with("agents.md")
        || path.ends_with("claude.md")
        || path.ends_with("context.md")
        || path.ends_with("copilot-instructions.md")
    {
        hints.push(LocalSourceRole::AgentGovernance);
        hints.push(LocalSourceRole::LocalStructuralDescription);
    }
    if path.contains("/adr") || path.contains("architecture") || path.ends_with("readme.md") {
        hints.push(LocalSourceRole::HumanProjectGround);
        hints.push(LocalSourceRole::LocalStructuralDescription);
        hints.push(LocalSourceRole::OrdinarySource);
    }
    hints.sort();
    hints.dedup();

    if !hints.is_empty() {
        return LocalSourceClassification {
            source: candidate.source.clone(),
            role: LocalSourceRole::Unresolved,
            evidence: LocalSourceRoleEvidence::FilenameHintOnly,
            project_lens: ProjectLens::SourcePool,
            candidates: hints,
            reason: "filename/path is only a discovery hint; role and authorship remain unresolved until source evidence or an owning relation establishes them".into(),
        };
    }

    LocalSourceClassification {
        source: candidate.source.clone(),
        role: LocalSourceRole::OrdinarySource,
        evidence: LocalSourceRoleEvidence::Unresolved,
        project_lens: ProjectLens::SourcePool,
        candidates: Vec::new(),
        reason: "no stronger role evidence is present; retain as ordinary source without inventing authority".into(),
    }
}

fn classified(
    candidate: &LocalSourceCandidate,
    role: LocalSourceRole,
    evidence: LocalSourceRoleEvidence,
    reason: &str,
) -> LocalSourceClassification {
    LocalSourceClassification {
        source: candidate.source.clone(),
        role,
        evidence,
        project_lens: role.project_lens(),
        candidates: Vec::new(),
        reason: reason.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionRelationView {
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    pub reversed: bool,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub provenance: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionResourceView {
    pub endpoint: ProjectMapEndpoint,
    pub relation: ReflectionRelationView,
}

/// Pithy bounded disclosure used by human and Agent surfaces alike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReflectionReadModel {
    pub version: String,
    pub anchor: ResourceRef,
    #[serde(default)]
    pub meaning: Vec<ReflectionResourceView>,
    #[serde(default)]
    pub descriptions: Vec<ReflectionResourceView>,
    #[serde(default)]
    pub code: Vec<ReflectionResourceView>,
    #[serde(default)]
    pub verification: Vec<ReflectionResourceView>,
    #[serde(default)]
    pub history: Vec<ReflectionResourceView>,
    #[serde(default)]
    pub other: Vec<ReflectionResourceView>,
}

/// Follow only explicit ProjectMap cross-lens bindings to a small bounded depth.
/// Provider-native graphs stay in their providers and are not copied here.
pub fn project_reflection(
    map: &ProjectMap,
    anchor: &ResourceRef,
    max_hops: usize,
    max_resources: usize,
) -> ProjectReflectionReadModel {
    let mut result = ProjectReflectionReadModel {
        version: PROJECT_REFLECTION_VERSION.into(),
        anchor: anchor.clone(),
        meaning: Vec::new(),
        descriptions: Vec::new(),
        code: Vec::new(),
        verification: Vec::new(),
        history: Vec::new(),
        other: Vec::new(),
    };
    if max_hops == 0 || max_resources == 0 || map.endpoint(anchor).is_none() {
        return result;
    }

    let mut seen = BTreeSet::from([anchor.clone()]);
    let mut queue = VecDeque::from([(anchor.clone(), 0usize)]);
    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_hops {
            continue;
        }
        for step in map.neighbours(&current) {
            if seen.len() > max_resources || !seen.insert(step.to.clone()) {
                continue;
            }
            let Some(endpoint) = map.endpoint(&step.to).cloned() else {
                continue;
            };
            let relation = relation_view(map, &step);
            let view = ReflectionResourceView { endpoint, relation };
            classify_reflection_resource(&mut result, view);
            if seen.len() >= max_resources {
                break;
            }
            queue.push_back((step.to, depth + 1));
        }
    }

    sort_views(&mut result.meaning);
    sort_views(&mut result.descriptions);
    sort_views(&mut result.code);
    sort_views(&mut result.verification);
    sort_views(&mut result.history);
    sort_views(&mut result.other);
    result
}

fn relation_view(map: &ProjectMap, step: &ProjectMapStep) -> ReflectionRelationView {
    let binding = binding_for_step(map, step);
    ReflectionRelationView {
        from: step.from.clone(),
        to: step.to.clone(),
        relation: step.relation.clone(),
        reversed: step.reversed,
        authority: step.authority,
        provider: step.provider.clone(),
        provenance: binding.map(|value| value.provenance.clone()).unwrap_or_default(),
    }
}

fn binding_for_step<'a>(map: &'a ProjectMap, step: &ProjectMapStep) -> Option<&'a ProjectMapBinding> {
    map.bindings().iter().find(|binding| {
        if step.reversed {
            binding.from == step.to
                && binding.to == step.from
                && binding.relation == step.relation
                && binding.reversible
        } else {
            binding.from == step.from
                && binding.to == step.to
                && binding.relation == step.relation
        }
    })
}

fn classify_reflection_resource(result: &mut ProjectReflectionReadModel, view: ReflectionResourceView) {
    let relation = view.relation.relation.as_str();
    match view.endpoint.lens {
        ProjectLens::SemanticWiki | ProjectLens::Canon => result.meaning.push(view),
        ProjectLens::Code | ProjectLens::Git => result.code.push(view),
        ProjectLens::Verification => result.verification.push(view),
        ProjectLens::Run | ProjectLens::Decision | ProjectLens::Evolution => result.history.push(view),
        ProjectLens::SourcePool
            if matches!(
                relation,
                "describes"
                    | "applies-to"
                    | "part-of"
                    | "owned-by"
                    | "constrains"
                    | "grounded-in"
                    | "implemented-by"
            ) => result.descriptions.push(view),
        _ => result.other.push(view),
    }
}

fn sort_views(values: &mut [ReflectionResourceView]) {
    values.sort_by(|left, right| {
        (&left.endpoint.resource, &left.relation.relation)
            .cmp(&(&right.endpoint.resource, &right.relation.relation))
    });
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionMapping {
    pub coordinate: String,
    pub semantic: ResourceRef,
    pub implementation: ResourceRef,
    #[serde(default = "implemented_by")]
    pub relation: String,
    #[serde(default)]
    pub description: Option<ResourceRef>,
    #[serde(default)]
    pub description_relation: Option<String>,
    #[serde(default)]
    pub expected_implementation_revision: Option<String>,
}

fn implemented_by() -> String {
    "implemented-by".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReflectionRelationFace {
    Semantic,
    Implementation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstitutiveReflectionRelation {
    pub from_coordinate: String,
    pub to_coordinate: String,
    pub relation: String,
    pub face: ReflectionRelationFace,
}

/// Target-owned law projected into AIKit for conformance. The generic primitive
/// does not know what the coordinate names mean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionLaw {
    pub id: String,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub unique_implementation: bool,
    pub mappings: Vec<ReflectionMapping>,
    #[serde(default)]
    pub constitutive_relations: Vec<ConstitutiveReflectionRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReflectionIssueKind {
    MissingSemanticCoordinate,
    MissingImplementationCoordinate,
    WrongMapping,
    DuplicateMapping,
    StaleMapping,
    MissingDescription,
    StructuralFlattening,
    InvalidLaw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionIssue {
    pub kind: ReflectionIssueKind,
    pub coordinate: String,
    pub detail: String,
    #[serde(default)]
    pub resources: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionVerification {
    pub law: String,
    pub passed: bool,
    #[serde(default)]
    pub issues: Vec<ReflectionIssue>,
}

/// Verify source-declared coordinate reflection against explicit ProjectMap
/// bindings. Label equality never counts as parity.
pub fn verify_reflection_law(map: &ProjectMap, law: &ReflectionLaw) -> ReflectionVerification {
    let mut issues = Vec::new();
    let mut by_coordinate = BTreeMap::<String, &ReflectionMapping>::new();
    let mut implementations = BTreeMap::<ResourceRef, Vec<String>>::new();

    for mapping in &law.mappings {
        if mapping.coordinate.trim().is_empty() || by_coordinate.insert(mapping.coordinate.clone(), mapping).is_some() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::InvalidLaw,
                coordinate: mapping.coordinate.clone(),
                detail: "coordinate identity is empty or duplicated in the declared reflection law".into(),
                resources: vec![mapping.semantic.clone(), mapping.implementation.clone()],
            });
            continue;
        }
        implementations
            .entry(mapping.implementation.clone())
            .or_default()
            .push(mapping.coordinate.clone());

        if map.endpoint(&mapping.semantic).is_none() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::MissingSemanticCoordinate,
                coordinate: mapping.coordinate.clone(),
                detail: "declared semantic coordinate is absent from ProjectMap".into(),
                resources: vec![mapping.semantic.clone()],
            });
        }
        let implementation_endpoint = map.endpoint(&mapping.implementation);
        if implementation_endpoint.is_none() {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::MissingImplementationCoordinate,
                coordinate: mapping.coordinate.clone(),
                detail: "declared implementation coordinate is absent from ProjectMap".into(),
                resources: vec![mapping.implementation.clone()],
            });
        }
        if implementation_endpoint.is_some()
            && map.endpoint(&mapping.semantic).is_some()
            && !has_binding(map, &mapping.semantic, &mapping.implementation, &mapping.relation)
        {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::WrongMapping,
                coordinate: mapping.coordinate.clone(),
                detail: format!(
                    "semantic and implementation coordinates exist but explicit `{}` reflection binding is absent",
                    mapping.relation
                ),
                resources: vec![mapping.semantic.clone(), mapping.implementation.clone()],
            });
        }
        if let (Some(expected), Some(endpoint)) = (
            mapping.expected_implementation_revision.as_ref(),
            implementation_endpoint,
        ) {
            if endpoint.revision.as_deref() != Some(expected.as_str()) {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::StaleMapping,
                    coordinate: mapping.coordinate.clone(),
                    detail: format!(
                        "implementation revision changed: expected {expected}, observed {}",
                        endpoint.revision.as_deref().unwrap_or("<unversioned>")
                    ),
                    resources: vec![mapping.implementation.clone()],
                });
            }
        }
        if let Some(description) = &mapping.description {
            if map.endpoint(description).is_none() {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::MissingDescription,
                    coordinate: mapping.coordinate.clone(),
                    detail: "declared local structural description is absent".into(),
                    resources: vec![description.clone()],
                });
            } else if let Some(relation) = &mapping.description_relation {
                if !has_binding(map, description, &mapping.implementation, relation)
                    && !has_binding(map, description, &mapping.semantic, relation)
                {
                    issues.push(ReflectionIssue {
                        kind: ReflectionIssueKind::MissingDescription,
                        coordinate: mapping.coordinate.clone(),
                        detail: format!(
                            "local description exists but its declared `{relation}` relation is absent"
                        ),
                        resources: vec![description.clone(), mapping.implementation.clone()],
                    });
                }
            }
        }
    }

    if law.unique_implementation {
        for (implementation, coordinates) in implementations {
            if coordinates.len() > 1 {
                issues.push(ReflectionIssue {
                    kind: ReflectionIssueKind::DuplicateMapping,
                    coordinate: coordinates.join(","),
                    detail: "multiple declared coordinates map to one implementation while uniqueness is required".into(),
                    resources: vec![implementation],
                });
            }
        }
    }

    for relation in &law.constitutive_relations {
        let Some(from) = by_coordinate.get(&relation.from_coordinate) else {
            issues.push(invalid_relation_issue(relation, "from-coordinate is absent from the law"));
            continue;
        };
        let Some(to) = by_coordinate.get(&relation.to_coordinate) else {
            issues.push(invalid_relation_issue(relation, "to-coordinate is absent from the law"));
            continue;
        };
        let (from_ref, to_ref) = match relation.face {
            ReflectionRelationFace::Semantic => (&from.semantic, &to.semantic),
            ReflectionRelationFace::Implementation => (&from.implementation, &to.implementation),
        };
        if !has_binding(map, from_ref, to_ref, &relation.relation) {
            issues.push(ReflectionIssue {
                kind: ReflectionIssueKind::StructuralFlattening,
                coordinate: format!("{}→{}", relation.from_coordinate, relation.to_coordinate),
                detail: format!(
                    "coordinate labels remain addressable but constitutive `{}` relation is absent on the {:?} face",
                    relation.relation, relation.face
                ),
                resources: vec![from_ref.clone(), to_ref.clone()],
            });
        }
    }

    ReflectionVerification {
        law: law.id.clone(),
        passed: issues.is_empty(),
        issues,
    }
}

fn invalid_relation_issue(relation: &ConstitutiveReflectionRelation, detail: &str) -> ReflectionIssue {
    ReflectionIssue {
        kind: ReflectionIssueKind::InvalidLaw,
        coordinate: format!("{}→{}", relation.from_coordinate, relation.to_coordinate),
        detail: detail.into(),
        resources: Vec::new(),
    }
}

fn has_binding(map: &ProjectMap, from: &ResourceRef, to: &ResourceRef, relation: &str) -> bool {
    map.bindings().iter().any(|binding| {
        binding.from == *from && binding.to == *to && binding.relation == relation
            || binding.reversible
                && binding.from == *to
                && binding.to == *from
                && binding.relation == relation
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_map::{ProjectMapBinding, ProjectMapEndpoint};
    use crate::resource::ResourceKind;

    fn endpoint(id: &str, lens: ProjectLens, kind: ResourceKind, authority: SourceAuthority) -> ProjectMapEndpoint {
        ProjectMapEndpoint {
            resource: ResourceRef::parse(id).unwrap(),
            kind,
            lens,
            authority,
            provider: None,
            revision: None,
            label: Some(id.into()),
        }
    }

    fn bind(map: &mut ProjectMap, from: &str, to: &str, relation: &str) {
        map.bind(ProjectMapBinding {
            from: ResourceRef::parse(from).unwrap(),
            to: ResourceRef::parse(to).unwrap(),
            relation: relation.into(),
            reversible: true,
            authority: SourceAuthority::Authored,
            provider: None,
            provenance: vec![ResourceRef::parse("source:relation").unwrap()],
        })
        .unwrap();
    }

    #[test]
    fn conventional_filename_is_only_a_hint() {
        let classification = classify_local_source(&LocalSourceCandidate {
            source: SourceRef::parse("source:agents").unwrap(),
            path: "crates/core/AGENTS.md".into(),
            authority: SourceAuthority::Observed,
            declared_role: None,
            adopted_role: None,
            generated: false,
            body_excerpt: None,
            metadata: BTreeMap::new(),
        });
        assert_eq!(classification.role, LocalSourceRole::Unresolved);
        assert_eq!(classification.evidence, LocalSourceRoleEvidence::FilenameHintOnly);
        assert!(classification.candidates.contains(&LocalSourceRole::AgentGovernance));
        assert!(classification
            .candidates
            .contains(&LocalSourceRole::LocalStructuralDescription));
    }

    #[test]
    fn declared_local_description_stays_an_ordinary_source_lens() {
        let classification = classify_local_source(&LocalSourceCandidate {
            source: SourceRef::parse("source:module-contract").unwrap(),
            path: "crates/core/CONTEXT.md".into(),
            authority: SourceAuthority::Authored,
            declared_role: Some(LocalSourceRole::LocalStructuralDescription),
            adopted_role: None,
            generated: false,
            body_excerpt: None,
            metadata: BTreeMap::new(),
        });
        assert_eq!(classification.role, LocalSourceRole::LocalStructuralDescription);
        assert_eq!(classification.project_lens, ProjectLens::SourcePool);
        assert_eq!(classification.evidence, LocalSourceRoleEvidence::Declared);
    }

    #[test]
    fn reflection_read_model_is_bidirectional_and_pithy() {
        let mut map = ProjectMap::new();
        for endpoint in [
            endpoint("canon:why", ProjectLens::Canon, ResourceKind::KnowledgeSource, SourceAuthority::Authored),
            endpoint("wiki:concept", ProjectLens::SemanticWiki, ResourceKind::KnowledgeNode, SourceAuthority::Authored),
            endpoint("source:module", ProjectLens::SourcePool, ResourceKind::KnowledgeSource, SourceAuthority::Authored),
            endpoint("code:impl", ProjectLens::Code, ResourceKind::CodeReference, SourceAuthority::Derived),
            endpoint("verify:test", ProjectLens::Verification, ResourceKind::Action, SourceAuthority::Observed),
            endpoint("run:42", ProjectLens::Run, ResourceKind::KnowledgeRoute, SourceAuthority::Observed),
        ] {
            map.add_endpoint(endpoint).unwrap();
        }
        bind(&mut map, "canon:why", "wiki:concept", "grounded-in");
        bind(&mut map, "wiki:concept", "source:module", "described-by");
        bind(&mut map, "source:module", "code:impl", "describes");
        bind(&mut map, "code:impl", "verify:test", "verified-by");
        bind(&mut map, "verify:test", "run:42", "evidenced-by");

        let from_meaning = project_reflection(
            &map,
            &ResourceRef::parse("wiki:concept").unwrap(),
            4,
            12,
        );
        assert!(!from_meaning.meaning.is_empty());
        assert!(!from_meaning.descriptions.is_empty());
        assert!(!from_meaning.code.is_empty());
        assert!(!from_meaning.verification.is_empty());

        let from_code = project_reflection(
            &map,
            &ResourceRef::parse("code:impl").unwrap(),
            3,
            12,
        );
        assert!(from_code
            .meaning
            .iter()
            .any(|value| value.endpoint.resource.as_str() == "wiki:concept"));
        assert!(from_code
            .descriptions
            .iter()
            .any(|value| value.endpoint.resource.as_str() == "source:module"));
    }

    #[test]
    fn reflection_verification_catches_stale_and_structurally_flattened_mapping() {
        let mut map = ProjectMap::new();
        let semantic_a = endpoint("wiki:a", ProjectLens::SemanticWiki, ResourceKind::KnowledgeNode, SourceAuthority::Authored);
        let semantic_b = endpoint("wiki:b", ProjectLens::SemanticWiki, ResourceKind::KnowledgeNode, SourceAuthority::Authored);
        let mut impl_a = endpoint("code:a", ProjectLens::Code, ResourceKind::CodeReference, SourceAuthority::Derived);
        impl_a.revision = Some("new-revision".into());
        let impl_b = endpoint("code:b", ProjectLens::Code, ResourceKind::CodeReference, SourceAuthority::Derived);
        for value in [semantic_a, semantic_b, impl_a, impl_b] {
            map.add_endpoint(value).unwrap();
        }
        bind(&mut map, "wiki:a", "code:a", "implemented-by");
        bind(&mut map, "wiki:b", "code:b", "implemented-by");
        // Intentionally omit the declared constitutive parent relation.

        let law = ReflectionLaw {
            id: "target-owned-coordinate-law".into(),
            source: Some(SourceRef::parse("source:coordinate-law").unwrap()),
            source_revision: Some("law-v1".into()),
            unique_implementation: true,
            mappings: vec![
                ReflectionMapping {
                    coordinate: "A".into(),
                    semantic: ResourceRef::parse("wiki:a").unwrap(),
                    implementation: ResourceRef::parse("code:a").unwrap(),
                    relation: "implemented-by".into(),
                    description: None,
                    description_relation: None,
                    expected_implementation_revision: Some("old-revision".into()),
                },
                ReflectionMapping {
                    coordinate: "B".into(),
                    semantic: ResourceRef::parse("wiki:b").unwrap(),
                    implementation: ResourceRef::parse("code:b").unwrap(),
                    relation: "implemented-by".into(),
                    description: None,
                    description_relation: None,
                    expected_implementation_revision: None,
                },
            ],
            constitutive_relations: vec![ConstitutiveReflectionRelation {
                from_coordinate: "A".into(),
                to_coordinate: "B".into(),
                relation: "parent-of".into(),
                face: ReflectionRelationFace::Implementation,
            }],
        };

        let result = verify_reflection_law(&map, &law);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|issue| issue.kind == ReflectionIssueKind::StaleMapping));
        assert!(result.issues.iter().any(|issue| issue.kind == ReflectionIssueKind::StructuralFlattening));
    }
}
