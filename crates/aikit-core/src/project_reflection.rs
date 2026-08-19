//! Typed Project reflection over the existing ProjectMap federation.
//!
//! This hardens Knowledge Navigation. It does not create another Wiki, universal
//! graph, or description ontology. Semantic meaning, native descriptions, exact
//! code, verification, and history retain their own authority and are related only
//! by explicit stable ProjectMap bindings.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::project_map::{ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint, ProjectMapStep};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority, SourceRef};

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
    /// Source role is metadata, not another hard ProjectLens identity.
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

/// Classify existing source by ownership/provenance and actual role. Conventional
/// filenames are discovery hints only: AGENTS.md, CLAUDE.md, CONTEXT.md, README,
/// ADRs, etc. never acquire human authorship or governance authority by location.
pub fn classify_local_source(candidate: &LocalSourceCandidate) -> LocalSourceClassification {
    if candidate.generated || candidate.authority == SourceAuthority::Generated {
        return classified(
            candidate,
            LocalSourceRole::DerivedDocumentation,
            LocalSourceRoleEvidence::Generated,
            "generated/projection material remains derivative and cannot self-promote to source",
        );
    }
    if let Some(role) = candidate.adopted_role {
        return classified(
            candidate,
            role,
            LocalSourceRoleEvidence::AdoptedRelation,
            "an explicit owning/adoption relation establishes this source role",
        );
    }
    if let Some(role) = candidate.declared_role {
        return classified(
            candidate,
            role,
            LocalSourceRoleEvidence::Declared,
            "the owning source contract explicitly declares this role",
        );
    }

    let metadata = candidate
        .metadata
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let body = candidate.body_excerpt.as_deref().unwrap_or_default().to_lowercase();
    let evidence = format!("{metadata} {body}");

    if candidate.authority == SourceAuthority::Derived
        && ["code-index", "gitnexus", "derived-code"]
            .iter()
            .any(|needle| evidence.contains(needle))
    {
        return classified(
            candidate,
            LocalSourceRole::CodeIndexObservation,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "derived provider evidence identifies structural observation rather than semantic authority",
        );
    }
    if evidence.contains("agent-maintained")
        && (evidence.contains("semantic wiki") || evidence.contains("okf"))
    {
        return classified(
            candidate,
            LocalSourceRole::AgentMaintainedWiki,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "provenance/content identifies Agent-maintained semantic knowledge",
        );
    }
    if candidate.authority == SourceAuthority::Authored
        && ["how agents should", "agent governance", "collaboration law"]
            .iter()
            .any(|needle| evidence.contains(needle))
    {
        return classified(
            candidate,
            LocalSourceRole::AgentGovernance,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "authored provenance plus governance semantics identifies standing Agent guidance",
        );
    }
    if candidate.authority == SourceAuthority::Authored
        && ["project purpose", "why this project exists", "authored project ground"]
            .iter()
            .any(|needle| evidence.contains(needle))
    {
        return classified(
            candidate,
            LocalSourceRole::HumanProjectGround,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "authored provenance plus Project-purpose semantics identifies human Project ground",
        );
    }
    if evidence.contains("local structural description")
        || (evidence.contains("module")
            && ["owns", "interface", "contract"]
                .iter()
                .any(|needle| evidence.contains(needle))
            && ["describes", "applies to"]
                .iter()
                .any(|needle| evidence.contains(needle)))
    {
        return classified(
            candidate,
            LocalSourceRole::LocalStructuralDescription,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content describes local structure/ownership/interface for a code region",
        );
    }
    if evidence.contains("working material") && evidence.contains("day") {
        return classified(
            candidate,
            LocalSourceRole::TemporalWorkingMaterial,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content identifies moving temporal working material rather than stable reference",
        );
    }
    if evidence.contains("method")
        && (evidence.contains("skill") || evidence.contains("contextsource"))
    {
        return classified(
            candidate,
            LocalSourceRole::Praxis,
            LocalSourceRoleEvidence::ProvenanceAndContent,
            "content identifies reusable/contextual praxis composition",
        );
    }

    let path = candidate.path.to_lowercase();
    let mut hints = Vec::new();
    if path.ends_with("agents.md")
        || path.ends_with("claude.md")
        || path.ends_with("context.md")
        || path.ends_with("copilot-instructions.md")
    {
        hints.extend([
            LocalSourceRole::AgentGovernance,
            LocalSourceRole::LocalStructuralDescription,
        ]);
    }
    if path.contains("/adr") || path.contains("architecture") || path.ends_with("readme.md") {
        hints.extend([
            LocalSourceRole::HumanProjectGround,
            LocalSourceRole::LocalStructuralDescription,
            LocalSourceRole::OrdinarySource,
        ]);
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
            reason: "filename/path is only a discovery hint; role and authorship remain unresolved until source evidence establishes them".into(),
        };
    }

    classified(
        candidate,
        LocalSourceRole::OrdinarySource,
        LocalSourceRoleEvidence::Unresolved,
        "no stronger role evidence is present; retain as ordinary source without inventing authority",
    )
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

/// One bounded read model for both human and Agent surfaces. It is intentionally
/// grouped by the questions the user asks rather than exposing a graph hairball.
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

/// Traverse only explicit ProjectMap bindings to a bounded depth. Rich code/wiki
/// provider graphs remain provider-owned and are entered through their native APIs.
pub fn project_reflection(
    map: &ProjectMap,
    anchor: &ResourceRef,
    max_hops: usize,
    max_resources: usize,
) -> ProjectReflectionReadModel {
    let mut view = ProjectReflectionReadModel {
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
        return view;
    }

    let mut seen = BTreeSet::from([anchor.clone()]);
    let mut queue = VecDeque::from([(anchor.clone(), 0usize)]);
    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_hops || seen.len() >= max_resources {
            continue;
        }
        for step in map.neighbours(&current) {
            if seen.len() >= max_resources || !seen.insert(step.to.clone()) {
                continue;
            }
            let Some(endpoint) = map.endpoint(&step.to).cloned() else {
                continue;
            };
            let relation = relation_view(map, &step);
            classify_reflection(&mut view, ReflectionResourceView { endpoint, relation });
            queue.push_back((step.to, depth + 1));
        }
    }

    for values in [
        &mut view.meaning,
        &mut view.descriptions,
        &mut view.code,
        &mut view.verification,
        &mut view.history,
        &mut view.other,
    ] {
        values.sort_by(|left, right| {
            (&left.endpoint.resource, &left.relation.relation)
                .cmp(&(&right.endpoint.resource, &right.relation.relation))
        });
    }
    view
}

fn relation_view(map: &ProjectMap, step: &ProjectMapStep) -> ReflectionRelationView {
    let provenance = map
        .bindings()
        .iter()
        .find(|binding| binding_matches_step(binding, step))
        .map(|binding| binding.provenance.clone())
        .unwrap_or_default();
    ReflectionRelationView {
        from: step.from.clone(),
        to: step.to.clone(),
        relation: step.relation.clone(),
        reversed: step.reversed,
        authority: step.authority,
        provider: step.provider.clone(),
        provenance,
    }
}

fn binding_matches_step(binding: &ProjectMapBinding, step: &ProjectMapStep) -> bool {
    if step.reversed {
        binding.reversible
            && binding.from == step.to
            && binding.to == step.from
            && binding.relation == step.relation
    } else {
        binding.from == step.from && binding.to == step.to && binding.relation == step.relation
    }
}

fn classify_reflection(view: &mut ProjectReflectionReadModel, resource: ReflectionResourceView) {
    let relation = resource.relation.relation.as_str();
    match resource.endpoint.lens {
        ProjectLens::SemanticWiki | ProjectLens::Canon => view.meaning.push(resource),
        ProjectLens::Code | ProjectLens::Git => view.code.push(resource),
        ProjectLens::Verification => view.verification.push(resource),
        ProjectLens::Run | ProjectLens::Decision | ProjectLens::Evolution => view.history.push(resource),
        ProjectLens::SourcePool
            if matches!(
                relation,
                "describes"
                    | "described-by"
                    | "applies-to"
                    | "part-of"
                    | "owned-by"
                    | "constrains"
                    | "implemented-by"
                    | "grounded-in"
            ) => view.descriptions.push(resource),
        _ => view.other.push(resource),
    }
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

/// Target-owned parity law. Generic AIKit never interprets the coordinate names.
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

/// Verify explicit source-declared semantic↔implementation parity. Identical
/// labels are never evidence of parity; the ProjectMap relation must exist.
pub fn verify_reflection_law(map: &ProjectMap, law: &ReflectionLaw) -> ReflectionVerification {
    let mut issues = Vec::new();
    let mut coordinates = BTreeMap::<String, &ReflectionMapping>::new();
    let mut implementation_uses = BTreeMap::<ResourceRef, Vec<String>>::new();

    for mapping in &law.mappings {
        if mapping.coordinate.trim().is_empty()
            || coordinates.insert(mapping.coordinate.clone(), mapping).is_some()
        {
            issues.push(issue(
                ReflectionIssueKind::InvalidLaw,
                &mapping.coordinate,
                "coordinate identity is empty or duplicated in the declared law",
                vec![mapping.semantic.clone(), mapping.implementation.clone()],
            ));
            continue;
        }
        implementation_uses
            .entry(mapping.implementation.clone())
            .or_default()
            .push(mapping.coordinate.clone());

        let semantic = map.endpoint(&mapping.semantic);
        let implementation = map.endpoint(&mapping.implementation);
        if semantic.is_none() {
            issues.push(issue(
                ReflectionIssueKind::MissingSemanticCoordinate,
                &mapping.coordinate,
                "declared semantic coordinate is absent from ProjectMap",
                vec![mapping.semantic.clone()],
            ));
        }
        if implementation.is_none() {
            issues.push(issue(
                ReflectionIssueKind::MissingImplementationCoordinate,
                &mapping.coordinate,
                "declared implementation coordinate is absent from ProjectMap",
                vec![mapping.implementation.clone()],
            ));
        }
        if semantic.is_some()
            && implementation.is_some()
            && !has_binding(map, &mapping.semantic, &mapping.implementation, &mapping.relation)
        {
            issues.push(issue(
                ReflectionIssueKind::WrongMapping,
                &mapping.coordinate,
                &format!("explicit `{}` reflection binding is absent", mapping.relation),
                vec![mapping.semantic.clone(), mapping.implementation.clone()],
            ));
        }
        if let (Some(expected), Some(endpoint)) = (
            mapping.expected_implementation_revision.as_ref(),
            implementation,
        ) {
            if endpoint.revision.as_deref() != Some(expected.as_str()) {
                issues.push(issue(
                    ReflectionIssueKind::StaleMapping,
                    &mapping.coordinate,
                    &format!(
                        "implementation revision changed: expected {expected}, observed {}",
                        endpoint.revision.as_deref().unwrap_or("<unversioned>")
                    ),
                    vec![mapping.implementation.clone()],
                ));
            }
        }
        if let Some(description) = &mapping.description {
            if map.endpoint(description).is_none() {
                issues.push(issue(
                    ReflectionIssueKind::MissingDescription,
                    &mapping.coordinate,
                    "declared local structural description is absent",
                    vec![description.clone()],
                ));
            } else if let Some(relation) = &mapping.description_relation {
                if !has_binding(map, description, &mapping.implementation, relation)
                    && !has_binding(map, description, &mapping.semantic, relation)
                {
                    issues.push(issue(
                        ReflectionIssueKind::MissingDescription,
                        &mapping.coordinate,
                        &format!("description exists but declared `{relation}` relation is absent"),
                        vec![description.clone(), mapping.implementation.clone()],
                    ));
                }
            }
        }
    }

    if law.unique_implementation {
        for (implementation, coords) in implementation_uses {
            if coords.len() > 1 {
                issues.push(issue(
                    ReflectionIssueKind::DuplicateMapping,
                    &coords.join(","),
                    "multiple coordinates map to one implementation while uniqueness is required",
                    vec![implementation],
                ));
            }
        }
    }

    for relation in &law.constitutive_relations {
        let Some(from) = coordinates.get(&relation.from_coordinate) else {
            issues.push(issue(
                ReflectionIssueKind::InvalidLaw,
                &format!("{}→{}", relation.from_coordinate, relation.to_coordinate),
                "constitutive relation from-coordinate is absent from the law",
                vec![],
            ));
            continue;
        };
        let Some(to) = coordinates.get(&relation.to_coordinate) else {
            issues.push(issue(
                ReflectionIssueKind::InvalidLaw,
                &format!("{}→{}", relation.from_coordinate, relation.to_coordinate),
                "constitutive relation to-coordinate is absent from the law",
                vec![],
            ));
            continue;
        };
        let (from_ref, to_ref) = match relation.face {
            ReflectionRelationFace::Semantic => (&from.semantic, &to.semantic),
            ReflectionRelationFace::Implementation => (&from.implementation, &to.implementation),
        };
        if !has_binding(map, from_ref, to_ref, &relation.relation) {
            issues.push(issue(
                ReflectionIssueKind::StructuralFlattening,
                &format!("{}→{}", relation.from_coordinate, relation.to_coordinate),
                &format!(
                    "coordinate names remain but constitutive `{}` relation is absent on {:?} face",
                    relation.relation, relation.face
                ),
                vec![from_ref.clone(), to_ref.clone()],
            ));
        }
    }

    ReflectionVerification {
        law: law.id.clone(),
        passed: issues.is_empty(),
        issues,
    }
}

fn issue(
    kind: ReflectionIssueKind,
    coordinate: &str,
    detail: &str,
    resources: Vec<ResourceRef>,
) -> ReflectionIssue {
    ReflectionIssue {
        kind,
        coordinate: coordinate.into(),
        detail: detail.into(),
        resources,
    }
}

fn has_binding(map: &ProjectMap, from: &ResourceRef, to: &ResourceRef, relation: &str) -> bool {
    map.bindings().iter().any(|binding| {
        (binding.from == *from && binding.to == *to && binding.relation == relation)
            || (binding.reversible
                && binding.from == *to
                && binding.to == *from
                && binding.relation == relation)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(
        id: &str,
        lens: ProjectLens,
        kind: ResourceKind,
        authority: SourceAuthority,
    ) -> ProjectMapEndpoint {
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
    fn filename_is_hint_not_authority() {
        let result = classify_local_source(&LocalSourceCandidate {
            source: SourceRef::parse("source:agents").unwrap(),
            path: "crates/core/AGENTS.md".into(),
            authority: SourceAuthority::Observed,
            declared_role: None,
            adopted_role: None,
            generated: false,
            body_excerpt: None,
            metadata: BTreeMap::new(),
        });
        assert_eq!(result.role, LocalSourceRole::Unresolved);
        assert_eq!(result.evidence, LocalSourceRoleEvidence::FilenameHintOnly);
        assert!(result.candidates.contains(&LocalSourceRole::AgentGovernance));
        assert!(result
            .candidates
            .contains(&LocalSourceRole::LocalStructuralDescription));
    }

    #[test]
    fn description_role_does_not_grow_project_lens() {
        let result = classify_local_source(&LocalSourceCandidate {
            source: SourceRef::parse("source:module-contract").unwrap(),
            path: "src/CONTEXT.md".into(),
            authority: SourceAuthority::Authored,
            declared_role: Some(LocalSourceRole::LocalStructuralDescription),
            adopted_role: None,
            generated: false,
            body_excerpt: None,
            metadata: BTreeMap::new(),
        });
        assert_eq!(result.project_lens, ProjectLens::SourcePool);
        assert_eq!(result.evidence, LocalSourceRoleEvidence::Declared);
    }

    #[test]
    fn read_model_traverses_meaning_description_code_and_proof_both_ways() {
        let mut map = ProjectMap::new();
        for value in [
            endpoint("canon:why", ProjectLens::Canon, ResourceKind::KnowledgeSource, SourceAuthority::Authored),
            endpoint("wiki:concept", ProjectLens::SemanticWiki, ResourceKind::KnowledgeNode, SourceAuthority::Authored),
            endpoint("source:module", ProjectLens::SourcePool, ResourceKind::KnowledgeSource, SourceAuthority::Authored),
            endpoint("code:impl", ProjectLens::Code, ResourceKind::CodeReference, SourceAuthority::Derived),
            endpoint("verify:test", ProjectLens::Verification, ResourceKind::Action, SourceAuthority::Observed),
            endpoint("run:42", ProjectLens::Run, ResourceKind::KnowledgeRoute, SourceAuthority::Observed),
        ] {
            map.add_endpoint(value).unwrap();
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
            .any(|item| item.endpoint.resource.as_str() == "wiki:concept"));
        assert!(from_code
            .descriptions
            .iter()
            .any(|item| item.endpoint.resource.as_str() == "source:module"));
    }

    #[test]
    fn target_owned_law_catches_staleness_and_structural_flattening() {
        let mut map = ProjectMap::new();
        for (id, lens, authority) in [
            ("wiki:a", ProjectLens::SemanticWiki, SourceAuthority::Authored),
            ("wiki:b", ProjectLens::SemanticWiki, SourceAuthority::Authored),
            ("code:a", ProjectLens::Code, SourceAuthority::Derived),
            ("code:b", ProjectLens::Code, SourceAuthority::Derived),
        ] {
            let mut value = endpoint(
                id,
                lens,
                if lens == ProjectLens::Code {
                    ResourceKind::CodeReference
                } else {
                    ResourceKind::KnowledgeNode
                },
                authority,
            );
            if id == "code:a" {
                value.revision = Some("new-revision".into());
            }
            map.add_endpoint(value).unwrap();
        }
        bind(&mut map, "wiki:a", "code:a", "implemented-by");
        bind(&mut map, "wiki:b", "code:b", "implemented-by");

        let law = ReflectionLaw {
            id: "target-owned-law".into(),
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
        assert!(result
            .issues
            .iter()
            .any(|item| item.kind == ReflectionIssueKind::StaleMapping));
        assert!(result
            .issues
            .iter()
            .any(|item| item.kind == ReflectionIssueKind::StructuralFlattening));
    }
}
