# Herdr consumer seam — sized assessment (issue #394 K6, 2026-09-22)

Author: ai-kit repair lane (fix/oi65-adapter-acceptance-20260922, base fe4b917).
Verdict: **not implemented in this lane** — the remaining seam is real but is
not a config/flag-sized change; forcing it would have produced a partial
honesty story. Sized design below.

## What already reaches herdr on fe4b917

The campaign's "no consumer reaches it" was measured on the installed
499b37ec binary, which predates the registry-generic field. On fe4b917:

- `crates/aikit-adapters/src/place_technology.rs` — `HerdrTechnology`
  registers `herdr` in `PlaceTechnologyRegistry::builtin()` and hands back a
  plan-scoped `HerdrWorkingEnvironment` (`working_environment(...)`,
  `mux_adapter() -> None` by declaration).
- `crates/aikit-cli/src/working_environment_field.rs` — `observe`/`act` are
  registry-generic; `provider/herdr/current` and instance refs
  (`provider/herdr/w6`) both resolve to herdr's own provider.
- `crates/aikit-cli/src/session_space_working_surface.rs` —
  `session-space working-surface observe|open|focus` and
  `terminal-attachment` follow the persisted binding's provider ref through
  that field; `herdr_binding_refresh` persists herdr workspace/pane evidence
  back into the binding. This is the consumer path #53/#274 asked for, and it
  works for herdr today.

## The two seams still missing

### S1 — provider selection at binding creation (the missing flag)

`SessionSpaceWorkingSurfaceBinding.provider` (`crates/aikit-core/src/
session_space_application.rs:189`) is chosen where the binding is authored;
no `aikit session-space` verb takes a provider argument, so an operator
cannot say "bind this surface to herdr" — they get whatever the authoring
path mints. The honest seam: a `--provider <provider-ref|technology>` flag
on the binding-creating verb, validated against `PlaceTechnologyRegistry`
(`detect()` must report installed; `working_environment()` must return
`Some`), persisted on the binding. The working-surface verbs then need zero
changes — they already follow the binding.

Scope: flag + validation + persistence in the binding-authoring path
(`crates/aikit-cli/src/session_space_cli.rs`, the store's authored-state
write), plus one binary test binding to herdr in an isolated scope (herdr
has no runtime state isolation — l3 finding D3 — so the test must stub or
skip live observation; that is the fiddly part).

### S2 — provider-native technologies in `session_stack`

`Service::session_stack` (`crates/aikit-cli/src/app/mod.rs:625`) hardwires
`MuxStack::detect(vec![Cmux, Tmux, Plain], Some(mux))`. A plan declaring
`mux = "herdr"` refuses `mux.technology_unsupported` — honest, but it means
`session-space up/diff/reconcile` cannot drive a herdr place at all; only
the working-surface path can. The design-fit seam is not "add herdr to the
vec" (herdr is deliberately not a `MuxAdapter`; it has no pane-split/kill
mux contract). It is a split before the stack: plans whose declared
technology resolves through `PlaceTechnologyRegistry` to a provider-native
entry route to the provider vocabulary (`observe`/`open`/`focus`), while
reconcile-grade operations (bring-to-spec, kill) either grow
provider-native inverses or are declared unavailable per technology.

Scope: this is the #114-sized piece — new dispatch in `session_stack`, a
reconcile-semantics decision per provider, and conformance cases for each
state. Not a repair-lane change.

## Recommendation

Land S1 first (one flag, one validation, one test; working-surface verbs
already follow the binding, so herdr becomes selectable end to end at the
surface operators actually use). Treat S2 as its own ticket with the
reconcile-semantics decision made by the owner, since "what reconcile means
without a mux contract" is a design position, not a code move.
