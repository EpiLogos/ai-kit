//! One deterministic Markdown syntax reading for rendering and authored links.
//! No HTML executes here, no model is called, and parsing does not resolve a
//! target or grant source access. Offsets are UTF-8 bytes in the original text.
use std::collections::BTreeMap;
use std::ops::Range;

use aikit_core::knowledge_okf::{AuthoredRelationAnchor, AuthoredRelationEvidence};
use aikit_core::resource::{SourceRef, SourceRevision};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MARKDOWN_DOCUMENT_VERSION: &str = "aikit.markdown-document/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownNode {
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<MarkdownNode>,
}
impl MarkdownNode {
    fn new(kind: &str, span: Range<usize>) -> Self {
        Self {
            kind: kind.into(),
            start_byte: span.start,
            end_byte: span.end,
            text: None,
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }
    fn attr(&mut self, key: &str, value: impl Into<Value>) {
        self.attributes.insert(key.into(), value.into());
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownLink {
    pub start_byte: usize,
    pub end_byte: usize,
    pub raw_token: String,
    pub destination: String,
    pub display: String,
    pub kind: String,
    /// External URL syntax is not a resolved local semantic address.
    pub external: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownDocument {
    pub version: String,
    pub byte_length: usize,
    pub blocks: Vec<MarkdownNode>,
    pub links: Vec<MarkdownLink>,
    pub headings: Vec<MarkdownNode>,
    pub tags: Vec<String>,
    pub properties: BTreeMap<String, Value>,
    pub warnings: Vec<String>,
}

fn text_of(node: &MarkdownNode) -> String {
    let mut text = node.text.clone().unwrap_or_default();
    for child in &node.children {
        text.push_str(&text_of(child));
    }
    text
}
fn external_destination(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("//")
        || lower.contains("://")
        || [
            "mailto:",
            "data:",
            "javascript:",
            "vbscript:",
            "file:",
            "tel:",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Hashtags in actual text events only: not code, HTML, link labels or escaped
/// syntax. The source spelling remains untouched; this is declared tag syntax.
fn inline_tags(markdown: &str, span: Range<usize>) -> Vec<String> {
    let text = &markdown[span.clone()];
    let mut tags = Vec::new();
    let mut previous = markdown[..span.start].chars().next_back();
    let mut chars = text.char_indices().peekable();
    while let Some((index, current)) = chars.next() {
        let boundary = previous.is_none_or(|c: char| c.is_whitespace() || "([{,;".contains(c));
        if current == '#' && boundary {
            let start = index + 1;
            let mut end = start;
            while let Some(&(offset, c)) = chars.peek() {
                if !c.is_alphanumeric() && !"_-/".contains(c) {
                    break;
                }
                chars.next();
                end = offset + c.len_utf8();
            }
            let tag = &text[start..end];
            if tag.chars().any(char::is_alphanumeric) {
                tags.push(tag.to_owned());
            }
        }
        previous = Some(current);
    }
    tags
}

/// A safe structural representation, not arbitrary owner-supplied HTML. The
/// consumer renders a closed vocabulary and treats raw HTML as literal text.
pub fn parse_markdown_document(markdown: &str) -> MarkdownDocument {
    let options = Options::ENABLE_WIKILINKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut stack = vec![MarkdownNode::new("document", 0..markdown.len())];
    let mut links = Vec::new();
    let mut headings = Vec::new();
    let mut properties = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut tags = Vec::new();
    let mut events = 0usize;
    for (event, span) in Parser::new_ext(markdown, options).into_offset_iter() {
        events += 1;
        if events > 200_000 || stack.len() > 128 {
            let mut text = MarkdownNode::new("text", 0..markdown.len());
            text.text = Some(markdown.to_owned());
            return MarkdownDocument {version: MARKDOWN_DOCUMENT_VERSION.into(), byte_length: markdown.len(), blocks: vec![text], links: Vec::new(), headings: Vec::new(), tags: Vec::new(), properties, warnings: vec!["Markdown nesting or node budget exceeded; original source is shown without link inference".into()]};
        }
        match event {
            Event::Start(tag) => {
                let mut node = MarkdownNode::new("group", span.clone());
                match tag {
                    Tag::Paragraph => node.kind = "paragraph".into(),
                    Tag::Heading { level, .. } => {
                        node.kind = "heading".into();
                        node.attr("level", level as u8);
                        node.attr("id", format!("wiki-heading-{}", span.start));
                    }
                    Tag::BlockQuote(_) => node.kind = "blockquote".into(),
                    Tag::CodeBlock(kind) => {
                        node.kind = "code-block".into();
                        if let pulldown_cmark::CodeBlockKind::Fenced(info) = kind {
                            node.attr("info", info.to_string());
                        }
                    }
                    Tag::List(start) => {
                        node.kind = "list".into();
                        if let Some(start) = start {
                            node.attr("start", start);
                        }
                    }
                    Tag::Item => node.kind = "item".into(),
                    Tag::Emphasis => node.kind = "emphasis".into(),
                    Tag::Strong => node.kind = "strong".into(),
                    Tag::Strikethrough => node.kind = "strikethrough".into(),
                    Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        ..
                    } => {
                        node.kind = "link".into();
                        node.attr("destination", dest_url.to_string());
                        node.attr("title", title.to_string());
                        node.attr(
                            "syntax",
                            if matches!(link_type, LinkType::WikiLink { .. }) {
                                "wiki"
                            } else {
                                "markdown"
                            },
                        );
                    }
                    Tag::Image {
                        dest_url, title, ..
                    } => {
                        node.kind = "image".into();
                        node.attr("destination", dest_url.to_string());
                        node.attr("title", title.to_string());
                    }
                    Tag::Table(alignments) => {
                        node.kind = "table".into();
                        node.attr(
                            "alignments",
                            Value::Array(
                                alignments
                                    .iter()
                                    .map(|align| {
                                        Value::String(format!("{align:?}").to_ascii_lowercase())
                                    })
                                    .collect(),
                            ),
                        );
                    }
                    Tag::TableHead => node.kind = "table-head".into(),
                    Tag::TableRow => node.kind = "table-row".into(),
                    Tag::TableCell => node.kind = "table-cell".into(),
                    Tag::FootnoteDefinition(label) => {
                        node.kind = "footnote".into();
                        node.attr("label", label.to_string());
                    }
                    Tag::MetadataBlock(_) => node.kind = "metadata".into(),
                    Tag::HtmlBlock => node.kind = "raw-html".into(),
                    _ => {}
                }
                stack.push(node);
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    continue;
                }
                let mut node = stack.pop().expect("an open Markdown element");
                if node.kind == "paragraph" {
                    let text = text_of(&node);
                    if let Some(anchor) = text
                        .split_whitespace()
                        .last()
                        .and_then(|token| token.strip_prefix('^'))
                    {
                        if !anchor.is_empty()
                            && anchor
                                .chars()
                                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                        {
                            node.attr("id", format!("wiki-block-{anchor}"));
                            node.attr("block_id", anchor.to_owned());
                        }
                    }
                }
                if node.kind == "link" || node.kind == "image" {
                    let destination = node
                        .attributes
                        .get("destination")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    links.push(MarkdownLink {
                        start_byte: node.start_byte,
                        end_byte: node.end_byte,
                        raw_token: markdown
                            .get(node.start_byte..node.end_byte)
                            .unwrap_or_default()
                            .to_string(),
                        external: external_destination(&destination),
                        destination,
                        display: text_of(&node),
                        kind: if node.kind == "image" {
                            "image".into()
                        } else {
                            node.attributes
                                .get("syntax")
                                .and_then(Value::as_str)
                                .unwrap_or("markdown")
                                .into()
                        },
                    });
                }
                if node.kind == "heading" {
                    headings.push(node.clone());
                }
                if node.kind == "metadata" {
                    let body = text_of(&node);
                    match serde_yaml::from_str::<BTreeMap<String, Value>>(&body) {
                        Ok(value) => properties = value,
                        Err(error) => {
                            warnings.push(format!("Markdown properties could not be read: {error}"))
                        }
                    }
                } else {
                    stack.last_mut().expect("document root").children.push(node);
                }
            }
            other => {
                let mut node = MarkdownNode::new("text", span);
                match other {
                    Event::Text(text) => {
                        if !stack.iter().any(|parent| {
                            matches!(
                                parent.kind.as_str(),
                                "code-block" | "metadata" | "link" | "image" | "raw-html"
                            )
                        }) {
                            tags.extend(inline_tags(markdown, node.start_byte..node.end_byte));
                        }
                        node.text = Some(text.into_string());
                    }
                    Event::Code(text) => {
                        node.kind = "code".into();
                        node.text = Some(text.into_string());
                    }
                    Event::Html(text) | Event::InlineHtml(text) => {
                        node.kind = "raw-html".into();
                        node.text = Some(text.into_string());
                    }
                    Event::SoftBreak => node.kind = "soft-break".into(),
                    Event::HardBreak => node.kind = "hard-break".into(),
                    Event::Rule => node.kind = "rule".into(),
                    Event::TaskListMarker(checked) => {
                        node.kind = "task".into();
                        node.attr("checked", checked);
                    }
                    Event::FootnoteReference(label) => {
                        node.kind = "footnote-reference".into();
                        node.text = Some(label.into_string());
                    }
                    Event::InlineMath(text) | Event::DisplayMath(text) => {
                        node.kind = "math".into();
                        node.text = Some(text.into_string());
                    }
                    _ => continue,
                }
                stack.last_mut().expect("document root").children.push(node);
            }
        }
    }
    if let Some(value) = properties.get("tags").or_else(|| properties.get("tag")) {
        match value {
            Value::String(value) => tags.extend(
                value
                    .split_whitespace()
                    .map(|tag| tag.trim_start_matches('#').to_owned()),
            ),
            Value::Array(values) => tags.extend(
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|tag| tag.trim_start_matches('#').to_owned()),
            ),
            _ => warnings.push("Markdown tags must be a string or an array of strings".into()),
        }
    }
    tags.retain(|tag| !tag.is_empty());
    tags.sort();
    tags.dedup();
    MarkdownDocument {
        version: MARKDOWN_DOCUMENT_VERSION.into(),
        byte_length: markdown.len(),
        blocks: stack.remove(0).children,
        links,
        headings,
        tags,
        properties,
        warnings,
    }
}

/// The compiler and renderer consume the SAME parse. Resolution is performed
/// later against the admitted source/Wiki candidate field, never by a browser.
pub fn authored_markdown_relations(
    source_ref: &SourceRef,
    revision: Option<&SourceRevision>,
    markdown: &str,
) -> Vec<AuthoredRelationEvidence> {
    authored_relations_from_document(source_ref, revision, &parse_markdown_document(markdown))
}

pub fn authored_relations_from_document(
    source_ref: &SourceRef,
    revision: Option<&SourceRevision>,
    document: &MarkdownDocument,
) -> Vec<AuthoredRelationEvidence> {
    document
        .links
        .iter()
        .filter(|link| !link.external)
        .filter_map(|link| {
            let (target, fragment) = match link.destination.split_once('#') {
                Some((target, fragment)) => (target, Some(fragment.to_owned())),
                None => (link.destination.as_str(), None),
            };
            let target = if target.is_empty() && fragment.is_some() {
                source_ref.as_str()
            } else {
                target
            };
            if target.trim().is_empty() {
                return None;
            }
            let mut evidence = AuthoredRelationEvidence::body_reference(
                source_ref.clone(),
                revision.cloned(),
                target.to_owned(),
                link.raw_token.clone(),
                (!link.display.is_empty()).then(|| link.display.clone()),
                fragment,
                AuthoredRelationAnchor::body(link.start_byte, link.end_byte),
            );
            if link.kind == "image" {
                evidence.relation = "embeds".into();
            }
            Some(evidence)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commonmark_and_wiki_share_exact_unicode_byte_occurrences() {
        let source = "🌿 [a **strong** label](<folder/A (one).md#Part> \"Title\") and [[Beta#^block|B]].\n\nA [reference][ref].\n\n[ref]: second.md#Heading\n";
        let document = parse_markdown_document(source);
        assert_eq!(document.links.len(), 3);
        assert_eq!(document.links[0].destination, "folder/A (one).md#Part");
        assert_eq!(document.links[0].display, "a strong label");
        assert_eq!(document.links[1].kind, "wiki");
        for link in document.links {
            assert_eq!(&source[link.start_byte..link.end_byte], link.raw_token);
        }
    }
    #[test]
    fn ordinary_frontmatter_is_not_an_okf_or_ql_admission_requirement() {
        let document = parse_markdown_document("---\ntitle: Work\naliases: [A, B]\ntags: [notes, design]\n---\n# One\n\n- [x] Ready\n\n```text\n[[not a link]]\n```\n");
        assert_eq!(document.tags, ["design", "notes"]);
        assert_eq!(document.properties["title"], "Work");
        assert!(document.links.is_empty());
        assert_eq!(document.headings.len(), 1);
        assert!(document.blocks.iter().all(|node| node.kind != "metadata"));
    }
    #[test]
    fn escape_code_external_and_raw_html_never_become_local_semantics() {
        let text = r"\[[escaped]] `[[code]]` [web](https://example.test) <script>alert(1)</script> [[Real]]";
        let relations =
            authored_markdown_relations(&SourceRef::parse("source:test").unwrap(), None, text);
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].raw_target, "Real");
        let document = parse_markdown_document(text);
        assert!(document.links.iter().any(|link| link.external));
    }
    #[test]
    fn inline_tags_and_block_anchors_are_source_qualified_syntax() {
        let document = parse_markdown_document(
            "A #reading/now and #哲学. `#code` \\#escaped [#label](other.md). ^claim-1\n",
        );
        assert_eq!(document.tags, ["reading/now", "哲学"]);
        assert_eq!(document.blocks[0].attributes["block_id"], "claim-1");
    }
    #[test]
    fn local_heading_keeps_source_identity_and_anchors_do_not_depend_on_utf16() {
        let source = SourceRef::parse("source:test").unwrap();
        let text = "🙂 [here](#Heading)";
        let relation = authored_markdown_relations(&source, None, text).remove(0);
        assert_eq!(relation.raw_target, "source:test");
        assert_eq!(relation.anchor.start_byte, Some(5));
        assert_eq!(relation.fragment.as_deref(), Some("Heading"));
    }
}
