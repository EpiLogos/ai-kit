//! Source-facing compilation for authored Wiki relations.
//!
//! This adapter joins ordinary retained Markdown, optional open OKF Properties,
//! stable owner/source identity and the existing `okf-wiki/v1` relation model.
//! It does not create another graph or backlink store: resolved authored evidence
//! becomes ordinary `WikiEdge(origin=Authored)` input to `SemanticWikiIndex`, while
//! unresolved/ambiguous evidence remains an inspectable source read model.

use std::collections::BTreeSet;

use aikit_core::knowledge_living::KnowledgeDependency;
use aikit_core::knowledge_okf::{
    materialize_authored_wiki_edge, okf_wiki_source_profile, resolve_authored_relation,
    AuthoredRelationCandidate, AuthoredRelationEvidence, AuthoredRelationResolution,
    OkfWikiSourceProfile,
};
use aikit_core::knowledge_wiki::{WikiEdge, WikiObject};
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::okf::{parse_authored_markdown_relations, parse_okf_markdown};

pub const AUTHORED_WIKI_SOURCE_VERSION: &str = "aikit.authored-wiki-source/v1";

/// One ordinary source interpreted as a semantic relation subject.
///
/// `subject_ref` and `source_ref` are deliberately distinct. A Flow therefore
/// participates by its stable FlowRef while its retained file/source identity
/// remains exact provenance, without reclassifying the Flow as a WikiNode.
///
/// `source_authority` is also deliberately distinct from `WikiEdgeOrigin::Authored`:
/// the latter says that the relation was explicit in source language; the former
/// says what epistemic standing the owning source currently has. An Agent-maintained
/// Flow can therefore contain an explicit authored link without that link being
/// misrepresented as human-authored authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredWikiSourceProjection {
    pub version: String,
    pub subject_ref: ResourceRef,
    pub source_ref: SourceRef,
    pub source_authority: SourceAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub locators: Vec<String>,
    #[serde(default)]
    pub relations: Vec<AuthoredRelationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAuthoredRelation {
    pub subject_ref: ResourceRef,
    pub source_authority: SourceAuthority,
    pub evidence: AuthoredRelationEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredWikiRelationCompilation {
    pub version: String,
    #[serde(default)]
    pub edges: Vec<WikiEdge>,
    #[serde(default)]
    pub pending: Vec<PendingAuthoredRelation>,
    /// Parsing/resolution/compilation is deterministic and never invokes a model.
    pub automatic_agent_or_model_invocation: bool,
}

/// Parse one retained Markdown source when the caller has no stronger source
/// standing. Seeing bytes proves observation, not authorship, so the conservative
/// default authority is `Observed`.
pub fn parse_authored_wiki_source(
    subject_ref: ResourceRef,
    source_ref: SourceRef,
    source_revision: Option<SourceRevision>,
    locators: Vec<String>,
    markdown: &str,
) -> Result<AuthoredWikiSourceProjection> {
    parse_authored_wiki_source_with_authority(
        subject_ref,
        source_ref,
        SourceAuthority::Observed,
        source_revision,
        locators,
        markdown,
    )
}

/// Parse one retained Markdown source with source-owner epistemic standing.
/// Plain Markdown is fully valid input. Frontmatter becomes OKF/Properties
/// semantics only when it explicitly carries a `type`, preserving ordinary
/// non-OKF YAML as another tool's source language.
pub fn parse_authored_wiki_source_with_authority(
    subject_ref: ResourceRef,
    source_ref: SourceRef,
    source_authority: SourceAuthority,
    source_revision: Option<SourceRevision>,
    locators: Vec<String>,
    markdown: &str,
) -> Result<AuthoredWikiSourceProjection> {
    let mut relations =
        parse_authored_markdown_relations(&source_ref, source_revision.as_ref(), markdown);
    let profile = parse_optional_okf_profile(markdown, &source_ref, source_revision.as_ref())?;

    let (profile_subject, title, aliases, mut metadata_relations) = match profile {
        Some(profile) => (
            profile.resource_ref,
            profile.title,
            profile.aliases,
            profile.relations,
        ),
        None => (None, None, Vec::new(), Vec::new()),
    };
    relations.append(&mut metadata_relations);

    Ok(AuthoredWikiSourceProjection {
        version: AUTHORED_WIKI_SOURCE_VERSION.into(),
        subject_ref: profile_subject.unwrap_or(subject_ref),
        source_ref,
        source_authority,
        source_revision,
        title,
        aliases,
        locators,
        relations,
    })
}

/// Compile all explicit authored relation evidence against the stable identities
/// already present in the current Wiki/source field.
pub fn compile_authored_wiki_relations(
    sources: &[AuthoredWikiSourceProjection],
    wiki_objects: &[WikiObject],
    additional_candidates: &[AuthoredRelationCandidate],
) -> Result<AuthoredWikiRelationCompilation> {
    let candidates = relation_candidates(sources, wiki_objects, additional_candidates);
    let mut edges = Vec::new();
    let mut pending = Vec::new();
    let mut edge_refs = BTreeSet::new();

    for source in sources {
        for evidence in &source.relations {
            let resolved = resolve_authored_relation(evidence, &candidates);
            if !matches!(
                resolved.resolution,
                AuthoredRelationResolution::Resolved { .. }
            ) {
                pending.push(PendingAuthoredRelation {
                    subject_ref: source.subject_ref.clone(),
                    source_authority: source.source_authority,
                    evidence: resolved,
                });
                continue;
            }

            let edge_ref = authored_edge_ref(&source.subject_ref, &resolved)?;
            if !edge_refs.insert(edge_ref.clone()) {
                return Err(AikitError::new(
                    "knowledge.authored_relation_duplicate_edge",
                    "two authored relation observations produced the same stable edge identity",
                )
                .with("edge", edge_ref.to_string()));
            }
            let mut edge =
                materialize_authored_wiki_edge(edge_ref, source.subject_ref.clone(), &resolved)?;
            edge.revision = authored_edge_revision(&resolved);
            if let Some(provenance) = edge.provenance.first_mut() {
                provenance.extensions.insert(
                    "source_authority".into(),
                    serde_json::to_value(source.source_authority).map_err(|error| {
                        AikitError::new(
                            "knowledge.authored_relation_serialize",
                            format!("could not serialize source authority: {error}"),
                        )
                    })?,
                );
            }
            edges.push(edge);
        }
    }

    edges.sort_by(|left, right| left.ref_id.cmp(&right.ref_id));
    pending.sort_by(|left, right| {
        left.subject_ref
            .cmp(&right.subject_ref)
            .then(left.evidence.raw_target.cmp(&right.evidence.raw_target))
            .then(left.evidence.relation.cmp(&right.evidence.relation))
    });

    Ok(AuthoredWikiRelationCompilation {
        version: AUTHORED_WIKI_SOURCE_VERSION.into(),
        edges,
        pending,
        automatic_agent_or_model_invocation: false,
    })
}

/// Every explicit source relation depends on the exact source revision which
/// carries it. This projects that deterministic dependency into the existing
/// Living Knowledge metabolism: a source revision/change can mark its semantic
/// subject affected without any background Agent/model interpretation.
pub fn authored_relation_dependencies(
    sources: &[AuthoredWikiSourceProjection],
) -> Vec<KnowledgeDependency> {
    let mut dependencies = sources
        .iter()
        .flat_map(|source| {
            source.relations.iter().map(|relation| KnowledgeDependency {
                dependent: source.subject_ref.clone(),
                source: source.source_ref.clone(),
                basis_revision: source.source_revision.clone(),
                relation: format!("authored-source:{}", relation.relation),
                provenance_ref: None,
                integrative: false,
            })
        })
        .collect::<Vec<_>>();
    dependencies.sort_by(|left, right| {
        left.dependent
            .cmp(&right.dependent)
            .then(left.source.cmp(&right.source))
            .then(left.relation.cmp(&right.relation))
    });
    dependencies.dedup_by(|left, right| {
        left.dependent == right.dependent
            && left.source == right.source
            && left.relation == right.relation
            && left.basis_revision == right.basis_revision
    });
    dependencies
}

/// Rebuild the existing SemanticWiki with source-compiled authored edges. This is
/// the acceptance path proving backlinks/search relations are consequences of
/// source plus canonical Wiki objects rather than a second persistent index.
pub fn rebuild_semantic_wiki_with_authored_relations(
    wiki_objects: &[WikiObject],
    compilation: &AuthoredWikiRelationCompilation,
) -> Result<SemanticWikiIndex> {
    let objects = wiki_objects
        .iter()
        .cloned()
        .chain(compilation.edges.iter().cloned().map(WikiObject::Edge));
    SemanticWikiIndex::rebuild(objects)
}

fn parse_optional_okf_profile(
    markdown: &str,
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
) -> Result<Option<OkfWikiSourceProfile>> {
    let Some(metadata) = frontmatter_metadata(markdown)? else {
        return Ok(None);
    };
    if !metadata.contains_key("type") {
        return Ok(None);
    }
    let document = parse_okf_markdown(markdown)?;
    okf_wiki_source_profile(&document, source_ref, source_revision).map(Some)
}

fn frontmatter_metadata(markdown: &str) -> Result<Option<Map<String, Value>>> {
    let markdown = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let Some(rest) = markdown
        .strip_prefix("---\r\n")
        .or_else(|| markdown.strip_prefix("---\n"))
    else {
        return Ok(None);
    };

    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim() == "---" {
            let yaml = &rest[..offset];
            let value: Value = serde_yaml::from_str(yaml).map_err(|error| {
                AikitError::new(
                    "knowledge.authored_wiki_invalid_frontmatter",
                    format!("malformed Markdown Properties/frontmatter: {error}"),
                )
            })?;
            return value.as_object().cloned().map(Some).ok_or_else(|| {
                AikitError::new(
                    "knowledge.authored_wiki_invalid_frontmatter",
                    "Markdown Properties/frontmatter must be a YAML mapping",
                )
            });
        }
        offset += line.len();
    }

    Err(AikitError::new(
        "knowledge.authored_wiki_invalid_frontmatter",
        "opening Markdown frontmatter delimiter has no closing delimiter",
    ))
}

fn relation_candidates(
    sources: &[AuthoredWikiSourceProjection],
    wiki_objects: &[WikiObject],
    additional: &[AuthoredRelationCandidate],
) -> Vec<AuthoredRelationCandidate> {
    let mut candidates = sources
        .iter()
        .map(|source| AuthoredRelationCandidate {
            ref_id: source.subject_ref.clone(),
            title: source.title.clone(),
            aliases: source.aliases.clone(),
            locators: source.locators.clone(),
        })
        .collect::<Vec<_>>();

    candidates.extend(wiki_objects.iter().map(|object| {
        let (title, aliases) = match object {
            WikiObject::Node(value) => (
                value.title.clone(),
                aliases_from_extensions(&value.extensions),
            ),
            WikiObject::Space(value) => (
                value.title.clone(),
                aliases_from_extensions(&value.extensions),
            ),
            _ => (None, Vec::new()),
        };
        AuthoredRelationCandidate {
            ref_id: object.ref_id().clone(),
            title,
            aliases,
            locators: Vec::new(),
        }
    }));
    candidates.extend_from_slice(additional);
    candidates
}

fn aliases_from_extensions(extensions: &std::collections::BTreeMap<String, Value>) -> Vec<String> {
    match extensions.get("aliases") {
        Some(Value::String(value)) if !value.trim().is_empty() => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn authored_edge_ref(
    subject_ref: &ResourceRef,
    evidence: &AuthoredRelationEvidence,
) -> Result<ResourceRef> {
    let target = match &evidence.resolution {
        AuthoredRelationResolution::Resolved { target_ref } => target_ref,
        _ => {
            return Err(AikitError::new(
                "knowledge.authored_relation_unresolved",
                "stable authored edge identity requires a resolved target",
            ))
        }
    };
    let anchor = serde_json::to_string(&evidence.anchor).map_err(|error| {
        AikitError::new(
            "knowledge.authored_relation_serialize",
            format!("could not serialize authored anchor for edge identity: {error}"),
        )
    })?;
    let channel = serde_json::to_string(&evidence.channel).map_err(|error| {
        AikitError::new(
            "knowledge.authored_relation_serialize",
            format!("could not serialize authored channel for edge identity: {error}"),
        )
    })?;
    let material = format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        subject_ref,
        evidence.source_ref,
        evidence.relation,
        target,
        channel,
        anchor
    );
    ResourceRef::parse(&format!(
        "wiki:edge:authored:{:016x}",
        stable_hash64(material.as_bytes())
    ))
}

fn authored_edge_revision(evidence: &AuthoredRelationEvidence) -> u64 {
    let material = format!(
        "{}\0{}\0{}\0{}\0{}",
        evidence
            .source_revision
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        evidence.relation,
        evidence.raw_target,
        evidence.raw_token,
        evidence.fragment.as_deref().unwrap_or_default()
    );
    stable_hash64(material.as_bytes()).max(1)
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::knowledge_okf::AuthoredRelationChannel;
    use aikit_core::knowledge_wiki::{WikiEdgeOrigin, WikiNode, OKF_WIKI_PROFILE};

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn s(raw: &str) -> SourceRef {
        SourceRef::parse(raw).unwrap()
    }

    fn node(id: &str, title: &str) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r(id),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some(title.into()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    #[test]
    fn plain_markdown_and_optional_okf_properties_join_one_authored_relation_field() {
        let source = parse_authored_wiki_source(
            r("source:note:flow"),
            s("source:note:flow"),
            Some(SourceRevision::parse("rev-7").unwrap()),
            vec!["notes/flow.md".into()],
            r#"---
type: Concept
resource: wiki:concept:flow
title: Flow
aliases: [Live Flow]
relations:
  develops: [Living Wiki]
producer_extension:
  preserved: true
---
The [[Living Wiki]] becomes inhabitable through [[Future Concept]].
"#,
        )
        .unwrap();

        assert_eq!(source.subject_ref.as_str(), "wiki:concept:flow");
        assert_eq!(source.source_authority, SourceAuthority::Observed);
        assert_eq!(source.aliases, vec!["Live Flow"]);
        assert_eq!(source.relations.len(), 3);
        assert_eq!(
            source
                .relations
                .iter()
                .filter(|relation| relation.channel == AuthoredRelationChannel::Body)
                .count(),
            2
        );
        assert!(source
            .relations
            .iter()
            .any(|relation| relation.relation == "develops"));
    }

    #[test]
    fn non_okf_frontmatter_does_not_capture_other_tools_metadata() {
        let source = parse_authored_wiki_source(
            r("source:note:ordinary"),
            s("source:note:ordinary"),
            None,
            vec!["notes/ordinary.md".into()],
            "---\ncssclasses: [wide]\nplugin-data: true\n---\nSee [[Flow]].\n",
        )
        .unwrap();
        assert_eq!(source.subject_ref.as_str(), "source:note:ordinary");
        assert_eq!(source.relations.len(), 1);
        assert_eq!(source.relations[0].raw_target, "Flow");
    }

    #[test]
    fn compilation_resolves_to_existing_wiki_and_rebuild_produces_backlink() {
        let wiki = vec![node("wiki:node:beta", "Beta")];
        let source = parse_authored_wiki_source_with_authority(
            r("source:note:alpha"),
            s("source:note:alpha"),
            SourceAuthority::Authored,
            Some(SourceRevision::parse("rev-1").unwrap()),
            vec!["notes/alpha.md".into()],
            "Alpha develops beside [[Beta]].\n",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[source], &wiki, &[]).unwrap();
        assert_eq!(compilation.edges.len(), 1);
        assert!(compilation.pending.is_empty());
        assert_eq!(compilation.edges[0].origin, WikiEdgeOrigin::Authored);
        assert_eq!(compilation.edges[0].relation, "references");
        assert_eq!(compilation.edges[0].from_ref.as_str(), "source:note:alpha");
        assert_eq!(compilation.edges[0].to_ref.as_str(), "wiki:node:beta");
        assert_eq!(
            compilation.edges[0].provenance[0].extensions["source_authority"],
            serde_json::json!("authored")
        );

        let first = rebuild_semantic_wiki_with_authored_relations(&wiki, &compilation).unwrap();
        let second = rebuild_semantic_wiki_with_authored_relations(&wiki, &compilation).unwrap();
        assert_eq!(first.revision(), second.revision());
        let backlinks = first.backlinks(&r("wiki:node:beta"));
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].resource.as_str(), "source:note:alpha");
        assert_eq!(backlinks[0].origin, WikiEdgeOrigin::Authored);
    }

    #[test]
    fn explicit_relation_origin_does_not_overwrite_agent_source_authority() {
        let wiki = vec![node("wiki:node:beta", "Beta")];
        let source = parse_authored_wiki_source_with_authority(
            r("flow:agent-thread"),
            s("source:flow:agent-thread"),
            SourceAuthority::Learned,
            Some(SourceRevision::parse("rev-agent-2").unwrap()),
            vec!["flows/agent-thread.md".into()],
            "Agent-maintained flow links [[Beta]].\n",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[source], &wiki, &[]).unwrap();
        assert_eq!(compilation.edges[0].origin, WikiEdgeOrigin::Authored);
        assert_eq!(
            compilation.edges[0].provenance[0].extensions["source_authority"],
            serde_json::json!("learned")
        );
    }

    #[test]
    fn unresolved_relation_survives_without_synthetic_graph_target() {
        let source = parse_authored_wiki_source(
            r("source:note:alpha"),
            s("source:note:alpha"),
            None,
            vec!["notes/alpha.md".into()],
            "See [[Future Concept]].\n",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[source], &[], &[]).unwrap();
        assert!(compilation.edges.is_empty());
        assert_eq!(compilation.pending.len(), 1);
        assert_eq!(compilation.pending[0].source_authority, SourceAuthority::Observed);
        assert_eq!(compilation.pending[0].evidence.raw_target, "Future Concept");
        assert!(matches!(
            compilation.pending[0].evidence.resolution,
            AuthoredRelationResolution::Unresolved
        ));
    }

    #[test]
    fn flow_subject_can_be_external_endpoint_without_becoming_wiki_node() {
        let wiki = vec![node("wiki:node:living-wiki", "Living Wiki")];
        let flow = parse_authored_wiki_source(
            r("flow:2026-08-24:thread"),
            s("source:flow:2026-08-24:thread"),
            Some(SourceRevision::parse("rev-flow-3").unwrap()),
            vec!["flows/2026-08-24-thread.md".into()],
            "Working through [[Living Wiki]].\n",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[flow], &wiki, &[]).unwrap();
        let index = rebuild_semantic_wiki_with_authored_relations(&wiki, &compilation).unwrap();
        assert!(index.resolve(&r("flow:2026-08-24:thread")).is_none());
        let neighbours = index.neighbours(&r("flow:2026-08-24:thread"), 8);
        assert_eq!(neighbours.len(), 1);
        assert_eq!(neighbours[0].resource.as_str(), "wiki:node:living-wiki");
        assert_eq!(
            index.backlinks(&r("wiki:node:living-wiki"))[0]
                .resource
                .as_str(),
            "flow:2026-08-24:thread"
        );
    }

    #[test]
    fn source_revision_changes_edge_revision_but_not_stable_identity() {
        let wiki = vec![node("wiki:node:beta", "Beta")];
        let parse = |revision: &str| {
            parse_authored_wiki_source(
                r("source:note:alpha"),
                s("source:note:alpha"),
                Some(SourceRevision::parse(revision).unwrap()),
                vec!["notes/alpha.md".into()],
                "See [[Beta]].\n",
            )
            .unwrap()
        };
        let first = compile_authored_wiki_relations(&[parse("rev-1")], &wiki, &[]).unwrap();
        let second = compile_authored_wiki_relations(&[parse("rev-2")], &wiki, &[]).unwrap();
        assert_eq!(first.edges[0].ref_id, second.edges[0].ref_id);
        assert_ne!(first.edges[0].revision, second.edges[0].revision);
    }

    #[test]
    fn authored_relations_feed_existing_living_knowledge_dependency_path() {
        let source = parse_authored_wiki_source_with_authority(
            r("wiki:node:flow"),
            s("source:flow"),
            SourceAuthority::Authored,
            Some(SourceRevision::parse("rev-9").unwrap()),
            vec!["flow.md".into()],
            "See [[Future Concept]] and [[Another]].\n",
        )
        .unwrap();
        let dependencies = authored_relation_dependencies(&[source]);
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0].dependent.as_str(), "wiki:node:flow");
        assert_eq!(dependencies[0].source.as_str(), "source:flow");
        assert_eq!(dependencies[0].basis_revision.as_ref().unwrap().as_str(), "rev-9");
        assert_eq!(dependencies[0].relation, "authored-source:references");
        assert!(!dependencies[0].integrative);
    }
}
