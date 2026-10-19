---
name: aikit-central-file-map-refresh
description: "METHOD: Keep every participating Central scope's persistent file map initialized and current — one `central.file-map.refresh` with `federated: true`, which Central answers scope by scope — run by a Routine's native runner with no model, no source registration and no knowledge compilation."
---

# Environmental file-map refresh

Semantic ref: `aikit:central-file-map-refresh`. Native owner: `EpiLogos/ai-kit` (the Routine and its native runner); the persistent file map and its staleness law are Central's.

This Method is environmental rhythm, not work. It keeps the bkmr-backed persistent file map — the substrate of Central's source pool and AIKit's `central-bkmr` knowledge provider — initialized and current. Its body is **native**: the dispatcher's native runner calls Central's Actions through the real `ctrl` binary and never opens a resident encounter or any other model run. An agent should not perform it by hand; when a map did not refresh, read the run receipt and repair the owner condition instead.

## What the body does

One call: `central.file-map.refresh {"federated": true, "embeddings": true}`. Central refreshes each scope it has — the root, every Work project, and external scopes such as `epi` and `legacy` that no `central.world` listing names — through the same locked, validated refresh a single scope gets, and answers one report per scope: entries, changes, diagnostics, seconds, or that scope's own error. Central's refresh is incremental: unchanged sources cost a revision check, withdrawn sources are withdrawn, and a row edited by hand in bkmr is reported for reconciliation, never overwritten. One scope that cannot refresh does not stop the others.

The body used to probe with a federated search and parse the absences it printed. That search re-hashes every roster file, was killed at its 120 s timeout on 8 of its first 11 runs, and could only name scopes that were uninitialised or hit; the maps were not maintained for three weeks. A caller no longer enumerates or spells scopes.

A Central from before the world refresh reads `federated` as nothing and refreshes the root alone. The body checks the answer is a world refresh and ends `failed` with that reason otherwise, never `completed`.

## What it never does

- It registers no source and withdraws none; the declaration model is untouched.
- It compiles no knowledge and answers no query.
- It recognises no Return and writes no recognition; it reflects on nothing.
- It reconciles nothing: a conflicting bkmr row fails that scope's refresh and is reported, for the owner to settle.

## Running it as a Routine

```text
aikit method prove --method skill/aikit/central-file-map-refresh --proof-json @proof.json
aikit routine create --name central-file-map-refresh --method skill/aikit/central-file-map-refresh \
  --proof-json @proven-basis.json \
  --trigger-json '{"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/central-file-map-0610","schedule":{"kind":"daily","time":"06:10"},"catch_up":{"mode":"latest","within_ms":604800000}}' \
  --authority-json @authority.json      # action_refs: central:action/central.file-map.refresh, granted, unattended
aikit routine enable routine/central-file-map-refresh --authority-json @authority.json
```

The trigger carries `catch_up: latest` within seven days. A daily occurrence is skipped forever when the machine is asleep or the gateway is down at 06:10 (the default policy, `skip-missed`), and from 26 Sep to 19 Oct only 10 of the 24 daily occurrences were admitted at all. The refresh is idempotent and converges the maps, so the first tick after a missed occurrence runs it once; it never runs it more than once for any number of misses.

No credential is bound: this Action is not token-gated. The gateway service fires it (`aikit gateway serve`, or the LaunchAgent from `aikit gateway install-service`); `aikit gateway tick` runs one pass by hand.

## Evidence

Each run writes `aikit.native-routine-run/v1` to `$AIKIT_HOME/state/routine-native-runs/<hash>.json`: every Action called with its input and outcome, each scope's entry and change counts, seconds and Central's diagnostics or error, and the explicit list of what was not done. No credential is involved. The dispatch record in `aikit gateway tick --json` names `method_body: native:central-file-map-refresh`.

## Failure and recovery

- A scope reports `ok: false` with Central's error: most often a bkmr row edited by hand (`Managed indexed description was edited in bkmr`) — reconcile it in bkmr or adopt it explicitly, then let the next occurrence refresh.
- A routine run ends `failed` when any scope failed or Central cannot refresh the world; the other scopes' outcomes are still in the receipt, and the next occurrence refreshes again; unchanged sources cost a revision check.
- `the Routine's admitted authority does not grant …`: the Routine was enabled with an authority that omits the declared Action.
