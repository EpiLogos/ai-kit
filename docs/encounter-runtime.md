# Native encounter runtime

The existing `aikit-session-space` companion owns resident ACP encounters. Its
`encounter-configure --provider-json` command records explicit native provider
argv; `encounter-start` starts the home-scoped owner, and `encounter
--request-json` dispatches its typed operations. Provider argv is never accepted
from an encounter IPC request. This is a generic ACP operation, not a Pi endpoint.

A canonical AgentSession must be attached to a retained SessionSpace. Open also
checks local Project context before launching its configured provider. Each
resident encounter has one native session, SQLite transcript, CAS composer and
provider connection, independent of UI clients. Current connection isolation is
an owner policy; it does not assert that ACP itself only permits one session per
connection. Owner restart retains transcript/composer but does not claim native
session continuity or silently reconnect.

The view schema is `aikit.encounter-view/v1`. Existing `blocks`, `more`, and `draft`
fields are retained. Blocks are bounded in count and UTF-8 bytes; cursor history
is separately bounded. `connection` discloses the native identity, current state,
configured provider id/label and transport failure. `actions` disclose operation
refs, enabled state and unavailable reasons. `permissions` contains actual pending
provider request objects. Exposed thinking text and raw ACP updates are preserved
in durable history; thinking blocks are a rendering projection of those bytes.

`permission` accepts the canonical AgentSession, `request_id` and an offered
`decision` (`selected` with `option_id`, or `cancelled`). The owner resolves the
actual pending request; the adapter validates the offered option and preserves
the native JSON-RPC id. Requests preserve `tool_call`, complete original `raw`
params and each choice's native `kind`. Durable requested/sent records distinguish
attempts from successful wire dispatch. A post-dispatch storage failure returns
an explicit uncertain outcome and must not trigger automatic replay.

Provider consent is not an Actuation grant, Factory decision, filesystem authority,
or sandbox override. Native provider tools retain their actual execution policy;
a consented tool can still fail. Tool updates remain native JSON in history and
view blocks. Cross-product Actuation admission remains a distinct owner operation;
this endpoint does not manufacture it.

Normal host turns have no total signal-count limit (`max_signals_per_turn = 0`).
A configured nonzero operational limit has its own observed terminal outcome.
Ordered disk-backed delivery, durable owner journaling, bounded interruption
retention and bounded reader pages decouple memory retention from turn duration.
Storage failures, process failures, protocol faults, explicit cancellation and
configured limits remain distinct. The owner drain outlives individual readers.

Real acceptance runners are `scripts/verify-resident-acp.py`,
`scripts/verify-resident-concurrent.py`, and
`scripts/verify-resident-permission.py`. They require explicit binary/provider
argv, create isolated native homes and use actual published ACP providers. They
never synthesize provider responses. Permission acceptance additionally checks
real tool execution or native sandbox refusal, offered-choice validation and
stale/wrong-session refusal. Unit protocol regressions are not substitutes for
these live provider receipts.
