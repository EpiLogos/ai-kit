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
| **Flow conversation request** | One Flow entry asked of one or several agents (the Encounter owner's request record, per-recipient delivery, reply reducer, `central.flow.append`); a recipient held by another Workcell is asked through that Workcell's gateway | landed locally (O-I#558); cross-Workcell over the gateway's `EncounterRelay` command since `encounter-request-relay`, ssh/exec only for a Workcell with no declared gateway or one that does not advertise the feature |
| **Shared material environments** | SessionSpace, herdr/tmux, Workcell runs — co-presence and shared action, not messaging | out of the gateway's lane; the gateway may carry invitations, never the work itself |

## One carrier, three journals — what is shared and what is not

A trace of the code at `64d4e1d1` (30 September 2026) found **three planes**
under these faces, not one queue:

| Plane | Journal | Turn runner | Return |
|---|---|---|---|
| Communique | `state/gateway.json` `communiques` | none — injected into the recipient body's next human prompt | a new Communique |
| Connector conversation | gateway stream journals + in-memory connector queues | the gateway's own agent host, per binding | the connector |
| Flow request (Encounter) | `encounters.sqlite3` | resident Encounter sessions | `central.flow.append` |

They are **not** merged, and this document does not claim they are. What the
operational architecture makes *one* is everything around them:

* **One carrier and one authentication.** Every cross-Workcell hop — Communique
  relay, occupancy, and now the Flow conversation request — travels as a
  gateway command over the authenticated carrier, under the same peer/owner
  scope, with the same feature negotiation. The Flow request no longer has its
  own ssh login, its own route declared inside the request, or its own
  (absent) handshake.
* **One running-identity and lifecycle.** One process, one build identity, one
  drain, one upgrade.
* **One addressing model** (V0, below): a registered agent is addressable;
  occupancy decides where it is embodied.

What stays separate on purpose, and what changed on 6 October 2026
(ai-kit#481 close-out on `f99760c3`):

* **The sender copy now learns remote delivery.** Every relay pass reads each
  forwarded Communique's fate back from the gateway it was relayed to
  (`CommuniqueFate` over the carrier); when the remote has recorded the
  delivery, the sender copy is re-stood `delivered` with a basis naming the
  remote gateway and the readback. A record the remote does not know stays
  exactly as it was, named unresolved — a readback never delivers on its own
  authority. (Six such records on the Mac were the original evidence.)
* **The connector plane now has durable attempt evidence.** Every outbound
  operation carries `attempts` / `last_attempt_at_unix_ms`, written BEFORE
  the connector is invoked: a crash mid-attempt leaves "attempted, outcome
  unknown" rather than a silently vanished send. On restart and on every
  reconnect the pump re-arms a connector's IDEMPOTENT pending operations
  (typing, edit, delete, react — a re-attempt either succeeds or fails into
  an honest receipt) and HOLDS outcome-unknown sends, which the owner
  resolves by evidence (`aikit gateway recover --deliveries` lists them;
  `recover --resolve <op> --state delivered|abandoned --evidence …` retires
  one with a receipt). Nothing is ever blindly re-sent.
* **A drain names what it did not serve.** A message admitted while a drain
  holds the engine is journalled (retained), named in the `DrainReport`
  (`admitted_unserved`), and named in the restart line and the upgrade
  receipt. It is never replayed.
* Durable Position routing is still decided in four places in
  `gateway_contact.rs` (`route_to_occupancy`, `place_instance`,
  `forward_pass_via`, `relay_attempt`) with slightly different answers for an
  unknown ledger. The relay pass and the ask path should call one function;
  they do not yet (ai-kit#481 item 1, open).
* Communiques wait for a human prompt; nothing wakes an idle body.

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

**V0 — Addressability derives from agency identity (the reconciliation, commissioned 2026-09-28).**
The owner's finding: the Position registry gated the entire contact plane
while remaining unpopulated — a second name for agents the system already
names (the first six minted Positions each wrapped exactly one existing
`agent/*` ref, 1:1). The model correction:

- Every registered agent profile is **addressable by default** at its
  identity. `/ask`, `send` and `who` resolve the agency registry; a missing
  Position means **"addressable, not currently embodied here"** — mail holds
  for the agency — never "does not exist".
- **Position survives as the occupancy projection**: tenure, succession and
  attribution verification (which Workcell embodies the agency now), valuable
  exactly where the relay needs material handover facts. It stops being a
  gate on contact.
- The decision to mint or not mint a Position therefore never mints or mutes
  an agent.

| # | Vertical | State |
|---|---|---|
| V0 | **Agency-identity addressability**: who/ask/send resolve agent profiles, joined with occupancy where present; unembodied ≠ nonexistent | landed (`who`/`send`/`/ask` resolve `agent-profile.list` — Central's authoritative registry — joined with the Position whose `eligible_agent_refs` name the agent, occupancy then routing exactly as for any Position; a profile with no Position is addressed at its identity and the Communique holds for the agency; `who` lists every registered agency with `embodied-here` / `embodied-elsewhere` / `not-currently-embodied`, naming its registry; 15 contact + 26 engine proofs, 73 adapters green) |
| V1 | **Cross-face attributable forward**: `/ask <position> <message>` in a connector chat → attributable Communique → relay → the recipient's Surface | landed (canonical `AskPosition` behind the `/ask` edge; `ContactAskRouter` routes with the exact `gateway send` laws — occupancy, cross-Workcell relay, held/vacant — attribution from Actuation's ledger, origin provenance in the attribution basis; the recipient receives it at their turn boundary, their chat face being V3; 7 ask proofs in `gateway_conversation_engine`, 73 gateway + 13 contact proofs green) |
| V2 | **Group-chat binding**: bind a group address, admit bots, enforce the loop budget | needs a live two-bot group to prove |
| V3 | **Bot-initiated outbound** from the engine (an agent messaging a user or peer unprompted through its bound connector) | after V1 exercises the same queue |
| V4 | **A2A endpoint face**: card + transport mapped to Communiques | after V1–V3 prove the native core |

Update this document only from executed evidence; a vertical moves rows
when its proof moves.
