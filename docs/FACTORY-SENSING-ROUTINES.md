# Project Factory sensing through Routines and Redis

Factory owns the policy, signals, coverage, work relations and read-only
`factory.telemetry-field/v1` result. AIKit's gateway dispatches a proven native
Method and caches that bounded field under one ProjectWorld key in the existing
Redis NOW service. Deleting Redis does not delete a signal or its disposition;
the next field refresh rebuilds the projection from Factory.

A project-local Method capsule uses `aikit.native-method/v1` metadata. Its
`body` is `factory-collect` for scheduled intake, or
`factory-field-refresh` for a cheap owner read after a relevant change. The
required `actions` are `factory:action/telemetry.collect` and
`factory:action/telemetry.field` for collection, or only the latter for
refresh. The `[metadata.native-method.factory]` table binds the absolute
`state` and `policy` paths plus `project_world_ref`. These values are part of
the Method revision and its proof, so a Routine cannot silently change its
Project or policy file at dispatch. The policy file itself remains
human-authored and grants no action authority.

```toml
[metadata.native-method]
schema = "aikit.native-method/v1"
body = "factory-collect"
actions = ["factory:action/telemetry.collect", "factory:action/telemetry.field"]

[metadata.native-method.factory]
state = "/absolute/project/.factory/development-state.json"
policy = "/absolute/project/ProjectCentral/user/factory-policy.json"
project_world_ref = "project:Example"
```

Create the project capsule as an AIKit Skill with a `METHOD:` description in
the project's configured source registry, then prove it with `aikit method
prove`. `aikit routine create` accepts a native time schedule and an authority
whose `action_refs` grant only the declared Factory actions; `aikit routine
enable` stores the admitted authority. `aikit routine show` confirms the
schedule, Method revision and gateway binding. `aikit gateway tick` or the
running gateway service dispatches the due operation. The native run receipt
under `$AIKIT_HOME/state/routine-native-runs/` records the exact project,
Factory command outcomes, source revision and Redis publish version.

The Project policy must enable `workflows.collect` and
`workflows.field-refresh` with schedules matching the saved AIKit Routine:
`every:<milliseconds>`, `cron:<five fields>`, or `daily:<HH:MM>`. A disabled
workflow, changed cadence or policy edit during a run refuses publication.

For change-triggered refresh, enable a companion Event Routine using the same
bound `factory-field-refresh` Method and only
`factory:action/telemetry.field` authority. Its trigger is exact to the
ProjectWorld:

```json
{"kind":"event","event_ref":"aikit.routine-event/v1:factory:field-changed:project:Example"}
```

Every gateway tick reads that Project's bounded native Factory field with a
five-second probe limit. Factory's field cursor covers its current sensing,
custody, Attempt and work state, plus the bounded native Position occupancy
reading. A changed cursor, or a missing Redis projection, passes a scoped
observation through the existing Routine proof and authority gate. The native
run requires exactly one enabled scheduled refresh Routine on that same Method
revision and checks its saved cadence against current policy before reading
Factory again and publishing with Redis compare-and-swap. The
scheduled refresh remains enabled at the policy's exact cadence as a recovery
fallback. Failed publication and Redis loss are retried on the next gateway
tick bucket; an unchanged owner/hot cursor dispatches nothing. A disabled
companion or disabled policy stops change dispatch.

The runner reads the Project's Redis version **before** running Factory or
reading its field. It requires Factory's field schema, project, source
revision, sensing sequence and observation time, then publishes with
compare-and-swap. The Redis metadata refuses a lower sensing sequence or an
observation time that has not advanced, including a version collision after
Redis loss. A later publisher cannot overwrite a newer projection with an
older read. Read one current projection
with:

```text
aikit now-context factory-sensing --config-file /absolute/redis-now.json \
  --project-world-ref project:Example --json
```

`available: false` means no hot projection exists; it does not mean Factory
has no signals. A failed Redis or Factory read fails the Routine with a stage
and owner error in its receipt. Recovery reads the native Factory field again
and republishes it; no whole-World census or `whoami` call is required.

## A song's take on a Routine's schedule

A song comes round again by a Routine (O-I #844; DLW manual §19.1 Songs: the
schedule is the routine's, and the song keeps none). A Method capsule whose
`body` is `factory-song-take` declares the one Factory action
`factory:action/workflow.perform` and binds the song's canon unit in the Run it
stands in, in a `[metadata.native-method.song]` table: the absolute `state`
path, the `run_ref` and the `unit`. A Method with the song body carries no
`factory` table, and another body carries no `song` table.

```toml
[metadata.native-method]
schema = "aikit.native-method/v1"
body = "factory-song-take"
actions = ["factory:action/workflow.perform"]

[metadata.native-method.song]
state = "/absolute/project/.factory/development-state.json"
run_ref = "run:…"
unit = "release-check"
```

Each run of the Routine is one take: the runner calls `factory workflow perform
<state> <run_ref> <unit> --json`, requires `factory.song-take/v1` and its take
number, and records the take, the unit, the Run it came from, the take's Run and
the clip in the native run receipt. Factory numbers the take; a refusal (the
clip's set has moved, the unit is no song, another perform took that number)
fails the run in Factory's words, and nothing else is called. Without a grant of
the perform action the run calls nothing. Software Factory's attempt Actions
start the take's units as they do any Run's.
