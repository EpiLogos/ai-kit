//! The harness profile instances: one `aikit.harness-profile/v1` document per
//! supported harness, embedded as TOML and parsed+validated once. The
//! documents carry only what the per-harness censuses evidenced (2026-09-16,
//! pi hooks added 2026-09-18) — an absent layer says nothing, a `none` model
//! dispatch carries its reason, and machine-specific paths are home-relative
//! so the data survives machines. Actuation's catalog stays the detection
//! authority; these documents are AIKit's handling declarations joined to it
//! by slug.
//!
//! ## Pi hooks (2026-09-18 census)
//!
//! Pi 0.84.4 carries hooks as TypeScript extension events, not shell commands
//! in a settings map: an extension module subscribes with
//! `pi.on("session_start", ...)` (pi's `packages/coding-agent/docs/extensions.md`),
//! and extensions are declared through the `extensions` array of
//! `~/.pi/agent/settings.json` or discovered in `~/.pi/agent/extensions/`
//! (global) and trust-gated `.pi/extensions/` (project-local). The managed
//! hook grammars (claude-hook-map, zcode-hook-wrapper) project
//! `aikit hook dispatch <client> <event>` shell-command entries into native
//! config; pi has no such seam. The pi profile therefore declares its hooks
//! layer `observed` — the event census feeds disclosure, and no projection is
//! claimed. A managed pi hooks layer would need a new carrier vehicle (an
//! owned TS extension file that spawns the dispatcher from each mapped
//! handler), deliberately not built in this pass; the dispatcher side
//! (`aikit hook dispatch pi <Event>`) is already client-agnostic and works.

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
"#,
    ),
    (
        // Hooks census 2026-09-18 (pi 0.84.4, docs/extensions.md): hooks are
        // TypeScript extension events — `session_start`, `input` (user prompt,
        // can transform), `tool_call` (pre-tool, can block), `tool_result`,
        // `session_shutdown`, `session_before_compact` — declared via the
        // settings `extensions` array or discovery dirs. No shell-command
        // seam exists for the managed hook grammars, so this layer stays
        // observed; `stop` and `notification` are omitted (no pi equivalent).
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

[hooks]
posture = "observed"
observe = [{ events = ["session-start", "user-prompt-submit", "pre-tool-use", "post-tool-use", "session-end", "pre-compact"], transports = ["extension-events"] }]

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
        TargetId::GEMINI_CLI => Some("gemini"),
        TargetId::KIMI => Some("kimi"),
        TargetId::OPENCLAW => Some("openclaw"),
        TargetId::CURSOR_CLI => Some("cursor-cli"),
        TargetId::QWEN_CODE => Some("qwen-code"),
        TargetId::OLLAMA => Some("ollama"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_profile_parses_validates_and_resolves_by_slug() {
        let all: Vec<_> = all().collect();
        assert!(
            all.len() >= 10,
            "the roster carries profiles: {}",
            all.len()
        );
        for (slug, profile) in all {
            assert_eq!(profile.slug, *slug, "slug must match its key");
            assert_eq!(profile.schema, "aikit.harness-profile/v1");
        }
        for slug in ["claude-code", "codex", "zcode", "pi", "openclaw"] {
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
    fn pi_hooks_declare_the_extension_event_census_without_a_projection_claim() {
        let pi = for_slug("pi").unwrap();
        let hooks = pi
            .hooks
            .as_ref()
            .expect("the 2026-09-18 census gives pi a hooks layer");
        assert_eq!(
            hooks.posture,
            aikit_core::harness_profile::LayerPosture::Observed,
            "pi has no shell-command hook seam, so the layer must not claim writes"
        );
        assert!(
            hooks.project.is_none(),
            "an observed hooks layer refuses a project declaration"
        );
        let declaration = hooks.observe.first().expect("one event census entry");
        assert_eq!(
            declaration.events,
            vec![
                "session-start".to_string(),
                "user-prompt-submit".to_string(),
                "pre-tool-use".to_string(),
                "post-tool-use".to_string(),
                "session-end".to_string(),
                "pre-compact".to_string(),
            ],
            "the six pi events the census proved, in AIKit kind spelling"
        );
        assert_eq!(
            declaration.transports.as_deref(),
            Some(["extension-events".to_string()].as_slice()),
            "pi's transport is extension events, not a settings hook map"
        );
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
}
