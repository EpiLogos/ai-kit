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
