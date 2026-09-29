//! The owner's per-harness default session permission mode
//! (`ai-kit:permissions:permissions.default-mode`).
//!
//! The value maps a harness to one of the mode ids that harness itself
//! advertises (`default`, `accept_edits`, `plan`, …). AIKit never interprets a
//! mode: it only asks the harness, when a new native session opens, to switch
//! to the configured mode if the harness advertised it. What each mode allows
//! remains the harness's decision.
//!
//! A key names a harness the way an encounter provider is configured: the
//! encounter provider id (`hermes-probe`) or the file name of the provider's
//! executable (`hermes-acp`, `claude-code-acp`). The provider id wins when both
//! are present.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde_json::{json, Value};

pub const SCHEMA: &str = "aikit.permission-default-modes/v1";
pub const SETTING_REF: &str = "ai-kit:permissions:permissions.default-mode";
const MAX_ID: usize = 128;
const MAX_ENTRIES: usize = 256;

fn path(home: &AikitHome) -> PathBuf {
    home.state()
        .join("config")
        .join("permission-default-modes.json")
}

fn failure(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("config.permission_defaults", message.to_string())
}

/// The stored map; empty when nothing was ever configured.
pub fn read(home: &AikitHome) -> Result<BTreeMap<String, String>> {
    let path = path(home);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(failure(format!("{}: {error}", path.display()))),
    };
    let document: Value = serde_json::from_slice(&bytes).map_err(failure)?;
    if document["schema"] != json!(SCHEMA) {
        return Err(failure(format!(
            "{} does not carry schema {SCHEMA}",
            path.display()
        )));
    }
    serde_json::from_value(document["modes"].clone()).map_err(failure)
}

/// Whether anything was ever authored (distinguishes "unset" from "{}").
pub fn declared(home: &AikitHome) -> Result<Option<BTreeMap<String, String>>> {
    if path(home).exists() {
        read(home).map(Some)
    } else {
        Ok(None)
    }
}

/// Replace the whole map atomically. An empty map is a valid declaration.
pub fn write(home: &AikitHome, modes: &BTreeMap<String, String>) -> Result<()> {
    let target = path(home);
    let parent = target
        .parent()
        .ok_or_else(|| failure("permission defaults path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(failure)?;
    let body =
        serde_json::to_vec_pretty(&json!({ "schema": SCHEMA, "modes": modes })).map_err(failure)?;
    let temp = parent.join(format!(
        ".permission-default-modes.{}.tmp",
        ulid::Ulid::generate()
    ));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&body)?;
        file.sync_all()?;
        std::fs::rename(&temp, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(failure)
}

/// Remove the declaration entirely: no default mode is requested anywhere.
pub fn clear(home: &AikitHome) -> Result<()> {
    match std::fs::remove_file(path(home)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure(error)),
    }
}

fn valid_harness_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_ID
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn valid_mode_id(mode: &str) -> bool {
    !mode.trim().is_empty()
        && mode.len() <= MAX_ID
        && !mode.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Validate a requested value; returns plain-language violations (empty =
/// valid). Mode ids are checked for shape only: which modes exist is the
/// harness's advertisement at session open, not something AIKit can know.
pub fn violations(value: &Value) -> Vec<String> {
    let Some(map) = value.as_object() else {
        return vec![
            "the default permission modes are an object mapping a harness (encounter provider \
             id or executable name) to one of that harness's own mode ids"
                .into(),
        ];
    };
    let mut out = Vec::new();
    if map.len() > MAX_ENTRIES {
        out.push(format!("at most {MAX_ENTRIES} harnesses may be named"));
    }
    for (key, mode) in map {
        if !valid_harness_key(key) {
            out.push(format!(
                "`{key}` is not a harness name: use the encounter provider id or the \
                 executable's file name (letters, digits, '-', '_', '.', at most {MAX_ID})"
            ));
        }
        match mode.as_str() {
            Some(mode) if valid_mode_id(mode) => {}
            _ => out.push(format!(
                "the mode for `{key}` must be a non-empty mode id without spaces, at most \
                 {MAX_ID} characters (for example \"default\" or \"accept_edits\")"
            )),
        }
    }
    out
}

/// Parse an already validated value.
pub fn from_value(value: &Value) -> Result<BTreeMap<String, String>> {
    let problems = violations(value);
    if let Some(first) = problems.first() {
        return Err(failure(first));
    }
    serde_json::from_value(value.clone()).map_err(failure)
}

/// The configured default for one encounter provider: its provider id first,
/// then its executable's file name.
pub fn lookup(
    modes: &BTreeMap<String, String>,
    provider_id: &str,
    argv: &[String],
) -> Option<(String, String)> {
    if let Some(mode) = modes.get(provider_id) {
        return Some((provider_id.to_owned(), mode.clone()));
    }
    let executable = argv
        .first()
        .and_then(|program| std::path::Path::new(program).file_name())
        .and_then(|name| name.to_str())?;
    modes
        .get(executable)
        .map(|mode| (executable.to_owned(), mode.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_looks_up_by_provider_then_executable() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        assert!(declared(&home).unwrap().is_none());
        assert!(read(&home).unwrap().is_empty());
        let value = json!({"hermes-acp":"accept_edits","claude-code":"default"});
        assert!(violations(&value).is_empty());
        write(&home, &from_value(&value).unwrap()).unwrap();
        let stored = read(&home).unwrap();
        assert_eq!(stored["hermes-acp"], "accept_edits");
        let argv = vec!["/Users/x/.local/bin/hermes-acp".to_string()];
        assert_eq!(
            lookup(&stored, "hermes-probe", &argv),
            Some(("hermes-acp".into(), "accept_edits".into()))
        );
        assert_eq!(
            lookup(&stored, "claude-code", &argv),
            Some(("claude-code".into(), "default".into()))
        );
        assert_eq!(lookup(&stored, "other", &["pi".into()]), None);
        clear(&home).unwrap();
        assert!(declared(&home).unwrap().is_none());
    }

    #[test]
    fn refuses_values_that_are_not_a_harness_to_mode_map() {
        assert!(!violations(&json!(["accept_edits"])).is_empty());
        assert!(!violations(&json!({"hermes acp":"default"})).is_empty());
        assert!(!violations(&json!({"hermes-acp":""})).is_empty());
        assert!(!violations(&json!({"hermes-acp":"accept edits"})).is_empty());
        assert!(!violations(&json!({"hermes-acp":true})).is_empty());
        assert!(violations(&json!({})).is_empty());
    }
}
