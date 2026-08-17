//! Text codec for Open Knowledge Format v0.2 Markdown envelopes.
//!
//! Core owns the open OKF data model and validation. This adapter owns YAML
//! syntax so parsing/serialization cannot pull codec concerns into the I/O-free
//! domain crate.

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
    use super::*;

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
}
