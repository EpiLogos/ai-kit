//! Platforms and projection targets.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{err, AikitError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Platform {
    /// The platform this binary is running on.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Windows => "windows",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A projection target: a consumer of the resolved capability view.
///
/// Open-ended on purpose — third-party adapters must be able to name themselves
/// without a core release.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetId(String);

impl TargetId {
    pub const SHELL: &'static str = "shell";
    pub const CLAUDE_CODE: &'static str = "claude-code";
    pub const CODEX: &'static str = "codex";
    pub const DEEPSEEK_HARNESS: &'static str = "deepseek-harness";
    pub const AGENT_SKILLS: &'static str = "agent-skills";
    pub const HOOKS: &'static str = "hooks";
    pub const GUIDANCE: &'static str = "guidance";

    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn shell() -> Self {
        Self::new(Self::SHELL)
    }
    pub fn claude_code() -> Self {
        Self::new(Self::CLAUDE_CODE)
    }
    pub fn codex() -> Self {
        Self::new(Self::CODEX)
    }
    pub fn deepseek_harness() -> Self {
        Self::new(Self::DEEPSEEK_HARNESS)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TargetId {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        if s.is_empty() {
            return err("target.malformed", "a target id may not be empty");
        }
        Ok(Self::new(s))
    }
}

/// Which multiplexer owns a session's topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MuxKind {
    Tmux,
    Cmux,
    Plain,
}

impl MuxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MuxKind::Tmux => "tmux",
            MuxKind::Cmux => "cmux",
            MuxKind::Plain => "plain",
        }
    }
}

impl fmt::Display for MuxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MuxKind {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "tmux" => MuxKind::Tmux,
            "cmux" => MuxKind::Cmux,
            "plain" | "none" => MuxKind::Plain,
            other => {
                return err(
                    "mux.unknown",
                    format!("`{other}` is not a known multiplexer"),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_platform_is_one_of_the_known_ones() {
        let p = Platform::current();
        assert!(matches!(p, Platform::Linux | Platform::Macos | Platform::Windows));
    }

    #[test]
    fn target_ids_are_open_ended_so_external_adapters_can_name_themselves() {
        let custom: TargetId = "my-editor".parse().unwrap();
        assert_eq!(custom.as_str(), "my-editor");
        assert!("".parse::<TargetId>().is_err());
    }

    #[test]
    fn plain_is_accepted_under_both_spellings() {
        assert_eq!("plain".parse::<MuxKind>().unwrap(), MuxKind::Plain);
        assert_eq!("none".parse::<MuxKind>().unwrap(), MuxKind::Plain);
    }
}
