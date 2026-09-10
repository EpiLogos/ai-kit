//! Structured transport codec for an explicit Contemplate Agent/model return.
//!
//! Transport hosts (ACP, A2A, local harnesses, tests) may carry text/bytes, but
//! they do not define knowledge semantics. AIKit owns the versioned return shape
//! and validates every Wiki upsert / integrative reading before the existing
//! Agent-Wiki maintenance and human proposal/Recognition return path can consume it.

use serde::Deserialize;
use serde_json::Value;

use crate::knowledge_living::{
    build_integrative_reading, ContemplateGenerated, IntegrativeWikiReading,
};
use crate::knowledge_wiki::WikiObject;
use crate::projectcentral::HumanSourceRevisionProposal;
use crate::{AikitError, Result};

pub const CONTEMPLATE_RETURN_VERSION: &str = "aikit.contemplate-return/v1";

#[derive(Debug, Deserialize)]
struct ContemplateReturnEnvelope {
    version: String,
    #[serde(default)]
    wiki_upserts: Vec<Value>,
    #[serde(default)]
    integrative_readings: Vec<IntegrativeWikiReading>,
    #[serde(default)]
    candidates: Vec<String>,
    #[serde(default)]
    tensions: Vec<String>,
    #[serde(default)]
    human_source_proposals: Vec<HumanSourceRevisionProposal>,
}

/// Parse one explicit model/harness response into AIKit's native return type.
///
/// Free-form prose is not a knowledge return. Each Wiki object uses the canonical
/// `WikiObject::parse` validator, and each integrative reading is rebuilt through
/// `build_integrative_reading` so basis DAG, exact source provenance and reversible
/// return-path laws are rechecked after transport.
pub fn parse_contemplate_generated(input: &str) -> Result<ContemplateGenerated> {
    let envelope: ContemplateReturnEnvelope = serde_json::from_str(input).map_err(|error| {
        AikitError::new(
            "knowledge.contemplate_return_invalid_json",
            format!("Contemplate return must be structured JSON: {error}"),
        )
    })?;
    if envelope.version != CONTEMPLATE_RETURN_VERSION {
        return Err(AikitError::new(
            "knowledge.contemplate_return_version_unsupported",
            format!(
                "Contemplate return version `{}` is not `{CONTEMPLATE_RETURN_VERSION}`",
                envelope.version
            ),
        ));
    }

    let wiki_upserts = envelope
        .wiki_upserts
        .iter()
        .map(WikiObject::parse)
        .collect::<Result<Vec<_>>>()?;

    let mut integrative_readings = Vec::with_capacity(envelope.integrative_readings.len());
    for reading in envelope.integrative_readings {
        integrative_readings.push(build_integrative_reading(
            reading.reading,
            reading.basis,
            reading.relations,
            reading.return_paths,
            reading.freshness,
        )?);
    }

    Ok(ContemplateGenerated {
        wiki_upserts,
        integrative_readings,
        candidates: envelope.candidates,
        tensions: envelope.tensions,
        human_source_proposals: envelope.human_source_proposals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_prose_cannot_become_a_knowledge_return() {
        let error = parse_contemplate_generated("I think the project should change.").unwrap_err();
        assert_eq!(error.code(), "knowledge.contemplate_return_invalid_json");
    }

    #[test]
    fn empty_versioned_return_is_valid_and_proposal_only_shape_is_native() {
        let result = parse_contemplate_generated(
            r#"{
              "version":"aikit.contemplate-return/v1",
              "wiki_upserts":[],
              "integrative_readings":[],
              "candidates":["candidate:relation"],
              "tensions":["open tension"],
              "human_source_proposals":[{
                "source":"central:source:project:test:README.md",
                "reason":"review wording",
                "evidence":[]
              }]
            }"#,
        )
        .unwrap();
        assert!(result.wiki_upserts.is_empty());
        assert!(result.integrative_readings.is_empty());
        assert_eq!(result.candidates, vec!["candidate:relation"]);
        assert_eq!(result.human_source_proposals.len(), 1);
    }

    #[test]
    fn invalid_wiki_object_is_rejected_at_ai_kit_boundary() {
        let error = parse_contemplate_generated(
            r#"{
              "version":"aikit.contemplate-return/v1",
              "wiki_upserts":[{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:test","revision":2,"type":""}]
            }"#,
        )
        .unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_invalid_node");
    }

    #[test]
    fn unknown_transport_version_fails_closed() {
        let error = parse_contemplate_generated(
            r#"{"version":"desktop.contemplate/v99"}"#,
        )
        .unwrap_err();
        assert_eq!(
            error.code(),
            "knowledge.contemplate_return_version_unsupported"
        );
    }
}
