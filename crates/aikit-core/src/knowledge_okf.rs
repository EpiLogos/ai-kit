//! Open Knowledge Format v0.2 data model and validation.
//!
//! OKF is intentionally open: `type` is the only universally required field and
//! unknown producer keys/types are data to preserve, not schema errors. Text/YAML
//! codecs live in `aikit-adapters`; core owns only the validated, I/O-free model.

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
    pub fn new(metadata: Map<String, Value>, body: impl Into<String>) -> Result<Self> {
        validate_okf(&metadata)?;
        Ok(Self {
            metadata,
            body: body.into(),
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
            findings
                .push("status: must be one of draft, stable, deprecated (OKF v0.2)".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_types_and_extensions_are_valid_core_data() {
        let metadata = serde_json::json!({
            "type": "Future Knowledge Object",
            "title": "Portable",
            "producer_extension": {"nested": ["one", "two"], "future_flag": true}
        })
        .as_object()
        .unwrap()
        .clone();
        let parsed = OkfDocument::new(metadata.clone(), "# Body\n").unwrap();
        assert_eq!(parsed.object_type(), "Future Knowledge Object");
        assert_eq!(parsed.metadata, metadata);
        assert_eq!(
            parsed.metadata["producer_extension"]["future_flag"],
            Value::Bool(true)
        );
    }

    #[test]
    fn okf_floor_rejects_missing_type_but_not_unknown_fields() {
        let metadata = serde_json::json!({"title":"No type", "new_field":42})
            .as_object()
            .unwrap()
            .clone();
        let error = OkfDocument::new(metadata, "body").unwrap_err();
        assert_eq!(error.code(), "knowledge.okf_invalid");
    }
}
