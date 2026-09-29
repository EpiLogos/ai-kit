# Harness connection truth campaign — scope

Status: scope authored and research pass complete 2026-09-22; executed in two
passes the same day. Pass one landed L1–L4 (schema `sessions.connect`,
ACP `session/resume`, MCP header auth, profile-derived providers, disclosure
join). Pass two landed the remaining lanes: the native MCP config write
behind the fallback (one shared projection entry), model-launcher fallback
variants, wave-2 ACP profiles for claude/codex, the grokbot → Grok Build
repurpose, and five roster additions (copilot, cline, kiro-cli, qoder,
droid). The live ACP walk proved the wire against real gemini (spawn →
initialize → session/open, with its auth gate surfaced verbatim; kimi's
installed `kimi acp` did not answer a spec-shaped initialize — owed to its
follow-up lane). Open remainers are carried in the ai-kit project NOW
records of 2026-09-22; the fullest are the tools-layer sweep-law gap
(ownership marker only recognizes dispatch-style commands) and the
shortlist tail (vibe, openhands, amp, crush, cagent). Companion to `2026-09-16-harness-profile-design.md` (the
harness-profile outline) and `docs/v2/24` (harness adapters). Commission:
ensure the harness-profile schema has adequate space for ACP- and
RPC-type connections, bring every harness adapter up to scratch as a
genuinely, natively working connection, and grow the harness roster.
Full per-harness research payload: `2026-09-22-harness-connection-truth-cards.md`.

## Where we are (evidence 2026-09-22)

AIKit already owns a real protocol stack — this work is about giving every
harness a path into it, not about building one:

- **ACP v1 client, complete**: `crates/aikit-adapters/src/agent_connection.rs`
  (initialize → session/new → session/prompt → cancel, permission requests,
  `session/update` ingest, model + reasoning selectors, version pinned to
  `ACP_STABLE_PROTOCOL_VERSION = 1`). Production wiring:
  `crates/aikit-cli/src/encounter_service.rs`. The `mcpServers` wire field is
  populated from trusted tool-protocol capsules
  (`crates/aikit-cli/src/encounter_mcp.rs`) and capability-gated.
- **Pi RPC client, complete but pi-only**: `pi_rpc_connection.rs` (typed JSONL
  frames, not JSON-RPC; attach-only, no MCP, no cwd by design).
- **Transport**: `connection_process.rs` (piped stdio child, scrubbed env,
  process group, JSON-line framing) under `agent_session_host.rs`.

What is missing is the per-harness half. Of the 17 client adapters in
`crates/aikit-adapters/src/clients/`, only 4 implement `ClientAdapter` (claude,
codex, zcode, broker). The other 13 project skills/hooks/configs but cannot
open a session. The one production ACP path takes its launch command from
freeform owner-authored provider JSON (`~/.aikit/state/encounter-providers/*.json`,
`encounter_service.rs`); no adapter derives or validates an ACP argv, so
harnesses that genuinely speak ACP — gemini (`--experimental-acp`), kimi
(`kimi acp`), hermes (`hermes acp`), plus adapter-wrapped claude/codex — are
connected by hand or not at all. Three profiles declare `protocol = "acp"`
(gemini, kimi, hermes-acp in `profiles.rs`) with no code that can act on it.

## Verdict: the schema does not yet have adequate space

`aikit.harness-profile/v1` (`crates/aikit-core/src/harness_profile.rs`,
`deny_unknown_fields`) declares protocol *families* but stores no connection
*endpoints*. Specific gaps:

1. **No launch/endpoint facts.** The sessions layer carries `protocol`,
   `open_modes`, capability flags — but no ACP entry argv, no rpc flags or
   socket, no env/cwd for the connection. `EncounterProvider.argv` has no
   profile-derived source. (Design intent said sessions become "profile facts";
   only refusal shapes landed.)
2. **Remote MCP has no auth surface.** `ToolServerRecord` = `{command,args,env,cwd,url}`
   (`capsule.rs`); the ACP http wire entry hardcodes `headers: []`
   (`encounter_mcp.rs`). A URL-only MCP server needing auth headers cannot be
   represented, and `cwd` is documented as not carried on the ACP wire.
3. **Capability negotiation is not joined to profile truth.** Runtime
   `ConnectionCapabilities` (`agent_connection.rs`) and profile
   `SessionCapabilityFlags` (`harness_profile.rs`) are parallel structures; a
   harness advertising `mcpCapabilities` at initialize is never checked against
   what its profile claims. Disclosure renders the sessions layer as posture
   only (`harness_disclosure.rs`) — protocol, open modes and flags are invisible.
4. **No fallback routing.** If the negotiated ACP session lacks `mcpServers`,
   the composed tool set is dropped (`UnsupportedByProtocol`) even when the
   harness has a native MCP config seam the tools layer could write.
5. **Resume/attach truth is asymmetric — and one refusal is stale.** The
   service refuses ACP reconnect citing "ACP v1 has no generic attach", but
   `session/resume` stabilized 2026-04-23 (resume without history replay,
   gated by `sessionCapabilities.resume`). aikit's version-1 pin is correct;
   the refusal is not. Pi reconnect is refused through the service;
   opencode's profile declares `attach` with zero implementing code;
   process-protocol harnesses are never launched with their native resume
   flags.
6. **Roster incompleteness.** 13 of 17 harnesses are `Reach::AdapterOnly`;
   the "a lot of harnesses" commission needs both finishing these and adding
   new harnesses (see census in the truth-cards companion).

## Target shape

- **`sessions.connect`** (schema addition, additive optional fields with serde
  defaults): per declared protocol, the endpoint truth — argv template, env,
  working directory, health probe — so the connection builder can derive and
  validate an `EncounterProvider` from the profile instead of freeform JSON.
- **Auth-capable remote tool servers**: `ToolServerRecord` gains header
  declarations as secret *refs* (never values), populated onto the ACP http
  wire entry; `cwd` either carried or refused with a named reason.
- **Negotiation join**: one read model joining profile-declared capability
  flags with runtime-negotiated `ConnectionCapabilities`, rendered in
  disclosure; drift between claim and wire becomes visible.
- **Fallback routing**: ACP session without `mcpServers` → the tools layer's
  native config write for that harness (managed seams: claude, zcode,
  openclaw today; more after research), recorded as a distinct activation
  path, never a silent drop.
- **Resume truth per harness**: profiles carry the real resume/attach facts
  from the research cards; the service stops refusing where the protocol
  genuinely supports reconnect (ACP `session/load`, pi attach) and refuses
  honestly where it does not.

## Research results (2026-09-22 — passes complete)

All four passes returned with evidence; full cards and corrections live in
`2026-09-22-harness-connection-truth-cards.md`. What changed the plan:

- **Our version-1 ACP pin is right** (v2 is an unstable draft), but the
  post-pin additions are real gaps in our client: `session/resume`,
  `session/close|list|delete`, boolean config options, elicitation. The
  "no generic attach" refusal is stale — resume exists.
- **First-party ACP is now common**: gemini (`--acp`, renamed from
  `--experimental-acp` in the 0.30+ line), opencode, hermes, goose,
  cursor-agent, qwen all verified with exact argv; claude and codex connect
  through maintained `@agentclientprotocol/*` adapter packages (the zed ones
  are archived); zcode has no connection face at all today.
- **MCP config shapes verified for 12 harnesses** (paths, formats, root keys,
  activation), with drift against profiles.rs observations named for repair
  (hermes YAML, goose `extensions` naming, antigravity `serverUrl` + moved
  path, opencode array `command`).
- **Roster corrections:** grokbot is not a coding harness (xAI's real CLI is
  Grok Build, `grok -p --output-format streaming-json`, ACP); antigravity
  now ships a first-class headless CLI (`agy -p --output-format stream-json`)
  so its IDE-owned posture is stale; ollama belongs to the models layer only
  (HTTP API, not a child process); kimi's binary identity on this machine is
  ambiguous (Moonshot kimi-cli vs a Claude-contract clone vs a shell
  function) — pinned before its lane.
- **Census pattern:** ACP is the standard entry point for new harnesses
  (copilot, cline, kiro-cli, qoder, vibe, openhands all ship `--acp` or
  `<bin> acp`). The marginal cost of a new harness is collapsing toward
  configuration — which is exactly the L1+L2 shape: one ACP connection
  builder plus per-profile connect facts onboards most future agents as
  data, not code. Top-10 expansion shortlist is in the cards file.

## Development lanes (post-research, each gated by the workspace suite)

- **L1 schema** — `sessions.connect` + tool-server header/auth-refs +
  disclosure fields, additive to `aikit.harness-profile/v1`; schema-derived
  conformance so a profile claiming acp without connect facts is a review
  failure.
- **L2 connection builder** — profile-derived, validated `EncounterProvider`
  construction (argv/env/cwd from `sessions.connect`), retiring freeform JSON
  as the only route; parity tests against the two hand-written providers
  (pi-rpc, gemini-acp) that work today.
- **L3 per-harness profile upgrades** — apply truth cards: connect facts,
  tools layers promoted to `managed` only where a true native seam was
  verified, capability flags set to verified truth, and the observation
  drift in profiles.rs repaired. Wave 1 (first-party ACP, argv verified):
  gemini (`--acp` on 0.30+, probe with fallback), opencode (`opencode acp`),
  hermes (`hermes acp`), goose (`goose acp`), cursor-agent (`cursor-agent
  acp`), qwen (`--acp`); kimi joins once its binary identity is pinned. Wave
  2 (adapter-wrapped ACP): claude (`@agentclientprotocol/claude-agent-acp`),
  codex (`@agentclientprotocol/codex-acp`), pi (community `pi-acp`, low
  priority — pi-rpc is already live), droid (`exec --output-format
  acp-daemon`; daemon lifetime needs a design decision). Wave 3 (managed-MCP
  config, shapes verified): zcode, openclaw, claude, codex, gemini, cursor,
  qwen, opencode, goose, hermes, antigravity. Wave 4 (headless structured,
  non-ACP): antigravity (`agy -p --output-format stream-json` — posture
  upgrade from IDE-owned), openclaw (`agent exec --json`). Stays out: zcode
  (no connection face today), aider (text-only), ollama (models layer only),
  dsh (re-census first), grokbot (retire/repurpose as Grok Build `grok`).
- **L4 encounter service** — fallback routing, resume/attach per truth,
  negotiation-join disclosure.
- **L5 tests/gates** — golden ACP transcript corpus per harness (mock agent
  transcripts), opt-in live tests keyed to installed binaries (extends
  `acp_harness_native.rs`), conformance additions; workspace `cargo test`
  green, no waived reds.
- **L6 roster expansion** — new adapters for top census candidates, each
  landing with the same profile + truth-card + golden structure.

## Follow-up agent

Read this file and `2026-09-22-harness-connection-truth-cards.md`, then
execute L1 → L2 first (they unblock every per-harness wave), running the
workspace suite as the gate. Do not promote any tools layer to `managed`
without the research card marking its seam verified.
