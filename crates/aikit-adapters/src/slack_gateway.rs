//! Public Slack connector surface for the AIKit Agency Gateway.
//!
//! Slack is an **egress-first** platform for a token alone. This module is the
//! stable import surface for that truth; the mechanics live in
//! [`crate::slack_bot_api`] and the live carrier in
//! [`crate::slack_gateway_curl`].
//!
//! What this cut conforms to, with deterministic fixtures in
//! `slack_bot_api.rs`:
//!
//! - **Send** — `chat.postMessage`, thread-aware (`thread_ts`); a reply to a
//!   message rides that message's thread.
//! - **Edit** — `chat.update`.
//! - **Delete** — `chat.delete`.
//! - **React** — `reactions.add` (Slack emoji name; surrounding colons are
//!   normalised away).
//! - **Threads** — `thread_ts` semantics on both directions.
//! - **Ingress** — `conversations.history` polling per configured channel,
//!   with a per-channel `ts` watermark, `users.info` display-name resolution
//!   and bot-id self-echo suppression. Message *file attachments* are carried
//!   as media metadata.
//!
//! What is deliberately absent, and why:
//!
//! - **Typing** — Slack has no typing indicator API. The capability is never
//!   advertised; a typing operation fails conformance rather than pretending.
//! - **Outbound media** — `files.upload` v2 is a three-call multipart flow
//!   outside the JSON transport seam this cut builds on. The capability is
//!   never advertised.
//! - **Real-time ingress** — a token cannot push. Socket Mode needs a
//!   WebSocket client (not built in this cut); an Events API webhook needs a
//!   public URL (an external prerequisite). Both are named in the connector's
//!   health detail; `conversations.history` polling is the ingress carrier
//!   until one of them lands.

pub use crate::slack_bot_api::{
    SlackBotApiTransport, SlackBotIdentity, SlackConnector, SlackConnectorConfig,
    SLACK_GATEWAY_CONNECTOR_VERSION, SLACK_WEB_API_BASE,
};
