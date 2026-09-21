//! Text codec for Open Knowledge Format v0.2 Markdown envelopes.
//!
//! Core owns the open OKF data model and validation. This adapter owns YAML and
//! Markdown syntax so parsing/serialization cannot pull codec concerns into the
//! I/O-free domain crate.

use aikit_core::knowledge_okf::AuthoredRelationEvidence;
use aikit_core::resource::{SourceRef, SourceRevision};
use aikit_core::{AikitError, OkfDocument, Result};
use serde_json::Value;

pub fn parse_okf_markdown(markdown: &str) -> Result<OkfDocument> {
    let (yaml, body) = split_frontmatter(markdown)?;
    let value: Value = serde_yaml::from_str(yaml).map_err(|error| {
        AikitError::new(
            "knowledge.okf_invalid_yaml",
            format!("malformed OKF YAML frontmatter: {error}"),
        )
    })?;
    let metadata = value.as_object().cloned().ok_or_else(|| {
        AikitError::new(
            "knowledge.okf_invalid_frontmatter",
            "OKF frontmatter must be a YAML mapping",
        )
    })?;
    OkfDocument::new(metadata, body)
}

pub fn render_okf_markdown(document: &OkfDocument) -> Result<String> {
    let yaml = serde_yaml::to_string(&document.metadata).map_err(|error| {
        AikitError::new(
            "knowledge.okf_serialize",
            format!("could not serialize OKF frontmatter: {error}"),
        )
    })?;
    Ok(format!("---\n{}---\n{}", yaml, document.body))
}

/// Parse explicit authored Markdown addressability without interpreting the
/// surrounding prose. Wikilinks and ordinary Markdown links become the weak
/// authored `references` relation; richer project predicates must come from
/// explicit metadata or a separately attributable derived reading.
///
/// Byte anchors are relative to the complete supplied source. YAML frontmatter,
/// fenced code and inline code are excluded. Explicit local embeds retain an
/// `embeds` relation instead of being confused with prose references.
pub fn parse_authored_markdown_relations(
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
    markdown: &str,
) -> Vec<AuthoredRelationEvidence> {
    crate::markdown_document::authored_markdown_relations(source_ref, source_revision, markdown)
}

fn split_frontmatter(markdown: &str) -> Result<(&str, &str)> {
    let markdown = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let Some(rest) = markdown.strip_prefix("---") else {
        return Err(AikitError::new(
            "knowledge.okf_missing_frontmatter",
            "missing OKF frontmatter envelope",
        ));
    };
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.okf_invalid_frontmatter",
                "opening OKF frontmatter delimiter must occupy its own line",
            )
        })?;

    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim() == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Ok((yaml, body));
        }
        offset += line.len();
    }

    Err(AikitError::new(
        "knowledge.okf_invalid_frontmatter",
        "missing closing OKF frontmatter delimiter",
    ))
}

#[cfg(test)]
mod tests {
    use aikit_core::knowledge_okf::{AuthoredRelationChannel, AuthoredRelationResolution};

    use super::*;

    fn source_ref() -> SourceRef {
        SourceRef::parse("source:wiki:flow").unwrap()
    }

    #[test]
    fn upstream_unknown_extensions_survive_parse_and_render() {
        let input = "---\ntype: Future Knowledge Object\ntitle: Portable\nproducer_extension:\n  nested: [one, two]\n  future_flag: true\n---\n# Body\n";
        let parsed = parse_okf_markdown(input).unwrap();
        assert_eq!(parsed.object_type(), "Future Knowledge Object");
        assert_eq!(parsed.metadata["producer_extension"]["future_flag"], true);
        let reparsed = parse_okf_markdown(&render_okf_markdown(&parsed).unwrap()).unwrap();
        assert_eq!(reparsed.metadata, parsed.metadata);
        assert_eq!(reparsed.body, parsed.body);
    }

    #[test]
    fn malformed_or_missing_envelopes_fail_closed() {
        assert_eq!(
            parse_okf_markdown("type: Note\n").unwrap_err().code(),
            "knowledge.okf_missing_frontmatter"
        );
        assert_eq!(
            parse_okf_markdown("---\ntype: [\n---\n")
                .unwrap_err()
                .code(),
            "knowledge.okf_invalid_yaml"
        );
    }

    #[test]
    fn wikilinks_and_markdown_links_preserve_authored_spelling_and_fragments() {
        let revision = SourceRevision::parse("rev-12").unwrap();
        let markdown = "See [[Flow]], [[knowledge/Living Wiki#Current whole|the Wiki]], and [Change Horizon](knowledge/change-horizon.md#impact).\n";
        let relations = parse_authored_markdown_relations(&source_ref(), Some(&revision), markdown);
        assert_eq!(relations.len(), 3);

        assert_eq!(relations[0].raw_target, "Flow");
        assert_eq!(relations[0].raw_token, "[[Flow]]");
        assert_eq!(relations[0].channel, AuthoredRelationChannel::Body);
        assert!(matches!(
            relations[0].resolution,
            AuthoredRelationResolution::Unresolved
        ));

        assert_eq!(relations[1].raw_target, "knowledge/Living Wiki");
        assert_eq!(relations[1].fragment.as_deref(), Some("Current whole"));
        assert_eq!(relations[1].display.as_deref(), Some("the Wiki"));

        assert_eq!(relations[2].raw_target, "knowledge/change-horizon.md");
        assert_eq!(relations[2].fragment.as_deref(), Some("impact"));
        assert_eq!(relations[2].display.as_deref(), Some("Change Horizon"));
        assert_eq!(
            &markdown
                [relations[2].anchor.start_byte.unwrap()..relations[2].anchor.end_byte.unwrap()],
            relations[2].raw_token
        );
    }

    #[test]
    fn parser_skips_frontmatter_and_code_but_retains_explicit_embeds() {
        let markdown = r#"---
type: Concept
relations:
  develops: ["[[metadata is not body]]"]
---
Visible [[Living Wiki]].
`[[inline literal]]`

```md
[[fenced literal]]
[also literal](literal.md)
```

![[embedded-note]]
Normal [Flow](flow.md).
"#;
        let relations = parse_authored_markdown_relations(&source_ref(), None, markdown);
        assert_eq!(relations.len(), 3);
        assert_eq!(relations[0].raw_target, "Living Wiki");
        assert_eq!(relations[1].raw_target, "embedded-note");
        assert_eq!(relations[1].relation, "embeds");
        assert_eq!(relations[2].raw_target, "flow.md");
    }

    #[test]
    fn external_markdown_links_are_left_for_external_resource_providers() {
        let markdown = "[site](https://example.com) and [local](wiki/local.md)";
        let relations = parse_authored_markdown_relations(&source_ref(), None, markdown);
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].raw_target, "wiki/local.md");
    }
}
