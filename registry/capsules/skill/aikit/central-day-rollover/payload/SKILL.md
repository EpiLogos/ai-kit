---
name: aikit-central-day-rollover
description: "METHOD: Open the civil Day and roll every Project NOW field into it with native Central Actions only — `central.time.policy`, `central.day.ensure` under that exact policy revision, `central.world`, then `projectcentral.now.rollover` per Project — run by a Routine's native runner with no model, no task completion, no Return recognition and no reflection."
---

# Environmental DAY rollover

Semantic ref: `aikit:central-day-rollover`. Native owner: `EpiLogos/ai-kit` (the Routine and its native runner); the Day and NOW semantics are Central's.

This Method is environmental rhythm, not work. It is meant to run unattended at the civil day boundary as a Routine bound to the gateway dispatcher, and its body is **native**: the dispatcher's native runner calls Central's Actions through the real `ctrl` binary and never opens a resident encounter or any other model run. An agent should not perform it by hand; when a Day did not open or a Project field did not roll, read the run receipt and repair the owner condition instead.

## What the body does, in order

1. `central.time.policy {}` — read the recognised civil-time policy fresh; its `revision` is the only basis the Day may be opened against.
2. `central.day.ensure {"expected_time_policy_revision": <revision>}` — open (or confirm) today's civil Day. Token-gated: the runner passes `CENTRAL_NATIVE_TOKEN` to this one child only, read from the location bound with `aikit routine credential`. Central permits a non-human principal here only when the policy says `automatic_day_rollover: true` and the native-action-authority source grants that principal `central.day.ensure`.
3. `central.world {}` — list the Projects whose ProjectCentral carries a NOW field.
4. `projectcentral.now.rollover {"project": P, "day": D-1, "next_day": D}` for each such Project, where D is the civil date Central just opened and D-1 the calendar day before it. Central carries live handoffs (`active`, `waiting`, `carried`) and releases resolved ones; a day already closed is reported `already-closed`, not retried.

## What it never does

- It completes no task, ticks nothing and marks no handoff resolved.
- It recognises no Return and writes no recognition; it reflects on nothing.
- It closes, completes or archives no NOW clearing: active Workcell root and child clearings carry, quiescent ones are reported released and stay retained (Central's `now_horizon` reading, when the installed `ctrl` reports it).
- It never calls `central.day.lifecycle`: closing the root Day document is human-only.

## Running it as a Routine

```text
aikit method prove --method skill/aikit/central-day-rollover --proof-json @proof.json
aikit routine create --name central-day-rollover --method skill/aikit/central-day-rollover \
  --proof-json @proven-basis.json \
  --trigger-json '{"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/central-day-0000","schedule":{"kind":"daily","time":"00:00"}}' \
  --authority-json @authority.json      # action_refs: the four central:action/… refs above, granted, unattended
aikit routine credential routine/central-day-rollover --env CENTRAL_NATIVE_TOKEN --location file:/ABSOLUTE/PATH/TO/TOKEN
aikit routine enable routine/central-day-rollover --authority-json @authority.json
```

The gateway service fires it (`aikit gateway serve`, or the LaunchAgent from `aikit gateway install-service`); `aikit gateway tick` runs one pass by hand.

## Evidence

Each run writes `aikit.native-routine-run/v1` to `$AIKIT_HOME/state/routine-native-runs/<hash>.json`: every Action called with its input and outcome, the Day opened, each Project's close, and the explicit list of what was not done. The credential never appears in it. The dispatch record in `aikit gateway tick --json` names `method_body: native:central-day-rollover`.

## Failure and recovery

- `policy_or_source_denied` on `central.day.ensure`: the policy does not allow automatic rollover or the grant does not cover this principal — an owner setting in Central, not something to work around.
- `stale_basis_or_identity_conflict`: the policy changed between steps 1 and 2; the next occurrence reads it fresh.
- A missing credential binding refuses before any process runs and names the `aikit routine credential` command.
