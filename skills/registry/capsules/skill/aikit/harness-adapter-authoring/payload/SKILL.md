# AIKit Harness Adapter Authoring

Use this when AIKit encounters a real harness/edition for which no accepted adapter exists.

The goal is not to make the target look like Claude, Codex, Pi, Cordis, or any other reference harness. The goal is to discover the target's **least-distorting native faculties**, bind the existing AIKit projection machinery to them, and prove what the target actually loaded.

## Work from the target outward

1. Identify the exact product, edition, version/source revision and installed/runtime condition.
2. Read the target's current native configuration, instruction, Skill/tool, lifecycle, session and extension faculties from primary source or direct observation.
3. Record supported, degraded, unsupported and unknown faculties using `aikit.harness-adapter/v1`.
4. Reuse `TargetAdapter`, `ProjectionPlan`, Generation/Procedure and existing resolver semantics. Do not build another resolver or package manager.
5. Prefer native Skill/tool/component/instruction facilities over generated compatibility wrappers when they preserve meaning more faithfully.
6. Preserve authored files and source ownership. Generated projection material is a destination, never the canonical source of a Skill or Method.
7. After projection, obtain target-native activation/reload evidence. A written file or successful generation is not proof that the running target loaded it.
8. Record lifecycle truth explicitly: live, restart-required, next-session, brokered, unsupported, retracted, or unknown.
9. Add conformance covering source ownership, idempotence, authored-file conflicts, activation truth, update/retract, unavailable faculties, identity non-collapse and Explain/provenance.
10. Retry admission through the public SDK once the adapter passes its conformance fixture.

## Preserve these boundaries

- Actuation owns what realised Agency/loop exists; AIKit owns how it is provisioned and projected.
- Agent, Agency, Harness, HarnessComposition, AgentSession, model, process and material binding remain distinct identities.
- Product/native sources own their Skills and Methods; AIKit indexes, resolves and projects references.
- Target-native runtime `Context` or plugin concepts do not redefine AIKit/Factory Context.
- Optional QL/MEF readings do not become target configuration requirements.

If the target cannot expose a claimed faculty, report that absence or degradation rather than fabricating a lowest-common-denominator emulation.
