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
