//! The harness profile instances: one `aikit.harness-profile/v1` document per
//! supported harness, embedded as TOML and parsed+validated once. The
//! documents carry only what the 2026-09-16 per-harness census evidenced —
//! an absent layer says nothing, a `none` model dispatch carries its reason,
//! and machine-specific paths are home-relative so the data survives
//! machines. Actuation's catalog stays the detection authority; these
//! documents are AIKit's handling declarations joined to it by slug.

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
project = { file = ".codex/hooks.json", format = "claude-hook-map", ownership-identity = "aikit hook dispatch codex" }

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
posture = "observed"
observe = { paths = ["~/.agents/skills", "~/.zcode/cli/plugins"] }
shared-tree = "zcode loads skills natively from the codex-managed ~/.agents/skills shared tree and from plugin-shipped skills; codex's managed projection is zcode's delivery, and AIKit projects no separate zcode skill seam."

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
    fn zcode_skills_declare_the_native_tree_they_actually_load() {
        // The 2026-09-18 machine reading: zcode loads skills natively from the
        // codex-managed `~/.agents/skills` shared tree and plugin-shipped
        // skills (the admission's NativeSkills evidence), so the honest
        // posture is `observed` — AIKit writes no zcode skill seam — and the
        // delivery relation is disclosed rather than denied.
        let zcode = for_slug("zcode").unwrap();
        let skills = zcode.skills.as_ref().expect("zcode declares skills");
        assert_eq!(
            skills.posture,
            aikit_core::harness_profile::LayerPosture::Observed,
            "zcode demonstrably loads a native skill tree, so neither brokered \
             (\"no native skill tree\") nor managed (AIKit writes nothing here) is true"
        );
        let observe = skills
            .observe
            .as_ref()
            .expect("the observed trees are named");
        assert!(observe.paths.iter().any(|path| path == "~/.agents/skills"));
        let shared_tree = skills
            .shared_tree
            .as_deref()
            .expect("the delivery relation is disclosed");
        assert!(
            shared_tree.contains("codex-managed ~/.agents/skills"),
            "the disclosure must name codex's projection as zcode's delivery: {shared_tree}"
        );
        assert!(
            !shared_tree.contains("never projected"),
            "status may not say \"never projected\" while sessions load the tree: \
             {shared_tree}"
        );
    }

    #[test]
    fn codex_hooks_name_the_project_relative_seam_the_descriptor_declares() {
        // Codex reads per-project `.codex/hooks.json` (its own config carries
        // `[hooks.state."<project>/.codex/hooks.json:..."]` entries), so a
        // home-level seam in the profile would declare a file the harness
        // never reads. The relative path is what makes the seam belong to the
        // working tree — and what `aikit apply` keeps current.
        let codex = for_slug("codex").unwrap();
        let hooks = codex.hooks.as_ref().expect("codex declares hooks");
        assert_eq!(
            hooks.posture,
            aikit_core::harness_profile::LayerPosture::Managed
        );
        let project = hooks.project.as_ref().expect("managed names its seam");
        assert_eq!(project.file, ".codex/hooks.json");
        assert!(
            !project.file.starts_with('~') && !std::path::Path::new(&project.file).is_absolute(),
            "the codex hook seam is project-relative, never machine-level"
        );
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
