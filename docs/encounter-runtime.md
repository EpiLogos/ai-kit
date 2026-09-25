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


## Prime RPC acting-body providers

A configured Encounter provider may use `prime-rpc` when the provider process
is Prime Agent's pinned JSONL RPC surface rather than ACP or Pi-RPC. This does
not create another AgentSession or model identity. AIKit still owns the
canonical AgentSession; the adapter observes Prime's native session id,
binds it explicitly and records the provider's exact `body_ref` and
`body_revision` in the resident reading.

The current Epi-Logos body is configured through:

```sh
aikit session-space -C <project> encounter-epi-prime-configure \
  --launcher <abs>/actuation-epi-prime \
  --prime-bin <abs>/prime-agent \
  --ql-bin <abs>/ql \
  --ql-revision <40-hex QL-MEF revision> \
  --body-revision <40-hex Actuation revision> \
  --skill-path <abs>/ql-relational \
  --research-bin <abs>/actuation-research \
  --faculty-config <abs>/faculty.json \
  [--central-ctrl-bin <abs>/ctrl --central-root <abs>/Central \
   --central-project <Project-key>]
```

AIKit also inserts its own current executable into the Actuation launcher so
Prime descendants can call the native `model-resolve` roster even when model
credential delivery uses a scrubbed final-child environment. When the optional
Central triple is supplied, Actuation carries only those explicit non-secret
owner paths/project key into Prime. The inherited Skill may then write/read
Central's existing `projectcentral.now.return` handoff records; no NOW store
or transcript is copied into AIKit.

The command records configuration only. It starts no provider and acquires no
credential. The existing Encounter open/first-Send boundary launches the body.

If an explicit AIKit model dispatch policy is present, the resolved native
provider/model is appended to the Actuation launcher and Prime `get_state`
must read back that same pair. If no model override is authored, Prime may use
its own configured model; AIKit records the model/provider observed from
`get_state` and does not misreport that as an AIKit selection.

Cancellation maps to Prime's native `abort`; ordered text/tool events and
`agent_end` map into the same Encounter journal/signals used by the other
protocols. Restart never trusts a persisted `active` flag: native identity,
body revision and model observation are re-resolved. An explicit body mismatch,
missing body revision, changed native session or selected-model disagreement is
a refusal, never a generic fallback.

For the Epi-Logos mode this provider advertises
`agent-body/epi-prime-ql`. O:I requests that semantic body only for a new
conversation in the Epi world; it does not hard-code this provider id and it
preserves explicit user provider overrides.
