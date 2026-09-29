---
name: aikit-central-file-map-refresh
description: "METHOD: Probe every participating Central scope for uninitialized or stale persistent file maps and refresh exactly those scopes — `central.file-map.search` federated, `central.world` for Project-name spelling, then `central.file-map.refresh` per affected scope — run by a Routine's native runner with no model, no source registration and no knowledge compilation."
---

# Environmental file-map refresh

Semantic ref: `aikit:central-file-map-refresh`. Native owner: `EpiLogos/ai-kit` (the Routine and its native runner); the persistent file map and its staleness law are Central's.

This Method is environmental rhythm, not work. It keeps the bkmr-backed persistent file map — the substrate of Central's source pool and AIKit's `central-bkmr` knowledge provider — initialized and current. Its body is **native**: the dispatcher's native runner calls Central's Actions through the real `ctrl` binary and never opens a resident encounter or any other model run. An agent should not perform it by hand; when a map did not refresh, read the run receipt and repair the owner condition instead.

## What the body does, in order

1. `central.file-map.search {"federated": true, "limit": 1}` — one federated probe across every participating scope. The hits are ignored; the absences are the signal: `"<world>: map not initialized"` and `"<source ref>: stale index; refresh required"`. (`limit: 0` would short-circuit Central's search before it inspects any scope, so the probe asks for one hit.)
2. `central.world {}` — the authority for Project-name spelling: world refs fold case (`project:quaternal-logic`), while a refresh accepts the real Project name (`Quaternal-Logic`).
3. `central.file-map.refresh {"embeddings": true}` — for the root scope (no `project` argument) and `{"project": P, "embeddings": true}` for each scope the probe named. Central's refresh is incremental: unchanged sources cost a revision check, withdrawn sources are withdrawn, and a row edited by hand in bkmr is reported for reconciliation, never overwritten.

## What it never does

- It registers no source and withdraws none; the declaration model is untouched.
- It compiles no knowledge and answers no query beyond the absence probe.
- It recognises no Return and writes no recognition; it reflects on nothing.
- It reconciles nothing: a conflicting bkmr row fails that scope's refresh and is reported, for the owner to settle.

## Running it as a Routine

```text
aikit method prove --method skill/aikit/central-file-map-refresh --proof-json @proof.json
aikit routine create --name central-file-map-refresh --method skill/aikit/central-file-map-refresh \
  --proof-json @proven-basis.json \
  --trigger-json '{"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/central-file-map-0610","schedule":{"kind":"daily","time":"06:10"}}' \
  --authority-json @authority.json      # action_refs: the three central:action/… refs above, granted, unattended
aikit routine enable routine/central-file-map-refresh --authority-json @authority.json
```

No credential is bound: none of these Actions is token-gated. The gateway service fires it (`aikit gateway serve`, or the LaunchAgent from `aikit gateway install-service`); `aikit gateway tick` runs one pass by hand.

## Evidence

Each run writes `aikit.native-routine-run/v1` to `$AIKIT_HOME/state/routine-native-runs/<hash>.json`: every Action called with its input and outcome, each refreshed scope's entry and change counts with Central's diagnostics, and the explicit list of what was not done. No credential is involved. The dispatch record in `aikit gateway tick --json` names `method_body: native:central-file-map-refresh`.

## Failure and recovery

- A scope reports `outcome: "failed"` with Central's error: most often a bkmr row edited by hand (`Managed indexed description was edited in bkmr`) — reconcile it in bkmr or adopt it explicitly, then let the next occurrence refresh.
- A routine run ends `failed` when any scope failed; the other scopes' outcomes are still in the receipt, and the next occurrence re-probes and retries only what is still absent or stale.
- `the Routine's admitted authority does not grant …`: the Routine was enabled with an authority that omits one of the three declared Actions.
