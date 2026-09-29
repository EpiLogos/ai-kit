//! AIKit-owned launch preferences. These select a harness-native model for a
//! new chat; they never author or override a governed model dispatch policy.
use crate::encounter_service::{EncounterProtocol, EncounterProvider};
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

pub const SETTING_REF: &str = "ai-kit:models:models.default";
const SCHEMA: &str = "aikit.model-defaults/v1";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefault {
    pub model_id: String,
    /// Human name supplied by the native model observation; never used for selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_provider: Option<String>,
}
pub type Defaults = BTreeMap<String, ModelDefault>;
fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("config.model_defaults", message.to_string())
}
fn path(home: &AikitHome) -> PathBuf {
    home.state().join("config/model-defaults.json")
}
fn write_json(target: &std::path::Path, value: &Value) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| error("Model defaults have no parent directory"))?;
    std::fs::create_dir_all(parent).map_err(error)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec_pretty(value).map_err(error)?)
        .map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    temp.persist(target).map_err(error)?;
    Ok(())
}
pub fn declared(home: &AikitHome) -> Result<Option<Defaults>> {
    let bytes = match std::fs::read(path(home)) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
    };
    let doc: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if doc["schema"] != SCHEMA {
        return Err(error("Unrecognised model defaults schema"));
    }
    from_value(&doc["models"]).map(Some)
}
pub fn read(home: &AikitHome) -> Result<Defaults> {
    Ok(declared(home)?.unwrap_or_default())
}
pub fn write(home: &AikitHome, models: &Defaults) -> Result<()> {
    from_value(&json!(models))?;
    write_json(&path(home), &json!({"schema":SCHEMA,"models":models}))
}
pub fn clear(home: &AikitHome) -> Result<()> {
    match std::fs::remove_file(path(home)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(error(e)),
    }
}
pub fn from_value(value: &Value) -> Result<Defaults> {
    let map: Defaults = serde_json::from_value(value.clone()).map_err(error)?;
    if map.len() > 256 {
        return Err(error("At most 256 harness defaults may be configured"));
    }
    for (id, model) in &map {
        if model.model_name.as_ref().is_some_and(|name| {
            name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control)
        }) {
            return Err(error(
                "A model display name must be readable text, at most 512 bytes",
            ));
        }
        if id.is_empty()
            || id.len() > 256
            || id.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(error("Use a configured encounter provider ID"));
        }
        for value in std::iter::once(&model.model_id).chain(model.native_provider.iter()) {
            if value.is_empty()
                || value.len() > 512
                || value.chars().any(|c| c.is_control() || c.is_whitespace())
            {
                return Err(error(
                    "Model and provider IDs must be non-empty native IDs without whitespace",
                ));
            }
        }
    }
    Ok(map)
}
pub fn violations(value: &Value) -> Vec<String> {
    from_value(value)
        .err()
        .map(|e| vec![e.message().to_owned()])
        .unwrap_or_default()
}

/// Preference resolution is strictly below explicit governed policy and
/// never runs again for a resumed native chat, even if the settings moved.
pub fn for_open(
    home: &AikitHome,
    provider: &EncounterProvider,
    reconnect: bool,
) -> Result<Option<ModelDefault>> {
    if reconnect || provider.model_policy.is_some() {
        return Ok(None);
    }
    Ok(read(home)?.get(&provider.id).cloned())
}

/// Only launch-owned RPC protocols receive argv flags. ACP selects from its
/// advertised session control after opening, through the existing selector.
pub fn launch_argv(
    provider: &EncounterProvider,
    default: Option<&ModelDefault>,
) -> Result<Vec<String>> {
    let mut argv = provider.argv.clone();
    if let Some(default) = default {
        if matches!(
            provider.protocol,
            EncounterProtocol::PiRpc | EncounterProtocol::PrimeRpc
        ) {
            if argv.is_empty() || argv.iter().any(|arg| arg == "--") {
                return Err(error("A model-default launch requires an executable and cannot append model flags after an option terminator"));
            }
            let native = default.native_provider.as_deref().ok_or_else(|| {
                error("This harness needs its native provider as well as its model ID")
            })?;
            // Conflicting explicit launch choices remain explicit; never rely
            // on a harness's duplicate-flag precedence.
            if argv.iter().any(|arg| {
                arg == "--model"
                    || arg == "--provider"
                    || arg.starts_with("--model=")
                    || arg.starts_with("--provider=")
            }) {
                return Err(error("The harness launch already declares a model or provider; remove that explicit choice before using a default"));
            }
            argv.extend([
                "--provider".into(),
                native.into(),
                "--model".into(),
                default.model_id.clone(),
            ]);
        }
    }
    Ok(argv)
}
fn session_path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-model-defaults").join(format!(
        "{}.json",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    ))
}
/// A task launcher consumes the exact choice made by the opening owner, not
/// a later preference that could change between preparation and final exec.
pub fn bind_session(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
    default: Option<&ModelDefault>,
) -> Result<()> {
    write_json(
        &session_path(home, session),
        &json!({"schema":"aikit.session-model-default/v1","provider":provider.id,"default":default}),
    )
}
pub fn for_session(
    home: &AikitHome,
    session: &ResourceRef,
    provider: &EncounterProvider,
) -> Result<Option<ModelDefault>> {
    let bytes = match std::fs::read(session_path(home, session)) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
    };
    let doc: Value = serde_json::from_slice(&bytes).map_err(error)?;
    if doc["schema"] != "aikit.session-model-default/v1" || doc["provider"] != provider.id {
        return Err(error("Model default belongs to another native provider"));
    }
    serde_json::from_value(doc["default"].clone()).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider() -> EncounterProvider {
        serde_json::from_value(json!({"id":"pi","label":"Pi","protocol":"pi-rpc","argv":["pi","--mode","rpc","--no-session"]})).unwrap()
    }
    #[test]
    fn source_policy_and_reconnect_do_not_read_or_replace_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        std::fs::create_dir_all(path(&home).parent().unwrap()).unwrap();
        std::fs::write(path(&home), "invalid preference bytes").unwrap();
        let mut p = provider();
        assert!(for_open(&home, &p, true).unwrap().is_none());
        p.model_policy=Some(serde_json::from_value(json!({"source":"source/policy","revision":"rev/1","path":"/unchanged-policy","content_digest":"unchanged"})).unwrap());
        assert!(for_open(&home, &p, false).unwrap().is_none());
        assert_eq!(p.model_policy.unwrap().content_digest, "unchanged");
        assert_eq!(
            std::fs::read_to_string(path(&home)).unwrap(),
            "invalid preference bytes"
        );
    }
    #[test]
    fn final_task_launch_uses_the_opening_choice_despite_later_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let p = provider();
        let session = ResourceRef::parse("agent-session/default-task").unwrap();
        let first =
            from_value(&json!({"pi":{"model_id":"first","native_provider":"zai"}})).unwrap();
        write(&home, &first).unwrap();
        let chosen = for_open(&home, &p, false).unwrap();
        bind_session(&home, &session, &p, chosen.as_ref()).unwrap();
        write(
            &home,
            &from_value(&json!({"pi":{"model_id":"second","native_provider":"zai"}})).unwrap(),
        )
        .unwrap();
        assert_eq!(
            for_open(&home, &p, false).unwrap().unwrap().model_id,
            "second"
        );
        let bound = for_session(&home, &session, &p).unwrap();
        let argv = launch_argv(&p, bound.as_ref()).unwrap();
        assert_eq!(
            &argv[argv.len() - 4..],
            &["--provider", "zai", "--model", "first"]
        );
        let mut wrong = p.clone();
        wrong.id = "other".into();
        assert!(for_session(&home, &session, &wrong).is_err());
        let mut terminated = p.clone();
        terminated.argv.push("--".into());
        assert!(launch_argv(&terminated, bound.as_ref()).is_err());
        let mut explicit = p;
        explicit.argv.extend(["--model".into(), "explicit".into()]);
        assert!(launch_argv(&explicit, bound.as_ref()).is_err());
    }
}
