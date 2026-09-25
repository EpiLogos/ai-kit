//! Read-only native harness disclosure. Profile declarations, observed files,
//! composed tool names and runtime activation are separate facts. No provider
//! is launched, no projection is written, and native command/env values never
//! leave this owner.
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}};
use aikit_adapters::{harness_disclosure::{disclose, NativeEntry, NativeObservation}, profiles, tool_sources::TOOLS_PROJECTION_OWNERSHIP};
use aikit_core::{harness_profile::HarnessProfile, AikitError, Result};
use serde_json::{json, Value};
use crate::app::Service;

const MAX_OBSERVATION_BYTES: u64 = 2 * 1024 * 1024;

/// Observe only the exact files/collections the admitted profile declares.
/// A missing file is observed absence; unreadable or unsupported content is
/// unknown, and cannot be turned into drift against an invented empty map.
fn observe_tools(profile: &HarnessProfile, machine_home: &Path, cwd: &Path) -> (NativeObservation, Value, bool) {
    let Some(tools) = &profile.tools else { return (NativeObservation::default(), json!({"state":"not-declared"}), false); };
    if tools.observe.is_empty() { return (NativeObservation::default(), json!({"state":"not-declared","reason":"The profile declares no native tools observation path"}), false); }
    let mut native = NativeObservation::default();
    let mut evidence = Vec::new();
    let mut complete = true;
    let mut names = BTreeSet::new();
    for declaration in &tools.observe {
        let path = crate::client::expand_seam(&declaration.path, machine_home, cwd);
        let reading = (|| -> std::result::Result<Option<Value>, String> {
            let metadata = match fs::metadata(&path) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("The declared native configuration could not be read".into()),
            };
            if !metadata.is_file() || metadata.len() > MAX_OBSERVATION_BYTES { return Err("The declared native configuration is not a bounded regular file".into()); }
            let bytes = fs::read_to_string(&path).map_err(|_| "The declared native configuration is not readable UTF-8".to_string())?;
            if path.extension().is_some_and(|extension| extension == "toml") {
                let value = toml::from_str::<toml::Value>(&bytes).map_err(|_| "The declared native TOML could not be decoded".to_string())?;
                serde_json::to_value(value).map(Some).map_err(|_| "Native TOML has an unsupported value".into())
            } else if path.extension().is_some_and(|extension| extension == "yaml" || extension == "yml") {
                Err("Native YAML observation is not available through this reader".into())
            } else { serde_json::from_str(&bytes).map(Some).map_err(|_| "The declared native JSON could not be decoded".into()) }
        })();
        match reading {
            Err(reason) => { complete = false; evidence.push(json!({"path":path,"state":"unavailable","reason":reason})); }
            Ok(None) => evidence.push(json!({"path":path,"state":"absent"})),
            Ok(Some(document)) => {
                let mut collection = &document;
                for segment in declaration.collection.split('.') { collection = collection.get(segment).unwrap_or(&Value::Null); }
                if !collection.is_null() && !collection.is_object() {
                    complete = false; evidence.push(json!({"path":path,"state":"unavailable","reason":"The declared tools collection is not an object"})); continue;
                }
                if let Some(entries) = collection.as_object() { for (name, entry) in entries {
                    if names.insert(name.clone()) {
                        // The ownership marker alone is public; command paths,
                        // arguments, headers, URLs and environment values are not.
                        let owned = ["command","args","url"].iter().any(|key| entry.get(key).is_some_and(|value| value.to_string().contains(TOOLS_PROJECTION_OWNERSHIP)));
                        native.mcp_servers.push(NativeEntry {name:name.clone(), detail:owned.then(|| TOOLS_PROJECTION_OWNERSHIP.to_owned())});
                    }
                }}
                evidence.push(json!({"path":path,"state":"observed"}));
            }
        }
    }
    (native, json!({"state":if complete {"observed"} else {"partial"},"sources":evidence}), complete)
}

pub fn reading(service: &Service, cwd: &Path, requested: Option<&str>) -> Result<Value> {
    let machine_home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| AikitError::new("harness_disclosure.home_unavailable", "The native machine home is unavailable"))?;
    let composed = crate::encounter_mcp::tool_source_entries_from_service(service)?;
    let selected = requested.map(|name| crate::client::catalog_slug_for(name)
        .or_else(|| profiles::slug_for_target(&aikit_core::TargetId::new(name))).unwrap_or(name));
    if selected.is_some_and(|slug| profiles::for_slug(slug).is_none()) { return Err(AikitError::new("harness_disclosure.unknown_harness", "The owner carries no admitted profile for this harness")); }
    let readings: Vec<_> = profiles::all().filter(|(slug,_)| selected.is_none_or(|selected| selected == *slug)).map(|(_,profile)| {
        let (native, observation, complete) = observe_tools(profile, &machine_home, cwd);
        let mut disclosure = disclose(profile, &native, &composed);
        if !complete { for layer in &mut disclosure.layers { if layer.layer == "tools" { layer.drift.clear(); } } }
        json!({"disclosure":disclosure,"native_tools":observation,
            "native_hooks":{"state":"unobserved","reason":"A declared hook seam does not prove that this session loaded it"},
            "composed_tools":{"state":"resolved","count":composed.len()},
            "activation":{"state":"unobserved","reason":"Projection written does not mean the target session loaded it"}})
    }).collect();
    Ok(json!({"schema":"aikit.harness-disclosure/v1","profiles":readings,
        "standing":"Native profile declarations and source observations; not installed-harness detection or session activation proof"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_native_file_observation_redacts_values_and_preserves_unknown() {
        let root = std::env::temp_dir().join(format!("aikit-harness-disclosure-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&root).unwrap();
        let mut profile = profiles::for_slug("claude-code").unwrap().clone();
        // Absolute declared paths keep this real file test independent of the
        // developer's harness-home overrides without mutating process env.
        profile.tools.as_mut().unwrap().observe[0].path = root.join(".claude.json").to_string_lossy().into_owned();
        fs::write(root.join(".claude.json"), r#"{"mcpServers":{"owned":{"command":"aikit tool-protocol private-secret"},"foreign":{"env":{"TOKEN":"private-secret"}}}}"#).unwrap();
        let (native, state, complete) = observe_tools(&profile, &root, &root);
        assert!(complete);assert_eq!(state["state"],"observed");assert_eq!(native.mcp_servers.len(),2);
        assert!(!serde_json::to_string(&native).unwrap().contains("private-secret"));
        let disclosure = disclose(&profile,&native,&[]);
        assert_eq!(disclosure.layers.iter().find(|layer|layer.layer=="tools").unwrap().drift.len(),1);
        fs::write(root.join(".claude.json"), "unreadable private-secret").unwrap();
        let (_, state, complete) = observe_tools(&profile, &root, &root);
        assert!(!complete);assert_eq!(state["state"],"partial");assert!(!state.to_string().contains("private-secret"));
        fs::remove_file(root.join(".claude.json")).unwrap();
        let (native, state, complete) = observe_tools(&profile, &root, &root);
        assert!(complete);assert!(native.mcp_servers.is_empty());assert_eq!(state["sources"][0]["state"],"absent");
        fs::remove_dir_all(root).unwrap();
    }
}
