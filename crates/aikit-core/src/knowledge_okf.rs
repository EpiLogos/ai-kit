//! Open Knowledge Format v0.2 parsing and validation.
//!
//! OKF is intentionally open: `type` is the only universally required field and
//! unknown producer keys/types are data to preserve, not schema errors. This
//! module is I/O-free; callers provide Markdown text and receive the parsed
//! frontmatter plus body.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{AikitError, Result};

pub const OKF_VERSION: &str = "0.2";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkfDocument {
    /// Complete frontmatter mapping, including keys AIKit does not understand.
    pub metadata: Map<String, Value>,
    pub body: String,
}

impl OkfDocument {
    pub fn parse(markdown: &str) -> Result<Self> {
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
        validate_okf(&metadata)?;
        Ok(Self {
            metadata,
            body: body.to_string(),
        })
    }

    pub fn object_type(&self) -> &str {
        self.metadata
            .get("type")
            .and_then(Value::as_str)
            .expect("validated OKF documents always have a string type")
    }

    pub fn title(&self) -> Option<&str> {
        self.metadata.get("title").and_then(Value::as_str)
    }

    pub fn sources(&self) -> &[Value] {
        self.metadata
            .get("sources")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Serialize the complete metadata map again. Formatting may change, but
    /// unknown keys and values survive semantically unchanged.
    pub fn to_markdown(&self) -> Result<String> {
        let yaml = serde_yaml::to_string(&self.metadata).map_err(|error| {
            AikitError::new(
                "knowledge.okf_serialize",
                format!("could not serialize OKF frontmatter: {error}"),
            )
        })?;
        Ok(format!("---\n{}---\n{}", yaml, self.body))
    }
}

pub fn validate_okf(metadata: &Map<String, Value>) -> Result<()> {
    let mut findings = Vec::new();
    match metadata.get("type").and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => {}
        _ => findings.push("type: required non-empty string (OKF v0.2)".to_string()),
    }

    for key in ["title", "description", "resource"] {
        if metadata.get(key).is_some_and(|value| !value.is_string()) {
            findings.push(format!("{key}: must be a string when present"));
        }
    }

    if let Some(tags) = metadata.get("tags") {
        match tags.as_array() {
            Some(values) if values.iter().all(Value::is_string) => {}
            _ => findings.push("tags: must be a list of strings".to_string()),
        }
    }

    if let Some(status) = metadata.get("status") {
        let valid = status
            .as_str()
            .is_some_and(|value| matches!(value, "draft" | "stable" | "deprecated"));
        if !valid {
            findings.push(
                "status: must be one of draft, stable, deprecated (OKF v0.2)".to_string(),
            );
        }
    }

    if let Some(sources) = metadata.get("sources") {
        match sources.as_array() {
            None => findings.push("sources: must be a list".to_string()),
            Some(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    let Some(source) = entry.as_object() else {
                        findings.push(format!("sources[{index}]: must be a mapping"));
                        continue;
                    };
                    let resource_ok = source
                        .get("resource")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty());
                    if !resource_ok {
                        findings.push(format!(
                            "sources[{index}].resource: required non-empty string"
                        ));
                    }
                    if source.get("id").is_some_and(|value| !value.is_string()) {
                        findings.push(format!("sources[{index}].id: must be a string"));
                    }
                    if source
                        .get("usage_count")
                        .is_some_and(|value| value.as_u64().is_none())
                    {
                        findings.push(format!(
                            "sources[{index}].usage_count: must be a non-negative integer"
                        ));
                    }
                }
            }
        }
    }

    if findings.is_empty() {
        Ok(())
    } else {
        Err(AikitError::new(
            "knowledge.okf_invalid",
            findings.join("; "),
        ))
    }
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
        let trimmed = line.trim_end_matches(|ch| ch == '\r' || ch == '\n');
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
    fn unknown_types_and_extensions_round_trip() {
        let input = "---\ntype: Future Knowledge Object\ntitle: Portable\nproducer_extension:\n  nested: [one, two]\n  future_flag: true\n---\n# Body\n";
        let parsed = OkfDocument::parse(input).unwrap();
        assert_eq!(parsed.object_type(), "Future Knowledge Object");
        assert_eq!(
            parsed.metadata["producer_extension"]["future_flag"],
            Value::Bool(true)
        );
        let reparsed = OkfDocument::parse(&parsed.to_markdown().unwrap()).unwrap();
        assert_eq!(reparsed.metadata, parsed.metadata);
        assert_eq!(reparsed.body, parsed.body);
    }

    #[test]
    fn okf_floor_rejects_missing_type_but_not_unknown_fields() {
        let error = OkfDocument::parse("---\ntitle: No type\nnew_field: 42\n---\nbody")
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.okf_invalid");
    }
}
