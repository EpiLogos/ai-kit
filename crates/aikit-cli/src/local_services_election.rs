//! The owner's election of the local decision provider and the local Redis NOW
//! service, as configuration-plane settings.
//!
//! Both elections were an explicit file passed to each command
//! (`--provider-file`, `--config-file`), invisible to the configuration plane and
//! so to `oi.profile/v1`. This records which election document AIKit uses by
//! default, as a *reference with the digest it was elected at*, never a copy:
//! the document stays the owner-native `aikit.decision-provider/v1` /
//! `aikit.redis-now-config/v1` that `aikit decide service` and
//! `aikit now-context service` write. Electing neither starts, stops or
//! provisions anything — lifecycle stays explicit — and neither mentions Workcell.
//!
//! A command that takes no explicit file resolves the election, and refuses when
//! the elected document has changed since it was elected (re-plan and re-apply),
//! so a changed provider can never be used as the one the owner elected.

use aikit_adapters::decision_endpoint::probe_models;
use aikit_core::{AikitError, Result};
use aikit_store::{AikitHome, RedisNowConfig, RedisNowStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const DECISION_SETTING_REF: &str = "ai-kit:local-services:decision.provider";
pub const REDIS_SETTING_REF: &str = "ai-kit:local-services:now.redis";
const SCHEMA: &str = "aikit.local-services-election/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Election {
    Decision,
    Redis,
}

impl Election {
    pub fn from_setting(setting_ref: &str) -> Option<Self> {
        match setting_ref {
            DECISION_SETTING_REF => Some(Self::Decision),
            REDIS_SETTING_REF => Some(Self::Redis),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Decision => "decision provider",
            Self::Redis => "Redis NOW",
        }
    }
    fn code(self) -> &'static str {
        match self {
            Self::Decision => "decision.no_election",
            Self::Redis => "now_context.no_election",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Elected {
    pub provider_file: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(default)]
    schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision: Option<Elected>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    redis: Option<Elected>,
}

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("config.local_services", message.to_string())
}

fn store_path(home: &AikitHome) -> PathBuf {
    home.state().join("config/local-services.json")
}

fn read_document(home: &AikitHome) -> Result<Document> {
    let bytes = match std::fs::read(store_path(home)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(e) => return Err(error(e)),
    };
    let document: Document = serde_json::from_slice(&bytes).map_err(error)?;
    if document.schema != SCHEMA {
        return Err(error("unrecognised local-services election schema"));
    }
    Ok(document)
}

fn write_document(home: &AikitHome, mut document: Document) -> Result<()> {
    document.schema = SCHEMA.into();
    let target = store_path(home);
    let parent = target
        .parent()
        .ok_or_else(|| error("no parent directory"))?;
    std::fs::create_dir_all(parent).map_err(error)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec_pretty(&document).map_err(error)?)
        .map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    temp.persist(&target).map_err(error)?;
    Ok(())
}

fn digest_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| error(format!("read {}: {e}", path.display())))?;
    if bytes.len() > 256 * 1024 {
        return Err(error(format!("{} exceeds 256 KiB", path.display())));
    }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// The owner-native check of the document a value points at. Returns the parsed
/// summary on success, the refusals otherwise.
fn check(election: Election, value: &Value) -> std::result::Result<(PathBuf, Value), Vec<String>> {
    let Some(raw) = value.as_str() else {
        return Err(vec![format!(
            "the {} election is the absolute path of its owner-native document, as a JSON string",
            election.label()
        )]);
    };
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(vec![format!("{raw} is not an absolute path")]);
    }
    let bytes = std::fs::read(&path).map_err(|e| vec![format!("cannot read {raw}: {e}")])?;
    if bytes.len() > 256 * 1024 {
        return Err(vec![format!("{raw} exceeds 256 KiB")]);
    }
    match election {
        Election::Decision => {
            let config = crate::decide::parse_provider_config(&bytes)
                .map_err(|e| vec![e.message().to_owned()])?;
            config
                .validate()
                .map_err(|e| vec![e.message().to_owned()])?;
            if config.mode == crate::decide::DecisionProviderMode::None {
                return Err(vec![
                    "mode none elects no provider; reset the setting instead".to_owned(),
                ]);
            }
            Ok((
                path,
                json!({
                    "mode": config.mode,
                    "address": config.address,
                    "model": config.limits.as_ref().map(|l| l.model.clone()),
                    "decision_model": config.decision_model.as_ref().map(|m| &m.artifact),
                }),
            ))
        }
        Election::Redis => {
            let config: RedisNowConfig =
                serde_json::from_slice(&bytes).map_err(|e| vec![e.to_string()])?;
            config
                .validate()
                .map_err(|e| vec![e.message().to_owned()])?;
            Ok((
                path,
                json!({
                    "address": config.address,
                    "key_prefix": config.key_prefix,
                    "database": config.database,
                }),
            ))
        }
    }
}

pub fn violations(election: Election, value: &Value) -> Vec<String> {
    check(election, value).err().unwrap_or_default()
}

/// Record the election: the document's path and the digest it has now.
pub fn write(home: &AikitHome, election: Election, value: &Value) -> Result<()> {
    let (path, _) = check(election, value).map_err(|problems| error(problems.join("; ")))?;
    let elected = Elected {
        provider_file: path.display().to_string(),
        sha256: digest_file(&path)?,
    };
    let mut document = read_document(home)?;
    match election {
        Election::Decision => document.decision = Some(elected),
        Election::Redis => document.redis = Some(elected),
    }
    write_document(home, document)
}

pub fn clear(home: &AikitHome, election: Election) -> Result<()> {
    let mut document = read_document(home)?;
    match election {
        Election::Decision => document.decision = None,
        Election::Redis => document.redis = None,
    }
    if document.decision.is_none() && document.redis.is_none() {
        return match std::fs::remove_file(store_path(home)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(error(e)),
        };
    }
    write_document(home, document)
}

pub fn elected(home: &AikitHome, election: Election) -> Result<Option<Elected>> {
    let document = read_document(home)?;
    Ok(match election {
        Election::Decision => document.decision,
        Election::Redis => document.redis,
    })
}

/// The file a command should use: the explicit one, else the election. A
/// missing election and a drifted one are distinct, named refusals.
pub fn resolve_file(
    home: &AikitHome,
    election: Election,
    explicit: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let Some(elected) = elected(home, election)? else {
        return Err(AikitError::new(
            election.code(),
            format!(
                "no {} file was given and none is elected; pass the file, or elect one with `aikit config plan --setting {}`",
                election.label(),
                match election {
                    Election::Decision => DECISION_SETTING_REF,
                    Election::Redis => REDIS_SETTING_REF,
                }
            ),
        ));
    };
    let path = PathBuf::from(&elected.provider_file);
    match digest_file(&path) {
        Ok(current) if current == elected.sha256 => Ok(path),
        Ok(_) => Err(AikitError::new(
            "config.election_drifted",
            format!(
                "{} has changed since it was elected as the {} (a service upgrade or an edit); re-plan and re-apply the election, or pass the file explicitly",
                path.display(),
                election.label()
            ),
        )),
        Err(e) => Err(AikitError::new(
            "config.election_unreadable",
            format!("the elected {} file is unavailable: {}", election.label(), e.message()),
        )),
    }
}

/// What the election and the thing it names look like right now, for the
/// disclosure plane: declared (the election), effective (whether the document
/// still is what was elected) and active (the live service answering).
pub fn disclosure(home: &AikitHome, election: Election) -> Value {
    let declared = match elected(home, election) {
        Ok(declared) => declared,
        Err(e) => return json!({"error": e.message()}),
    };
    let Some(declared) = declared else {
        return json!({"declared": null, "effective": null, "active": null});
    };
    let path = PathBuf::from(&declared.provider_file);
    let current = digest_file(&path);
    let document_state = match &current {
        Ok(current) if *current == declared.sha256 => "current",
        Ok(_) => "changed-since-elected",
        Err(_) => "unreadable",
    };
    let summary = check(election, &json!(declared.provider_file))
        .map(|(_, summary)| summary)
        .unwrap_or(Value::Null);
    let active = if document_state == "unreadable" {
        Value::Null
    } else {
        probe(election, &path)
    };
    json!({
        "declared": {"provider_file": declared.provider_file, "sha256": declared.sha256},
        "effective": {"document": document_state, "reading": summary},
        "active": active,
    })
}

/// A bounded, read-only look at the named service. Never starts anything.
fn probe(election: Election, path: &Path) -> Value {
    match election {
        Election::Decision => {
            let Ok(bytes) = std::fs::read(path) else {
                return Value::Null;
            };
            let Ok(config) = crate::decide::parse_provider_config(&bytes) else {
                return Value::Null;
            };
            let Ok(endpoint) = config.endpoint() else {
                return json!({"reachable": null, "reason": "no endpoint to probe for this mode"});
            };
            match probe_models("curl", &endpoint, 1500, None) {
                Ok(card) => {
                    json!({"reachable": true, "models": card["models"].as_array().map(|m| m.iter().filter_map(|e| e["name"].as_str()).collect::<Vec<_>>())})
                }
                Err(e) => json!({"reachable": false, "reason": e.message()}),
            }
        }
        Election::Redis => {
            let Ok(bytes) = std::fs::read(path) else {
                return Value::Null;
            };
            let Ok(mut config) = serde_json::from_slice::<RedisNowConfig>(&bytes) else {
                return Value::Null;
            };
            config.connect_timeout_ms = config.connect_timeout_ms.min(500);
            config.io_timeout_ms = config.io_timeout_ms.min(1000);
            match RedisNowStore::new(config).and_then(|store| store.status(None)) {
                Ok(status) => json!({"reachable": true, "redis_version": status.redis_version}),
                Err(e) => json!({"reachable": false, "reason": e.message()}),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().join("aikit"));
        (dir, home)
    }

    fn decision_file(dir: &Path, name: &str, mode: &str) -> String {
        let path = dir.join(name);
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schema": "aikit.decision-provider/v1", "mode": mode,
                "address": "127.0.0.1:8019",
                "limits": {"timeout_ms": 1000, "max_attempts": 1,
                           "max_input_tokens_per_attempt": 100,
                           "max_output_tokens_per_attempt": 100, "model": "kev-latest"}
            }))
            .unwrap(),
        )
        .unwrap();
        path.display().to_string()
    }

    #[test]
    fn an_election_is_a_reference_with_its_digest_and_drift_is_refused() {
        let (dir, home) = home();
        let file = decision_file(dir.path(), "p.json", "endpoint");
        assert!(resolve_file(&home, Election::Decision, None)
            .unwrap_err()
            .code()
            .contains("no_election"));
        write(&home, Election::Decision, &json!(file)).unwrap();
        assert_eq!(
            resolve_file(&home, Election::Decision, None).unwrap(),
            PathBuf::from(&file)
        );
        // An explicit file always wins and never consults the election.
        assert_eq!(
            resolve_file(&home, Election::Decision, Some("/x".into())).unwrap(),
            PathBuf::from("/x")
        );
        // The document changes (as after a service upgrade): not the elected one.
        decision_file(dir.path(), "p.json", "managed-local");
        assert_eq!(
            resolve_file(&home, Election::Decision, None)
                .unwrap_err()
                .code(),
            "config.election_drifted"
        );
        clear(&home, Election::Decision).unwrap();
        assert!(!store_path(&home).exists());
    }

    #[test]
    fn only_a_valid_owner_document_can_be_elected() {
        let (dir, _) = home();
        assert!(!violations(Election::Decision, &json!("relative.json")).is_empty());
        assert!(!violations(Election::Decision, &json!(5)).is_empty());
        assert!(!violations(Election::Decision, &json!("/nonexistent/p.json")).is_empty());
        let none = decision_file(dir.path(), "none.json", "none");
        let refusal = violations(Election::Decision, &json!(none));
        assert!(
            refusal.iter().any(|m| m.contains("config_invalid")
                || m.contains("mode none")
                || m.contains("no endpoint")),
            "{refusal:?}"
        );
        let good = decision_file(dir.path(), "good.json", "endpoint");
        assert!(violations(Election::Decision, &json!(good)).is_empty());
        let redis = dir.path().join("redis.json");
        std::fs::write(&redis, br#"{"schema":"wrong"}"#).unwrap();
        assert!(!violations(Election::Redis, &json!(redis.display().to_string())).is_empty());
    }
}
