//! Text codec for Open Knowledge Format v0.2 Markdown envelopes.
//!
//! Core owns the open OKF data model and validation. This adapter owns YAML and
//! Markdown syntax so parsing/serialization cannot pull codec concerns into the
//! I/O-free domain crate.

use aikit_core::knowledge_okf::{AuthoredRelationAnchor, AuthoredRelationEvidence};
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
/// fenced code, inline code and Obsidian embed syntax are excluded from this
/// portable first tranche.
pub fn parse_authored_markdown_relations(
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
    markdown: &str,
) -> Vec<AuthoredRelationEvidence> {
    let body_start = optional_frontmatter_body_start(markdown).unwrap_or(0);
    let mut relations = Vec::new();
    let mut base = body_start;
    let mut fence: Option<(u8, usize)> = None;

    for line in markdown[body_start..].split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some((marker, minimum)) = fence {
            if fence_marker(trimmed)
                .is_some_and(|(found, count)| found == marker && count >= minimum)
            {
                fence = None;
            }
            base += line.len();
            continue;
        }
        if let Some(marker) = fence_marker(trimmed) {
            fence = Some(marker);
            base += line.len();
            continue;
        }

        parse_inline_links(source_ref, source_revision, line, base, &mut relations);
        base += line.len();
    }

    relations
}

fn parse_inline_links(
    source_ref: &SourceRef,
    source_revision: Option<&SourceRevision>,
    line: &str,
    base: usize,
    output: &mut Vec<AuthoredRelationEvidence>,
) {
    let bytes = line.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'`' {
            let run = byte_run(bytes, cursor, b'`');
            if let Some(close) = find_byte_run(bytes, cursor + run, b'`', run) {
                cursor = close + run;
            } else {
                break;
            }
            continue;
        }

        if bytes[cursor] == b'['
            && cursor + 1 < bytes.len()
            && bytes[cursor + 1] == b'['
            && (cursor == 0 || bytes[cursor - 1] != b'!')
        {
            if let Some(close) = find_pair(bytes, cursor + 2, b']', b']') {
                let end = close + 2;
                let inner = &line[cursor + 2..close];
                if let Some((target, display, fragment)) = parse_wikilink_inner(inner) {
                    output.push(AuthoredRelationEvidence::body_reference(
                        source_ref.clone(),
                        source_revision.cloned(),
                        target,
                        line[cursor..end].to_string(),
                        display,
                        fragment,
                        AuthoredRelationAnchor::body(base + cursor, base + end),
                    ));
                }
                cursor = end;
                continue;
            }
        }

        if bytes[cursor] == b'[' && (cursor == 0 || bytes[cursor - 1] != b'!') {
            if let Some(label_close) = find_byte(bytes, cursor + 1, b']') {
                let open_paren = label_close + 1;
                if open_paren < bytes.len() && bytes[open_paren] == b'(' {
                    if let Some(target_close) = find_byte(bytes, open_paren + 1, b')') {
                        let end = target_close + 1;
                        let label = &line[cursor + 1..label_close];
                        let destination = line[open_paren + 1..target_close].trim();
                        if let Some((target, fragment)) = parse_markdown_destination(destination) {
                            output.push(AuthoredRelationEvidence::body_reference(
                                source_ref.clone(),
                                source_revision.cloned(),
                                target,
                                line[cursor..end].to_string(),
                                (!label.is_empty()).then(|| label.to_string()),
                                fragment,
                                AuthoredRelationAnchor::body(base + cursor, base + end),
                            ));
                        }
                        cursor = end;
                        continue;
                    }
                }
            }
        }

        cursor += 1;
    }
}

fn parse_wikilink_inner(inner: &str) -> Option<(String, Option<String>, Option<String>)> {
    let (target_with_fragment, display) = match inner.split_once('|') {
        Some((target, display)) => (target.trim(), Some(display.to_string())),
        None => (inner.trim(), None),
    };
    if target_with_fragment.is_empty() {
        return None;
    }
    let (target, fragment) = split_fragment(target_with_fragment);
    if target.is_empty() {
        return None;
    }
    Some((target.to_string(), display, fragment.map(ToOwned::to_owned)))
}

fn parse_markdown_destination(destination: &str) -> Option<(String, Option<String>)> {
    if destination.is_empty() {
        return None;
    }
    // The portable first tranche addresses local/source Wiki relations. External
    // URLs remain ordinary Markdown until a provider explicitly owns them as an
    // external semantic target.
    let lower = destination.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
    {
        return None;
    }
    let destination = destination
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(destination);
    let (target, fragment) = split_fragment(destination);
    if target.is_empty() {
        return None;
    }
    Some((target.to_string(), fragment.map(ToOwned::to_owned)))
}

fn split_fragment(raw: &str) -> (&str, Option<&str>) {
    match raw.split_once('#') {
        Some((target, fragment)) => (target, Some(fragment)),
        None => (raw, None),
    }
}

fn fence_marker(line: &str) -> Option<(u8, usize)> {
    let bytes = line.as_bytes();
    let marker = *bytes.first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let count = byte_run(bytes, 0, marker);
    (count >= 3).then_some((marker, count))
}

fn byte_run(bytes: &[u8], start: usize, byte: u8) -> usize {
    let mut cursor = start;
    while cursor < bytes.len() && bytes[cursor] == byte {
        cursor += 1;
    }
    cursor - start
}

fn find_byte_run(bytes: &[u8], start: usize, byte: u8, minimum: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor] == byte {
            let count = byte_run(bytes, cursor, byte);
            if count >= minimum {
                return Some(cursor);
            }
            cursor += count.max(1);
        } else {
            cursor += 1;
        }
    }
    None
}

fn find_pair(bytes: &[u8], start: usize, first: u8, second: u8) -> Option<usize> {
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == first && bytes[cursor + 1] == second {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_byte(bytes: &[u8], start: usize, byte: u8) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|offset| start + offset)
}

fn optional_frontmatter_body_start(markdown: &str) -> Option<usize> {
    let bom = if markdown.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let source = &markdown[bom..];
    let rest = source
        .strip_prefix("---\r\n")
        .or_else(|| source.strip_prefix("---\n"))?;
    let rest_offset = markdown.len() - rest.len();
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim() == "---" {
            return Some(rest_offset + offset + line.len());
        }
        offset += line.len();
    }
    None
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
    fn parser_skips_frontmatter_code_and_embed_syntax() {
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
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].raw_target, "Living Wiki");
        assert_eq!(relations[1].raw_target, "flow.md");
    }

    #[test]
    fn external_markdown_links_are_left_for_external_resource_providers() {
        let markdown = "[site](https://example.com) and [local](wiki/local.md)";
        let relations = parse_authored_markdown_relations(&source_ref(), None, markdown);
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].raw_target, "wiki/local.md");
    }
}
