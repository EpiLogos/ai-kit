# Authorised Routine invocation evidence

`aikit.routine-invocation-evidence/v1` is AIKit's public evidence that one exact
Routine occurrence passed AIKit's Routine, Method proof, trigger and supplied
authority-relation checks. It is an invocation request, not evidence that an
Action ran or completed.

The authorisation command accepts the complete Routine and Method bodies plus
owner-supplied occurrence and authority-validation provenance:

```text
aikit --json routine authorise-invocation --request-json @request.json
aikit --json routine invocation <invocation-ref>
aikit --json routine invocations
```

AIKit checks that the Routine is enabled, its exact source revision and proof
basis match the supplied Method, the trigger observation belongs to that
Routine, and the authority receipt's ref, revision, standing and timestamp match
the Routine authority relation. AIKit does not independently query Actuation or
another external authority provider at this file/CLI boundary. The authority
receipt is owner-supplied provenance with `owner-attested` standing; inconsistent
or stale payloads fail before the invocation ledger changes. The trigger and
authority timestamps are likewise caller-reported observations, not AIKit clock
readings.

Accordingly, `proof_standing` is `current-on-supplied-basis`: the proof matches
the exact bodies in this request, but this CLI has not looked those bodies up in
their canonical source. External/current owner revalidation remains required at
the later execution boundary.

An identical admission is idempotent. Reusing an `invocation_ref` with different
semantic owner facts fails closed. A trigger observation is globally bound to
one invocation. Provider retry and restart deliveries may add
distinct delivery receipts to the same invocation without changing either the
Routine or invocation identity; each `delivery_ref` is globally bound to that
invocation and conflicting reuse fails.

Provider evidence is optional. A manual Routine invocation is valid without a
scheduler binding, job identifier, delivery identifier or restart identifier.
AIKit stores no scheduler job registry and does not manufacture unavailable
provider evidence.

The checked-in public evidence schema and conformance fixtures are:

- `schemas/aikit.routine-invocation-evidence.v1.schema.json`
- `fixtures/routine-invocation/authorised-scheduled-request.json`
- `fixtures/routine-invocation/authorised-scheduled-evidence.json`

The request and private ledger are strict native serde inputs, not separately
published JSON Schema contracts. Unknown fields at any invocation security
boundary are rejected; the real CLI tests exercise request and ledger behavior.
