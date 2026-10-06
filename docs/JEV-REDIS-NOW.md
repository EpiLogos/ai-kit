# Jev + Redis NOW operative context

This source documents the native AIKit implementation for O:I #65 / AIKit #388.
It is an operator surface for the existing owners, not a new source registry,
Wiki, workflow store, scheduler or memory ontology.

## Ownership

- Central remains the root meta-Project and owns durable source identity, NOW/source lifecycle and BKMR-backed source location.
- AIKit resolves source/Wiki/practice/Factory relevance, invokes Jev when selected, prepares participant-specific operative context and delivers it through the native encounter path.
- Workcell owns Redis as target material (process, persistence, health, resource bounds, lifecycle). Redis does not own semantic NOW state.
- Factory owns Run/Journey/workflow/participant/dependency/Return meaning. AIKit reads its native CLI surface and pins the exact Run/Journey basis used for preparation.
- Actuation remains the owner of acting Agency, Activity, invocation/usage/evidence attribution and Return. AIKit's Jev invocation receipt carries stable invocation and usage evidence for that relation.

## Commands

General Jev access:

```sh
aikit --json jev invoke \
  --request-file jev-request.json \
  --limits-file jev-limits.json \
  --credential-ref 'keychain://...' 

aikit --json jev validate \
  --request-file jev-request.json \
  --response-file jev-response.json
```

Prepared NOW context:

```sh
aikit --json now-context status --config-file redis-now.json
aikit --json now-context prepare --request-file prepare.json
aikit --json now-context inspect \
  --config-file redis-now.json \
  --participant-ref participant/factory/implementer
aikit --json now-context append-change \
  --config-file redis-now.json \
  --participant-ref participant/factory/implementer \
  --change-file change.json
aikit --json now-context revoke \
  --config-file redis-now.json \
  --participant-ref participant/factory/implementer \
  --disclosure-revision REV
```

`publish` exists for owner-resolved prepared views and requires an explicit
`--expected-version` CAS basis. Normal configured encounter entry uses the
automatic preparation path; a worker is not required to remember a planning
tool call.

## Local Redis without Workcell: `aikit now-context service`

AIKit connected to Redis but never started it; the process, reference
configuration and health came from Workcell's declared services. An
installation without Workcell now has the lifecycle from AIKit, on the same
core as `aikit decide service` (recorded process identity, identity-checked
stop, foreign-listener refusal, rollback on upgrade). No verb discovers, runs
or requires `workcell` or `factory`.

```sh
aikit now-context service provision [--port 6381] [--maxmemory-mb 256] [--redis-server ABS]
aikit now-context service start | status | stop | restart
aikit now-context service upgrade --redis-server ABS
```

- `provision` checks `redis-server --version` (reference series ≥ 8.10), hashes
  the executable, and generates `redis.conf` (loopback, `appendonly yes`,
  `appendfsync everysec`, finite `maxmemory`, `maxmemory-policy noeviction`) and
  the `aikit.redis-now-config/v1` election `redis-now.json` in
  `<AIKIT_HOME>/services/redis-now`.
- `start` is ready only when PING answers **and** the live `INFO` reading
  conforms to the profile (AOF on, finite maxmemory, noeviction, ≥ 8.10); a
  non-conforming Redis is a named failure, not a success. A Redis this service
  did not start on the same port is reported `foreign-listener`, never adopted.
- `stop` sends TERM (Redis flushes its append-only file) and keeps the data;
  `restart` and `upgrade` reuse the same data directory. `upgrade` refuses an
  older-series executable before touching the running service, and returns to the
  previous executable if the new one does not come up conforming.
- Nothing flushes or deletes data.

### Prepared context on provider rows, and what a turn actually received

A configured provider row carries `now_context` only if something sets it.
`aikit-session-space encounter-now-context-configure --provider-id ID
--redis-config redis-now.json [--prepare-request REQ] [--required]` sets (or with
`--withdraw` removes) that one field on any stored row (Pi, Prime, other),
validates the preparation request against the Redis election at configuration,
and reports what it elects (`selection.mode`: `none`, `all`, `jev`, `provider`).
A request with `selection.mode = provider` and the `decision-provider.json`
written by `aikit decide service` makes the local Kev rank the candidates before
the first turn.

Observability: each turn that carried prepared context appends a
`now-context-delivered` journal event whose receipt records the prepared
version, digest, basis digest and change cursor, **and** the decision-provider
identity digest and decision invocation behind the view's selection (read from
the delivered view's own basis). The encounter view returns the latest of these
as `now_context_receipts` beside `prepared_context_receipts`, including
`now-context-degraded` and the two `…-uncertain` kinds. A turn that carried none
therefore shows none.

Provider selection only offers Kev candidates whose `external_egress` is
`allowed` and `agent_visibility` is `payload` (the Jev rule), even when the
elected provider is a local loopback one. Central-read sources default to
egress `denied`, so they are withheld from a local Kev as well; relaxing that
for a `local-protocol` standing is a law change for the owner, not made here.

## Redis material configuration

```json
{
  "schema": "aikit.redis-now-config/v1",
  "address": "127.0.0.1:6381",
  "database": 0,
  "key_prefix": "aikit-now",
  "username": null,
  "credential_ref": null,
  "allow_remote": false,
  "connect_timeout_ms": 1000,
  "io_timeout_ms": 1000,
  "prepared_ttl_seconds": 21600,
  "coordination_retention_seconds": 1209600
}
```

The built-in Workcell Redis NOW profile is loopback-only and requires
persistent AOF, finite maxmemory and `maxmemory-policy noeviction`.
Remote Redis is an explicit operator-owned service boundary with ACL/TLS rather
than an implicit widening of the reference profile. Setup and tests must never
flush an existing database.

## Preparation request

```json
{
  "schema": "aikit.now-preparation-request/v1",
  "redis": { "...": "aikit.redis-now-config/v1 fields" },
  "project_ref": "project/example",
  "now_ref": "now/example",
  "participant_ref": "participant/factory/implementer",
  "agent_session": "agent-session/example",
  "concern": "Implement the bounded Factory change",
  "disclosure_revision": "policy-revision",
  "practice_refs": ["skill/aikit/operation"],
  "central": {
    "root": "/resolved/Central",
    "project": "example",
    "ctrl_bin": "/resolved/bin/ctrl",
    "source_refs": ["source/example"]
  },
  "matrix": {
    "manifest": "/resolved/ProjectCentral/user/telos/capability-matrix.json",
    "csv": "/resolved/ProjectCentral/user/telos/capability-matrix.csv",
    "view_id": null,
    "capability_refs": [],
    "full_scope": true,
    "agent_visibility": "payload",
    "external_egress": "allowed"
  },
  "factory": {
    "state": "/resolved/factory-state",
    "run_ref": "run/example",
    "factory_bin": "/resolved/bin/factory",
    "workflow_unit_refs": ["workflow-unit/example"]
  },
  "wiki_queries": ["relevant capability relation"],
  "candidate_items": [],
  "continuation": null,
  "expected_version": 0,
  "external_provider": true,
  "allow_redis_env_import": false,
  "selection": { "mode": "all" }
}
```

For Jev-assisted selection, `selection` uses mode `jev` with a native
credential reference, bounded `JevLimits`, caller-supplied state and optional
relevance threshold. Direct agent-formulated Jev questions remain available
through `aikit jev invoke`; document/Wiki selection is one application of the
same protocol.

The matrix carriers live in the project's telos folder (`ProjectCentral/user/telos/`, the integrated day/now/telos field placement); the optional `matrix` block reads the existing `ql-capability-matrix/1`
manifest and CSV directly. Choose exactly one scope: `full_scope: true` or an
explicit `capability_refs` list. Full scope enumerates every declared
capability before relevance selection. The prepared candidate retains the
view title/semantics, ordered row/column axes, whole-account anchor,
need/operation/outcome, implementation status, standing, source/account routes
and relation questions. Manifest and CSV digests join the preparation basis, so
a matrix edit while Jev is running refuses the late publication. Egress is an
explicit input because a public matrix and a private authored matrix are not
the same disclosure boundary.


## Delivery and invalidation

A prepared view retains exact source/dependency/disclosure/Factory basis,
participant identity, SessionSpace/AgentSession relation, source-linked
Knowledge frames, neighbouring work, continuation and the Jev invocation ref
when selection used Jev.

Before external-provider disclosure, cached payload is revalidated for current
source standing and egress. Revocation removes the participant's prepared
payload at the disclosure boundary. Late preparation cannot replace a newer
version: publish is compare-and-swap against the expected version and the
native source/Factory basis is re-read before publication.

The encounter provider configuration may select NOW preparation. On first
entry/re-entry/continuation, AIKit prepares or reads the participant view before
the provider turn, adds the prepared context to the supported encounter
payload, and records the prepared version/digest actually accepted for
delivery. Selected-but-unavailable Redis/Jev is surfaced as degradation rather
than represented as an enhanced success.

Change delivery is participant-specific and replayable. Source/dependency/
Return changes append to independent participant cursors; one participant
cannot consume another participant's only notification. Heartbeats and warm
cache reads do not require Jev inference.

## Local decision models (optional, local-first)

`action/model/decide` is provider-neutral: the typed Noul/Choice/Score
questions and the strict answer validation do not belong to one vendor. The
hosted TypeSafe/Jev provider keeps its own law at its transport (concrete
`jev-…` versions, native credentials, finite tariff reservation). Local and
self-hosted endpoints speak the same shape under a different standing, with
the laws an operator-owned endpoint actually has: an explicitly
unauthenticated local server needs no invented API key, no fabricated tariff
exists because there is no price source, and usage is still required on every
answer.

### The election

Exactly one placement is elected through an `aikit.decision-provider/v1`
configuration:

| mode | placement | notes |
|---|---|---|
| `none` | no decision service | the ordinary path; unrelated work never requires one and never falls back to hosted inference on its own |
| `managed-local` | a loopback model service whose lifecycle the Workcell product owns where it is installed | the serving process, material, health, restart and cleanup belong to Workcell's declared services; the election itself never requires a `workcell` executable |
| `endpoint` | a SystemOne-compatible endpoint | beyond loopback this requires an explicit `allow_remote` election and HTTPS; also the honest placement of the local service AIKit provisions itself (below) |
| `hosted` | the TypeSafe/Jev API | unchanged law: native credential, `JevLimits`, concrete returned version |

The decision provider is independent of the acting (coding/writing) models:
it is never resolved from the model roster, and acting-provider disclosure
does not cover it. Electing local serving of private text is not an election
to send that text to a cloud worker; an Omarchy tunnel keeps its remote
placement identity.

### Commands

```sh
aikit --json decide status \
  --provider-file decision-provider.json          # actual placement, selected model,
                                                   # install/load state, license, card
aikit --json decide status \
  --provider-file decision-provider.json --probe  # + one real bounded Noul diagnostic
aikit --json decide invoke \
  --provider-file decision-provider.json \
  --request-file jev-request.json
```

`aikit jev invoke` and `aikit jev validate` are unchanged. In a NOW
preparation request, `selection` may use `mode: "provider"` with an explicit
`provider_file`, caller-supplied `state` and an **explicit**
`relevance_threshold` — thresholds are never silently copied from one
provider's calibration to another's. The elected provider's identity digest
(mode, standing, model, artifact identity) joins the prepared basis, so a
provider, runtime or calibration change moves the basis digest and cannot
present cached decisions as the same basis; canonical source revisions and
NOW state are unaffected.

### First local recipe: Kev-0.8B via the existing `/v1/systemone` shape

[jaredpalmer/kev](https://github.com/jaredpalmer/kev) (Apache-2.0) is a family
of Jev-like decision models serving the same typed protocol on loopback. The
evaluated candidates on this ground's Apple M4 (16 GB): **Kev-4B** (balanced;
~8 GB resident weights in bf16, model card targets 32 GB Macs) and
**Kev-0.8B** (low-resource; ~1.7 GB resident). **Kev-0.8B is what fits this
machine's honest headroom, so it is the only model the recipe installs**; the
transport is model-agnostic, so a larger self-hosted model is admitted later
by re-electing it, not by silently downloading weights.

Pins for the installed cut: upstream `5920c5fe4ca8e0970ed4209ac2c9b8e18bea5109`,
adapter `jaredpalmer/kev-0.8b`, base `Qwen/Qwen3.5-0.8B-Base @ dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68`,
mlx-lm `>=0.31.3,<0.32` on Metal, backbone as stored (bf16), fp32 pointer
head, checkpoint-fitted temperature. `scripts/decision-local/install-kev.sh`
installs and records SHA-256s in a material manifest;
`scripts/decision-local/kev-decision-service.example.json` declares the
serving process through Workcell's existing declared-services path (loopback
bind, readiness probe, idempotent start, owned stop); starting runs with
`HF_HUB_OFFLINE=1`, proving cached weights can start and answer without
outbound network. The launcher also carries the serving optimizations this
recipe depends on: `KEV_PREFIX_CACHE=8` (the knowledge-driven pattern repeats
one state across many questions; upstream measures ~5x on repeat state) and a
warm-at-start packed Noul/Choice/Score request, so Metal kernel compilation
happens at start and "started" means ready-at-speed. The input ceiling
(16 384 tokens) admits packed selections while staying near the checkpoint's
trained state envelope (~7.5k tokens): keep the shared state lean and put
per-candidate detail in the question entries — small models lose accuracy on
long states, so narrowing scope beats fattening the state.

### Local lifecycle without Workcell: `aikit decide service`

An installation that does not have Workcell gets the complete Kev lifecycle
from AIKit itself. Nothing in these verbs discovers, runs or requires a
`workcell` or `factory` executable, and the generated election is mode
`endpoint`, never `managed-local`.

```sh
aikit decide service provision [--port 8019] [--service-dir DIR] [--recipe-file R]
aikit decide service start     [--ready-timeout-secs 600] [--no-warm]
aikit decide service status    [--verify-material] [--probe]
aikit decide service stop      [--grace-secs 15]
aikit decide service restart
aikit decide service upgrade   [--recipe-file R] [--force]
```

- The service directory defaults to `<AIKIT_HOME>/services/decision/kev-0.8b`
  and holds `service.json` (state), `decision-material-manifest.json`
  (SHA-256 of every pinned artifact), `decision-provider.json` (the `endpoint`
  election to pass to `aikit decide`/`now-context`), `service.log`, the pinned
  upstream checkout and its environment.
- The recipe pins upstream, adapter and base by full revision (the adapter by
  revision, where the scripts only named the repository). A branch or tag is
  refused.
- `start` adopts only a process this service started (pid + start time +
  command line recorded). A listener on the same port that it did not start —
  for example Workcell's own Kev — is reported as `foreign-listener`, never
  adopted, stopped or restarted. Ready means the pinned model card answers
  (name, run and base must match the recipe) and one real warm decision
  completed through the elected provider.
- `stop` is identity-checked TERM then KILL and idempotent. `upgrade` moves to a
  different pinned recipe and returns to the previous cut, restarting it if it
  was running, when the new cut cannot be provisioned or does not come up
  healthy; the receipt says whether the rollback held.
- The service process does not inherit `WORKCELL_*` variables.
- Redis has its own verbs on the same lifecycle core (below).

### Meaning, disclosure and evaluation

- Probabilities come from the endpoint's own calibration; none are invented
  from label-only answers, and a local model result grants no authority and
  supplies no human Recognition.
- Mandatory sources are never placed under optional semantic pruning: a
  provider selection only ranks egress-approved candidates, exactly as the
  Jev selection always has, and a full-scope assessment is never silently
  truncated.
- No mandatory LLM approval call is added after permitted judgments; the
  ordinary path (selection `all`) remains the default and needs no decision
  service at all.
- Decision-provider disclosure is evaluated separately from acting-provider
  disclosure: a selection receipt records the elected decision identity, not
  an acting model's.
- A fresh installation without weights stays fully usable: mode `none` (or an
  elected-but-unavailable local service, which is a visible failure) never
  becomes a silent cloud fallback, and disabling the service is separate from
  deleting its weights.

## Cloud acceptance

`.github/workflows/jev-redis-now.yml` exercises:

- strict general Jev protocol and controlled HTTP transport;
- bounded retries/cancellation/spend reservations and incomplete-answer failure;
- real Redis 8.10 with persistent AOF/no-eviction settings;
- Redis restart over the same data directory;
- native `aikit jev` and `aikit now-context` command discovery;
- prepared-context CAS, cursor/replay, revocation and disclosure isolation;
- prepared context crossing the native encounter boundary before the turn.

Live Jev/model-provider behaviour remains a distinct environment observation:
production code and controlled protocol tests do not impersonate a live
credentialed provider episode.


## External protocol and owner basis checked for this cut

Checked against current public provider/owner sources on **22 September 2026**:

- TypeSafe's published System One OpenAPI surface is `POST https://api.typesafe.ai/v1/systemone` with Bearer authentication and typed Noul / Choice / Score questions. Responses are admitted only when the requested answer set is complete and the provider supplies the actual returned `model` and `usage`. The public model-list route remains `GET /v1/models`.
- TypeSafe's public Jev launch material states the launch input tariff as **$0.042 per million input tokens** with output free. AIKit records the exact tariff source/model in `JevLimits`, reserves finite worst-case spend before an attempt, and refuses to apply a tariff to a different returned model. The controlled cloud proof requests/returns `jev-1.13.0`; that is controlled protocol evidence, not a claim about which live model an installed credential will receive.
- Redis material is consumed through Workcell's merged native owner slice, Workcell PR #98 / merge `9d627ff4d90de2cddf237b14f9854a64e1b5819e`. The reference profile targets the Redis OSS **8.10** series with AOF persistence, finite maxmemory, `noeviction`, loopback binding and explicit recovery/cleanup law.
- Central's document/capability carrier is the existing `ql-capability-matrix/1` protocol. NOW preparation reads the declared manifest + CSV directly, retains the view question/axes/account links, supports either a bounded selected capability inventory or full declared scope, and revalidates both carrier revisions before publication. No second matrix registry is introduced.
- Factory meaning comes from its current developmental Run/Journey/WorkflowUnit readers. Actuation attribution comes from its existing durable ActuationStream, model-usage and Activity contracts. These owner records remain authoritative; Redis retains only the operative projection.

The installed live-provider episode must record the actual provider-returned model, usage and effective local service versions again. These dated external checks establish the implementation basis; they are not a substitute for that live observation.

## World projection (`world` family)

Beside each participant's prepared view, the `world` family holds one
`aikit.world-projection/v1` per Position (or per AgentSession when no Position
resolved): the refs, revisions, cursors and digests of the joined
`aikit whoami` reading — World, Project World, Position revision, occupant
generation, Agent/Agency/AgentSession/SessionSpace, Workcell, root/child NOW
revisions, current-work refs and digest, Return destination, prepared-view
version and each peer's occupancy word — plus each facet's state and one-line
summary. It never holds spec, Wiki or NOW bodies.

Keys are `{prefix}:world:{blake3(subject)}` and `{prefix}:world-meta:{blake3(subject)}`.
Publication is compare-and-swap exactly like the prepared view: the new
version must be the stored version plus one, and a late writer is refused as
stale. The projection's `identity_digest` must match its identity or it is
refused on write and read.

```sh
aikit whoami --publish --redis-config redis-now.json   # live joins, then CAS publish
aikit whoami --hot --redis-config redis-now.json       # projection first (age + basis), live fallback
aikit whoami --rebuild --redis-config redis-now.json   # recompute from owners, republish
```

`AIKIT_WORLD_REDIS_CONFIG` may name the configuration instead of the flag; an
encounter provider's `now_context` block is accepted as well. Losing the
projection loses no identity: every value is recomputed from its owner, and
the tests prove publish → delete → rebuild yields identical identity refs.

## Joined proof

The reusable runner is:

```sh
python3 scripts/jev-redis/joined_proof.py --help
```

The hosted joined job builds the candidate AIKit plus **current Central, Factory and Actuation owner mains**, installs real **bkmr 7.6.7**, starts real **Redis 8.10**, and runs one disposable Factory undertaking through:

1. ordinary owner discovery;
2. Redis-prepared context with no Jev selection;
3. Jev-assisted preparation using the production transport's explicitly controlled loopback standing.

The episode registers ordinary source plus the current Central account and capability-matrix manifest/CSV through Central's real file-map/BKMR route. Its matrix arm uses AIKit's native matrix reader and accounts for the complete declared capability inventory in this bounded fixture, rather than passing a hand-shaped catalogue to Jev.

It also proves distinct implementer/related-worker/verifier readings, verifier-private isolation, related Return/change delivery, fresh-session continuation, stale CAS refusal, source change while Jev is in flight, malformed-provider refusal, disclosure revocation, Redis-unavailable failure followed by persisted restart, durable Wiki/practice Return, later-participant reuse, and Jev invocation/usage attribution through Actuation. A fresh runner independently reviews the joined evidence artifact.

The controlled comparison deliberately leaves worker-model performance, human corrections and live TypeSafe behavior unavailable. The same runner is the installed-world/live-provider acceptance runner; local integration replaces the controlled endpoint with the native real credential reference and supplies the admitted Agency/Actuation/session refs without changing the fixtures or acceptance logic.
