# Native encounter runtime

The `aikit session-space` verb family of the main `aikit` binary owns resident ACP encounters (folded from the former `aikit-session-space` companion binary, O-I #376). Its
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
  --faculty-config <abs>/faculty.json
```

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
