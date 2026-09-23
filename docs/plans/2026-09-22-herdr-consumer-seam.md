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

## Settled 2026-09-23 — S1 and S2 built (feat/herdr-consumer-seams-20260923)

Both seams were built by the herdr consumer-seam lane on top of main
(597452a2). The owner directed building over deciding; the two semantic
choices the note left open are settled here as built.

### S1 as built — provider selection at binding creation

The binding-creating verb is `aikit session-space stage --intent-json` with a
`bind-working-surface` intent. It now takes
`--provider <technology|provider-ref>`:

- Accepted forms: a technology name (`herdr`) or a provider ref, canonical
  (`provider/herdr/current`) or instance (`provider/herdr/w6`). A technology
  name persists as the technology-canonical ref; a named ref persists exactly
  as named, so an instance selection stays the instance.
- Validation, against `PlaceTechnologyRegistry::builtin()`: the technology
  must be registered, its real detection (`detect()`) must report it
  installed, and the build must be able to drive it as a working environment
  — `working_environment()` **or** `mux_adapter()`. This extends the note's
  literal `working_environment()`-must-return-Some by one step: a mux-backed
  provider (tmux, cmux) is a legitimate binding provider the working-surface
  path already drives, so refusing it would be a dishonest refusal. herdr
  passes exactly as the note intended.
- Persistence: the validated ref is written to
  `SessionSpaceWorkingSurfaceBinding.provider` with a provenance line naming
  the flag. The working-surface verbs (`observe/open/focus/attach`) are
  untouched — they already follow the binding.
- One honesty guard the note did not name: a staged plan that already
  declares a *different* place technology than `--provider` names is refused
  (`session_space.provider_plan_conflict`) — one binding cannot name both,
  and `session_stack` routing reads the plan, so a contradiction here would
  resurface as a mux-shaped answer later.
- Absent flag: the intent passes through exactly as staged. Zero change.
- Error codes: `session_space.provider_unregistered`,
  `provider_not_installed`, `provider_undrivable`,
  `provider_selection_malformed`, `provider_flag_misplaced`,
  `provider_plan_conflict`.

### S2 as built — reconcile for provider-native technologies

The semantic settled, in one sentence: **reconcile for a provider-native
technology is the provider's own interface, answered or honestly silent —
never the mux contract approximated.** Concretely, in
`crates/aikit-cli/src/session_provider_reconcile.rs`, applied as a split at
the top of `Service::session_reconcile`:

- Routing is a registry question, answered without any probe: the plan's
  declared technology must resolve to an entry with no mux adapter and a
  working environment. No declared technology, a built-in mux, or an
  unregistered name returns `None` and the mux path runs byte-for-byte as
  before.
- Non-destructive reconcile (`CreateOrAttach`) = the provider's own
  `open()` — its create-or-attach primitive — followed by *reflecting* what
  it reported: health, the provider-native place id, the binding count
  (`standing: "reflected"`). A provider that answers but reports itself
  degraded or unavailable is still an answer; it is reflected with a warning,
  not laundered into failure.
- The provider refusing or failing to answer (its `open()` errors — including
  spawn failure when the binary is absent) degrades to the named honest state
  `standing: "protocol-opacity"`, carrying the provider's refusal verbatim.
  No crash, no exit-code failure, and above all no mux-shaped answer
  fabricated around the gap. Routing deliberately runs no `detect()`: the
  provider's own failure is the truth about reachability, and one probe less
  is one fewer way to mistake absence for refusal.
- Destructive reconcile (`--destructive`, `Exact`/kill) has no provider-native
  inverse in this build and is declared unavailable without contacting the
  provider (`standing: "declared-unavailable"`), naming the working-surface
  verbs as the route that drives the place today. Growing provider-native
  inverses remains the #114-sized follow-up.
- Wire shape: the `session reconcile` reply gains a `provider_native` object
  (`{technology, provider, standing}`) only on the provider-native path; the
  mux path's reply is byte-identical to before.

### Test doubles — herdr never spawned

`PlaceTechnologyRegistry::from_entries` composes a registry from exactly the
given entries (the composition the registry doc already promised), so tests
supply herdr test doubles implementing `PlaceTechnologyAdapter` +
`WorkingEnvironmentProvider` in test scope: canned observations, canned
refusals, detection by construction. No test in this change spawns herdr or
touches `~/.config/herdr/herdr.sock`; the destructive-routing test proves
herdr routes through the *real* builtin registry with zero I/O.

Pins for "unchanged": the mux reconcile path is untouched code behind the
`None` return and remains covered by the existing
`session_integration.rs` tmux tests (run in this lane's gates), plus unit
tests asserting the router yields `None` for mux, unregistered, and
undeclared plans.
