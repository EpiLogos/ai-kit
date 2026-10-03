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

The invocation `cwd` is an identity-anchored read/material location inside one
of Central's native repository, worktree or NOW grants. It is not itself a
write grant: a registered checkout root remains a valid `cwd` when Central
correctly refuses ambiguous write/remove approval for that root because
`.git`, `.central` or `ProjectCentral` is protected below it. Only the explicit
selected directories and allocated NOW contents enter the Workcell writable
boundary. AIKit retains the `cwd` device/inode/path identity and rechecks it at
configuration, continuation and launch; removal, replacement, a protected or
sibling directory, or changed Central basis refuses before provider start.
Bindings prepared with the former write-destination anchor format require an
explicit reprepare of the same task request rather than silent conversion.

The native write boundary is supported unprivileged Linux Landlock. It protects
the documented regular-file write/create/remove/rename-link/truncate operations
and descendants, not reads, network/delegated services, metadata, privileged
processes or live revocation after launch. Authority/source/lease checks run again
before subsequent turns; they do not revoke a running turn's kernel rules when
policy changes. Unsupported protection fails closed.

## Public owner operations

These are local owner configuration operations, not gateway/IPC mutation input:

```text
aikit session-space -C WORLD encounter-task-configure \
  --agent-session agent-session/example --request-json @task.json \
  [--expected-revision task-binding/CURRENT]

aikit session-space -C WORLD encounter-task-read \
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
`central.work.policy`, `central.now.list`, `central.now.read`,
`central.now.allocate` and `central.work.validate`, then constructs and inspects
the exact native
`workcell.write-boundary/v1` requirements. The record retains the full owner
readings, native path identities, provider, launcher, Agency and task revisions.
`ready:true` means configuration prepared, not execution, completion or Return.

When Central has already allocated this exact Task in the selected scope, AIKit
reads the unique matching NOW and requires the same purpose, ordered participant
and source references, active lifecycle and current source revision. It forwards
that record's immutable work references, parent and Workcell to Central's
idempotent allocation operation. Central derives the child horizon and remains
the final identity fence. This preserves a preallocated child NOW and the
original pending Task request; it neither creates a replacement Task nor reparents
an existing NOW. A changed or ambiguous record refuses preparation. Workcell-root
NOWs require their own native owner operation and are not ordinary Task NOWs.

Use returned `launcher.id` with the existing encounter `open`, `send`, `delivery`,
`read` and `reconnect` operations in [CAW-NATIVE-DELIVERY.md](CAW-NATIVE-DELIVERY.md).
Actual launch rejects a different provider/protocol/source admission/cwd before
creating its process. A new receipt cannot retrofit an old unconfined resident.
`encounter-task-exec` checks actual process cwd and retained revision before
replacing itself with Workcell's native launcher. Central and Workcell control
bearers are removed before provider execution; credential isolation against
hostile same-UID readers remains a separate material/installed requirement.

The embedded Codex ACP profile launches through `npx`. Its package and runtime
cache is derived from this Task's actual Central allocation at
`T/runtime/npm-cache`, then selected after the model environment is scrubbed.
The exact native Workcell inspection must admit the allocated writable subtree
and required filesystem operations. A protected ancestor is compatible with
that explicit subtree; Workcell determines the protection relation. Existing
runtime/cache directories must be canonical directories rather than symlinks.
AIKit creates no cache directory before the material boundary applies and
neither inherits ambient npm cache settings nor widens the Task grant.

The same embedded Codex ACP Task route derives `CODEX_SQLITE_HOME` as
`<actual Task T>/runtime/codex-sqlite`, checks the real canonical directory
chain, and sets it after the final model-environment scrub. No unsandboxed
launcher creates it; native Codex creates absent SQLite material only after
Workcell applies the existing Task aperture. Existing material that is a
symlink or not a directory is refused before provider start.

This route preserves `HOME`, `CODEX_HOME`, the native executable/login binding,
and the Task, Agency and AgentSession identity. Codex's persisted `sqlite_home`
and managed requirements retain their native precedence over the environment;
this change is no permission to override those sources. It does not relocate
Codex's helper aliases, rollout/session history or authentication refresh store.
Codex 0.154 places aliases under `CODEX_HOME/tmp/arg0` (failure warns and
continues), and rollout persistence still uses its actual configuration home.
Its app-server also opens `<CODEX_HOME>/installation_id` with write access
after SQLite initialization, even when the existing identifier is valid.
SQLite placement alone therefore does not establish successful server startup.
A supported no-prompt startup and same-session replay must establish the next
native material requirement before claiming operative continuation; no
credential/config copy, synthetic home, ephemeral session or global home grant
is a fallback.

The CAW delivery gate runs real isolated native Task preparation, actual npm
cache writes and public Codex ACP `--version`, plus production TaskExec redirection
refusal. These cases carry no credentials or prompt. Native account opening,
inference and attributable Factory Return require their separate operational
proof; public package execution and version are not that acceptance.

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

A configured managed service can run the existing `aikit session-space
encounter-serve` owner (in the main `aikit` binary) with its real native socket and AIKit home. Closing the
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

## Reusing an existing Workcell run

`encounter-task-configure` accepts an optional `prepared_run_scope` object:

```json
{"prepared_run_scope":{"run_slug":"selected-run","expected_demand_digest":"sha256:<native run digest>"}}
```

The caller still supplies the existing Central task, selected native Agency
and authority, provider, canonical `cwd`, and selected directories. The cwd
must be the run's already allocated worktree. Configuration does not create a
new worktree and cannot combine this input with `material_host` allocation.
For this path, `workcell_boundary_bin` may be omitted: AIKit resolves `workcell`
and `workcell-write-boundary` from its native process environment, requires the
same installation directory, and retains both canonical executable paths and
the canonical Workcell state root. A renderer cannot select an executable.

AIKit first checks native Agency/task authority and the selected run/demand. It
then allocates or resumes the exact Central NOW, obtains Central's real write
requirements, and asks Workcell to prepare that exact boundary. Workcell
actualises the existing Agency source through Actuation, checks its task Action
and authority, and refuses a changed run or a boundary missing the selected
worktree. The scope retains the run revision, demand digest, admitted Agency
source/digest, native inspection, and selected material identity. AIKit compares
the complete inspection with the actual execution boundary and rechecks it
before provider launch. Changed source, run revision, material path identity,
policy or NOW refuses execution; there is no weaker fallback.

A preparation refusal closes only a NOW newly created by this operation,
through Central's authenticated revision-checked lifecycle action. The failed
task keeps the allocation and cleanup receipt (or the precise unconfirmed
cleanup reason). Existing NOWs, run worktrees and material are preserved. A
closed clearing requires explicit native re-entry before retry; configuration
does not silently reopen it.

`encounter-agency-mint --for-task` requests the task Action in addition to the
ordinary chat Actions. Actuation still judges the request against the unchanged
standing grant and bounds. Omitting the flag retains ordinary chat minting.

The native regression
`native_prepared_run_preserves_authority_and_existing_worktree` in
`crates/aikit-cli/tests/caw_task_dispatch.rs` uses actual Central, Actuation and
Workcell processes and a disposable native material run. It is deliberately
ignored by the generic suite and must be run with the source-built owner paths
in the maintained CAW environment; a generic green suite is not proof of this
joined path.


### Original input and Task runtime material

For the embedded profile-derived Codex ACP/npx Task, AIKit selects native
provider material members; Workcell supplies the additive
`workcell.runtime-projection/v1` / `exec-runtime` operation. Original
HOME/CODEX_HOME remain unchanged. AIKit preserves the selected original
invocation route, qualifying a relative route once against the actual cwd,
alongside the expected canonical input directory. Workcell resolves that
requested route to the same held origin at initial admission, before
material setup, before mounting and at the final assembled-view checkpoint.
An unchanged alias is permitted; retargeting, missing input or actual IO
failure refuses with its native cause. The final checkpoint checks the
assembled view while original lower members remain checked through their
original held fd. These are checkpoints, not atomic exclusion of arbitrary
external writers. The same original canonical input directory is the
readonly lower view. Auth/config members are explicitly immutable and
mechanically disjoint from mutable members. No credentials or config files are
copied. Missing original roots, redirected/nonordinary present immutable members, and unsupported native
namespace/overlay enforcement refuse before provider execution.

The exact admitted Task T backs npm cache, SQLite and the durable
`native-codex-runtime` material. Its selected tmp/log/sessions/archived_sessions/
shell_snapshots views have readonly original lower history and Task-owned
copy-on-write continuations. Its installation_id/history.jsonl/models_cache.json file views also retain
readonly original lower values and Task-owned continuations. These are provider runtime material;
it does not replace an Agent, Agency, canonical AgentSession or provider thread
ID. Same-Task re-entry retains those physical uppers and the same runtime file.
A fresh exclusive readonly skeleton per launch cannot turn retained Task entries
into auth/config input. The enclosing retained runtime directories must actually
be owner-private; no retained user material is silently chmodded.

This operation preserves the existing policy revision/digest, Task fencing,
write aperture and protocol owner. Only privately generated native mounted
aliases receive Landlock rules. Namespace capabilities retire before the body;
no alternate HOME, backend, global write or new supervisor is used. Direct native
projection IO failures retain phase and original kind/errno. Existing Workcell
boundary errors retain their typed owner error (its string-based ABI cannot
recover an earlier erased errno). Both retain body `executed:false`,
possible material setup separately, and `automatic_retry:false`.

This is a Linux-supported provider seam, not kernel/installed readiness proof.
Unsupported Linux user namespaces/overlay or other platforms refuse. Original
managed/persisted sqlite_home/log_dir precedence remains native; an incompatible
explicit path is an actual refusal, not a successful empty startup. External
keyring/network credential operations remain outside filesystem enforcement;
no auth-refresh authority is inferred. File-based auth/config writes are denied.
The actual Original Factory owner must still establish no-prompt startup and
same canonical Session/thread re-entry with current context/auth/model after the
exact Workcell and AIKit source bodies are installed. Controlled filesystem and
EOF cases do not substitute for that replay. All added definitions are UNRUN
under the current local resource hold.

The closed unpublished projection request now requires `requested_input_root`;
draft struct/JSON callers must supply that real selected coordinate. The
runtime filesystem tests use the same existing native bounded capture
owner and retain its actual raw bytes/typed failure observation before
refusing qualification. Eight real Linux definitions (the original five
plus unchanged/retargeted/missing alias controls) are defined, not run.
