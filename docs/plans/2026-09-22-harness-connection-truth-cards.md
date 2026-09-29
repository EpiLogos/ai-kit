# Harness connection truth cards — research pass 2026-09-22

Companion to `2026-09-22-harness-connection-truth-scope.md`. Four parallel
research passes (ACP protocol, MCP surfaces, headless/resume/auth, landscape
census), each claim tagged: **[VL]** verified-live (command run on this Mac),
**[VD]** verified-docs (URL fetched 2026-09-22), **[VR]** verified against the
official ACP registry JSON (cdn.agentclientprotocol.com/registry/v1/latest),
**[VS]** verified from upstream source, **[PO]** prose-only. This file is the
durable copy; scratch transcripts in /tmp are volatile. Feeds development
lanes L1–L6 in the scope doc.

## ACP spec truth (protocol version is still 1)

- `protocolVersion` remains **1**; SDK releases separately (latest
  `agent-client-protocol` 1.9.1, 2026-09-18). **V2 is an unstable draft**
  (announced 2026-07-20) — do not target it. [VD]
- Transport: client spawns the agent; JSON-RPC 2.0 over stdio, newline-
  delimited UTF-8, stdout exclusively protocol messages. aikit's framing in
  `connection_process.rs` is correct. [VD]
- Post-pin additions a version-1-pinned client misses: **`session/resume`**
  (stabilized 2026-04-23; like `session/load` but no history replay, gated by
  `sessionCapabilities.resume`), `session/close`, `session/list`,
  `session/delete`, **config options** (model / mode / model_config /
  thought_level categories via `session/set_config_option`; aikit already
  implements set_config_option for model + reasoning), boolean options,
  elicitation (SDK 1.7.0), `additionalDirectories`, terminal auth, `logout`.
  `clientInfo` is SHOULD today, required in a future version. [VD]
- Auth: top-level `authenticate {methodId}`; `authMethods` in initialize
  response; `session/new` may return `auth_required`. [VD]
- mcpServers wire shapes confirmed identical to aikit's: stdio
  `{name,command,args,env:[{name,value}]}` (no `type`), http
  `{type:"http",name,url,headers}` gated by `mcpCapabilities.http`. [VD]

## MCP spec truth (revision 2026-07-28)

- Transports: stdio and Streamable HTTP. The 2026-07-28 revision removed the
  GET stream, protocol-level sessions (`Mcp-Session-Id`) and
  `Last-Event-ID` resumability; most installed harnesses still speak the
  initialize-handshake era on the wire — detect by probing. [VD]
- Remote auth: OAuth 2.1 with RFC 9728 Protected Resource Metadata; stdio
  credentials come from the environment, never the spec. [VD]
- Practical consequence for aikit: per-harness config shapes below are what
  the tools layer writes; header/secret refs are needed for http entries
  (codex `bearer_token_env_var`, claude `headers`+`headersHelper`, goose
  `headers`+`client_secret_key`, etc.).

## Per-harness connection truth

### claude (claude 2.1.263)
- ACP: not native; adapter `npx @agentclientprotocol/claude-agent-acp@0.81.0`
  (Agent SDK; the old `@zed-industries/claude-code-acp` repo is gone) [VL+VR].
- MCP-client: `~/.claude.json` (user+local) and `.mcp.json` (project), key
  `mcpServers`; entry `{type: stdio|http|sse|ws, command, args, env, url,
  headers, headersHelper, timeout}`; `${VAR}` expansion; precedence
  local>project>user. `claude mcp` CLI. [VL+VD]
- MCP-server: `claude mcp serve`. [VL+VD]
- Headless: `claude -p --output-format stream-json --verbose
  --include-partial-messages`; NDJSON system/assistant/user/result.
- Resume: `-r/--resume <id>`, `-c`, `--fork-session`, `--session-id`;
  store `~/.claude/projects/<dashed-cwd>/<uuid>.jsonl`. Auth:
  `ANTHROPIC_API_KEY`, OAuth in `~/.claude.json`, `--bare` strict-key mode.

### codex (codex-cli 0.155.1)
- ACP: not native; adapter `npx @agentclientprotocol/codex-acp@1.13.0` (zed
  package archived at 0.16.0). `codex app-server` is its own protocol, not
  ACP. [VL+VR]
- MCP-client: `~/.codex/config.toml` `[mcp_servers.<name>]` with
  `command/args/env/cwd/enabled/startup_timeout_sec/tool_timeout_sec/
  enabled_tools/approval_mode`; remote via `url`, `bearer_token_env_var`,
  `http_headers`, `auth:"oauth"`. `codex mcp add`. Next-session pickup. [VL+VD]
- MCP-server: **none** (no serve subcommand in 0.155.1). [VL]
- Headless: `codex exec --json` (JSONL events), `--output-schema`,
  `-C <dir>`, sandbox modes. Resume: `codex exec resume <id|--last>`, fork;
  store `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Auth: ChatGPT OAuth,
  `OPENAI_API_KEY` via `codex login --with-api-key`, `~/.codex/auth.json` or
  keyring.

### gemini (gemini 0.29.5 installed — far behind registry 0.60.0)
- ACP: native first-party. Flag **renamed: `--acp` replaces the deprecated
  `--experimental-acp` in the 0.30+ line**; registry argv
  `npx @google/gemini-cli@0.60.0 --acp`. Installed 0.29.5 still takes
  `--experimental-acp`. Known tool_call/permission ordering bug fixed only in
  0.62.0-nightly. Probe `--acp` then fall back. [VL+VD+VR]
- MCP-client: `~/.gemini/settings.json` / `.gemini/settings.json`, key
  `mcpServers`; entry needs exactly one of `command` | `url` (SSE) |
  `httpUrl` (streamable HTTP); optional `headers`, `timeout`, `trust`,
  `includeTools/excludeTools`, `oauth`. `gemini mcp add`. [VL+VD]
- MCP-server: none. Headless: `gemini -p --output-format stream-json`; exit
  codes 0/1/42/53. Resume: `-r latest|<idx>`, `--list-sessions`; store
  `~/.gemini/tmp/<hash>/chats/`. Auth: `GEMINI_API_KEY`, Google OAuth,
  Vertex/ADC envs.

### zcode (no ACP, no headless face documented)
- ACP: **none** — absent from ACP registry and agents page. [VR+VD]
- MCP-client: `~/.zcode/cli/config.json` key `mcp.servers`; workspace
  `.zcode/config.json` / `zcode.json`; fallbacks `~/.agents/mcp.json`;
  stdio `{command,args,cwd,env,enabled,timeoutMs}`, remote `{url,headers}`;
  schema strict (unknown key silently drops the server). [VL skill doc]
- MCP-server: none. Headless: none documented — TUI + `zcode --web` only.
  Auth: Z.ai login; `ZCODE_SERVER_AUTH_TOKEN`. → projection + MCP-config
  managed only; no process/ACP connection until a print/ACP face ships.

### pi (pi 0.84.4)
- ACP: none native; community adapter `npx pi-acp@0.0.33`. [VL+VR]
- MCP-client: **no native MCP client** (author's stated position; verified
  absence). aikit's bridge at Work/epi/bimba-portable/pi-mcp-bridge remains
  the path. [VL]
- Connection: `--mode rpc` JSONL (aikit `pi_rpc_connection.rs`, verified);
  headless `pi -p --mode json`. Resume: `--continue/--resume/--session/
  --session-id/--fork`; store `~/.pi/agent/sessions/<encoded-cwd>/`. Auth:
  per-provider envs + `~/.pi/agent/auth.json`; `pi auth print-api-key`.
  Known 0.84.4 gap: `-p` print mode emits no shutdown events.

### opencode (opencode 1.18.30)
- ACP: native `opencode acp` (accepts `--cwd`, `--print-logs`). [VL+VR]
- MCP-client: `~/.config/opencode/opencode.json` / project `opencode.json`,
  key `mcp`; local `{type:"local", command:[ARRAY], environment, enabled}`,
  remote `{type:"remote", url, headers, enabled}` — **command is an array**,
  unlike most harnesses. `opencode mcp add`. [VL+VD]
- MCP-server: none (`opencode serve` is its own HTTP API). Headless:
  `opencode run --format json`; `--attach <serve-url>` reuses a warm server.
  Resume: `-c/-s <id>/--fork`; sessions in `~/.local/share/opencode/`
  SQLite db. Auth: `opencode auth login`, `~/.local/share/opencode/auth.json`.

### goose (docs/source-only; registry 1.51.0)
- ACP: native `goose acp`; dual-role (also consumes ACP providers). [VR+VD]
- MCP-client: `config.yaml` (XDG/macOS paths), key **`extensions`** — variants
  `stdio {cmd, args, envs, env_keys, timeout, cwd, bundled}`,
  `streamable_http {uri, headers, client_id, client_secret_key, scopes}`;
  SSE variant removed from main. Malformed entries skipped with warning. [VS]
- MCP-server: `goose mcp <server>` (bundled servers). Headless:
  `goose run -t --output-format json|stream-json`; resume `goose run -n <name>
  -r`; store `~/.local/share/goose/sessions/sessions.db` (SQLite). Auth:
  `GOOSE_PROVIDER__API_KEY` envs, keyring or `secrets.yaml` fallback.

### qwen (docs-only; registry 0.24.3)
- ACP: native, registry argv `npx @qwen-code/qwen-code@0.24.3 --acp
  --experimental-skills` (gemini fork; recent versions may run ACP under a
  `qwen serve` parent). [VR+VD]
- MCP-client: `~/.qwen/settings.json` / `.qwen/settings.json`, key
  `mcpServers`; same shape as gemini (`command` | `url` | `httpUrl`,
  `headers`, `trust`). [VD]
- Headless: `qwen -p --output-format stream-json`. Resume: `--continue/
  --resume`. Auth: `OPENAI_API_KEY`+`OPENAI_BASE_URL`,
  `BAILIAN_CODING_PLAN_API_KEY`, `DASHSCOPE_API_KEY`; free OAuth tier
  discontinued 2026-04-15.

### kimi (IDENTITY HAZARD — resolve before its lane)
- Three passes disagree on what `kimi` is here: MoonshotAI/kimi-cli (docs:
  `kimi acp`, `~/.kimi/mcp.json`, `kimi mcp add`, kimi-cli 1.6) [VD]; a local
  Python clone exposing the Claude Code contract (`kimi --help` byte-identical
  to claude, reports 2.1.263, `kimi -p --output-format stream-json`, store
  `~/.kimi/`) [VL]; and a shell function wrapping `claude` [VL]. Pin the
  package and binary path first; do not derive launch facts from PATH name.
- If Moonshot kimi-cli: ACP `kimi acp`; MCP `~/.kimi/mcp.json` top-level
  `mcpServers`, `kimi mcp add -t stdio|http`, OAuth `kimi mcp auth`; no
  MCP-server mode.

### hermes (hermes 0.21.1)
- ACP: native `hermes acp` (--check/--setup/--accept-hooks). [VL+VD]
- MCP-client: `~/.hermes/config.yaml` (YAML) top-level `mcp_servers:` mapping
  `{command, args, env}` + `mcp_discovery_timeout`; `hermes mcp
  add/list/test/configure`. [VL]
- MCP-server: `hermes mcp serve` (conversations exposed to other agents). [VL]
- Headless: `hermes -z "<prompt>"` — plain text only, no structured output;
  `--usage-file` side JSON. Resume: `-r <id|title|latest>`, `-c`; store
  `~/.hermes/sessions`. Auth: pooled `~/.hermes/auth.json`, `hermes auth
  add/priority`; `hermes proxy` = OpenAI-compatible local proxy. Note:
  `--accept-hooks` auto-approves shell hooks — security-relevant at launch.

### aider (docs-only)
- ACP: none. MCP: none first-party (FAQ confirms). Headless:
  `aider --message --yes-always` — plain text only. No session ids;
  `--restore-chat-history`, `--load`. Auth: provider env keys/.env.
  → process family, text-only; tools layer stays Unsupported.

### cursor (cursor-agent; docs-only, registry 2026.09.18)
- ACP: native `cursor-agent acp` (registry binary dist). [VR]
- MCP-client: `.cursor/mcp.json` (project) / `~/.cursor/mcp.json` (global),
  key `mcpServers`; stdio `{type:"stdio",command,args,env,envFile}`, remote
  `{url,headers}`; `${env:NAME}` interpolation. [VD]
- Headless: `cursor-agent -p --output-format json|stream-json`
  (`--stream-partial-output` for deltas). Resume: `--resume <chat-id>`,
  `--continue`, `agent ls/resume`. Auth: `agent login` OAuth or
  `CURSOR_API_KEY`.

### openclaw (openclaw 2026.1.30)
- ACP: `openclaw acp` exists but is an **ACP bridge backed by the Gateway**
  (routes gateway sessions over WebSocket to an ACP stdio frontend) — not a
  plain spawn-an-agent face; connection lifetime tied to the Gateway. [VL+VD]
- MCP-client: `~/.openclaw/mcp.json` top-level `mcpServers`
  `{command,args,env}` (verified live); newer docs add managed `mcp.servers`,
  `config/mcporter.json`, `openclaw mcp` verbs — large installed-vs-docs
  drift; pin per version. [VL+VD]
- Headless: `openclaw agent exec --message --json` (stable documented
  envelope: ok/status/final/payloads/usage/costUsd/model/sessionId);
  gateway-backed `openclaw agent --session-id`. Sessions keyed
  `agent:<id>:<key>` in gateway state. Auth: config JSON5, SecretRef
  `secretref-env:VAR` markers, `--auth-env-only`.

### grokbot (ROSTER CORRECTION — not a coding harness)
- `grok-bot` 0.2.2 CLI manages bots/groups only (no coding surface, no MCP,
  no headless) [VL]. xAI's real coding CLI is **Grok Build** (`grok`):
  `grok -p --output-format streaming-json`, `~/.grok/config.toml`, ACP
  supported, `XAI_API_KEY` [VD]. Propose: retire/repurpose the grokbot
  adapter as `grok` (Grok Build) with fresh cards; headless JSON schema not
  yet documented upstream.

### ollama (ollama 0.12.6) — model server, not a child process
- Connection family is HTTP: `ollama serve` on `http://127.0.0.1:11434`,
  `POST /api/chat` (NDJSON streaming) + OpenAI-compatible `/v1` +
  Anthropic-compatible `/v1/messages`. No sessions (caller owns messages); no
  MCP; cloud via `OLLAMA_API_KEY` bearer. [VL+VD] → models layer only.

### antigravity (CORRECTION — no longer IDE-only)
- Headless CLI exists: **`agy -p --output-format json|stream-json`** with
  `--json-schema`, `--continue/--conversation <id>`, `--effort`,
  exit codes 0/1/2, documented NDJSON events (init/step_update/result,
  `tool_info`, `subagent_info`). Auth: cached interactive login; headless
  never prompts (exits with auth-required instead of hanging). [VD]
- MCP-client: global `~/.gemini/config/mcp_config.json`, workspace
  `.agents/mcp_config.json`, key `mcpServers`; entry exactly-one-of
  `command` | `serverUrl` (docs reject legacy `url`/`httpUrl`); `headers`,
  `oauth`, `disabledTools`. (aikit's observed path
  `~/.gemini/antigravity/mcp_config.json` is stale.) [VD]
- ACP: none found. → upgrade profile from "IDE-owned observed" to a real
  process-family candidate.

### dsh (DeepSeek Harness; docs-only, moving fast)
- No headless/JSON face documented; `npx @deepseek-ai/dsh web` is the
  documented launch; prose-only mentions of ACP stdio (`--profile acp`) and a
  JSON-RPC sdk profile; MCP only as opt-in config overlays. Keep brokered
  posture; re-census before any lane.

## Expansion shortlist (census, ranked — all [VD] unless noted)

1. **GitHub Copilot CLI** (`copilot`) — `copilot --acp` (public preview
   2026-01-28); MCP via session/new mcpServers. Effort S.
2. **Factory droid** — no first-party ACP (community adapter); use
   `droid exec --output-format json|stream-jsonrpc` headless; MCP
   `~/.factory/mcp.json` `mcpServers`, `droid mcp add`. Effort S.
3. **Cline** — `cline --acp --auto-approve true`; `CLINE_API_KEY`; no batch
   headless JSON. Effort S.
4. **Kiro CLI** (ex Amazon Q, `kiro-cli`) — `kiro-cli acp [--agent]`;
   headless docs exist; q/kiro binary-identity split is the hard part. S.
5. **Mistral Vibe** (`vibe`, OSS) — ACP per README; `vibe --prompt --output
   json|streaming`; TOML `[[mcp_servers]]` in `.vibe/config.toml`. S.
6. **OpenHands** — `openhands acp --resume <id|--last> --streaming`;
   headless + MCP pages first-party; settings model is web-app-era. M.
7. **Sourcegraph Amp** (`amp`) — no native ACP (community bridge); Execute
   Mode + Streaming JSON docs; `amp.mcpServers` in settings.json. M.
8. **Charm Crush** — no ACP (open issue #2091) and no headless mode; MCP
   config `.crushrc`/`crushrc` only → config-projection adapter for now. M.
9. **Qoder CLI** — `qoder --acp`, `QODER_PERSONAL_ACCESS_TOKEN`; headless
   undocumented. S.
10. **Docker cagent** — ACP registry listing possibly stale (README silent);
    YAML `toolsets` MCP; confirm surface post-rename. M.
Dropped: iFlow (shut down 2026-04-17), Devin (cloud API), Conductor/Windsurf/
Trae/Tabnine (no local harness CLI), Zed (ACP client, not agent). Watchlist:
Kilo (`kilo`, ACP advertised, unverified), Stakpak, VT Code, Junie, fast-agent
(registry listings).

## Corrections to aikit's current records (act on these in L3)

1. **Stale refusal:** "ACP v1 has no generic attach operation"
   (`agent_connection.rs` refusal) — superseded: `session/resume` stabilized
   2026-04-23; add resume where `sessionCapabilities.resume` negotiates true.
2. **profiles.rs observation drift:** missing/incorrect observations —
   hermes MCP (`~/.hermes/config.yaml` YAML `mcp_servers`), goose (`extensions`
   key, `cmd`/`envs`/`uri` naming, SSE removed), antigravity path
   (`~/.gemini/config/mcp_config.json`, `serverUrl`), claude project-scope
   `.mcp.json`, codex per-tool approval keys; opencode `command` is an array.
3. **Roster:** grokbot is not a coding harness (see card); zcode has no
   connection face today; ollama belongs to the models layer only.
4. **Gemini flag:** profiles/census prose says `--experimental-acp`; the
   0.30+ line wants `--acp` (installed binary is 0.29.5 — also stale).
5. **MCP-server modes verified:** claude `mcp serve`, hermes `mcp serve`,
   goose `mcp <server>`; codex has none (any census prose saying otherwise is
   wrong).
6. **kimi identity:** resolve package vs Claude-contract clone before
   deriving any launch facts (card above).
