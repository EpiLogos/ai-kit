# Selected catalogue model to native resident

AIKit #294 continues #274/#275 on #291. This operation selects a deliberately configured native body for the already admitted Agent and World. How a bound policy reaches the harness is decided by the harness profile's declared models dispatch, joined by the launch program — never by protocol assumption: per-invocation argv flags where the census observed them (pi, `--provider`/`--model`), the native session's own model configuration where the harness selects through its `model` config key (claude-code and codex, which bind one provider natively; and the provider-plural ACP harnesses whose per-session selection the encounter decides). A `none` dispatch refuses with its declared reason, and a launch program that joins no profile refuses rather than delivering through an assumed surface. A bound policy is delivered or the open fails; it is never silently ignored.

## What determines eligibility

A configured body's `model_policy` is an exact `EncounterRequiredSource`: source reference, source revision, absolute path and BLAKE3 content digest. Its JSON has schema `aikit.model-dispatch-policy/v1`, selected `agent_ref`, `world_ref`, `authority_ref`, `bounds_refs`, canonical `model_ref`, `provider_ref`, `native_provider`, `provider_native_id`, `expires_at_unix_ms`, and optional `credential`. The canonical catalogue must contain that Model and native route. The current native Agency must permit `action/aikit/model-realise`; policy Agent/World/authority/bounds must match its actual determination. A Profile is not required and neither a profile nor this model policy grants authority.

A credential declaration has `requirement_ref`, `credential_ref`, `target_env` and optional `from_env`. It selects an existing native credential provider or an explicit environment import. Missing, revoked or expired material is a refusal, not a reason to pass an inventory reference as a secret. Only the selected key and retained runtime environment reach the final model child. Source import variables, unrelated ambient credentials and Central/Workcell control bearers are not inherited. No secret value is written into the persistent model record. Actual installed keychain and commercial-provider uptake remain separate proof.

The Pi connection explicitly selects the native provider/model and checks its actual model-state response. An ACP resident receives the selection after the session exists, through the session's own model configuration: a harness that advertises no model selector, or a model outside its advertised list, refuses; the delivery is confirmed by provider readback. An unknown or contradictory native state is not selected-model success in either protocol. The source, catalogue, credential and native model basis are checked again before subsequent turns. This is provider-reported native state, not independent verification of a commercial inference backend.

## Existing public paths

Provision the existing native AgentSession/Agency and provider first. Supply `model_policy` alongside the provider's existing protocol and argv configuration. Use the existing encounter socket with action `open-model` and a `request` object containing `space`, `agent_session`, `cwd`, canonical `model_ref`, optional `provider_ref`, optional configured `body`, and `expected_agency`.

`expected_agency` is the complete actual `agency_admission` object returned by native composition. The owner compares it with its current admission rather than treating caller-supplied identity labels as authority. Zero eligible bodies refuses; multiple eligible bodies require explicit body selection. Configuration does not silently create a second task, Agency or fallback.

The ordinary application service `Service::realise_model(composed, model, provider)` uses this same native operation. Its composition argument contains `agency_admission` and an explicit `resident_target` object with `space`, `agent_session`, absolute `socket`, and optional `body`.

The existing CLI reaches that application service:

```text
aikit --json -C WORKING_COPY compose --agent AGENT --world WORLD \
  --agency-source EXACT_BASIS_JSON --model MODEL --provider PROVIDER \
  --realise --resident-target TARGET_JSON_FILE
```

`TARGET_JSON_FILE` contains the resident target object described above. Composition supplies the actual Agency admission; the target only identifies where this deliberate selection is to become resident. An absent owner or target cannot be replaced by an instantiation record.

The result has schema `aikit.model-realisation/v2`. `selected:true` with `executed:false` means the selected native model/body has been observed and opened. An inference result requires existing addressed `send` followed by attributable delivery/response readback. Direct work has no mandatory Factory ancestry. Task-bound sessions retain #291's exact placement/material/expected-task guards and Workcell boundary at execution.

For an explicitly pinned Pi body or a profile-derived Codex ACP body, this
result also carries `factory_selection`.
Its `aikit.explicit-model-selection/v1` receipt retains the exact model route,
policy/catalogue basis, native session and SessionSpace, admitted Agency source
and WorldBinding, and an existing `aikit.harness-composition/v2` determination.
The composition target binds the validated Pi or Codex profile and actual underlying provider argv,
owner launcher, cwd, required context and model-basis digests. A task launcher is
not mistaken for the underlying harness. `aikit.sorted-json/v1` specifies
recursive object-key ordering for the cross-owner BLAKE3 digests.

The compatibility field `ranking_policy: EXPLICIT_PIN` means a deliberate source
selection, not a ranking calculation. The thin Pi or Codex ACP composition claims
no ambient Skills, extensions or tools; empty component selections are explicit.
Other protocols and unrecognised launchers receive no Factory composition receipt.
Factory validates the receipt and its exact participant/session/source join at
attempt admission, then still uses the actual task/authority guards at dispatch.
Opening and selecting the real Pi process proves neither inference nor remote
provider credentials. The `actual_pi_open_emits_factory_selection_without_inference`
case emits an actual owner receipt for the Factory consumer tests; controlled
protocol nonregression and live provider execution retain separate evidence.

The real Codex ACP selected-model case requires a private, current ChatGPT
Codex login. It is compiled and run only when an operator explicitly enables
`codex-account-native` on that account's host. The mandatory credential-free
CAW workflow still runs all its existing ignored tests. To execute the Codex
acceptance without sending a prompt, set `AIKIT_CAW_ACTUATION_BIN` to the exact
native Actuation executable and run:

```text
cargo test --locked -p aikit-cli --features codex-account-native \
  --test caw_native_delivery \
  model_proof::application::actual_codex_acp_open_emits_factory_selection_without_inference \
  -- --exact --ignored --nocapture
```

This test preserves its real login, native model-readback and no-inference
assertions; a missing private login is a refusal in this explicit run, not a
hosted-CI skip presented as success.

## Proof and remaining work

The maintained `caw_native_delivery` target includes controlled model tests plus the public application and compose-CLI paths. They exercise scoped key delivery, profile-independent selection, actual native protocol response, changed catalogue/policy, revoked/missing credentials, unavailable authority, unknown or contradictory model facts, duplicate delivery, and removal of the running owner. Controlled keys and replies are not commercial-model or installed evidence. Only an actually executed exact-head CI result establishes the tests' standing.

This connection does not complete other protocol-specific model adapters, gateway/Routine invocation, Central reviewed receiving, Factory full-feature verification, installed material acceptance or human Recognition. Those remain the existing programme's operations, not consequences inferred from opening a resident. No whole-feature or local-testing readiness verdict is issued here.
