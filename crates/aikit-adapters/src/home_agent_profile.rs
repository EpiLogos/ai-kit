//! AIKit intake of the home-store agent seeds (`<home>/agent-profiles`).
//!
//! These are AIKit-home-authored agent seeds: standing identity only — id,
//! name, description, world tie in prose. No harness and no model live here;
//! binding happens at instantiation, when a live harness registers itself
//! through Actuation. AIKit consumes the seed as identity, never as runtime
//! selection.
//!
//! Discovery is exactly-one: one seed is disclosed, zero is an honest absence,
//! and several ask for specificity rather than guess.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One home-store agent seed, parsed from a `schema = 1` TOML document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeAgentProfile {
    pub schema: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HomeAgentProfileDocument {
    agent: HomeAgentProfile,
}

/// What discovery found: valid seeds and the files that could not be read.
/// Problems are disclosed, never swallowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeAgentProfileDiscovery {
    pub profiles: Vec<HomeAgentProfile>,
    pub sources: Vec<PathBuf>,
    pub problems: Vec<String>,
}

impl HomeAgentProfileDiscovery {
    /// The exactly-one seed, when discovery found precisely one.
    pub fn exactly_one(&self) -> Option<&HomeAgentProfile> {
        if self.profiles.len() == 1 {
            self.profiles.first()
        } else {
            None
        }
    }
}

/// Discover agent seeds under `<home>/agent-profiles`, one directory deep
/// (e.g. `epilogos/oi-development.toml`). Missing root is a valid absence.
pub fn discover_home_agent_profiles(home: &Path) -> HomeAgentProfileDiscovery {
    let mut discovery = HomeAgentProfileDiscovery::default();
    let root = home.join("agent-profiles");
    let Ok(entries) = fs::read_dir(&root) else {
        return discovery;
    };
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(children) = fs::read_dir(&path) {
                files.extend(
                    children
                        .flatten()
                        .map(|child| child.path())
                        .filter(|child| {
                            child
                                .extension()
                                .is_some_and(|extension| extension == "toml")
                        }),
                );
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            files.push(path);
        }
    }
    files.sort();
    for file in files {
        match fs::read_to_string(&file)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_home_agent_profile(&text))
        {
            Ok(profile) => {
                discovery.profiles.push(profile);
                discovery.sources.push(file);
            }
            Err(error) => discovery
                .problems
                .push(format!("{}: {error}", file.display())),
        }
    }
    discovery
}

/// Parse one `schema = 1` home agent seed from TOML text.
pub fn parse_home_agent_profile(text: &str) -> Result<HomeAgentProfile, String> {
    let document: HomeAgentProfileDocument =
        toml::from_str(text).map_err(|error| format!("invalid agent seed document: {error}"))?;
    let mut agent = document.agent;
    if agent.schema != 1 {
        return Err(format!("unsupported agent seed schema {}", agent.schema));
    }
    if agent.id.trim().is_empty() || agent.id != agent.id.trim() {
        return Err("agent seed has no usable id".to_owned());
    }
    agent.id = agent.id.trim().to_owned();
    Ok(agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: &str = r#"
[agent]
schema = 1
id = "agent/epilogos/test-seed"
name = "Test Seed"
description = "Standing identity only; harness binds at instantiation."

[agent.persona]
guidance = []
"#;

    #[test]
    fn parses_standing_identity_and_ignores_other_tables() {
        let profile = parse_home_agent_profile(SEED).unwrap();
        assert_eq!(profile.id, "agent/epilogos/test-seed");
        assert_eq!(profile.name.as_deref(), Some("Test Seed"));
        assert_eq!(profile.schema, 1);
    }

    #[test]
    fn rejects_unknown_schema() {
        let text = SEED.replace("schema = 1", "schema = 2");
        assert!(parse_home_agent_profile(&text).is_err());
    }

    #[test]
    fn discovery_is_exactly_one_and_discloses_problems() {
        let home = std::env::temp_dir().join(format!("aikit-home-seed-{}", std::process::id()));
        let seed_dir = home.join("agent-profiles").join("epilogos");
        fs::create_dir_all(&seed_dir).unwrap();
        fs::write(seed_dir.join("test-seed.toml"), SEED).unwrap();
        fs::write(seed_dir.join("broken.toml"), "not = valid =").unwrap();

        let discovery = discover_home_agent_profiles(&home);
        assert_eq!(discovery.profiles.len(), 1);
        assert_eq!(
            discovery.exactly_one().unwrap().id,
            "agent/epilogos/test-seed"
        );
        assert_eq!(
            discovery.problems.len(),
            1,
            "the broken file is disclosed, not swallowed"
        );

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn missing_root_is_a_valid_absence() {
        let home = std::env::temp_dir().join(format!("aikit-home-absent-{}", std::process::id()));
        let discovery = discover_home_agent_profiles(&home);
        assert!(discovery.profiles.is_empty());
        assert!(discovery.problems.is_empty());
        assert!(discovery.exactly_one().is_none());
    }
}
