# Gateway connector matrix — Hermes specimen vs O:I reality

Derived 2026-09-25 from direct inspection of the installed Hermes Agent
v0.21.1 (`~/.hermes/hermes-agent`, 30 shipped platform adapters under
`gateway/platform_registry.py` + `plugins/platforms/`) against the landed O:I
connector seam (`aikit.gateway-connector/v1`). Evidence standing follows the
suite's classes: D deterministic, C cross-product conformance, P real
provider, M physical world, H human acceptance.

| Hermes platform (installed) | Hermes form here | O:I classification | Evidence standing |
|---|---|---|---|
| Telegram | long-poll Bot API; configured (`@nara_hermesbot`) | **native O:I adapter exists** — `telegram_gateway.rs` + curl live transport | D: fake-transport suite; C: connector conformance 7/7; P: live getMe/sendMessage/exclusive-poll green (message 3775); P pending: full ingress→turn→reply conversation loop |
| Slack | plugin adapter; configured with creds | **native O:I adapter exists (egress-first)** — `slack_bot_api.rs` + `slack_gateway_curl.rs` through the same connector contract; egress: chat.postMessage/update/delete, reactions.add, threads (thread_ts); **no Typing** (Slack has no typing API — never advertised), **no outbound media** (files.upload v2 is a multipart flow outside the JSON transport); ingress = `conversations.history` polling with per-channel ts watermark, users.info display names, bot-id self-echo suppression; real-time ingress needs Socket Mode (a WebSocket client — not built) or a public Events API webhook (external prerequisite) | D: fake-transport suite (20 slack fixtures: translation, self-echo, cursor paging, rate-limit envelope, capability truth); C: connector descriptor conformance + `slack` factory arm + CLI declaration flow; P: pending owner-authorised token staging at `~/.aikit/credentials/slack.token` (live proof skeleton `slack_gateway_live.rs` ready and inert) |
| iMessage | Photon plugin → Spectrum-ts bridge (NOT osascript) | **external/sidecar body appropriate** — a bridge process speaking the stdio wire contract (`gateway-connector-specimen` proved that seam end to end) | C: specimen wire connector conformance; no iMessage bridge body yet |
| WhatsApp (Meta Cloud API + plugin) | webhook-based | **external prerequisite** — needs Meta app + webhook endpoint; sidecar or native later | none beyond contract |
| Discord | plugin; auto-threads, voice | **external prerequisite** — bot token + outbound gateway WS client not yet built | none beyond contract |
| Mattermost, WeCom (+Callback), LINE, IRC, Matrix, SimpleX, ntfy, Google Chat, DingTalk, Feishu, Email, SMS (Twilio), Teams, BlueBubbles, QQBot, Weixin, Yuanbao, Buzz, Raft, Home Assistant | plugin/built-in adapters; only Mattermost/WeCom/WhatsApp/HA configured in Hermes, none carry O:I world accounts | **external prerequisite** (no credentials/accounts on this World) — each becomes an adapter through the same connector contract when a real account exists | none beyond contract |
| Generic webhook platform + dynamic subscriptions | inbound HTTP | **absent by scope** — the stdio wire host is the generic programmatic surface this commission landed; an inbound HTTP surface is a later carrier through the same protocol | C (wire host) |
| OpenAI-compatible API server, peer bot-to-bot | HTTP API between gateways | **absent by scope** — O:I's cross-gateway relation is Workcell relay (`gateway remote`/occupancy routing), different by design; peer-style DMs are a later seam | M: two-Workcell relay is landed and live |

Local Apple capabilities (Notes, Reminders, Find My, local Messages): Hermes
ships these as **skills/tools** (`skills/apple/*`, osascript-based), not as
messaging transports — iMessage-as-transport is the separate Photon bridge.
O:I equivalent routes through the Skill/Capability extension path, not the
Gateway connector seam. **Different by design**, not a gap.

Update this table only from executed evidence; a capability moves rows when
its proof moves, never from intention.

## Operational parity — lifecycle, upgrade and remote reach

The table above is about platforms. This one is about how the gateway is run,
kept current and reached. Installed Hermes: v0.21.1 (2026.9.7), checkout
`20f7ef4df5e1`; upstream `main` `7239625ae1b7` (2026-09-30); live documentation
read 2026-09-30 through a summarising fetch. **Shipped** = present in the
installed source (file:line in the research record); **upstream** = newer than
installed; **claim** = documented but not found in the installed source.

| Behaviour | Hermes | O:I (this change) | Standing |
|---|---|---|---|
| Restart requested from inside the gateway survives the restart | detached updater; result recorded before the disruptive step; the next gateway claims the pending marker and reports — **shipped** | durable transaction; worker under the service manager (LaunchAgent / transient unit), not a child; the new gateway re-adopts an orphan and delivers the receipt once | D: scripted-driver suite (worker death, dead-worker resume, receipt delivered once) and real-binary suite (real gateway processes, detached worker, scripted supervisor and installer: upgrade, install failure, flip-then-fail rollback, broken new build rolled back, foreground left running, SIGTERM drain). M: real launchd (Mac) and real systemd (Omarchy) under controlled instances, five scenarios each; I: the real services of both machines upgraded through `gateway upgrade apply` (`docs/implementation/GATEWAY-OPERATIONS-ACCEPTANCE.md`) |
| The requester hears the outcome | `/restart`/`/update` notice through the requester's own adapter — **shipped** | `/upgrade apply` receipt announced into the asking conversation | D |
| Drain | refuse new turns, wait ≤1800 s, interrupt with `resume_pending` — **shipped** | bounded grace (default 60 s), interrupt and **record**; no auto-resume | D |
| Stop by signal drains | SIGUSR1 / exit codes 75, 78 — **shipped** | `SIGTERM`/`SIGINT` drain then exit 0 | D (real binary) |
| The running version is checked after the update | `code_sha` comparison across the fleet — **shipped** | the process's own revision, pid, start time and executable digest; upgrade verifies a *different* process runs the *expected image* | D (real binary: a stale resident is found, upgraded, and the new pid states the new revision). I: Mac pid 47288 and Omarchy pid 2348420 each state revision `6e452a600a4c` and their executable digest |
| Stale resident noticed | fleet check at update time — **shipped** | `gateway doctor`, `oi doctor`, `oi update --check` | D |
| Rollback | update backups (`--backup`) — **shipped** | installer rollback + verification of the previous build | D |
| Update skips the gateway restart | `--no-gateway-restart` — **upstream** (v0.21.4); documented, **absent from the installed parser** | `upgrade apply` without `--install` restarts onto an installed build; with it, the installer is the managed one | — |
| Multiplexed single gateway per host | default upstream (v0.21.3–5) — **upstream**; installed default is one per profile | one gateway per Workcell; placement is a separate axis | not adopted |
| Remote client modes | loopback / LAN bind / Tailscale (docs only) / SSH tunnel for Desktop — **shipped (partial)** | `local-ipc`, `loopback-service`, `private-tailnet`, `tailscale-serve`, `ssh-tunnel` composed; `remote-authenticated-endpoint` refused by default; Funnel never configured | see `GATEWAY-OPERATING-MODES.md` |
| Client auth | API key mandatory even on loopback, non-empty, constant-time — **shipped** | peer/owner tokens; owner-only token files; bind class guard | D |
| Pairing / DM admission | pairing codes, allowlists, `decline` (upstream) | per-binding ingress policy (Allow/Pair/Deny + owner allowlist) | C |
| A request to an agent on another Workcell | not applicable (one machine per gateway) | one route, negotiated once: `EncounterRelay` over the gateway carrier when the peer advertises `encounter-request-relay`, else the named legacy ssh route | R/I: Mac→Omarchy, installed `6e452a600a4c`, controlled ACP body, ssh broken before each send, gateway stopped 30 s mid-flight, 2 requests → 2 prompts, each reply included once (`docs/implementation/GATEWAY-OPERATIONS-ACCEPTANCE.md`) |
| Delivery ledger | at-least-once, 3 attempts, 24 h — **shipped** | unreceipted operations retained and named, never blindly re-sent | D |

Documentation-versus-code discrepancies found in the specimen and **not
carried**: live docs say launchd uses `KeepAlive.SuccessfulExit=false` while
the installed plist uses `KeepAlive true`; docs describe
`--no-gateway-restart` which the installed parser lacks; docs promise a 5–15 s
outage while installed restarts are drain-first.

