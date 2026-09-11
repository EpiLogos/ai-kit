# Native task admission and protected resident dispatch

Continuation of #277/#275 through PR #286 and dependent #291; parent
EpiLogos/O-I#220 and EpiLogos/Factory#195. The existing encounter owner consumes
Central's actual placement/NOW operations and Workcell's actual protected
protocol launch and persistent storage/service operations. No second session
owner, mandatory Central Profile or Factory ancestry is introduced.

## What the task adds

A task-bound AgentSession retains its selected Agency, Central task/NOW identity
and source revisions, exact requested working directory, narrowly selected
writable directories, complete required protection, and original native provider
configuration. The existing ACP/Pi host launches and communicates with the
provider. Workcell's stdio-preserving `exec` applies the required material boundary
before the provider starts, without putting wrapper text into the protocol.

Central remains the policy/NOW authority; Workcell remains the material owner.
AIKit neither guesses a task directory nor filters inconvenient protected paths.
A protected descendant beneath a requested writable ancestor is an unsupported
material request, not permission to discard that protection.

The native write boundary is supported unprivileged Linux Landlock. It protects
the documented regular-file write/create/remove/rename-link/truncate operations
and descendants, not reads, network/delegated services, metadata, privileged
processes or live revocation after launch. Authority/source/lease checks run again
before subsequent turns; they do not revoke a running turn's kernel rules when
policy changes. Unsupported protection fails closed.

## Public owner operations

These are local owner configuration operations, not gateway/IPC mutation input:

```text
aikit-session-space -C WORLD encounter-task-configure \
  --agent-session agent-session/example --request-json @task.json \
  [--expected-revision task-binding/CURRENT]

aikit-session-space -C WORLD encounter-task-read \
  --agent-session agent-session/example
```

The canonical SessionSpace/AgentSession must already exist and the selected
Agency must be provisioned through `encounter-agency-configure`. Its actual native
Actuation determination must permit `action/aikit/encounter-send` and
`action/aikit/encounter-task`. The supplied authority must occur in that
determination and the selected Agent in the Central task participants. Membership
or a profile grants neither operation. At this cut the native World and Central
policy scope must agree exactly; no alias is invented for an unresolved relation.

A complete protected-process task request is:

```json
{
  "central": {
    "ctrl_bin": "/absolute/ctrl",
    "central_root": "/absolute/World",
    "project": null,
    "task_ref": "task:example",
    "purpose": "The bounded work to perform",
    "participant_refs": ["agent:selected"],
    "source_refs": ["source:selected-basis"]
  },
  "provider": {
    "id": "native-body",
    "label": "Explicitly configured native body",
    "protocol": "acp",
    "argv": ["/absolute/provider", "--stdio"]
  },
  "cwd": "/absolute/World/Work/project/src",
  "selected_directories": ["/absolute/World/Work/project/src"],
  "workcell_boundary_bin": "/absolute/workcell-write-boundary",
  "authority_ref": "authority:existing-task-grant"
}
```

`EncounterProvider.required_context` may also be supplied. Agent context is still
materialised from the exact source/digest admission; `source_refs` alone do not
deliver bytes or authorise private retrieval. The task adds actual NOW/output/cwd/
policy relations to the provider prompt. Neither allocation nor configuration
commissions a Factory Run, invokes a model or records human Recognition.

Preparation persists a pending request before Central allocation. It calls
`central.work.policy`, `central.now.allocate`, `central.now.read` and
`central.work.validate`, then constructs and inspects the exact native
`workcell.write-boundary/v1` requirements. The record retains the full owner
readings, native path identities, provider, launcher, Agency and task revisions.
`ready:true` means configuration prepared, not execution, completion or Return.

Use returned `launcher.id` with the existing encounter `open`, `send`, `delivery`,
`read` and `reconnect` operations in [CAW-NATIVE-DELIVERY.md](CAW-NATIVE-DELIVERY.md).
Actual launch rejects a different provider/protocol/source admission/cwd before
creating its process. A new receipt cannot retrofit an old unconfined resident.
`encounter-task-exec` checks actual process cwd and retained revision before
replacing itself with Workcell's native launcher. Central and Workcell control
bearers are removed before provider execution; credential isolation against
hostile same-UID readers remains a separate material/installed requirement.

## Persistent material and the actual encounter host

To require persistent Workcell material, add this field to the same task request:

```json
{
  "material_host": {
    "workcell_bin": "/absolute/workcell",
    "endpoint": "127.0.0.1:7400",
    "workcell_ref": "workcell:explicit-local-host",
    "demand_ref": "demand:task-example-attempt-1",
    "required_services": ["service:actual-encounter-owner"],
    "encounter_service": "service:actual-encounter-owner"
  }
}
```

Omitting `material_host` retains explicitly unhosted protected-process operation;
it is never disclosed as persistent Workcell hosting. A configured requirement
cannot be removed by passing an unhosted replacement request. `encounter_service`
is optional when only persistent storage/other services are required, and must
be among `required_services` when supplied. Only that explicit relation claims
the actual encounter owner is hosted by Workcell.

The selected endpoint is authenticated by Workcell's existing control protocol.
Its native `status` must name the selected Workcell. This local-resident adapter
requires a loopback endpoint and same-object directory observation; it does not
pretend that an arbitrary remote service is the local resident's host.

The actual Central NOW must be declared at the selected Workcell using the native
`workcell.directory-storage/v1` owner configuration. The adapter exposes the exact
Central-derived declaration; it does not guess the path or rewrite a host's
configuration. Managed/target-owned services likewise use the existing native
Workcell service declarations. In a disposable campaign these declarations are
explicit controlled setup; they are not private governance adoption. Dynamic
registration and physical remote placement are not inferred from this operation.

AIKit constructs a native `ExecutionDemand` with required writable/shared,
external/preserve NOW storage, explicit required services, and exact opaque
Agent/Agency/WorldBinding/session/task/NOW/source/policy/authority subjects. It
calls real native `prepare`, retains the material world and demand, then calls
`inspect` and `observe`. All required bindings must actually be present/healthy;
storage must match the exact path and filesystem object, not a same-spelled path.

Every launch and turn repeats native inspection and provider observation. A
released, superseded, replaced, absent or unreachable required binding refuses
further dispatch. A healthy unrelated service cannot stand in for the encounter:
when `encounter_service` is selected, its fresh native material binding must name
the actual encounter owner's process. This check runs in the owner, not in a
configuration client or its protocol child. Workcell's native provider still owns
process-identity validation and liveness; the consumer does not invent those facts.

A configured managed service can run the existing `aikit-session-space
encounter-serve` owner with its real native socket and AIKit home. Closing the
client does not cancel that service. Preparing it is not the same as proving its
semantic readiness: successful native open/send/response is the operation proof.

## Continuation, refusal and history

Changed/withheld policy or NOW source, inactive task, changed Agency, expired
lease, unsupported protection or changed material refuses another effect. Guard
evaluation does not allocate another NOW, renew a lease, reactivate archived work
or replace a Candidate. Explicit reconfiguration uses the current task revision.
Every replaced pending/ready task record is retained as exact owner-private
history before publication, including its material, NOW/source and request basis.

Incomplete preparation may recover only the same request. The native Workcell
demand key owns idempotency and uncertainty; AIKit does not mint another demand to
hide unknown effects. Missing replies/timeouts remain uncertain. A released or
superseded world cannot automatically masquerade as a resumed body. Native
Workcell recover/inspect and explicit task/body re-resolution remain separate;
full hosted-body rehydration and relocation are not claimed complete by storage
host restart. Historical receipts and source bytes remain retained.

A task-bound session cannot silently become another task. A new body is not merely
opening another view: existing native continuation rules, recorded provider/argv/
cwd identity and exact task revision still apply. Not every provider supports
rehydration, compaction or relocation. Current Direct work needs no Factory state.

Addressed delivery retains the existing durable reservation and canonical
AgentSession/delivery identity. An admitted replay reads the same receipt rather
than sending again. Historical delivery readback remains available after source
withdrawal or material release. Releasing storage detaches it through Workcell;
it does not delete the Central source, T artifacts or pending Return.

## Executable proof and limits

The existing `CAW native delivery` workflow pins native Central, Workcell and
Actuation and builds real product binaries. It runs original native ACP/Pi
scenarios, three placement cases, and the maintained task test target:

```sh
cargo test --locked -p aikit-adapters --test caw_native_placement \
  -- --ignored --nocapture --test-threads=1
cargo test --locked -p aikit-cli --test caw_task_dispatch \
  -- --ignored --nocapture --test-threads=1
```

Inputs are `AIKIT_CAW_CTRL_BIN`, `AIKIT_CAW_WORKCELL_BOUNDARY_BIN`,
`AIKIT_CAW_ACTUATION_BIN` and, for persistent material,
`AIKIT_CAW_WORKCELL_BIN`/`AIKIT_CAW_WORKCELL_SERVICE_BIN`. Tests use actual native
control/encounter processes and an actual controlled ACP child. No commercial
model or personal credentials are required.

The original task cases assert actual context/cwd, prohibited-write denial,
authorised source/T output, attributable response, alternative-provider refusal,
duplicate delivery, source withdrawal, removed NOW, wrong cwd and missing authority.
The material cases add exact attachment, control-host restart without duplicate
work, continued admitted turns, release/old-result preservation, absent/foreign
host refusal, forbidden requirement removal, actual Workcell-hosted encounter,
and refusal when a different healthy process is falsely named as that owner.
The workflow requires positive markers emitted only after real dispatch assertions;
a skipped target, missing connection or constant success receipt cannot pass.

`CONTROLLED_NATIVE_RETURN` is protocol/kernel integration evidence, not a real
model, installed harness or human assessment. T output is not Central reviewed
receiving/inclusion, independent verification, Recognition or full-feature
completion. Consult the exact CI run for executed standing; a documented test
is not a claim that it passed.

Factory attempt consumption, full body-loss recovery, catalogue-to-resident model
selection, gateway/Routine invocation and reviewed receiving remain distinct
production joins. The full programme retains P01–P28, exact Candidate/worktree/
source/test/diff/NOW basis, environment continuity, assisted commissioning,
Candidate comparison, documents, recurrence, installed/material acceptance and
fresh independent full-feature verification.
