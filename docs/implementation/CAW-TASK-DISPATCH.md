# Native task admission and protected resident dispatch

Continuation of #277/#275 through PR #286; parent EpiLogos/O-I#220 and
EpiLogos/Factory#195. This contract connects the existing encounter owner to
Central's actual placement/NOW operations and Workcell's actual protected
protocol launch. It does not introduce another session owner, require a Central
Profile, or turn Direct work into a Factory Commission.

## What the task adds

A task-bound AgentSession retains its actual selected Agency, Central task/NOW
identity and source revisions, exact requested working directory, narrowly
selected writable directories, complete required protection, and original
native provider configuration. The existing ACP/Pi host still launches and
communicates with the provider. Its process is replaced through Workcell's
stdio-preserving `exec` boundary before the provider starts.

Central remains the policy/NOW authority; Workcell remains the material owner.
AIKit neither guesses a task directory nor filters out inconvenient protected
paths. A protected descendant beneath a requested writable ancestor is an
unsupported material request, not permission to discard that protection.

The current native boundary is supported unprivileged Linux Landlock. It protects
the documented regular-file write/create/remove/rename-link/truncate operations
and descendants, not reads, network/delegated services, metadata, privileged
processes or live revocation after launch. Current authority/source/lease checks
run again before subsequent admitted turns; they do not revoke a running turn's
kernel rules when a policy changes. Unsupported protection fails closed.

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
Agency must already be provisioned through `encounter-agency-configure`.
Its actual native Actuation determination must permit both
`action/aikit/encounter-send` and `action/aikit/encounter-task`. The supplied
authority must occur in that determination and the selected Agent must occur in
the Central task participants. Membership or a profile does not grant either
operation. At this cut the native World and Central policy scope must agree
exactly; no alias is invented for an unresolved World relation.

A complete task request is:

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

The normal `EncounterProvider.required_context` may also be supplied. Agent
context is still materialised from the existing exact source/digest admission;
`source_refs` alone do not deliver source bytes or authorise private retrieval.
The task adds its actual NOW/output/cwd/policy relation to the real prompt.

Preparation persists a pending request before Central allocation. It calls
`central.work.policy`, `central.now.allocate`, `central.now.read`, and
`central.work.validate`; then it constructs and inspects the exact native
`workcell.write-boundary/v1` requirements. The retained record includes the
complete owner readings, native path identities, original provider, launcher,
Agency revision and task-binding revision. `ready:true` means this configuration
was prepared, not that a provider ran, a task completed or a Return was received.

Use the returned `launcher.id` with the existing encounter `open`, `send`,
`delivery`, `read` and `reconnect` operations documented in
[CAW-NATIVE-DELIVERY.md](CAW-NATIVE-DELIVERY.md). No new transport is introduced.
The actual launch rejects a different provider, protocol, source admission or
working directory before creating its process. A configuration receipt cannot
retrofit an older unconfined resident. The internal `encounter-task-exec` command
also checks its actual process cwd and exact retained revision before replacing
itself with the native Workcell launcher; it emits no wrapper bytes into the
ACP/Pi stream. Central's native bearer is removed before provider execution.

## Continuation, refusal and history

A changed/withheld policy or NOW source, inactive task, changed Agency, expired
lease, unsupported protection or changed material identity refuses a new effect.
Guard evaluation does not allocate another NOW, renew a lease, reactivate an
archived task or replace a Candidate. Read the current task record and use its
revision for an explicit configuration change. An interrupted pending
preparation may only recover the same request; it cannot disappear into a new
unconfined configuration. Native owner timeouts remain uncertain, not automatic
retry permission.

A task-bound session cannot silently become another task. A revised body is not
represented as opening another view: existing native continuation rules still
apply, including recorded provider/argv/cwd identity. Rebinding a changed body
or expired grant requires the explicit supported new-attempt/re-resolution path;
this cut does not claim that every rehydration or relocation path is finished.

Addressed delivery uses the existing durable reservation and canonical
AgentSession/delivery identity. Replaying the same admitted delivery reads its
receipt rather than sending again. Historical delivery readback remains
available after source withdrawal. Closing a client view does not cancel the
resident owner; native owner shutdown remains a separate operation.

The policy-to-material adapter also emits the native directory-storage declaration
and requirement for the allocated NOW. **Those values are not yet a call to the
persistent Workcell storage/service prepare lifecycle.** The task operation here
executes the real write-boundary path; storage attachment, hosted owner recovery,
Factory attempt consumption and Routine/gateway invocation remain explicit joins,
not implementation inferred from those emitted values.

## Executable proof and limits

The existing `CAW native delivery` workflow pins native Central, Workcell and
Actuation and builds actual product binaries. It executes:

```sh
cargo test --locked -p aikit-adapters --test caw_native_placement \
  -- --ignored --nocapture --test-threads=1
cargo test --locked -p aikit-cli --test caw_task_dispatch \
  -- --ignored --nocapture --test-threads=1
```

The tests require `AIKIT_CAW_CTRL_BIN`, `AIKIT_CAW_WORKCELL_BOUNDARY_BIN` and
`AIKIT_CAW_ACTUATION_BIN`. The task campaign uses an actual controlled ACP child,
checks selected context and actual cwd, attempts prohibited writes, writes into
authorised source and native T, receives an attributed response, and tests
alternate-provider refusal, duplicate delivery, source withdrawal, removed NOW,
wrong cwd and missing authority. The positive marker is emitted only after the
actual native protected dispatch assertions pass. Existing ordinary no-task
ACP/Pi delivery cases remain in the same workflow.

`CONTROLLED_NATIVE_RETURN` is protocol/kernel integration evidence, not a real
model, installed harness or human assessment. A provider response and T artifact
are not Central reviewed receiving/inclusion, independent verification,
Recognition or whole-feature completion. Consult the exact CI run for executed
standing; adding this document or a test does not confer a passing result.

The full programme still retains P01–P28, Candidate worktree/content/test/diff
basis, environment continuity, assisted commissioning, Candidate comparison,
receiving/document semantics, recurrence, installed/material acceptance and
fresh independent full-feature verification.
