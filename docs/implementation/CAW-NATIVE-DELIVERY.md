# CAW native participation and delivery — implementation cut

Tracks: #274 → #275; independent #277 and #276 code. Dispatch authority:
EpiLogos/O-I#220, Factory#195 revision 3. This document records an implementation
cut, **not closure of those tickets or end-to-end continuous work acceptance**.

## What is executable

`aikit-session-space` remains the native session owner. The same encounter
store, owner-only IPC, canonical AgentSession and actual ACP/Pi host carry human
interaction and addressed machine requests. There is no parallel chat daemon or
new human document store. Machine requests never use or mutate the human draft.

An explicit `EncounterAgencyBinding` relates a canonical session to the actual
Agent, Agency and WorldBinding admitted from an exact native Actuation source.
A Central profile is optional. An unmanaged cwd/root World is valid: no
ProjectCentral, profile adoption, default selection or durable AgentSet edit is
performed merely to send a request. Source admission is a supplied-source
Actuation validation; it is **not** independent evidence of installed runtime
liveness, personal policy recognition or global revocation discovery.

`agency_admission::admit_agency` runs the existing native operation:

```text
actuation agency actualise <exact-staged-source.json> --json
```

It checks source bytes and digest before and after the call, exit status, schema,
requester, exact differentiated Agent/Agency/WorldBinding/World/scope, bounds,
determination and authority relations. Repeated consequential session operations
readmit the current source. The admitted source must permit
`action/aikit/encounter-send`; presence or a profile cannot confer that action.

The supplied selected-Agent context is read at the actual prompt boundary and
checked against its source digest. Its bytes and provenance reach the provider,
not just a JSON composition receipt. Another selected Agent's source is not
added to a group recipient's prompt. Native consent and source admission remain
separate checks. Imported source text cannot reconfigure the owner.

## Native operations

Provision through the **local native owner**, not a gateway message:

```text
aikit-session-space -C <cwd> encounter-agency-configure \
  --agent-session agent-session/example \
  --binding-json @binding.json [--expected-revision <current-revision>]

aikit-session-space -C <cwd> encounter-configure --provider-json @provider.json
aikit-session-space -C <cwd> encounter-serve --socket <private-dir>/owner.sock
```

`EncounterAgencyBinding` is the maintained Rust/JSON type in
`crates/aikit-cli/src/encounter_agency.rs`. It requires an explicit active flag,
revision, exact native identities, source basis, Actuation executable, permitted
senders, packet-source disclosure and optional exact context admission. A CAS
update needs a new revision and the current revision. A canonical AgentSession
cannot silently change its enduring Agent. Configuration is deliberately absent
from the IPC request enum. The configuration contains source references/digests,
not copied human source content or secret material.

Provider configuration adds `protocol: "acp" | "pi-rpc"`; the existing omitted
field defaults to ACP. `argv` is owner configuration, never imported request data.

Use the existing `encounter --request-json @request.json --socket ...` command.
The maintained request enum now includes `send`, `send-group`, `delivery` and
`reconnect`, in addition to existing `open`, `read`, `view`, permissions and human
interaction operations. An addressed request is:

```json
{
  "action": "send",
  "agent_session": "agent-session/example",
  "turn": {
    "delivery_ref": "delivery/example-001",
    "sender": "agent:sender",
    "expected_binding_revision": "revision:current",
    "packet": {
      "text": "The explicit bounded request",
      "source_refs": ["source:explicitly-shared"],
      "audience": ["agent:selected"]
    }
  }
}
```

For a group, the top-level action is `send-group`, with `delivery_ref`, `sender`,
`packet` and `recipients: [{agent_session, expected_binding_revision}]`. All
recipient identities and the exact audience are checked before the first
transport call. Fanout is **not atomic**: every recipient gets an independent,
durable result and failures stay attributable. No private-source widening or
implicit default participant selection is performed.

Read the actual delivery with:

```json
{"action":"delivery","agent_session":"agent-session/example","delivery_ref":"delivery/example-001"}
```

Read ordered response events with the existing `read` operation. They carry the
canonical `delivery_ref` only when their actual connection generation matches
the pending delivery. A transport ACK is `submitted`, never `returned`.
`returned` means the native host observed a completed provider turn; it does not
mean the task succeeded, an artifact was received, or a human recognised prose.

`reconnect` takes the same `space`, `agent_session`, `provider` and `cwd` as an
open. It loads the **recorded native session identity**, retaining canonical
identities, provider protocol/argv digest, context admission and history.
Contradictory native identities and unsupported continuation are refusals.
Pi currently supports native attach, not an invented session-load operation.
ACP load can replay updates before its null/empty result; those updates remain
on the awaiting canonical lane. They do not mark old-generation work complete.

A repeated exact `(AgentSession, delivery_ref)` returns its durable receipt and
never resends, including after owner restart. Reusing that identity for different
content/sender/participation is a conflict. One unresolved machine turn per
session prevents concurrent unattributable prompts. A failure or lost transport
is not automatic retry permission.

Explicit recovery of an unresolved request is available through:

```text
aikit-session-space encounter-delivery-reconcile \
  --agent-session <ref> --delivery-ref <ref> \
  --expected-phase <dispatching|submitted|uncertain> --evidence-ref <native-ref>
```

This records operator-supplied evidence correlation as `reconciled-no-replay`.
It neither verifies that external evidence by itself nor manufactures success.

## Composition and model safety

`aikit compose --agent <ref> --world <ref> --agency-source <basis.json>` selects
an explicit native source basis while preserving optional Central authored
material and opaque identities. The basis contains source_ref, revision,
absolute path and `blake3:<digest>`. No fallbacks mint Agency, WorldBinding or
Actuation identity from a SessionSpace, provider or cwd.

The existing Central Paśu producer now retains declared AgentSet members without
profiles as declared participants. This does not make them live, authorised,
resident or defaults. Excluded/unreadable Project disclosure fails closed.
`join_admitted_participants` is also available for exact admitted WorldBindings;
**ordinary World-wide discovery does not yet enumerate all runtime admissions
through that helper**.

Model route ranking excludes unusable credentials, denied fallback routes,
provider/native-id mismatches and explicit cost-ceiling violations. A native
instantiation result must match the selected request and schema; null, arbitrary
JSON, mismatched identity and widened evidence/access are not success.
Workcell's material plan is not a running body. Recording an instantiation is
not execution: its result explicitly says `executed:false`.

**Remaining #274 caller work:** `Service::realise_model` no longer hardcodes
policy/contract/harness eligibility to true. Those gates remain fail-closed
until the actual resolver supplies their positive basis. The generic catalogue
selection → credentials materialisation → configured native session execution
join is not finished. The addressed native session path above is executable,
but it must not be advertised as completing that catalogue pipeline.

## Independently executable #277 adapter

`aikit_adapters::placement_enforcement` provides the internal consumer port
`PlacementOwner::{resolve_and_allocate, validate_write}`, `guard`, `coverage`,
`canonical_write_target`, `claude_pre_tool_response` and
`project_claude_hook`. **These are AIKit Rust ports, not invented Central CLI
operations or a purported published Central wire schema.**

Every guard evaluation resolves the allocation/policy basis again. Native
owner decisions distinguish approved project/source writes from misplaced
scratch; the adapter does not assert that every file belongs under NOW.
Refusals retain an actual valid NOW destination and exact policy revision.
Canonical paths, missing-parent targets, traversal, symlinks and stale owner
revisions are exercised in controlled filesystem tests.

Blocking coverage is restricted to the implemented Claude pre-tool settings-map
adapter when the actual Actuation descriptor advertises matching events and
blocking semantics. A Codex `can_block:false` descriptor is not promoted to veto
coverage. Opaque shell/process writes require a verified material boundary and
cannot borrow structured-file hook coverage. The adapter does not rewrite
arbitrary shell commands. Hook projection retains foreign siblings and retracts
only its owned command.

**Not yet connected:** Central#153's published placement/allocation/write-plan
operations; Workcell#72's verified effective material restriction receipt;
actual hook installation/CAS/retraction through every supported native harness;
mandatory placement admission in the continuous-work dispatch caller. The
plain protocol session caller above is not a claim of task-write confinement.

## Independently executable #276 recurrence

`aikit_core::recurrence::{plan_recurrence, occurrence_delivery_ref}` consumes
owner-resolved instants and exact policy revisions. It has no independent
calendar/timezone interpretation and never turns a Day boundary into an action.
The planner handles disabled schedules, exact-instant skip-missed, latest-only
and bounded catch-up, future occurrences, backwards clocks, duplicate owner
occurrences, mixed/stale policy basis and extreme timestamp arithmetic.

**Not yet connected:** Central#150's native time policy/occurrence reading,
the maintained routine invocation store/attempt dispatch, and #277's fresh
placement/participation enforcement at every effect. This is implemented planner
logic, **not a running scheduler or a completed restart campaign**.

## Exact web-code joins remaining

| Owner | Unfinished operation/join |
|---|---|
| AIKit #274 | Positive source-backed model policy/contract/harness gates, live credential materialisation and catalogue choice into actual native execution; general current-World admission discovery. |
| AIKit #275 | Existing gateway ingest/connector delivery → addressed native session and filtered response egress. Gateway stream append is still not execution. Bounded external sender mapping, response disclosure, service recovery and compaction/relocation context re-admission need integration. |
| Central #153 → AIKit #277 | Publish/consume exact native effective placement policy, idempotent NOW allocation and revision-checked write validation operations. No operation names were guessed from issue prose. |
| Workcell #72 → AIKit #277 | Current verified effective confinement/placement receipt and limitations for opaque process writes and replacement bodies. |
| Central #150 → AIKit #276 | Native time policy, source revision and resolved occurrence operations; then connect routine attempts and actual dispatch with fresh guard admission. |
| Central #152 → headless Return | Receiving/retained artifact inclusion is not implemented by the native session receipt. It remains the Central owner join. |
| Actuation #58 queue | Current authoritative actuality/identity/authority readings beyond supplied-source validation and native instantiation fact/response provenance stay in the existing serial owner lane. AIKit did not start an Actuation branch. |

None of the AIKit-owned gaps above is reclassified as LOCAL PROOF. No ticket is
closed by this cut, and Factory#221 must not treat gateway ingress or
instantiation-record success as an executed development attempt.

## Tests and proof standing

The mandatory `CAW native delivery` workflow builds the actual AIKit binaries,
checks out the pinned Actuation source and invokes its native executable. The
ACP/Pi providers are real child processes implementing controlled protocol
fixtures. Every reply is marked `FIXTURE_REPLY`. They prove the caller, source
bytes, IPC, native host, ordering, duplicate refusal and recovery code—not a
commercial model or installed harness.

Run locally against an exact Actuation source checkout:

```sh
AIKIT_CAW_ACTUATION_BIN=/absolute/Actuation/bin/actuation \
  cargo test --locked -p aikit-cli --test caw_native_delivery \
  -- --ignored --nocapture --test-threads=1
cargo test --locked -p aikit-adapters --test caw_realisation --test caw_enforcement \
  --test model_realisation_end_to_end --test central_entities \
  --test agent_connection_v2 --test agent_session_host_v2
cargo test --locked -p aikit-core --test caw_recurrence --test caw_route_eligibility
cargo test --locked -p aikit-store --test caw_delivery
cargo clippy --locked --workspace --all-targets -- -D warnings
```

### Local proof still required after those code joins

Real installed binary/source parity; actual credential leases and model/harness
responses; selected root and Project context in the installed environment;
recognised personal policy activation; loaded interception with a real denial
and successful retry; Workcell restriction rather than advisory coverage;
persistent gateway/scheduler lifetime across restart/sleep/reboot; cross-Day
continuation and late Return; actual second placement; private source preservation
and human document assessment. No private Control was read or changed by this
repository implementation.
