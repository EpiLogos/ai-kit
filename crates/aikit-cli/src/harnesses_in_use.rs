//! The harnesses the owner uses (`ai-kit:harnesses:harnesses.in-use`).
//!
//! Actuation's catalog declares every harness it knows (24 on 18 Oct 2026),
//! and detection probes each one: a harness that happens to be installed but
//! is never used (openclaw, whose `--version` hangs past its probe bound) is
//! detected, probed and listed as installable on every `aikit client status`.
//! Detection stays detection-first — what is on the machine is disclosed — but
//! the owner narrows which harnesses the suite works with, and this setting is
//! that narrowing: a list of Actuation catalog slugs (`claude-code`, `codex`,
//! `pi`, …). When it is declared, `aikit client status` asks Actuation to
//! detect only those (`actuation harness detect --only …`) and lists only
//! them, beside AIKit's own broker row. Unset, nothing narrows: every
//! catalogued harness is detected, as before. A slug the installed catalog
//! does not declare is Actuation's refusal, named on every row.

use std::path::PathBuf;

use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde_json::{json, Value};

pub const SCHEMA: &str = "aikit.harnesses-in-use/v1";
pub const SETTING_REF: &str = "ai-kit:harnesses:harnesses.in-use";
const MAX_SLUG: usize = 64;
const MAX_ENTRIES: usize = 64;

fn path(home: &AikitHome) -> PathBuf {
    home.state().join("config").join("harnesses-in-use.json")
}

fn failure(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("config.harnesses_in_use", message.to_string())
}

/// The declared list, or None when the owner never narrowed (every harness).
pub fn declared(home: &AikitHome) -> Result<Option<Vec<String>>> {
    let path = path(home);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(format!("{}: {error}", path.display()))),
    };
    let document: Value = serde_json::from_slice(&bytes).map_err(failure)?;
    if document["schema"] != json!(SCHEMA) {
        return Err(failure(format!(
            "{} does not carry schema {SCHEMA}",
            path.display()
        )));
    }
    serde_json::from_value(document["harnesses"].clone())
        .map(Some)
        .map_err(failure)
}

/// The narrowing in force: the declared list when it names at least one
/// harness, else None (an empty list narrows nothing rather than hiding every
/// harness).
pub fn in_force(home: &AikitHome) -> Result<Option<Vec<String>>> {
    Ok(declared(home)?.filter(|harnesses| !harnesses.is_empty()))
}

/// Replace the list atomically.
pub fn write(home: &AikitHome, harnesses: &[String]) -> Result<()> {
    let target = path(home);
    let parent = target
        .parent()
        .ok_or_else(|| failure("harnesses-in-use path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(failure)?;
    let body = serde_json::to_vec_pretty(&json!({ "schema": SCHEMA, "harnesses": harnesses }))
        .map_err(failure)?;
    let temp = parent.join(format!(".harnesses-in-use.{}.tmp", ulid::Ulid::generate()));
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

/// Remove the declaration: every catalogued harness is detected again.
pub fn clear(home: &AikitHome) -> Result<()> {
    match std::fs::remove_file(path(home)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(failure(error)),
    }
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= MAX_SLUG
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Validate a requested value; plain-language violations (empty = valid).
/// Slugs are checked for shape only: which harnesses exist is the installed
/// Actuation catalog's answer, not something AIKit can know here.
pub fn violations(value: &Value) -> Vec<String> {
    let Some(list) = value.as_array() else {
        return vec![
            "the harnesses in use are a list of Actuation catalog slugs, for example \
             [\"claude-code\", \"codex\", \"pi\"]"
                .into(),
        ];
    };
    let mut out = Vec::new();
    if list.len() > MAX_ENTRIES {
        out.push(format!("at most {MAX_ENTRIES} harnesses may be named"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for item in list {
        match item.as_str() {
            Some(slug) if valid_slug(slug) => {
                if !seen.insert(slug) {
                    out.push(format!("`{slug}` is named twice"));
                }
            }
            _ => out.push(format!(
                "{item} is not a catalog slug: lowercase letters, digits and '-', at most \
                 {MAX_SLUG} characters (`actuation harness catalog` lists them)"
            )),
        }
    }
    out
}

/// The list a valid value names, in the owner's order.
pub fn from_value(value: &Value) -> Result<Vec<String>> {
    let problems = violations(value);
    if !problems.is_empty() {
        return Err(failure(problems.join("; ")));
    }
    serde_json::from_value(value.clone()).map_err(failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("aikit"));
        (dir, home)
    }

    #[test]
    fn unset_narrows_nothing_and_an_empty_list_is_not_in_force() {
        let (_dir, home) = home();
        assert_eq!(declared(&home).unwrap(), None);
        assert_eq!(in_force(&home).unwrap(), None);
        write(&home, &[]).unwrap();
        assert_eq!(declared(&home).unwrap(), Some(vec![]));
        assert_eq!(in_force(&home).unwrap(), None);
    }

    #[test]
    fn a_written_list_reads_back_in_order_and_clears() {
        let (_dir, home) = home();
        let list = vec![
            "claude-code".to_owned(),
            "codex".to_owned(),
            "pi".to_owned(),
        ];
        write(&home, &list).unwrap();
        assert_eq!(in_force(&home).unwrap(), Some(list));
        clear(&home).unwrap();
        assert_eq!(declared(&home).unwrap(), None);
        clear(&home).unwrap();
    }

    #[test]
    fn violations_name_shape_duplicates_and_non_lists() {
        assert!(violations(&json!(["claude-code", "hermes-acp"])).is_empty());
        assert_eq!(violations(&json!("claude-code")).len(), 1);
        let problems = violations(&json!(["Claude", "codex", "codex", 3]));
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("named twice")));
        assert!(from_value(&json!(["pi", "zcode"])).unwrap() == vec!["pi", "zcode"]);
        assert!(from_value(&json!(["Pi"])).is_err());
    }
}
