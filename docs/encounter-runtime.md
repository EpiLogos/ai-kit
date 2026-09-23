# Native encounter runtime

The `aikit session-space` verb family of the main `aikit` binary owns resident ACP encounters (folded from the former `aikit-session-space` companion binary, O-I #376). Its
`encounter-configure --provider-json` command records explicit native provider
argv; `encounter-start` starts the home-scoped owner, and `encounter
--request-json` dispatches its typed operations. The companion command remains a
supported auto-discovery entry to that owner. Provider argv is never accepted
from an encounter IPC request. Configured providers may use the stable ACP route
or the native Pi RPC route; controlled protocol fixtures establish owner
semantics, while actual-provider acceptance remains separate evidence.

A canonical AgentSession must be attached to a retained SessionSpace. Open also
checks local Project context before launching its configured provider. Each
resident encounter has one native session, SQLite transcript, CAS composer and
provider connection, independent of UI clients. Current connection isolation is
an owner policy; it does not assert that ACP itself only permits one session per
connection. Owner restart retains transcript/composer but does not claim native
session continuity or silently reconnect.

Native startup has one 90-second cumulative control deadline covering provider
initialization, native session open/load, and an admitted model selection. It is
shorter than the 120-second owner IPC deadline, so the service can record the
outcome before its caller gives up. An expired deadline is checked before each
control dispatch. Timeout stops and reaps the exact owned process group when
cleanup can be confirmed; uncertain cleanup remains visibly blocked rather than
authorizing a replacement launch. Startup does not hold the global residents map
while external protocol work runs, so View and Read for this or another session
remain available. A duplicate open for the same session is refused promptly.

The encounter journal records a generation-bound reservation, exact attempted
body and Agency basis, and then binding, reconciliation, or refusal. The body
basis includes the configured and effective launch command digests, task/model
and context basis without raw credential material. Agency, task, provider,
context, and model source state are rechecked immediately before binding. After
an owner restart, an unmatched reservation is shown as `RecoveryRequired`; a
refusal without confirmed cleanup is `CleanupUncertain`. Both disable another
open and prevent owner shutdown from claiming all children stopped. The typed
`reconcile-native-open` request requires the exact generation, explicit confirmed
cleanup, and an evidence reference. Its receipt labels that confirmation as an
operator attestation and does not claim the owner observed process exit. It
records cleanup reconciliation without claiming provider success, replaying a
turn, or launching a replacement process.

For task-bound and selected-model launchers, credential delivery occurs in the
owned child and is covered by the startup deadline. Direct profile launches may
resolve profile-declared credentials synchronously in the service before spawn;
that separate parent-side lookup is not interrupted by the child control
deadline.

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
