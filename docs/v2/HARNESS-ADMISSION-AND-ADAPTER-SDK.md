# Harness admission and public adapter SDK

Status: executable contract for AIKit #114, stacked on the active #108/#113 implementation line.

AIKit owns **how** a situated Actuation is provisioned into a concrete harness. The harness itself remains an existing technological world with its own native faculties, source ownership, lifecycle and runtime concepts.

The adoption lifecycle is:

```text
discover exact product / edition / version / source condition
        ↓
record target-native faculties + evidence
        ↓
resolve the existing AIKit Context / praxis / resources
        ↓
TargetAdapter plans the least-distorting native Projection
        ↓
existing Generation / Procedure machinery applies it
        ↓
target-native activate / reload / next-session / restart semantics
        ↓
observe what the target actually loaded
        ↓
Explain / History can distinguish source, projection and activation evidence
```

`crates/aikit-core/src/harness_admission.rs` publishes `aikit.harness-adapter/v1`. It deliberately extends the existing `TargetAdapter`; it is not another resolver or projection engine.

## Admission

`HarnessAdmissionDescriptor` identifies the adapter, exact target/edition, optional version/source revision and observed native faculties. Supported faculties require evidence. Unsupported and unknown faculties are first-class outcomes rather than being normalised away.

An admission may retain a stable `realised_actuation_ref` from Actuation #16. That ref tells AIKit **what** acting condition it is provisioning; it does not make Actuation an AIKit-owned type.

## Activation truth

`ProjectionPlan` already describes what AIKit intends to project and `ActivationEffect` already distinguishes immediate/live/restart/next-session/brokered/unsupported states. `HarnessActivationObservation` adds the evidence-backed other half: what the installed/running target was actually observed to have loaded.

A `Loaded` observation must cite evidence for the exact projection digest. `verify_activation_truth` rejects overclaims such as reporting a target loaded a change immediately when the accepted adapter says the target only reads it next session or after restart.

Therefore:

```text
projection written != target loaded
resolved in AIKit != active in harness
adapter claim != observed result
```

## Unsupported targets

An unrecognised target returns `HarnessCompatibilityGap`, not synthetic support. The gap preserves the exact target/edition/version and whatever faculties were actually visible, then points to:

```text
aikit:harness-adapter-sdk/v1
skill/aikit/harness-adapter-authoring
```

The authoring Skill requires evidence for source ownership, idempotent projection, authored-file conflict handling, activation/reload truth, update/retract behaviour, unavailable faculties, identity non-collapse and Explain/provenance before retrying admission.

## Native source and praxis

Skills and Methods stay owned by their native/product/Central sources. AIKit resolves their stable refs through the #108 praxis system and uses a target's native Skill/tool/instruction/component faculties where that is the least-distorting representation. Generated target material remains a projection destination.

This contract does not require Central, O:I, Factory or QL-MEF for minimal AIKit operation. QL/MEF can remain an optional provider/read layer over the same ordinary Context when requested.
