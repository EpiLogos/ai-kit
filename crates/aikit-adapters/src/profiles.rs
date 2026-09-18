//! The harness profile instances: one `aikit.harness-profile/v1` document per
//! supported harness, embedded as TOML and parsed+validated once. The
//! documents carry only what the per-harness censuses evidenced — the
//! 2026-09-16 census for the first ten documents, the 2026-09-18
//! harness-adapter sort-out for the additions (each grounded in that
//! harness's admission census in this package and, where installed, the live
//! Omarchy machine) — an absent layer says nothing, a `none` model dispatch
//! carries its reason, and machine-specific paths are home-relative so the
//! data survives machines. Actuation's catalog stays the detection
//! authority; these documents are AIKit's handling declarations joined to it
//! by slug.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use aikit_core::harness_profile::HarnessProfile;
use aikit_core::platform::TargetId;

const EMBEDDED_PROFILES: &[(&str, &str)] = &[
    (
        "claude-code",
        r#"
schema = "aikit.harness-profile/v1"
slug = "claude-code"
edition = "cli"

[presence]
executables = ["claude"]
config-dir = "~/.claude"

[skills]
posture = "managed"
prefix = ".claude/skills"
observe = { paths = ["~/.claude/skills"] }

[guidance]
posture = "observed"
observe = ["~/.claude/CLAUDE.md"]

[hooks]
posture = "managed"
observe = [{ events = ["session-start", "user-prompt-submit", "pre-tool-use", "post-tool-use", "stop", "session-end", "notification", "pre-compact"] }]
project = { file = "~/.claude/settings.json", format = "claude-hook-map", ownership-identity = "aikit hook dispatch claude" }

[tools]
posture = "managed"
observe = [{ path = "~/.claude.json", collection = "mcpServers" }]
project = { file = "~/.claude.json", key = "mcpServers", format = "mcp-servers-record", merge = "preserve-foreign-sweep-owned" }
activation = "next-session-only"

[models]
posture = "observed"
dispatch = { native-provider-binding = { provider-ref = "provider:anthropic", selector-kind = "config-key", selector-name = "model" } }

[sessions]
posture = "observed"
protocol = "process"

[settings]
posture = "observed"
observe = ["~/.claude/settings.json"]

# The harness's own enforcement surface, declared for the configuration plane
# as ai-kit:claude:<key>. Grounded in the live config: the PreToolUse entry is
# the Central filesystem guardrail riding the AIKit hook chain (aikit hook
# dispatch claude PreToolUse); herdr's SessionStart stays foreign and is not
# AIKit's to declare.
[[settings.trust-settings]]
key = "hooks.fs-guardrail"
title = "PreToolUse filesystem guardrail"
description = "The PreToolUse enforcement entry Central ground law (filesystem-guardrails.md) requires in this harness; it rides the AIKit hook chain and blocks out-of-bounds writes with exit 2."
config = "~/.claude/settings.json hooks.PreToolUse[].hooks[].command"
value-schema = { type = "scalar" }
scopes = ["machine"]
"#,
    ),
    (
        "codex",
        r#"
schema = "aikit.harness-profile/v1"
slug = "codex"
edition = "cli"

[presence]
executables = ["codex"]
config-dir = "~/.codex"

[skills]
posture = "managed"
prefix = ".agents/skills"
shared-tree = "Project-stable selections only by default; shared trees are brokered unless the shared projection is explicitly accepted."
observe = { paths = ["~/.codex/skills"] }

[guidance]
posture = "observed"
observe = ["~/.codex/AGENTS.md"]

[hooks]
posture = "managed"
observe = [{ events = ["session-start", "user-prompt-submit", "pre-tool-use", "post-tool-use", "stop", "session-end", "notification", "pre-compact"], transports = ["hooks-json-file"] }]
project = { file = "~/.codex/hooks.json", format = "claude-hook-map", ownership-identity = "aikit hook dispatch codex" }

[tools]
posture = "observed"
observe = [{ path = "~/.codex/config.toml", collection = "mcp_servers" }]

[models]
posture = "observed"
dispatch = { native-provider-binding = { provider-ref = "provider:openai", selector-kind = "config-key", selector-name = "model" } }

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume"]

[settings]
posture = "observed"
observe = ["~/.codex/config.toml"]

# The harness's own trust surface, declared for the configuration plane as
# ai-kit:codex:<key> (the general trust pattern: declare what the harness's
# own config supports; the plane derives the disclosure). Grounded in the
# live config and Control/agents/governance/filesystem-guardrails.md.
[[settings.trust-settings]]
key = "projects.trust_level"
title = "Project workspace trust"
description = "Which project roots codex marks trusted. This ground trusts exactly the declared project roots, never $HOME wholesale."
config = '~/.codex/config.toml [projects."<root>"] trust_level'
value-schema = { type = "enum", options = ["trusted"] }
scopes = ["project"]

[[settings.trust-settings]]
key = "home.trust_level"
title = "Home wholesale trust"
description = "Whether $HOME is ever trusted wholesale. The 2026-09-16 narrowing removed ~/Work and tm02-field-test trust; absence is the declaration — HOME is a dwelling, not a workspace."
config = "~/.codex/config.toml (absence is the declaration)"
value-schema = { type = "boolean" }
scopes = ["machine"]
"#,
    ),
    (
        "zcode",
        r#"
schema = "aikit.harness-profile/v1"
slug = "zcode"
edition = "cli"

[presence]
config-dir = "~/.zcode/cli"

[skills]
posture = "brokered"
shared-tree = "No native skill tree; capability delivery is brokered with a fallback, never projected."

[hooks]
posture = "managed"
observe = [{ events = ["session-start", "user-prompt-submit", "pre-tool-use", "post-tool-use", "stop", "session-end", "permission-request", "post-tool-use-failure"] }]
project = { file = "~/.zcode/cli/config.json", format = "zcode-hook-wrapper", ownership-identity = "aikit hook dispatch zcode" }

[tools]
posture = "managed"
observe = [{ path = "~/.zcode/cli/config.json", collection = "mcp.servers" }]
project = { file = "~/.zcode/cli/config.json", key = "mcp.servers", format = "mcp-servers-record", merge = "preserve-foreign-sweep-owned" }
activation = "next-session-only"

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog declares no native provider binding for zcode; declaring one would be a guess, not a reading." } }

[sessions]
posture = "observed"
protocol = "process"

[settings]
posture = "observed"
observe = ["~/.zcode/cli/config.json"]

# The harness's own enforcement surface, declared for the configuration plane
# as ai-kit:zcode:<key>. Grounded in the live config: the PreToolUse entry is
# the Central filesystem guardrail riding the AIKit hook chain (aikit hook
# dispatch zcode PreToolUse); hooks.enabled true is its master switch.
[[settings.trust-settings]]
key = "hooks.fs-guardrail"
title = "PreToolUse filesystem guardrail"
description = "The PreToolUse enforcement entry Central ground law (filesystem-guardrails.md) requires in this harness; it rides the AIKit hook chain and blocks out-of-bounds writes with exit 2."
config = "~/.zcode/cli/config.json hooks.events.PreToolUse[].hooks[].command"
value-schema = { type = "scalar" }
scopes = ["machine"]
"#,
    ),
    (
        "pi",
        r#"
schema = "aikit.harness-profile/v1"
slug = "pi"
edition = "cli"

[presence]
executables = ["pi"]
config-dir = "~/.pi/agent"

[skills]
posture = "managed"
prefix = ".pi/skills"
observe = { paths = ["~/.pi/agent/skills"] }
shared-tree = "Reloads skills on /reload; isolated tree required for isolation."

[guidance]
posture = "observed"
observe = ["~/.pi/agent/AGENTS.md"]

[tools]
posture = "brokered"

[models]
posture = "observed"
dispatch = "provider-plural"
roster-note = "Provider and model chosen per invocation (--provider/--model); encounter model policy pins the native selector."

[sessions]
posture = "observed"
protocol = "rpc"
open-modes = ["attach"]
capabilities = { ordered-streaming = true, cancellation = true }
"#,
    ),
    (
        "gemini",
        r#"
schema = "aikit.harness-profile/v1"
slug = "gemini"
edition = "cli"

[presence]
executables = ["gemini"]
config-dir = "~/.gemini"

[skills]
posture = "brokered"
shared-tree = "Authored skill trees (~/.gemini/skills, .gemini/skills) are brokered; symlinks only, no writes."
observe = { paths = ["~/.gemini/skills"] }

[guidance]
posture = "observed"
observe = ["~/.gemini/GEMINI.md"]

[tools]
posture = "observed"
observe = [{ path = "~/.gemini/settings.json", collection = "mcpServers" }]

[models]
posture = "observed"
dispatch = "provider-plural"

[sessions]
posture = "observed"
protocol = "acp"
capabilities = { ordered-streaming = true, cancellation = true, permission-requests = true }
"#,
    ),
    (
        "kimi",
        r#"
schema = "aikit.harness-profile/v1"
slug = "kimi"
edition = "cli"

[presence]
executables = ["kimi"]
config-dir = "~/.kimi"

[skills]
posture = "brokered"
shared-tree = "No skill projection; a generated project AGENTS.md lists composed capsules."
observe = { paths = ["~/.kimi/skills"] }

[tools]
posture = "observed"
observe = [{ path = "~/.kimi/mcp.json", collection = "mcpServers" }]

[models]
posture = "observed"
dispatch = "provider-plural"
compatibility-note = "Config carries a default model; candidates are gated by the harness compatibility facts of the kimi adapter census."

[sessions]
posture = "observed"
protocol = "acp"
open-modes = ["resume"]
capabilities = { ordered-streaming = true, cancellation = true }
"#,
    ),
    (
        "openclaw",
        r#"
schema = "aikit.harness-profile/v1"
slug = "openclaw"
edition = "cli"

[presence]
executables = ["openclaw"]
config-dir = "~/.openclaw"

[skills]
posture = "brokered"
shared-tree = "Per-agent workspaces carry authored identity and memory files (SOUL.md, MEMORY.md); an unmanaged projection write would violate authored-file ownership."
observe = { paths = ["~/.openclaw/workspace/AGENTS.md"] }

[tools]
posture = "managed"
observe = [{ path = "~/.openclaw/mcp.json", collection = "mcpServers" }]
project = { file = "~/.openclaw/mcp.json", key = "mcpServers", format = "mcp-servers-record", merge = "preserve-foreign-sweep-owned" }
activation = "restart-client"

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog capability descriptor is undeclared for openclaw; no provider binding is recorded." } }

[sessions]
posture = "observed"
protocol = "process"
"#,
    ),
    (
        "cursor-cli",
        r#"
schema = "aikit.harness-profile/v1"
slug = "cursor-cli"
edition = "cli"

[presence]
executables = ["cursor-agent"]
config-dir = "~/.cursor"

[skills]
posture = "brokered"
shared-tree = "No skill projection; one always-applied AIKit rule file is written by the existing adapter path."

[guidance]
posture = "observed"
observe = ["~/.cursor/rules"]

[tools]
posture = "observed"
observe = [{ path = "~/.cursor/mcp.json", collection = "mcpServers" }]

[models]
posture = "observed"
dispatch = { none = { reason = "Cursor's model surface is subscription-mediated; no config-key binding is recorded." } }

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume"]
"#,
    ),
    (
        "qwen-code",
        r#"
schema = "aikit.harness-profile/v1"
slug = "qwen-code"
edition = "cli"

[presence]
executables = ["qwen"]

[skills]
posture = "brokered"
shared-tree = "No skill projection; a generated QWEN.md lists composed capsules."

[tools]
posture = "observed"

[models]
posture = "observed"
dispatch = "provider-plural"

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume"]
"#,
    ),
    (
        "ollama",
        r#"
schema = "aikit.harness-profile/v1"
slug = "ollama"
edition = "custom"

[presence]
executables = ["ollama"]
service-url = "http://127.0.0.1:11434"

[skills]
posture = "observed"

[tools]
posture = "observed"

[models]
posture = "observed"
dispatch = "provider-plural"
roster-note = "Local model serving; the models facet inventory reads /api/tags."

[sessions]
posture = "observed"
protocol = "process"
"#,
    ),
    (
        "hermes",
        // Evidence: live Omarchy machine 2026-09-18 (hermes v0.19.0; ~/.hermes
        // with config.yaml, SOUL.md, skills/ directories; `hermes --help`
        // showing --provider/-m/--resume and the skills/hooks/mcp subcommands)
        // plus the catalog r10 descriptor. No AIKit hermes adapter exists, so
        // every writable posture is refused by the absence of a seam, not by
        // silence.
        r#"
schema = "aikit.harness-profile/v1"
slug = "hermes"
edition = "cli"

[presence]
executables = ["hermes"]
config-dir = "~/.hermes"

[skills]
posture = "brokered"
shared-tree = "SOUL.md, memories and the skills tree are hermes-authored and -managed; projection stays brokered until a hermes adapter owns a seam."
observe = { paths = ["~/.hermes/skills"] }

[guidance]
posture = "observed"
observe = ["~/.hermes/SOUL.md"]

[models]
posture = "observed"
dispatch = "provider-plural"
roster-note = "Provider and model chosen per invocation (--provider/-m) or the config.yaml model defaults (observed 2026-09-18: provider zai, model glm-5.3-flash); hermes carries its own provider catalog."

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume"]
"#,
    ),
    (
        "hermes-acp",
        // Evidence: live Omarchy machine 2026-09-18 (`hermes acp --help`:
        // "Start Hermes Agent in ACP mode for editor integration",
        // hermes-acp 0.19.0, shares ~/.hermes). The catalog edition is
        // "acp-bridge", which AIKit's HarnessEditionKind vocabulary has no
        // member for; `cli` is the least-distorting member (a command-line
        // bridge process) and the sessions layer carries the ACP truth.
        r#"
schema = "aikit.harness-profile/v1"
slug = "hermes-acp"
edition = "cli"

[presence]
executables = ["hermes-acp"]
config-dir = "~/.hermes"

[skills]
posture = "brokered"
shared-tree = "The ACP bridge rides the hermes config tree (~/.hermes); it owns no independent skill surface."

[sessions]
posture = "observed"
protocol = "acp"
"#,
    ),
    (
        "grok-bot",
        // Evidence: catalog r10 descriptor (cli+service daemon, aliases
        // gbot, executable names grok-bot/gbot, config-dir ~/.grokbot) and
        // the clients/grokbot.rs admission census (brokered). Absent on the
        // 2026-09-18 machine; nothing is claimed beyond the record.
        r#"
schema = "aikit.harness-profile/v1"
slug = "grok-bot"
edition = "custom"

[presence]
executables = ["grok-bot", "gbot"]
config-dir = "~/.grokbot"

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog capability-gap document records the grok-bot surface as unobserved, including the daemon's provider binding; no model dispatch is declared." } }

[sessions]
posture = "observed"
protocol = "process"
"#,
    ),
    (
        "gemini-antigravity",
        // Evidence: catalog r10 descriptor (edition ide, config-dir
        // ~/.gemini/antigravity, detected inside the gemini config tree) and
        // the clients/antigravity.rs admission census (brokered, Ide).
        // Absent on the 2026-09-18 machine.
        r#"
schema = "aikit.harness-profile/v1"
slug = "gemini-antigravity"
edition = "ide"

[presence]
config-dir = "~/.gemini/antigravity"

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog capability-gap document records the antigravity surface as unobserved beyond detection; no model dispatch is declared." } }
"#,
    ),
    (
        "aider",
        // Evidence: clients/aider.rs admission census (process-per-invocation
        // CLI reading .aider.conf.yml and --read files; no skill tree, tool
        // protocol or delegation; brokered). Absent on the 2026-09-18
        // machine.
        r#"
schema = "aikit.harness-profile/v1"
slug = "aider"
edition = "cli"

[presence]
executables = ["aider"]

[skills]
posture = "brokered"
shared-tree = "No skill tree, tool protocol or delegation surface; a process-per-invocation CLI that reads .aider.conf.yml and --read files at startup (aider census)."

[guidance]
posture = "observed"
observe = ["CONVENTIONS.md"]

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog declares no capability document for aider and the census records no provider binding surface; no model dispatch is declared." } }

[sessions]
posture = "observed"
protocol = "process"
"#,
    ),
    (
        "deepseek-harness",
        // Evidence: clients/dsh.rs admission census (composition-managed
        // faculties — Cordis Host/Client plugins, agent presets, skills,
        // model tools; no fixed tree to project; brokered; edition Custom).
        // No executable observed on any machine; presence says nothing
        // rather than guessing a binary name.
        r#"
schema = "aikit.harness-profile/v1"
slug = "deepseek-harness"
edition = "custom"

[skills]
posture = "brokered"
shared-tree = "Composition-managed (Cordis Host/Client plugins, agent presets): there is no fixed skill tree to project (DSH census)."

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog declares no capability document for deepseek-harness and the census records composition-managed model tools, not a provider binding; no model dispatch is declared." } }

[sessions]
posture = "observed"
protocol = "process"
"#,
    ),
    (
        "goose",
        // Evidence: clients/goose.rs admission census (config
        // ~/.config/goose/config.yaml; .goosehints project instructions;
        // Claude-compatible Agent Skills via the built-in skills extension;
        // 15+ providers via config; resumable sessions; brokered). Absent on
        // the 2026-09-18 machine.
        r#"
schema = "aikit.harness-profile/v1"
slug = "goose"
edition = "cli"

[presence]
executables = ["goose"]
config-dir = "~/.config/goose"

[skills]
posture = "brokered"
shared-tree = "Claude-compatible Agent Skills (SKILL.md) via the built-in skills extension; this census revision brokers projection."

[guidance]
posture = "observed"
observe = [".goosehints"]

[models]
posture = "observed"
dispatch = "provider-plural"
roster-note = "15+ providers (Anthropic, OpenAI, Google, Ollama, OpenRouter, ...) selected through config (goose census)."

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume"]
"#,
    ),
    (
        "opencode",
        // Evidence: clients/opencode.rs admission census (opencode.ai docs:
        // global ~/.config/opencode, opencode.json, skills/ config
        // subdirectory, AGENTS.md rules, plugins, MCP local+remote) plus the
        // live Omarchy machine 2026-09-18 (opencode 1.18.30; opencode.json
        // present; the published config schema names the top-level `mcp`
        // collection). Installed here; the Actuation descriptor lives on the
        // Mac's catalog line only, so detection still cannot see it.
        r#"
schema = "aikit.harness-profile/v1"
slug = "opencode"
edition = "cli"

[presence]
executables = ["opencode"]
config-dir = "~/.config/opencode"

[skills]
posture = "brokered"
shared-tree = "skills/ is a documented config subdirectory (global and project scope); this census revision brokers projection (opencode census)."
observe = { paths = ["~/.config/opencode/skills"] }

[guidance]
posture = "observed"
observe = ["~/.config/opencode/AGENTS.md"]

[tools]
posture = "observed"
observe = [{ path = "~/.config/opencode/opencode.json", collection = "mcp" }]

[models]
posture = "observed"
dispatch = { none = { reason = "The catalog declares no capability document for opencode and the census records no provider binding surface; no model dispatch is declared." } }

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume", "attach"]
"#,
    ),
];

fn parsed() -> &'static BTreeMap<&'static str, HarnessProfile> {
    static CACHE: OnceLock<BTreeMap<&'static str, HarnessProfile>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut map = BTreeMap::new();
        for (slug, document) in EMBEDDED_PROFILES {
            let profile: HarnessProfile = toml::from_str(document)
                .unwrap_or_else(|e| panic!("embedded profile {slug} must parse: {e}"));
            profile
                .validate()
                .unwrap_or_else(|e| panic!("embedded profile {slug} must validate: {e}"));
            map.insert(*slug, profile);
        }
        map
    })
}

/// The validated profile for one catalog slug, if this package carries one.
pub fn for_slug(slug: &str) -> Option<&'static HarnessProfile> {
    parsed().get(slug)
}

/// Every embedded profile, keyed by catalog slug.
pub fn all() -> impl Iterator<Item = (&'static str, &'static HarnessProfile)> {
    parsed().iter().map(|(slug, profile)| (*slug, profile))
}

/// The catalog slug a client TargetId resolves to, mirroring the registry's
/// `catalog_slug` joins. Absent entries have no catalog-side record.
pub fn slug_for_target(target: &TargetId) -> Option<&'static str> {
    match target.as_str() {
        TargetId::CLAUDE_CODE => Some("claude-code"),
        TargetId::CODEX => Some("codex"),
        TargetId::ZCODE => Some("zcode"),
        TargetId::PI => Some("pi"),
        TargetId::GEMINI => Some("gemini"),
        TargetId::GEMINI_CLI => Some("gemini"),
        TargetId::KIMI => Some("kimi"),
        TargetId::OPENCLAW => Some("openclaw"),
        TargetId::CURSOR_CLI => Some("cursor-cli"),
        TargetId::QWEN_CODE => Some("qwen-code"),
        TargetId::OLLAMA => Some("ollama"),
        TargetId::AIDER => Some("aider"),
        TargetId::DEEPSEEK_HARNESS => Some("deepseek-harness"),
        TargetId::GOOSE => Some("goose"),
        TargetId::GROK_BOT => Some("grok-bot"),
        TargetId::ANTIGRAVITY => Some("gemini-antigravity"),
        TargetId::OPENCODE => Some("opencode"),
        TargetId::HERMES => Some("hermes"),
        TargetId::HERMES_ACP => Some("hermes-acp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::harness_profile::{TrustSettingScope, TrustValueKind};

    #[test]
    fn every_embedded_profile_parses_validates_and_resolves_by_slug() {
        let all: Vec<_> = all().collect();
        assert!(
            all.len() >= 18,
            "the roster carries profiles: {}",
            all.len()
        );
        for (slug, profile) in all {
            assert_eq!(profile.slug, *slug, "slug must match its key");
            assert_eq!(profile.schema, "aikit.harness-profile/v1");
        }
        for slug in [
            "claude-code",
            "codex",
            "zcode",
            "pi",
            "openclaw",
            "hermes",
            "hermes-acp",
            "grok-bot",
            "gemini-antigravity",
            "aider",
            "deepseek-harness",
            "goose",
            "opencode",
        ] {
            assert!(for_slug(slug).is_some(), "{slug} must resolve");
        }
    }

    #[test]
    fn managed_layers_declare_projects_and_activation_truth() {
        let openclaw = for_slug("openclaw").unwrap();
        let tools = openclaw.tools.as_ref().unwrap();
        assert_eq!(
            tools.posture,
            aikit_core::harness_profile::LayerPosture::Managed
        );
        let project = tools.project.as_ref().unwrap();
        assert_eq!(project.file, "~/.openclaw/mcp.json");
        assert_eq!(project.key, "mcpServers");
        assert_eq!(
            tools.activation,
            Some(aikit_core::harness_profile::ActivationEffectName::RestartClient)
        );

        let claude = for_slug("claude-code").unwrap();
        let hooks = claude.hooks.as_ref().unwrap();
        assert_eq!(
            hooks.project.as_ref().unwrap().ownership_identity,
            "aikit hook dispatch claude"
        );
    }

    #[test]
    fn brokered_and_observed_layers_carry_no_project_declarations() {
        for (slug, profile) in all() {
            if let Some(tools) = &profile.tools {
                let managed = tools.posture == aikit_core::harness_profile::LayerPosture::Managed;
                assert_eq!(
                    tools.project.is_some(),
                    managed,
                    "{slug}: only managed tools layers may declare a project"
                );
            }
            if let Some(hooks) = &profile.hooks {
                let managed = hooks.posture == aikit_core::harness_profile::LayerPosture::Managed;
                assert_eq!(
                    hooks.project.is_some(),
                    managed,
                    "{slug}: only managed hooks layers may declare a project"
                );
            }
        }
    }

    #[test]
    fn model_dispatch_none_always_carries_its_reason() {
        for (slug, profile) in all() {
            if let Some(models) = &profile.models {
                if let aikit_core::harness_profile::ModelDispatchPosture::None { reason } =
                    &models.dispatch
                {
                    assert!(!reason.trim().is_empty(), "{slug}: none without a reason");
                }
            }
        }
    }

    #[test]
    fn pi_sessions_declare_the_refusal_boundary_as_data() {
        let pi = for_slug("pi").unwrap();
        let sessions = pi.sessions.as_ref().unwrap();
        assert_eq!(
            sessions.protocol,
            aikit_core::harness_profile::SessionProtocol::Rpc
        );
        assert_eq!(sessions.open_modes, vec!["attach".to_owned()]);
        assert!(!sessions.capabilities.mcp_servers);
        assert!(!sessions.capabilities.additional_directories);
        assert!(!sessions.capabilities.reconnect);
    }

    #[test]
    fn trust_declaring_profiles_declare_their_trust_settings_as_data() {
        // The general harness-trust pattern: each declaring profile carries
        // its trust/permissions declarations in its own settings layer, and
        // every declaration is plane-ready (key grammar, scopes, enum
        // options) so the configuration plane can surface it unchanged.
        let expected: &[(
            &str,
            &[(&str, aikit_core::harness_profile::TrustSettingScope)],
        )] = &[
            (
                "claude-code",
                &[("hooks.fs-guardrail", TrustSettingScope::Machine)],
            ),
            (
                "codex",
                &[
                    ("projects.trust_level", TrustSettingScope::Project),
                    ("home.trust_level", TrustSettingScope::Machine),
                ],
            ),
            (
                "zcode",
                &[("hooks.fs-guardrail", TrustSettingScope::Machine)],
            ),
        ];
        for (slug, keys) in expected {
            let profile = for_slug(slug).expect("embedded profile");
            let layer = profile
                .settings
                .as_ref()
                .unwrap_or_else(|| panic!("{slug} declares trust settings but no settings layer"));
            assert_eq!(
                layer.posture,
                aikit_core::harness_profile::LayerPosture::Observed,
                "trust declarations are disclosure-only: {slug}"
            );
            let declared: Vec<_> = layer
                .trust_settings
                .iter()
                .map(|t| (t.key.as_str(), t.scopes[0]))
                .collect();
            assert_eq!(&declared[..], *keys, "declared keys/scopes for {slug}");
            for declaration in &layer.trust_settings {
                assert!(
                    !declaration.config.is_empty(),
                    "{slug} {} names its config",
                    declaration.key
                );
                if declaration.value_schema.kind == TrustValueKind::Enum {
                    assert!(
                        !declaration.value_schema.options.is_empty(),
                        "{slug} {} enum carries options",
                        declaration.key
                    );
                }
            }
        }
        // The posture truth cuts both ways: profiles that declare no trust
        // surface carry no trust declarations.
        for (slug, profile) in all() {
            if slug == "claude-code" || slug == "codex" || slug == "zcode" {
                continue;
            }
            let declared = profile
                .settings
                .as_ref()
                .map(|layer| layer.trust_settings.len())
                .unwrap_or(0);
            assert_eq!(declared, 0, "{slug} declares no trust settings yet");
        }
    }

    #[test]
    fn target_ids_join_to_their_catalog_slugs() {
        assert_eq!(
            slug_for_target(&TargetId::claude_code()),
            Some("claude-code")
        );
        assert_eq!(slug_for_target(&TargetId::openclaw()), Some("openclaw"));
        assert_eq!(slug_for_target(&TargetId::qwen_code()), Some("qwen-code"));
        assert_eq!(slug_for_target(&TargetId::gemini_cli()), Some("gemini"));
        // The catalog slug itself resolves through the same table (the
        // registry's join key for the gemini client is TargetId::GEMINI).
        assert_eq!(slug_for_target(&TargetId::gemini()), Some("gemini"));
        for (target, slug) in [
            (TargetId::aider(), "aider"),
            (TargetId::deepseek_harness(), "deepseek-harness"),
            (TargetId::goose(), "goose"),
            (TargetId::grok_bot(), "grok-bot"),
            (TargetId::antigravity(), "gemini-antigravity"),
            (TargetId::opencode(), "opencode"),
        ] {
            assert_eq!(slug_for_target(&target), Some(slug));
        }
        assert!(slug_for_target(&TargetId::shell()).is_none());
        for target in [
            TargetId::claude_code(),
            TargetId::codex(),
            TargetId::zcode(),
            TargetId::pi(),
        ] {
            let slug = slug_for_target(&target).expect("registry join");
            assert!(
                for_slug(slug).is_some(),
                "{slug}: target must have a profile"
            );
        }
    }

    #[test]
    fn every_registered_harness_target_resolves_to_a_profile() {
        // The registry roster and the embedded profiles must cover the same
        // harnesses: every non-broker registry target resolves to a profile.
        // The gemini client joins by catalog slug (TargetId::GEMINI), the
        // rest by their own target id.
        let targets = [
            TargetId::claude_code(),
            TargetId::codex(),
            TargetId::zcode(),
            TargetId::aider(),
            TargetId::antigravity(),
            TargetId::cursor_cli(),
            TargetId::deepseek_harness(),
            TargetId::gemini(),
            TargetId::goose(),
            TargetId::grok_bot(),
            TargetId::kimi(),
            TargetId::opencode(),
            TargetId::openclaw(),
            TargetId::pi(),
            TargetId::qwen_code(),
            TargetId::ollama(),
            TargetId::hermes(),
            TargetId::hermes_acp(),
        ];
        for target in targets {
            let slug = slug_for_target(&target)
                .unwrap_or_else(|| panic!("{target}: no catalog slug join"));
            assert!(
                for_slug(slug).is_some(),
                "{target} joins to profile `{slug}` but no embedded profile carries it"
            );
        }
    }
}
