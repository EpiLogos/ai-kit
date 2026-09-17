//! Entity addressing through the one resolver path (W10 V4, D9 fold).
//!
//! The Vāk grammar's referent-side `@` expressions are the only addressing
//! surface: a `@name` atom inside a ResolveExpression resolves to the pasu
//! entity nodes materialised by the Central entity projection. The
//! `aikit.participant-address/v1` fragment's target kinds
//! ([`ParticipantTargetKind`]) survive as resolution vocabulary — the shape
//! validator below is the same law — and are no longer a separate transport.
//! Tests live at resolver level.

use std::collections::BTreeSet;

use crate::application_context::world_inhabitation::ParticipantTarget;
use crate::knowledge_wiki_index::SemanticWikiIndex;
use crate::resource::{parse_resolve_expression, ResolveExpression};
use crate::{AikitError, ResourceRef, Result};

pub const ENTITY_ADDRESS_ERROR: &str = "knowledge.entity_address_unresolved";

/// Resolve a Vāk expression's referent-side `@name` addresses to pasu entity
/// node refs, in expression order. The whole expression is parsed by the one
/// grammar first, so an address is only ever resolved as part of a valid
/// expression; a non-address expression resolves to nothing here.
pub fn resolve_participant_expression(
    index: &SemanticWikiIndex,
    raw: &str,
) -> Result<Vec<ResourceRef>> {
    // The strict grammar, not the plain-text search lowering: an address is
    // a referent-side expression and only resolves as part of a valid one.
    let expression = parse_resolve_expression(raw)?;
    let addresses = collect_addresses(&expression);
    let mut resolved = Vec::new();
    for address in addresses {
        resolved.push(resolve_one(index, &address)?);
    }
    Ok(resolved)
}

/// The pasu entity nodes of the index, as (subject key, ref) pairs.
pub fn pasu_entities(index: &SemanticWikiIndex) -> Vec<(String, String, ResourceRef)> {
    let mut entities = Vec::new();
    for reference in index.discover() {
        if let Some(crate::WikiObject::Node(node)) = index.resolve(&reference) {
            if node.node_type != "pasu" {
                continue;
            }
            let Some(form) = node.extensions.get("aikit.pasu/v1") else {
                continue;
            };
            let subject_ref = form
                .get("subject_ref")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned();
            let subject_id = subject_ref
                .rsplit(':')
                .next()
                .unwrap_or_default()
                .to_owned();
            entities.push((subject_id, subject_ref, node.ref_id.clone()));
        }
    }
    entities
}

fn collect_addresses(expression: &ResolveExpression) -> Vec<String> {
    let mut addresses = Vec::new();
    match expression {
        ResolveExpression::Subject { value } => {
            if value.starts_with('@') && value.len() > 1 {
                addresses.push(value.clone());
            }
        }
        ResolveExpression::Address { expression, .. } => {
            addresses.extend(collect_addresses(expression));
        }
        ResolveExpression::Unary { expression, .. } => {
            addresses.extend(collect_addresses(expression));
        }
        ResolveExpression::Binary { left, right, .. } => {
            addresses.extend(collect_addresses(left));
            addresses.extend(collect_addresses(right));
        }
        ResolveExpression::Frame { expression } => {
            addresses.extend(collect_addresses(expression));
        }
    }
    addresses
}

fn resolve_one(index: &SemanticWikiIndex, address: &str) -> Result<ResourceRef> {
    // The participant-address law is the vocabulary: shape validated by the
    // same rules the fragment enforces for To:/@ targets.
    ParticipantTarget::validate_address_shape(address)?;
    let needle = address.trim_start_matches('@');
    let entities = pasu_entities(index);
    // Deterministic match order: exact subject id, then subject-ref suffix,
    // then title token.
    for (subject_id, _, reference) in &entities {
        if subject_id == needle {
            return Ok(reference.clone());
        }
    }
    // The full subject ref answers too (`@central:pasu:agent:hermes`).
    for (_, subject_ref, reference) in &entities {
        if subject_ref == needle {
            return Ok(reference.clone());
        }
    }
    for reference in index.discover() {
        if let Some(crate::WikiObject::Node(node)) = index.resolve(&reference) {
            if node.node_type != "pasu" {
                continue;
            }
            if let Some(title) = &node.title {
                if title
                    .to_lowercase()
                    .split(|c: char| c.is_whitespace() || c == ':' || c == ',')
                    .any(|token| token == needle.to_lowercase())
                {
                    return Ok(node.ref_id.clone());
                }
            }
        }
    }
    Err(AikitError::new(
        ENTITY_ADDRESS_ERROR,
        format!("no pasu entity answers the address `{address}`"),
    ))
}

/// Deterministic ordering helper for multi-address expressions: the same
/// expression always resolves to the same sequence.
pub fn resolve_deduped(index: &SemanticWikiIndex, raw: &str) -> Result<Vec<ResourceRef>> {
    let resolved = resolve_participant_expression(index, raw)?;
    let mut seen = BTreeSet::new();
    Ok(resolved
        .into_iter()
        .filter(|reference| seen.insert(reference.as_str().to_owned()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_wiki_index::SemanticWikiIndex;
    use crate::{SemanticRevision, WikiNode, WikiObject, WikiProvenanceRef, OKF_WIKI_PROFILE};
    use std::collections::BTreeMap;

    fn pasu_node(ref_id: &str, form: &str, subject_ref: &str, title: &str) -> WikiObject {
        let mut extensions = BTreeMap::new();
        extensions.insert(
            "aikit.pasu/v1".to_owned(),
            serde_json::json!({"form": form, "subject_ref": subject_ref}),
        );
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse(ref_id).unwrap(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: crate::SourceRef::parse("test/carrier").unwrap(),
                source_revision: Some(SemanticRevision::Text("r1".into())),
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "pasu".into(),
            title: Some(title.into()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions,
        })
    }

    fn index_with(nodes: Vec<WikiObject>) -> SemanticWikiIndex {
        SemanticWikiIndex::rebuild(nodes).expect("index")
    }

    /// The D9 fold: `@name` referent-side addresses resolve entity nodes
    /// through the one Vāk grammar — no separate addressing transport.
    #[test]
    fn probe2() {
        let e = parse_resolve_expression("@central-operators @hermes").unwrap();
        println!("{e:?}");
    }

    #[test]
    fn vak_addresses_resolve_pasu_entities_through_the_one_grammar() {
        let index = index_with(vec![
            pasu_node(
                "wiki:node:identity",
                "nara",
                "central:pasu:nara:local",
                "Nara identity source",
            ),
            pasu_node(
                "wiki:node:pasu:agent:hermes",
                "agent",
                "central:pasu:agent:hermes",
                "Pasu entity (agent: hermes)",
            ),
            pasu_node(
                "wiki:node:pasu:agent-set:central-operators",
                "agent-set",
                "central:pasu:agent-set:central-operators",
                "Pasu entity (agent-set: central-operators)",
            ),
        ]);

        // A bare @name subject resolves to the nara entity.
        let resolved = resolve_participant_expression(&index, "@local").unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].as_str(), "wiki:node:identity");

        // Agent and agent-set forms resolve by subject id; multiple
        // participants compose through the grammar's operators (adjacent
        // atoms are one subject phrase, so a bare join is not an address).
        assert_eq!(
            resolve_participant_expression(&index, "@hermes").unwrap()[0].as_str(),
            "wiki:node:pasu:agent:hermes"
        );
        let both = resolve_deduped(&index, "@central-operators x @hermes").unwrap();
        assert_eq!(both.len(), 2);
        assert_eq!(
            both[0].as_str(),
            "wiki:node:pasu:agent-set:central-operators"
        );
        assert_eq!(both[1].as_str(), "wiki:node:pasu:agent:hermes");

        // The full subject ref also answers.
        assert_eq!(
            resolve_participant_expression(&index, "@central:pasu:agent:hermes").unwrap()[0]
                .as_str(),
            "wiki:node:pasu:agent:hermes"
        );

        // An unknown participant is an honest error, not a silent miss.
        let missing = resolve_participant_expression(&index, "@stranger").unwrap_err();
        assert_eq!(missing.code(), ENTITY_ADDRESS_ERROR);

        // Non-address expressions resolve nothing here (lexical search stays
        // the search path).
        assert!(resolve_participant_expression(&index, "hermes")
            .unwrap()
            .is_empty());
    }
}
