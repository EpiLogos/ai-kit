# Required context at the resident ACP boundary

`EncounterProvider.required_context` is an opt-in source prerequisite on the existing native provider configuration. Providers without it retain optional-context behavior. The configuration does not grant Actuation authority, adopt human governance, or install a policy store.

Each required source records its canonical `source`, owner-issued opaque `revision`, absolute material `path`, and `content_digest` in the explicit form `blake3:` followed by 64 lowercase hexadecimal digits. The byte digest binds the material associated with the revision; it does not replace the source owner's revision or prove its current authorship standing. Files must be regular and at most 4 MiB each; one admission names 1–128 distinct sources. Source bodies are not written to encounter history.

Before starting an ACP process, the resident service checks every required file against its pin. Missing, malformed or changed material returns an attributed admission error before process launch. The service repeats the check before submitting each prompt and before a non-rejection provider permission response. Draft submission occurs after checking, so a refused prompt preserves the draft. Cancellation and explicit native rejection remain possible when required material is unavailable. A changed required-context configuration pauses the existing resident rather than silently changing its admitted basis.

The optional `source_activations` use existing `ContextActivationReceipt` semantics. Optional `projection` and `activation` must appear together and use `ProjectionPlan` and `HarnessActivationObservation`; the observation must name the exact projection digest and target. Source activation evidence must name a source in the required basis, and where a projection is supplied its target must agree. These records remain distinct from the current material check. Historical configuration evidence does not establish that a newly opened native session loaded anything. Journal events explicitly report `fresh_runtime_loading_observed: false` for material verification.

Current limitations are deliberate and visible: the preflight is a point-in-time material check, not an atomic snapshot consumed by every harness tool; the gate does not police built-in Pi tools or replace native Action/Workcell enforcement. It does not automatically resolve Profile governance into effective context. Owner-issued revision metadata is retained, while current local bytes are checked separately. Proving source loading and governed tool effects requires the actual supported ACP integration and its evidence.

Use the existing native `aikit-session-space encounter-configure --provider-json` operation to configure the provider; it is not an IPC capability. JSON may include `required_context` with `sources`, `source_activations`, `projection` and `activation`. An owner can omit activation evidence by using an empty source-activation list and null projection/activation; that remains a source prerequisite only. Do not manufacture loading receipts to fill these fields.

Run focused real acceptance:

```
CARGO_INCREMENTAL=0 cargo test --locked -p aikit-cli --test encounter_context_admission -- --nocapture
CARGO_INCREMENTAL=0 cargo test --locked -p aikit-cli --test compose_native_central -- --ignored --nocapture
```

The first uses real temporary sources, the actual SessionSpace/encounter store and an actual OS process effect to prove missing/stale refusal before launch. The optional-provider case reaches that effect and then fails ACP negotiation because the executable is intentionally a normal OS command, not a simulated agent. The second requires real native Central and proves authored-basis disclosure plus broken-source refusal through the `aikit compose` binary. Neither test pretends to prove a live model loaded policy. Use the separate actual ACP acceptance for that further boundary.
