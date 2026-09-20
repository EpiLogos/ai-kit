//! The harness profile instances: one `aikit.harness-profile/v1` document per
//! supported harness, embedded as TOML and parsed+validated once. The
//! documents carry only what the per-harness censuses evidenced — the
//! 2026-09-16 census for the first ten documents, the 2026-09-18
//! harness-adapter sort-out for the additions, the 2026-09-18 pi hooks
//! census, and the 2026-09-19 key-delivery census (each grounded in that
//! harness's admission census in this package and, where installed, the live
//! Omarchy machine) — an absent layer says nothing, a `none` model dispatch
//! carries its reason, and machine-specific paths are home-relative so the
//! data survives machines. Actuation's catalog stays the detection authority;
//! these documents are AIKit's handling declarations joined to it by slug.
//!
//! ## Key delivery (2026-09-19 census)
//!
//! The models layer's `key-delivery` records, per provider a harness can
//! serve, the env var its native launch reads for that provider's key — or
//! the own-login fact that it authenticates through a store of its own. The
//! launch path materialises a *bound* credential through the same seam the
//! selected-model path uses and injects it under the declared variable into
//! the scrubbed final-child environment. Where a provider is declared
//! env-var-only (no own-login fact), an unbound key refuses the launch with
//! the bind remediation rather than silently starting a body that cannot
//! authenticate; where an own-login fact exists, an unbound key is absent and
//! the harness's native login stands — availability disclosure already
//! reports it. Coverage honesty beats coverage theater: zcode, opencode,
//! openclaw, cursor-cli and ollama record why no env-var key path is declared
//! for them, and no variable was invented to fill the table.
//!
//! ## Pi hooks (2026-09-18 census; carrier commissioned same day)
//!
//! Pi 0.84.4 carries hooks as TypeScript extension events, not shell commands
//! in a settings map: an extension module subscribes with
//! `pi.on("session_start", ...)` (pi's `packages/coding-agent/docs/extensions.md`),
//! and extensions are declared through the `extensions` array of
//! `~/.pi/agent/settings.json` or discovered in `~/.pi/agent/extensions/`
//! (global) and trust-gated `.pi/extensions/` (project-local). The managed
//! hook grammars (claude-hook-map, zcode-hook-wrapper) project
//! `aikit hook dispatch <client> <event>` shell-command entries into native
//! config; pi has no such seam. The managed seam pi does have is the
//! `extensions` array, so the pi hooks layer projects through it — one
//! first-party extension carrier (`hook/aikit/pi-extension-carrier`)
//! whose handlers spawn the dispatcher per event and forward its verdicts
//! back into pi. The carrier is content-addressed at projection time, the
//! single owned entry in the array, swept and replaced by the
//! `pi-extensions-record` grammar; individual hook capsules are never
//! projected into pi — they ride the dispatcher chains the carrier feeds.
//! The carrier is a `hook` capsule, so a new revision is Unseen until
//! `aikit trust record` reviews it: an untrusted revision is never
//! projected and a swept one is removed whole — pi never loads a revision
//! the owner has not reviewed.

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

[models.key-delivery]
env-var = [{ provider-ref = "provider:anthropic", env-var = "ANTHROPIC_API_KEY" }]
own-login = [{ provider-ref = "provider:anthropic", note = "claude login keeps OAuth material in its own credential store (~/.claude/.credentials.json); the API key delivers only when credential:anthropic is bound" }]

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
project = { file = ".codex/hooks.json", format = "claude-hook-map", ownership-identity = "aikit hook dispatch codex" }

[tools]
posture = "observed"
observe = [{ path = "~/.codex/config.toml", collection = "mcp_servers" }]

[models]
posture = "observed"
dispatch = { native-provider-binding = { provider-ref = "provider:openai", selector-kind = "config-key", selector-name = "model" } }

[models.key-delivery]
env-var = [{ provider-ref = "provider:openai", env-var = "OPENAI_API_KEY" }]
own-login = [{ provider-ref = "provider:openai", note = "codex login writes its own auth store (~/.codex/auth.json); the API key delivers only when credential:openai is bound" }]

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

[models.key-delivery]
note = "zcode authenticates through its own managed login; the 2026-09-19 machine reading found no key or auth entries in its CLI config and the catalog records no provider binding, so no env-var key path is declared."

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
        // Hooks census 2026-09-18 (pi 0.84.4, docs/extensions.md): hooks are
        // TypeScript extension events — `session_start`, `input` (user prompt,
        // can transform), `tool_call` (pre-tool, can block), `tool_result`,
        // `session_shutdown`, `session_before_compact` — declared via the
        // settings `extensions` array or discovery dirs. No shell-command
        // seam exists for the managed hook grammars, so the carrier vehicle
        // (hook/aikit/pi-extension-carrier) projects through the `extensions`
        // array: one content-addressed, trust-gated extension whose handlers
        // spawn `aikit hook dispatch pi <Event>`. `stop` and `notification`
        // are omitted (no pi equivalent).
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
posture = "managed"
observe = [{ events = ["session-start", "user-prompt-submit", "pre-tool-use", "post-tool-use", "session-end", "pre-compact"], transports = ["extension-events"] }]
project = { file = "~/.pi/agent/settings.json", format = "pi-extensions-record", ownership-identity = "aikit-hook-carrier" }
activation = "next-session-only"

[tools]
posture = "brokered"

[models]
posture = "observed"
dispatch = "provider-plural"
roster-note = "Provider and model chosen per invocation; encounter model policy pins the native selector."
argv-selectors = { provider = "--provider", model = "--model" }

[models.key-delivery]
note = "pi keeps provider keys in its own auth store (~/.pi/agent/auth.json); a selected-model dispatch delivers its policy-named credential explicitly, so no blanket env-var key path is declared here."

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

[models.key-delivery]
env-var = [{ provider-ref = "provider:gemini", env-var = "GEMINI_API_KEY" }]
own-login = [{ provider-ref = "provider:gemini", note = "Login with Google (OAuth) is Gemini CLI's default auth; the API key delivers only when credential:gemini is bound" }]

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

[models.key-delivery]
env-var = [{ provider-ref = "provider:moonshot", env-var = "MOONSHOT_API_KEY" }]

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

[models.key-delivery]
note = "openclaw keeps model auth in its own config auth profiles (~/.openclaw/openclaw.json); no env-var key path is declared."

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

[models.key-delivery]
note = "cursor-agent authenticates through Cursor's own subscription login; no env-var key path is declared."

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

[models.key-delivery]
env-var = [{ provider-ref = "provider:dashscope", env-var = "DASHSCOPE_API_KEY" }]

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

[models.key-delivery]
note = "local model serving reads no provider key; there is nothing to deliver."

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

[models.key-delivery]
note = "opencode authenticates through its own per-provider auth store (opencode auth login) and per-provider config; no fixed env-var key path is declared for its native launch."

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

/// The profile whose presence executables name this launch program. The join
/// is the basename of the program AIKit was configured to spawn (`claude`,
/// `pi`, `opencode`) against each profile's declared `presence.executables`.
/// A bridge or wrapper program (a node launcher, a shell) joins nothing: an
/// encounter that does not name the harness executable itself gets no
/// key-delivery declarations, which is the honest absence.
pub fn for_argv_program(program: &str) -> Option<&'static HarnessProfile> {
    let name = std::path::Path::new(program).file_name()?.to_string_lossy();
    parsed().values().find(|profile| {
        profile
            .presence
            .as_ref()
            .is_some_and(|presence| presence.executables.iter().any(|e| *e == name))
    })
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
    fn pi_hooks_declare_the_extension_event_census_and_the_carrier_seam() {
        let pi = for_slug("pi").unwrap();
        let hooks = pi
            .hooks
            .as_ref()
            .expect("the 2026-09-18 census gives pi a hooks layer");
        assert_eq!(
            hooks.posture,
            aikit_core::harness_profile::LayerPosture::Managed,
            "the extension carrier is a real seam: pi's settings `extensions` array"
        );
        let project = hooks
            .project
            .as_ref()
            .expect("a managed hooks layer names the seam it projects into");
        assert_eq!(project.file, "~/.pi/agent/settings.json");
        assert_eq!(
            project.format,
            aikit_core::harness_profile::MergeGrammar::PiExtensionsRecord,
            "the carrier registers through the extensions array, not a hook map"
        );
        assert_eq!(
            project.ownership_identity,
            crate::hook_sources::HOOKS_PROJECTION_OWNERSHIP,
            "the sweep identity is the carrier file-name marker"
        );
        assert_eq!(
            hooks.activation,
            Some(aikit_core::harness_profile::ActivationEffectName::NextSessionOnly),
            "pi reads extensions at session start; a running TUI can /reload"
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
    fn every_declared_key_delivery_variable_is_lawful_and_single_per_provider() {
        // Validation runs at embedded-parse time (a bad declaration panics in
        // parsed()); this test additionally pins the whole declared table so
        // the delivery surface is legible in one place.
        let expected: &[(&str, &str, &str)] = &[
            ("claude-code", "provider:anthropic", "ANTHROPIC_API_KEY"),
            ("codex", "provider:openai", "OPENAI_API_KEY"),
            ("gemini", "provider:gemini", "GEMINI_API_KEY"),
            ("kimi", "provider:moonshot", "MOONSHOT_API_KEY"),
            ("qwen-code", "provider:dashscope", "DASHSCOPE_API_KEY"),
        ];
        for (slug, provider_ref, env_var) in expected {
            let profile = for_slug(slug).unwrap_or_else(|| panic!("{slug} must resolve"));
            let delivery = profile
                .models
                .as_ref()
                .unwrap_or_else(|| panic!("{slug} must declare models"))
                .key_delivery
                .as_ref()
                .unwrap_or_else(|| panic!("{slug} must declare key delivery"));
            assert_eq!(
                delivery.env_var.len(),
                1,
                "{slug}: exactly one declared delivery variable"
            );
            assert_eq!(delivery.env_var[0].provider_ref, *provider_ref);
            assert_eq!(delivery.env_var[0].env_var, *env_var);
        }
    }

    #[test]
    fn env_var_only_declarations_name_their_own_login_fallbacks_where_they_exist() {
        // claude, codex and gemini can serve their provider through their own
        // login store, so an unbound key must not refuse their launch. kimi
        // and qwen-code have no evidenced own-login store: their declared key
        // is required, and an unbound binding refuses the launch loudly.
        for slug in ["claude-code", "codex", "gemini"] {
            let profile = for_slug(slug).unwrap();
            let delivery = profile
                .models
                .as_ref()
                .unwrap()
                .key_delivery
                .as_ref()
                .unwrap();
            assert_eq!(
                delivery.own_login.len(),
                1,
                "{slug}: the own-login fallback must be declared"
            );
            assert_eq!(
                delivery.own_login[0].provider_ref, delivery.env_var[0].provider_ref,
                "{slug}: the fallback covers the declared provider"
            );
        }
        for slug in ["kimi", "qwen-code"] {
            let profile = for_slug(slug).unwrap();
            let delivery = profile
                .models
                .as_ref()
                .unwrap()
                .key_delivery
                .as_ref()
                .unwrap();
            assert!(
                delivery.own_login.is_empty(),
                "{slug}: no own-login store is evidenced, so none may be declared"
            );
        }
    }

    #[test]
    fn harnesses_without_an_env_var_key_path_declare_the_fact_instead() {
        // Coverage honesty beats coverage theater: no variable was invented
        // for these harnesses; their key posture is a named note.
        for slug in [
            "zcode",
            "pi",
            "openclaw",
            "cursor-cli",
            "ollama",
            "opencode",
        ] {
            let profile = for_slug(slug).unwrap_or_else(|| panic!("{slug} must resolve"));
            let delivery = profile
                .models
                .as_ref()
                .unwrap_or_else(|| panic!("{slug} must declare models"))
                .key_delivery
                .as_ref()
                .unwrap_or_else(|| panic!("{slug} must declare its no-env-path fact"));
            assert!(
                delivery.env_var.is_empty(),
                "{slug}: no env-var delivery may be invented"
            );
            let note = delivery
                .note
                .as_deref()
                .unwrap_or_else(|| panic!("{slug}: the no-env-path fact must be stated"));
            assert!(!note.trim().is_empty());
        }
    }

    #[test]
    fn pi_declares_no_env_var_delivery_keeping_its_policy_path_the_only_route() {
        // The selected-model policy names pi's credential and its target
        // variable explicitly; a profile-declared variable would silently
        // add a second delivery route to a path whose law is explicitness.
        let pi = for_slug("pi").unwrap();
        let delivery = pi.models.as_ref().unwrap().key_delivery.as_ref().unwrap();
        assert!(delivery.env_var.is_empty());
    }

    #[test]
    fn argv_selectors_are_declared_exactly_where_a_surface_was_observed() {
        // Only a harness whose own command surface was observed reading
        // per-invocation flags may declare them. Today that is pi (its
        // `--help` census and the encounter selected-model path both name
        // --provider/--model); every other provider-plural profile leaves the
        // field absent rather than invent a selector.
        for (slug, profile) in all() {
            let Some(models) = &profile.models else {
                continue;
            };
            assert_eq!(
                models.argv_selectors.is_some(),
                slug == "pi",
                "{slug}: argv selectors must be declared exactly where observed"
            );
        }
        let pi = for_slug("pi").unwrap();
        let selectors = pi
            .models
            .as_ref()
            .unwrap()
            .argv_selectors
            .as_ref()
            .expect("pi observed --provider/--model");
        assert_eq!(selectors.provider, "--provider");
        assert_eq!(selectors.model, "--model");
    }

    #[test]
    fn launch_programs_join_to_profiles_by_basename() {
        assert_eq!(
            for_argv_program("/Users/admin/.local/bin/pi").map(|p| p.slug.as_str()),
            Some("pi")
        );
        assert_eq!(
            for_argv_program("claude").map(|p| p.slug.as_str()),
            Some("claude-code")
        );
        assert_eq!(
            for_argv_program("opencode").map(|p| p.slug.as_str()),
            Some("opencode")
        );
        // A bridge or wrapper joins nothing: no declarations, no delivery.
        assert!(for_argv_program("/opt/homebrew/bin/node").is_none());
        assert!(for_argv_program("/bin/sh").is_none());
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
