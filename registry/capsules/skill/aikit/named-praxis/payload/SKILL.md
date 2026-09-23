---
name: aikit-named-praxis
description: "METHOD: Recognise a successfully performed, verified path and bind it to a reusable named Routine through the real `aikit method prove` -> `aikit routine create`/`enable`/`run-now` chain; use after a real result was actually checked, never to pre-register a hoped-for pattern."
---

# Name, prove and reuse a praxis path

Semantic ref: `aikit:named-praxis`. Native owner: `EpiLogos/ai-kit`. This Method is QL-MEF practice **XP09** — "Recognise, name, expand and reuse a praxis path" — bound at its actual owner: AIKit's Method/Routine machinery, not a QL Skill.

## Contract metadata

- Executable support (verified against the installed `aikit` binary's own `--help`): `aikit method list`, `aikit method prove --method <REF> --proof-json <JSON|@FILE>`, `aikit routine create --name <NAME> --method <REF> --proof-json <JSON|@FILE> --trigger-json <JSON|@FILE> --authority-json <JSON|@FILE>`, `aikit routine show|list|enable|disable|run-now|reprove|delete <ROUTINE_REF>`, `aikit routine invocations|invocation`.
- Governing source: QL-MEF `docs/kernel-rebuild/OPTIMISATION-AND-LEARNING.md` §B08/B09/B11 and `PRE-K8-AGENT-WORLD-LOCK.md` AW3 ("Join agent-led Search/Resolve, actual path evidence, contextual familiarity and Recognition into `= name` reusable praxis... Automation uses the existing Routine/authority/proof relation."). `docs/kernel-rebuild/VAK-OIKONOMIA-KNOWLEDGE-RETURN.md` names this exact seam as "not verified end to end in the preceding source reads" and assigns AW3 to "prove the existing route or complete it through the current owner APIs" — this Skill is that verification and the resulting operative Method.
- Real, deterministic gate: "No proof, no Routine." `aikit method prove` requires the Method to be catalogue-resolved at an exact revision and a `MethodProofInput` carrying the actual run's Activity/Return/Evidence/verification refs and `invocation_succeeded`/`verification_passed` facts. A Routine cannot be created without a `ProvenMethodBasis` from that step.

## When

Use after a Method (a Skill whose description begins `METHOD:`) was actually invoked, its result actually checked against real retrieval/output/execution, and the pattern is judged worth reusing with new inputs later. Do not use it to register an untested procedure, a plausible-sounding plan, or a one-off workaround that has no independent verification.

## Inputs

The exact Method ref (`aikit method list` shows current ids); the real proof material for that run — Activity/Return/Evidence refs, whether invocation succeeded, whether verification passed; a chosen Routine name and description; a trigger (`manual`, `event`, `external`, or an `aikit.time-schedule/v1` record); an explicit `RoutineAuthority` (authority_ref, revision, action_refs, `granted`, `unattended`) scoped to only what the reused pattern actually needs — schedule/event triggers additionally require unattended authority.

## Authority

`aikit method prove` and `aikit routine create` do not themselves execute anything; a created Routine sits in `Draft` until explicitly `enable`d, and `enable` mints a fresh authority receipt at the current revision. Naming a pattern is not a grant, a scheduler, or a promise of correctness on new input — it is source-owned expandable praxis with parameters, scope and evidence, and it remains revisable, retirable and re-provable (`reprove`) when the underlying Method changes.

## Procedure

1. Confirm the Method actually ran and its result was actually checked: retrieve its Activity/Return/Evidence refs and whether invocation and verification both actually passed. Do not proceed on a summary of a prior agent's confidence.
2. `aikit method prove --method <REF> --proof-json <JSON|@FILE>` with that real proof material. This is the recognition gate; a rejected proof stops here.
3. `aikit routine create --name <NAME> --method <REF> --proof-json <proven-basis-JSON> --trigger-json <JSON|@FILE> --authority-json <JSON|@FILE>` — scope the authority to exactly what the reused pattern needs, nothing wider.
4. `aikit routine show <ROUTINE_REF>` to read back the stored proof/trigger/authority/binding exactly as recorded before enabling it.
5. `aikit routine enable <ROUTINE_REF>` only when the person/owner has actually authorised reuse; this mints the fresh enable-time authority receipt.
6. Reuse: `aikit routine run-now <ROUTINE_REF>` (or let its trigger fire), then `aikit routine invocation <REF>` / `invocations` to read the admitted occurrence evidence. Test the pattern on genuinely new input, a deliberate near-miss that should not trigger it, and a stale/retired version that must not act beyond current authority.
7. When the underlying Method changes, `aikit routine reprove` — the Routine returns to `Disabled` and must be explicitly re-enabled. Retire with `aikit routine delete` only after disabling; it refuses while `Enabled`.

## Outputs

The exact Method ref/revision, the proof envelope, the created/updated Routine's id, state (`Draft`/`Enabled`/`Disabled`/`Stale-proof`), trigger, authority and binding, and the admitted invocation evidence from any actual reuse. State plainly whether the Routine has actually been exercised on new input yet, or only created.

## Verification

`aikit routine list --state <STATE>` and `aikit routine show <REF>` against the real stored state; `aikit routine invocations` for the admitted, stable-identity-ordered occurrence record. A Routine existing in `Draft` or `Disabled` is not evidence of reuse; only an admitted invocation envelope is.

## Failure and recovery

`aikit method prove` refuses without a catalogue-resolved Method at an exact revision or a complete proof; `aikit routine create` refuses without a valid `ProvenMethodBasis`; `enable` refuses schedule/event triggers lacking unattended authority. Preserve the exact refusal. Do not hand-construct a `ProvenMethodBasis` to bypass a genuine gate, and do not silently widen `action_refs`/`unattended` beyond what the person authorised.

## Continuity

Record the Routine ref, its proof/authority basis and any admitted invocations in the actual NOW/issue record so a later session can find and reuse the named pattern rather than re-performing and re-proving the same path. Composes with QL's `ql-logos-return` (the T/T′ envelope that names a candidate `practice_ref`) and `ql-evidence-report` (the human-facing account of what was recognised and why).
