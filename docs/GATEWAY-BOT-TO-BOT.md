# One inter-now communication system, four faces

The owner's clarification, recorded: bot-to-bot, bot-to-factory-agent,
group chats, cross-Workcell hops, shared tmux/SessionSpace co-presence and
A2A are not separate messaging systems. There is **one** communication
system — the Agency Gateway's attributable traffic between situated
agencies — and everything else is a **face** of it.

## The faces

| Face | What it is | State |
|---|---|---|
| **Communique** | Position→Position attributable contact; journal; cross-Workcell relay by occupancy; held/vacant; delegation into Factory custody | landed (`gateway send/inbox/relay`, 13 contact proofs, live mac⇄omarchy) |
| **Connector conversation** | A Surface face on an external platform (Telegram, Slack, …) bound to an agent session; per-sender admission | landed (live on `@Ohisysbot`, streaming) |
| **A2A** | The external interoperability projection — for agencies **outside** the World; bindings, presence, exchange authority, agent cards, messages | primitives landed (`aikit-core/src/a2a.rs`, `a2a_card.rs`); no gateway face yet |
| **Shared material environments** | SessionSpace, herdr/tmux, Workcell runs — co-presence and shared action, not messaging | out of the gateway's lane; the gateway may carry invitations, never the work itself |

Laws that survive every face: **Position ≠ Agent ≠ AgentSession ≠ bot
identity**. A bot is a Surface identity of one situated agency; a message
from it carries that agency's session and stream refs; attributability never
dissolves into "a chat said".

## Bot-to-bot capacity, scoped

1. **In one group chat** — a group is ONE `ConversationAddress` (`scope_id:
   "group"`) with per-sender admission; a bound group may admit other bots
   (`SenderKind::Bot`). **Loop law**: an agent never answers a message
   authored by its own connector identity; a bot-to-bot answer carries a
   decreasing reply budget (default: a bot may answer a bot once; that answer
   is never re-answered). This is the O:I analogue of the specimen's bot-loop
   guard.
2. **Cross-gateway, outside groups** — this is the EXISTING relay, not a new
   system: an agent's connector-originated message to another agency becomes
   an attributable **Communique** to that agency's Position (origin
   provenance: agent session + connector address), routed by occupancy across
   declared Workcells exactly as `gateway send` routes. If the recipient
   holds a bound connector conversation, delivery surfaces **in their
   chat** — a bound bot may be messaged proactively; if not, the Communique
   waits at the Position (held/vacant law, unchanged).
3. **Factory agents** — factory agents ARE Positions, so (2) reaches them
   today one-way, and `gateway delegate` crosses a Communique into custody.
   A factory agent answering back **into a bot chat** requires that agency
   to hold a connector Surface — honest boundary: multi-Surface agencies are
   future work, not faked.
4. **A2A** — the door for agencies outside the World (Hermes, external
   systems): an A2A card and message transport mapped onto the same
   Communique/connector semantics. Inside the World, the native relations
   stay richer than A2A (per O-I #154 §6: communique, session contribution,
   delegation, session fork, co-actuation).

## Verticals

| # | Vertical | State |
|---|---|---|
| V1 | **Cross-face attributable forward**: `/ask <position> <message>` in a connector chat → attributable Communique → relay → the recipient's Surface | this commission |
| V2 | **Group-chat binding**: bind a group address, admit bots, enforce the loop budget | needs a live two-bot group to prove |
| V3 | **Bot-initiated outbound** from the engine (an agent messaging a user or peer unprompted through its bound connector) | after V1 exercises the same queue |
| V4 | **A2A endpoint face**: card + transport mapped to Communiques | after V1–V3 prove the native core |

Update this document only from executed evidence; a vertical moves rows
when its proof moves.
