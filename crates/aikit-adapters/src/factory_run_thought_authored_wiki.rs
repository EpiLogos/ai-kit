//! Factory RunThought → authored SemanticWiki / Living Knowledge attachment.
//!
//! Factory remains the owner of Run, RunThought lifecycle and the source/artifact
//! anchor carried by each Thought. AIKit consumes the language-neutral
//! `factory.build-cognitive-view/v1` snapshot plus an already-authorised source
//! disclosure, then projects the exact Thought passage through the existing authored
//! source compiler. No Factory crate dependency, second parser or second graph is
//! introduced.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::knowledge_living::KnowledgeDependency;
use aikit_core::knowledge_okf::{AuthoredRelationCandidate, AuthoredRelationChannel};
use aikit_core::knowledge_wiki::WikiObject;
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::authored_wiki_source::{
    authored_relation_dependencies, compile_authored_wiki_relations,
    parse_authored_wiki_source_with_authority, rebuild_semantic_wiki_with_authored_relations,
    AuthoredWikiRelationCompilation, AuthoredWikiSourceProjection,
};

pub const FACTORY_RUN_THOUGHT_AUTHORED_WIKI_VERSION: &str =
    "aikit.factory-run-thought-authored-wiki/v1";
pub const FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT: &str = "factory.build-cognitive-view/v1";
pub const FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT: &str =
    "factory.build-cognitive-view-provider/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryBuildCognitiveProvenance {
    pub owner: String,
    pub factory_state_revision: u64,
    pub run_revision: u64,
    pub run_map_revision: u64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThoughtPassage {
    pub start_byte: u64,
    pub end_byte: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThoughtProducer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThought {
    pub id: String,
    pub run_ref: String,
    pub anchor_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passage: Option<FactoryRunThoughtPassage>,
    #[serde(default)]
    pub producer: FactoryRunThoughtProducer,
    #[serde(default)]
    pub run_map_subject_refs: Vec<String>,
    #[serde(default)]
    pub related_refs: Vec<String>,
    #[serde(default)]
    pub relation_evidence_refs: Vec<String>,
    pub lifecycle: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryBuildCognitiveView {
    pub run_ref: String,
    pub thought_count: usize,
    #[serde(default)]
    pub thoughts: Vec<FactoryRunThought>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryBuildCognitiveSnapshot {
    pub contract: String,
    pub provider_contract: String,
    pub revision: u64,
    pub provenance: FactoryBuildCognitiveProvenance,
    pub view: FactoryBuildCognitiveView,
}

impl FactoryBuildCognitiveSnapshot {
    pub fn from_json(input: &str) -> Result<Self> {
        let snapshot: Self = serde_json::from_str(input).map_err(|error| {
            AikitError::new(
                "factory_run_thought.invalid_snapshot_json",
                format!("invalid Factory cognitive snapshot JSON: {error}"),
            )
        })?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<()> {
        if self.contract != FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT {
            return Err(AikitError::new(
                "factory_run_thought.unsupported_contract",
                format!(
                    "unsupported Factory cognitive contract `{}`; expected `{FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT}`",
                    self.contract
                ),
            ));
        }
        if self.provider_contract != FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT {
            return Err(AikitError::new(
                "factory_run_thought.unsupported_provider_contract",
                format!(
                    "unsupported Factory cognitive provider `{}`; expected `{FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT}`",
                    self.provider_contract
                ),
            ));
        }
        if self.provenance.owner != "factory" {
            return Err(AikitError::new(
                "factory_run_thought.wrong_owner",
                "Factory cognitive snapshot provenance must remain Factory-owned",
            ));
        }
        if self.view.run_ref.trim().is_empty() {
            return Err(AikitError::new(
                "factory_run_thought.empty_run_ref",
                "Factory cognitive view requires a RunRef",
            ));
        }
        if self.view.thought_count != self.view.thoughts.len() {
            return Err(AikitError::new(
                "factory_run_thought.count_mismatch",
                "Factory cognitive thoughtCount does not match the supplied Thought records",
            ));
        }

        let mut thought_ids = BTreeSet::new();
        for thought in &self.view.thoughts {
            if thought.id.trim().is_empty() {
                return Err(AikitError::new(
                    "factory_run_thought.empty_thought_id",
                    "Factory RunThought id cannot be empty",
                ));
            }
            if !thought_ids.insert(thought.id.clone()) {
                return Err(AikitError::new(
                    "factory_run_thought.duplicate_thought_id",
                    format!("duplicate Factory RunThought id `{}`", thought.id),
                ));
            }
            if thought.run_ref != self.view.run_ref {
                return Err(AikitError::new(
                    "factory_run_thought.run_mismatch",
                    format!(
                        "Factory RunThought `{}` belongs to `{}` rather than cognitive view `{}`",
                        thought.id, thought.run_ref, self.view.run_ref
                    ),
                ));
            }
            if thought.anchor_ref.trim().is_empty() {
                return Err(AikitError::new(
                    "factory_run_thought.empty_anchor_ref",
                    format!("Factory RunThought `{}` has an empty anchorRef", thought.id),
                ));
            }
        }
        Ok(())
    }
}

/// Already-authorised source payload for one Factory anchor.
///
/// Source retrieval/eligibility remains provider-owned. This adapter accepts the
/// disclosed body after that decision, exactly like the Flow adapter accepts an
/// already-authorised standing context. The boolean is a guard proving that
/// materialisation did not itself invoke an Agent/model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThoughtSourceDisclosure {
    pub anchor_ref: String,
    pub source_ref: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
    pub source_authority: SourceAuthority,
    #[serde(default)]
    pub locators: Vec<String>,
    pub body: String,
    pub automatic_agent_or_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThoughtProjection {
    /// AIKit address of Factory's canonical `(RunRef, RunThoughtId)` pair. This is
    /// a projection address, not a second Factory-owned identity.
    pub subject_ref: ResourceRef,
    pub thought: FactoryRunThought,
    pub source_ref: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
    pub source_authority: SourceAuthority,
}

#[derive(Debug)]
pub struct FactoryRunThoughtAuthoredWiki {
    pub version: String,
    pub run_ref: ResourceRef,
    pub factory_snapshot_revision: u64,
    pub factory_state_revision: u64,
    pub run_revision: u64,
    pub run_map_revision: u64,
    pub thoughts: Vec<FactoryRunThoughtProjection>,
    pub source_projections: Vec<AuthoredWikiSourceProjection>,
    pub compilation: AuthoredWikiRelationCompilation,
    pub dependencies: Vec<KnowledgeDependency>,
    pub index: SemanticWikiIndex,
    /// Attachment is deterministic and never invokes an Agent/model.
    pub automatic_agent_or_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactoryRunThoughtAuthoredWikiStatus {
    pub version: String,
    pub run_ref: ResourceRef,
    pub thoughts: usize,
    pub resolved_relations: usize,
    pub pending_relations: usize,
    pub living_dependencies: usize,
    pub semantic_wiki_revision: String,
    pub automatic_agent_or_model_invocation: bool,
}

impl FactoryRunThoughtAuthoredWiki {
    pub fn status(&self) -> FactoryRunThoughtAuthoredWikiStatus {
        FactoryRunThoughtAuthoredWikiStatus {
            version: self.version.clone(),
            run_ref: self.run_ref.clone(),
            thoughts: self.thoughts.len(),
            resolved_relations: self.compilation.edges.len(),
            pending_relations: self.compilation.pending.len(),
            living_dependencies: self.dependencies.len(),
            semantic_wiki_revision: self.index.revision().to_string(),
            automatic_agent_or_model_invocation: self.automatic_agent_or_model_invocation,
        }
    }
}

/// Attach Factory's Run-scoped cognitive field to the existing authored relation,
/// SemanticWiki and Living Knowledge paths.
///
/// Factory supplies stable cognitive identity/provenance. The source owner supplies
/// authorised bytes and epistemic standing. AIKit owns parsing, relation resolution,
/// backlinks and deterministic source-dependency impact.
pub fn factory_run_thought_authored_wiki(
    snapshot_json: &str,
    disclosures: &[FactoryRunThoughtSourceDisclosure],
    wiki_objects: &[WikiObject],
    additional_candidates: &[AuthoredRelationCandidate],
) -> Result<FactoryRunThoughtAuthoredWiki> {
    let snapshot = FactoryBuildCognitiveSnapshot::from_json(snapshot_json)?;
    let run_ref = ResourceRef::parse(&snapshot.view.run_ref)?;

    for disclosure in disclosures {
        if disclosure.anchor_ref.trim().is_empty() {
            return Err(AikitError::new(
                "factory_run_thought.empty_disclosure_anchor",
                "Factory RunThought source disclosure requires an anchorRef",
            ));
        }
        if disclosure.automatic_agent_or_model_invocation {
            return Err(AikitError::new(
                "factory_run_thought.non_deterministic_disclosure",
                "Factory RunThought source disclosure must remain deterministic",
            ));
        }
    }

    let mut thoughts = Vec::new();
    let mut source_projections = Vec::new();
    let mut base_dependencies = Vec::new();

    for thought in &snapshot.view.thoughts {
        let disclosure = select_disclosure(thought, disclosures)?;
        let subject_ref = thought_subject_ref(&thought.run_ref, &thought.id)?;
        let (markdown, passage_offset) =
            thought_markdown(&disclosure.body, thought.passage.as_ref())?;
        let source_revision = disclosure.source_revision.clone();

        let mut projection = parse_authored_wiki_source_with_authority(
            subject_ref.clone(),
            disclosure.source_ref.clone(),
            disclosure.source_authority,
            source_revision.clone(),
            Vec::new(),
            markdown,
        )?;

        // A RunThought remains the semantic subject even when its complete source
        // carries an OKF `resource`. Source-level title/aliases likewise do not
        // silently redefine the Thought or make several Thoughts sharing one source
        // indistinguishable relation candidates.
        projection.subject_ref = subject_ref.clone();
        projection.title = thought
            .passage
            .as_ref()
            .and_then(|passage| passage.label.clone());
        projection.aliases.clear();
        projection.locators.clear();
        shift_body_relation_anchors(&mut projection, passage_offset)?;

        base_dependencies.push(KnowledgeDependency {
            dependent: subject_ref.clone(),
            source: disclosure.source_ref.clone(),
            basis_revision: source_revision.clone(),
            relation: "factory-run-thought:source-basis".into(),
            provenance_ref: None,
            integrative: false,
        });
        thoughts.push(FactoryRunThoughtProjection {
            subject_ref,
            thought: thought.clone(),
            source_ref: disclosure.source_ref.clone(),
            source_revision,
            source_authority: disclosure.source_authority,
        });
        source_projections.push(projection);
    }

    thoughts.sort_by(|left, right| left.subject_ref.cmp(&right.subject_ref));
    source_projections.sort_by(|left, right| left.subject_ref.cmp(&right.subject_ref));

    let mut compilation =
        compile_authored_wiki_relations(&source_projections, wiki_objects, additional_candidates)?;
    annotate_factory_provenance(&mut compilation, &thoughts)?;

    let mut dependencies = base_dependencies;
    dependencies.extend(authored_relation_dependencies(&source_projections));
    dependencies.sort_by(|left, right| {
        left.dependent
            .cmp(&right.dependent)
            .then(left.source.cmp(&right.source))
            .then(left.relation.cmp(&right.relation))
    });
    dependencies.dedup_by(|left, right| {
        left.dependent == right.dependent
            && left.source == right.source
            && left.basis_revision == right.basis_revision
            && left.relation == right.relation
            && left.provenance_ref == right.provenance_ref
            && left.integrative == right.integrative
    });

    let index = rebuild_semantic_wiki_with_authored_relations(wiki_objects, &compilation)?;

    Ok(FactoryRunThoughtAuthoredWiki {
        version: FACTORY_RUN_THOUGHT_AUTHORED_WIKI_VERSION.into(),
        run_ref,
        factory_snapshot_revision: snapshot.revision,
        factory_state_revision: snapshot.provenance.factory_state_revision,
        run_revision: snapshot.provenance.run_revision,
        run_map_revision: snapshot.provenance.run_map_revision,
        thoughts,
        source_projections,
        compilation,
        dependencies,
        index,
        automatic_agent_or_model_invocation: false,
    })
}

fn select_disclosure<'a>(
    thought: &FactoryRunThought,
    disclosures: &'a [FactoryRunThoughtSourceDisclosure],
) -> Result<&'a FactoryRunThoughtSourceDisclosure> {
    let candidates = disclosures
        .iter()
        .filter(|disclosure| disclosure.anchor_ref == thought.anchor_ref)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(AikitError::new(
            "factory_run_thought.source_undisclosed",
            format!(
                "Factory RunThought `{}` anchor `{}` has no authorised source disclosure",
                thought.id, thought.anchor_ref
            ),
        ));
    }

    if let Some(expected_revision) = thought.anchor_revision.as_deref() {
        let exact = candidates
            .into_iter()
            .filter(|disclosure| {
                disclosure.source_revision.as_ref().map(SourceRevision::as_str)
                    == Some(expected_revision)
            })
            .collect::<Vec<_>>();
        return match exact.as_slice() {
            [disclosure] => Ok(*disclosure),
            [] => Err(AikitError::new(
                "factory_run_thought.source_revision_mismatch",
                format!(
                    "Factory RunThought `{}` requires anchor revision `{expected_revision}`",
                    thought.id
                ),
            )),
            _ => Err(AikitError::new(
                "factory_run_thought.source_disclosure_ambiguous",
                format!(
                    "Factory RunThought `{}` has more than one disclosure for exact anchor revision `{expected_revision}`",
                    thought.id
                ),
            )),
        };
    }

    match candidates.as_slice() {
        [disclosure] => Ok(*disclosure),
        _ => Err(AikitError::new(
            "factory_run_thought.source_disclosure_ambiguous",
            format!(
                "Factory RunThought `{}` omits anchorRevision while multiple disclosures exist for `{}`",
                thought.id, thought.anchor_ref
            ),
        )),
    }
}

fn thought_subject_ref(run_ref: &str, thought_id: &str) -> Result<ResourceRef> {
    ResourceRef::parse(format!("{run_ref}/thought/{thought_id}"))
}

fn thought_markdown<'a>(
    body: &'a str,
    passage: Option<&FactoryRunThoughtPassage>,
) -> Result<(&'a str, usize)> {
    let Some(passage) = passage else {
        return Ok((body, 0));
    };
    let start = usize::try_from(passage.start_byte).map_err(|_| {
        AikitError::new(
            "factory_run_thought.invalid_passage",
            "Factory RunThought passage start exceeds this platform's addressable source size",
        )
    })?;
    let end = usize::try_from(passage.end_byte).map_err(|_| {
        AikitError::new(
            "factory_run_thought.invalid_passage",
            "Factory RunThought passage end exceeds this platform's addressable source size",
        )
    })?;
    if start >= end
        || end > body.len()
        || !body.is_char_boundary(start)
        || !body.is_char_boundary(end)
    {
        return Err(AikitError::new(
            "factory_run_thought.invalid_passage",
            format!(
                "Factory RunThought passage {start}..{end} is not a valid UTF-8 byte range for the disclosed source"
            ),
        ));
    }
    Ok((&body[start..end], start))
}

fn shift_body_relation_anchors(
    projection: &mut AuthoredWikiSourceProjection,
    passage_offset: usize,
) -> Result<()> {
    if passage_offset == 0 {
        return Ok(());
    }
    for relation in &mut projection.relations {
        if relation.channel != AuthoredRelationChannel::Body {
            continue;
        }
        if let Some(start) = relation.anchor.start_byte.as_mut() {
            *start = start.checked_add(passage_offset).ok_or_else(|| {
                AikitError::new(
                    "factory_run_thought.invalid_passage",
                    "Factory RunThought relation anchor overflowed source byte coordinates",
                )
            })?;
        }
        if let Some(end) = relation.anchor.end_byte.as_mut() {
            *end = end.checked_add(passage_offset).ok_or_else(|| {
                AikitError::new(
                    "factory_run_thought.invalid_passage",
                    "Factory RunThought relation anchor overflowed source byte coordinates",
                )
            })?;
        }
    }
    Ok(())
}

fn annotate_factory_provenance(
    compilation: &mut AuthoredWikiRelationCompilation,
    thoughts: &[FactoryRunThoughtProjection],
) -> Result<()> {
    let by_subject = thoughts
        .iter()
        .map(|thought| (thought.subject_ref.clone(), thought))
        .collect::<BTreeMap<_, _>>();

    for edge in &mut compilation.edges {
        let Some(thought) = by_subject.get(&edge.from_ref) else {
            continue;
        };
        let Some(provenance) = edge.provenance.first_mut() else {
            continue;
        };
        provenance.extensions.insert(
            "factory_run_thought".into(),
            serde_json::to_value(thought).map_err(|error| {
                AikitError::new(
                    "factory_run_thought.serialize_provenance",
                    format!("could not serialize Factory RunThought provenance: {error}"),
                )
            })?,
        );
        provenance.producer_ref = thought
            .thought
            .producer
            .agent_ref
            .as_deref()
            .map(ResourceRef::parse)
            .transpose()?;
        provenance.generation_ref = thought
            .thought
            .producer
            .execution_ref
            .as_deref()
            .map(ResourceRef::parse)
            .transpose()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::knowledge_living::{
        deterministic_knowledge_impact, KnowledgeChangeHorizon, KnowledgeObservedSource,
    };
    use aikit_core::knowledge_wiki::{WikiEdgeOrigin, WikiNode, WikiObject, OKF_WIKI_PROFILE};
    use aikit_core::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn target() -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r("wiki:node:beta"),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some("Beta".into()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    fn snapshot(revision: Option<&str>, start: usize, end: usize) -> String {
        serde_json::json!({
            "contract": FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT,
            "providerContract": FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT,
            "revision": 7,
            "provenance": {
                "owner": "factory",
                "factoryStateRevision": 7,
                "runRevision": 4,
                "runMapRevision": 1,
                "source": "canonical FactoryBuildState -> canonical Run/RunThoughtField"
            },
            "view": {
                "runRef": "run:01ARZ3NDEKTSV4RRFFQ69G5FAB",
                "thoughtCount": 1,
                "thoughts": [{
                    "id": "pattern-reading",
                    "runRef": "run:01ARZ3NDEKTSV4RRFFQ69G5FAB",
                    "anchorRef": "source/run-thinking.md",
                    "anchorRevision": revision,
                    "passage": {
                        "startByte": start,
                        "endByte": end,
                        "label": "Pattern reading"
                    },
                    "producer": {
                        "agentRef": "agent:mahamaya",
                        "agencyRef": "agency:factory-run",
                        "agentSessionRef": "agent-session:one",
                        "executionRef": "execution:one"
                    },
                    "runMapSubjectRefs": ["run-map-subject:current-work"],
                    "relatedRefs": ["claim:01ARZ3NDEKTSV4RRFFQ69G5FAD"],
                    "relationEvidenceRefs": ["relation-evidence:t3-pattern"],
                    "lifecycle": "active"
                }]
            }
        })
        .to_string()
    }

    fn disclosure(body: &str, revision: &str) -> FactoryRunThoughtSourceDisclosure {
        FactoryRunThoughtSourceDisclosure {
            anchor_ref: "source/run-thinking.md".into(),
            source_ref: SourceRef::parse("source:factory:run-thinking").unwrap(),
            source_revision: Some(SourceRevision::parse(revision).unwrap()),
            source_authority: SourceAuthority::Learned,
            locators: vec!["runs/current/thinking.md".into()],
            body: body.into(),
            automatic_agent_or_model_invocation: false,
        }
    }

    #[test]
    fn factory_thought_passage_enters_existing_wiki_backlink_and_living_dependency_paths() {
        let body = "Earlier scratch.\nPattern: [[Beta]] is now visible.\nLater scratch.\n";
        let start = body.find("Pattern:").unwrap();
        let end = start + "Pattern: [[Beta]] is now visible.\n".len();
        let projected = factory_run_thought_authored_wiki(
            &snapshot(Some("rev-42"), start, end),
            &[disclosure(body, "rev-42")],
            &[target()],
            &[],
        )
        .unwrap();

        assert_eq!(projected.thoughts.len(), 1);
        assert_eq!(projected.source_projections.len(), 1);
        let thought_ref = r("run:01ARZ3NDEKTSV4RRFFQ69G5FAB/thought/pattern-reading");
        assert_eq!(projected.thoughts[0].subject_ref, thought_ref);
        assert_eq!(projected.compilation.edges.len(), 1);
        assert_eq!(projected.compilation.edges[0].origin, WikiEdgeOrigin::Authored);
        assert_eq!(projected.compilation.edges[0].from_ref, thought_ref);
        assert_eq!(projected.compilation.edges[0].to_ref, r("wiki:node:beta"));
        assert_eq!(
            projected.compilation.edges[0].provenance[0].extensions["source_authority"],
            serde_json::json!("learned")
        );
        assert_eq!(
            projected.compilation.edges[0].provenance[0].producer_ref,
            Some(r("agent:mahamaya"))
        );
        assert_eq!(
            projected.compilation.edges[0].provenance[0].generation_ref,
            Some(r("execution:one"))
        );
        assert_eq!(
            projected.compilation.edges[0].provenance[0].extensions["factory_run_thought"]
                ["thought"]["relationEvidenceRefs"][0],
            serde_json::json!("relation-evidence:t3-pattern")
        );

        let authored_anchor = &projected.source_projections[0].relations[0].anchor;
        assert_eq!(
            authored_anchor.start_byte,
            Some(body.find("[[Beta]]").unwrap())
        );
        assert_eq!(
            authored_anchor.end_byte,
            Some(body.find("[[Beta]]").unwrap() + "[[Beta]]".len())
        );

        let backlinks = projected.index.backlinks(&r("wiki:node:beta"));
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].resource, thought_ref);

        assert!(projected.dependencies.iter().any(|dependency| {
            dependency.dependent == thought_ref
                && dependency.source.as_str() == "source:factory:run-thinking"
                && dependency.relation == "factory-run-thought:source-basis"
                && dependency
                    .basis_revision
                    .as_ref()
                    .map(SourceRevision::as_str)
                    == Some("rev-42")
        }));
        assert!(projected.dependencies.iter().any(|dependency| {
            dependency.dependent == thought_ref
                && dependency.relation == "authored-source:references"
        }));

        let impact = deterministic_knowledge_impact(
            &KnowledgeChangeHorizon {
                provider: "factory-source-test".into(),
                cursor: 2,
                sources: vec![KnowledgeObservedSource {
                    source: SourceRef::parse("source:factory:run-thinking").unwrap(),
                    revision: Some(SourceRevision::parse("rev-43").unwrap()),
                    available: true,
                }],
                changes: Vec::new(),
            },
            &projected.dependencies,
        )
        .unwrap();
        assert!(impact
            .affected
            .iter()
            .any(|affected| affected.resource == thought_ref));
        assert!(!projected.automatic_agent_or_model_invocation);
    }

    #[test]
    fn factory_anchor_revision_must_match_authorised_disclosure_exactly() {
        let body = "Pattern: [[Beta]].\n";
        let error = factory_run_thought_authored_wiki(
            &snapshot(Some("rev-expected"), 0, body.len()),
            &[disclosure(body, "rev-other")],
            &[target()],
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            "factory_run_thought.source_revision_mismatch"
        );
    }

    #[test]
    fn passage_must_be_valid_for_exact_disclosed_source_bytes() {
        let body = "é [[Beta]]\n";
        let error = factory_run_thought_authored_wiki(
            &snapshot(Some("rev-1"), 1, body.len()),
            &[disclosure(body, "rev-1")],
            &[target()],
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code(), "factory_run_thought.invalid_passage");
    }

    #[test]
    fn missing_factory_revision_requires_unambiguous_anchor_disclosure() {
        let body = "Pattern: [[Beta]].\n";
        let error = factory_run_thought_authored_wiki(
            &snapshot(None, 0, body.len()),
            &[disclosure(body, "rev-1"), disclosure(body, "rev-2")],
            &[target()],
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            "factory_run_thought.source_disclosure_ambiguous"
        );
    }
}
