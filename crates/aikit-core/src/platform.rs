//! Platforms and projection targets.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{AikitError, Result, err};

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
    pub const ZCODE: &'static str = "zcode";
    pub const AIDER: &'static str = "aider";
    pub const CURSOR_CLI: &'static str = "cursor-cli";
    pub const GEMINI_CLI: &'static str = "gemini-cli";
    pub const GOOSE: &'static str = "goose";
    pub const OPENCODE: &'static str = "opencode";
    pub const QWEN_CODE: &'static str = "qwen-code";
    // Round 3: ids align to Actuation catalog slugs.
    pub const ANTIGRAVITY: &'static str = "gemini-antigravity";
    pub const GROK_BOT: &'static str = "grok-bot";
    pub const KIMI: &'static str = "kimi";
    pub const OLLAMA: &'static str = "ollama";
    pub const OPENCLAW: &'static str = "openclaw";
    pub const PI: &'static str = "pi";
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
    pub fn zcode() -> Self {
        Self::new(Self::ZCODE)
    }
    pub fn aider() -> Self {
        Self::new(Self::AIDER)
    }
    pub fn cursor_cli() -> Self {
        Self::new(Self::CURSOR_CLI)
    }
    pub fn gemini_cli() -> Self {
        Self::new(Self::GEMINI_CLI)
    }
    pub fn goose() -> Self {
        Self::new(Self::GOOSE)
    }
    pub fn opencode() -> Self {
        Self::new(Self::OPENCODE)
    }
    pub fn qwen_code() -> Self {
        Self::new(Self::QWEN_CODE)
    }
    pub fn antigravity() -> Self {
        Self::new(Self::ANTIGRAVITY)
    }
    pub fn grok_bot() -> Self {
        Self::new(Self::GROK_BOT)
    }
    pub fn kimi() -> Self {
        Self::new(Self::KIMI)
    }
    pub fn ollama() -> Self {
        Self::new(Self::OLLAMA)
    }
    pub fn openclaw() -> Self {
        Self::new(Self::OPENCLAW)
    }
    pub fn pi() -> Self {
        Self::new(Self::PI)
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
                );
            }
        })
    }
}

/// Which technology owns a session's place: an open, validated name.
///
/// The built-ins are tmux, cmux and plain, but the name is deliberately not an
/// enum: a working-surface plan must be able to name the technology that owns
/// its place — `herdr` today, something else tomorrow — without a core release,
/// exactly as [`TargetId`] lets external adapter targets name themselves. What
/// a build can *drive* for a given name is a registry question downstream
/// (see `aikit-adapters`), never a parse failure here: an unregistered name is
/// a first-class declared-unsupported outcome, not a malformed plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaceTechnology(String);

impl PlaceTechnology {
    pub const TMUX: &'static str = "tmux";
    pub const CMUX: &'static str = "cmux";
    pub const PLAIN: &'static str = "plain";

    /// The longest name the discipline allows. Kept small so a typo in a
    /// persisted plan fails as a name problem, not as a mystery.
    pub const MAX_LEN: usize = 32;

    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn tmux() -> Self {
        Self::new(Self::TMUX)
    }

    pub fn cmux() -> Self {
        Self::new(Self::CMUX)
    }

    pub fn plain() -> Self {
        Self::new(Self::PLAIN)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The built-in multiplexer this name names, when it is one.
    ///
    /// A closed-set consumer (the store's session records, the mux stack,
    /// install plans) asks this one question and keeps its `MuxKind`; nothing
    /// upstream needs to refuse an open name on its behalf.
    pub fn known(&self) -> Option<MuxKind> {
        match self.0.as_str() {
            Self::TMUX => Some(MuxKind::Tmux),
            Self::CMUX => Some(MuxKind::Cmux),
            Self::PLAIN => Some(MuxKind::Plain),
            _ => None,
        }
    }

    /// The naming discipline: lowercase ASCII letters, digits and hyphens,
    /// one to [`Self::MAX_LEN`] characters. The same discipline plans,
    /// provider refs and registry entries all answer to, so a name that
    /// parses here is a name every layer can compare.
    fn is_well_formed(raw: &str) -> bool {
        !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }
}

impl fmt::Display for PlaceTechnology {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<MuxKind> for PlaceTechnology {
    fn from(kind: MuxKind) -> Self {
        Self::new(kind.as_str())
    }
}

impl FromStr for PlaceTechnology {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        if Self::is_well_formed(s) {
            Ok(Self::new(s))
        } else {
            err(
                "mux.technology_malformed",
                format!(
                    "`{s}` is not a place-technology name (lowercase letters, digits and \
                     hyphens, 1-{max} characters)",
                    max = Self::MAX_LEN
                ),
            )
        }
    }
}

impl Serialize for PlaceTechnology {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PlaceTechnology {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_platform_is_one_of_the_known_ones() {
        let p = Platform::current();
        assert!(matches!(
            p,
            Platform::Linux | Platform::Macos | Platform::Windows
        ));
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

    #[test]
    fn place_technology_names_are_open_so_a_plan_can_name_its_place() {
        let herdr: PlaceTechnology = "herdr".parse().unwrap();
        assert_eq!(herdr.as_str(), "herdr");
        // Open like TargetId: a name this build has no adapter for still names
        // a place. What can drive it is a downstream registry question.
        assert_eq!(herdr.known(), None);
        let arbitrary: PlaceTechnology = "future-thing-2".parse().unwrap();
        assert_eq!(arbitrary.known(), None);
    }

    #[test]
    fn built_in_place_technologies_map_to_their_mux_kinds_and_back() {
        assert_eq!(PlaceTechnology::tmux().known(), Some(MuxKind::Tmux));
        assert_eq!(PlaceTechnology::cmux().known(), Some(MuxKind::Cmux));
        assert_eq!(PlaceTechnology::plain().known(), Some(MuxKind::Plain));
        assert_eq!(
            PlaceTechnology::from(MuxKind::Tmux),
            PlaceTechnology::tmux()
        );
        assert_eq!(
            PlaceTechnology::from(MuxKind::Cmux).as_str(),
            MuxKind::Cmux.as_str()
        );
    }

    #[test]
    fn place_technology_names_answer_to_one_discipline() {
        // Uppercase refused.
        assert!("Herdr".parse::<PlaceTechnology>().is_err());
        assert!("TMUX".parse::<PlaceTechnology>().is_err());
        // Too long refused.
        let long = "a".repeat(PlaceTechnology::MAX_LEN + 1);
        assert!(long.parse::<PlaceTechnology>().is_err());
        let exact = "a".repeat(PlaceTechnology::MAX_LEN);
        assert!(exact.parse::<PlaceTechnology>().is_ok());
        // Empty refused.
        assert!("".parse::<PlaceTechnology>().is_err());
        // Other characters refused.
        assert!("herdr_room".parse::<PlaceTechnology>().is_err());
        assert!("herdr room".parse::<PlaceTechnology>().is_err());
        let error = "Herdr".parse::<PlaceTechnology>().unwrap_err();
        assert_eq!(error.code(), "mux.technology_malformed");
    }

    #[test]
    fn persisted_place_technology_values_round_trip_byte_identically() {
        // The same strings MuxKind has always serialized as.
        for name in ["tmux", "cmux", "plain"] {
            let wire = serde_json::to_string(&PlaceTechnology::new(name)).unwrap();
            assert_eq!(wire, format!("\"{name}\""));
            let parsed: PlaceTechnology = serde_json::from_str(&wire).unwrap();
            assert_eq!(parsed.as_str(), name);
        }
        // And an open name round trips too.
        let wire = serde_json::to_string(&PlaceTechnology::new("herdr")).unwrap();
        assert_eq!(wire, "\"herdr\"");
        let parsed: PlaceTechnology = serde_json::from_str(&wire).unwrap();
        assert_eq!(parsed.known(), None);
    }

    #[test]
    fn the_wire_refuses_names_that_break_the_discipline() {
        // Serde goes through the same validation as parse: a document cannot
        // carry a name the rest of the system could not compare.
        assert!(serde_json::from_str::<PlaceTechnology>("\"Herdr\"").is_err());
        assert!(serde_json::from_str::<PlaceTechnology>("\"\"").is_err());
        let long = format!("\"{}\"", "a".repeat(PlaceTechnology::MAX_LEN + 1));
        assert!(serde_json::from_str::<PlaceTechnology>(&long).is_err());
    }
}
