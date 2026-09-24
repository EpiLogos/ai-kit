# ADR 0005 — Harness×model routing portal and user-built alias families

Status: proposed. Stages 1 and 2 implemented 2026-09-19 (`aikit alias`,
`aikit harness run`); Stage 0 of the delivery runs in production on the
owner's Omarchy machine as the pre-integration reference
(`examples/alias-families/agents/`; live test evidence 2026-09-15 in the O-I
campaign archive, `campaign-evidence/2026-09-15-launcher-portal/`).

## Context

The owner asked for three things that turn out to be one design:

1. one clean command on the Omarchy machine that brings pi, hermes and codex
   up together in a single persistent room, with proper tmux-era fallback;
2. the realisation that users should be able to build their own *command
   families* over the suite, with agents helping — the old Mac `epi` CLI was a
   first attempt at exactly this and aged into scattered prior art;
3. the Claude model-shim hack (`glm`/`kimi`/`deepc` shell functions sourcing
   profile files that carry literal API tokens in dotfiles) generalised into a
   safe, owned portal for harness×model routing.

What already exists in the suite (all evidence-backed, 2026-09-15):

- **Harness primitive** — `aikit client install|launch|status` over the static
  registry in `crates/aikit-cli/src/client.rs` (claude, codex, … with aliases,
  catalog slugs, install seams).
- **Model-route primitive** — `aikit model-catalogue` and the route join in
  `crates/aikit-adapters/src/actuation_model_routes.rs`
  (`ModelRouteJoin`, `ProviderReachability`, `CredentialEvidence`).
- **Credential primitive** — `aikit credential setup|explain|list`; resolution
  per `central.security/v1` in `crates/aikit-adapters/src/secret_resolver.rs`
  (`varlock://` > `pass://` > `keychain://`, presence-not-exposure).
- **Room primitive** — `aikit session up|attach|diff|reconcile|down` over
  portable session topology; Workcell's `workcell place request|release`
  (`workcell.place-grant/v1`, PR #83) for generation-proofed claims of herdr
  workspaces and tmux sessions.
- **Owner verbs** — the `oi config` plane with per-product contributions
  (ai-kit already contributes `resolution.profiles` as a profileable ref).

What is missing is the composition layer between these primitives and the
user. Nothing today lets a user (helped by an agent) compose a named family of
commands out of harnesses, routes, profiles and rooms. The `epi` CLI and the
zsh shim functions are that layer's ghost: unowned, undeployed from any
repository, and unsafe (literal tokens in `~/.zshrc`, `~/.claude/profiles/*`).

## Decision

Name four primitives and fix their ownership:

1. **Harness** — an executable agent client. Owned by ai-kit's client
   registry; Actuation owns harness-capability and detection intakes.
2. **Model route** — a named binding of (provider source, endpoint, credential
   *ref*, model mapping). Owned by ai-kit's model catalogue. Routes carry
   references, never material; Actuation realises a route when an AgentRef is
   enacted (`aikit compose`), and the CLI's existing law stands: a stable
   AgentRef is enacted, not a profile, model, session or display label.
3. **Profile** — a named, scopeable binding of routes to harnesses. Scopes are
   the config plane's machine / project / agent-session.
4. **Alias family** — a user-owned manifest of named commands composed from
   the primitives above, installed as an ordinary command family. Families are
   versioned *data* (manifest + templates), inspectable and checkable, never
   code baked into products; agents propose edits to the manifest through the
   ordinary propose path, the owner adopts.

**Security contract (UI07, binding on every stage).** A key enters a machine
once, through the owner's chosen front door, and lives only behind a resolver
scheme. The portal handles references and presence: every surface
confirms-without-revealing; nothing prints, logs, stores or transmits key
material; there is no plaintext fallback. Retirement of the Mac shims means
their token literals move into the keychain and each profile conf becomes a
route manifest citing `keychain://service/account`.

**Room commands are family entries, not a special case.** `up / attach /
status / down` compose place grants (Workcell) with provider shaping (herdr
natively, tmux as the thin fallback) exactly as the Stage 0 example does.
An `epi` successor is another family over the same primitives: `sesh` → room
verbs, `code <provider>` → harness×route launcher, `slot` → model-catalogue
and route-explain surfaces.

**Ownership boundaries.**

| Concern | Owner |
|---|---|
| harness registry, launch/install seams | ai-kit |
| model catalogue, route join, credential surfaces | ai-kit |
| route realisation, AgentRef/Agency law | Actuation |
| place census and generation-proofed grants | Workcell |
| profile defaults, owner disclosure | O-I configuration plane |
| key material | keychain behind `central.security/v1` resolvers only |

## Staged delivery

- **Stage 0 (landed, pre-integration).** `examples/alias-families/agents/` —
  the Omarchy flagship: `agents up|attach|status|down|doctor|persist` with
  `--provider auto|herdr|tmux` and `--json`, built on `workcell place request`
  and herdr/tmux natives, UI07-clean (presence checks only). Its `room.conf`
  slot table is the manifest seed: name/kind/cwd rows a family manifest will
  generalise.
- **Stage 1 (landed 2026-09-19).** `aikit alias list|check|install` over
  owner-owned `aikit.alias-family/v1` manifests under
  `<AIKIT home>/alias-families/*.toml`. Validation cites the harness registry
  (`aikit client`'s registry, joined to profile slugs the same way) and the
  model catalogue by reference; unknown harness slugs, unknown model refs and
  ill-shaped values are refused. `install` emits thin launcher scripts as
  generated data under `<AIKIT home>/alias-families/installed/<family>/` —
  executable files the owner places on PATH by hand, exactly as the Stage 0
  flagship documents — and refuses a family with any entry the profile facts
  cannot carry.
- **Stage 2 (landed 2026-09-19).** `aikit harness run --harness <slug>
  --model <model:stable-id> [--provider <provider-ref>] [--dry-run]
  [-- <passthrough>]`: the catalogue join (detection + harness dispatch
  reachability + credential presence, never material) selects the route; the
  harness profile's models layer decides how the model choice reaches the
  harness — observed `--provider/--model` argv selectors where they were
  observed (pi), a refusal in the profile's own words where they were not;
  key delivery materialises only through the existing scrubbed-env seam
  (`ModelEnvironment`) under the profile-declared variable, never inline.
  `--dry-run` discloses the composed argv and delivered variable names
  without spawning or materialising. Honest boundaries are enforced, not
  advertised: `None{reason}` dispatches, selector-less provider-plural
  harnesses, and config-key native bindings without an observed one-shot
  selector refuse with the named remediation. Codex is a narrow observed
  exception: its one-shot CLI accepts `--model`, so an exact `provider:openai`
  route can use the selected model after `codex login status` confirms a
  ChatGPT login on the selected executable. An absent API-key binding is
  disclosed as Codex own-login, the final child is scrubbed of ambient API
  keys, and revoked or expired bindings still refuse. This is authentication
  readiness, not a claim that the model is entitled or that inference costs
  nothing; a real launch determines that. The ACP Encounter keeps its
  profile-declared session `model` config-key delivery and pins the adapter's
  `CODEX_PATH` to the same installed executable whose login was checked.
- **Stage 3.** Config-plane integration for family and profile defaults at
  machine scope, sequenced after the in-flight configuration work lands so it
  extends the shared registry rather than colliding with it.

## Consequences

- The suite gains a user-held command layer without new daemons or a new
  product: families are data over existing seams.
- Harness×model routing becomes auditable: routes and families are versioned
  documents with provenance, so drift between what a user thinks they run and
  what is installed becomes discoverable.
- UI07 stops being honoured by discipline alone: the safe path (refs +
  presence) becomes the only path the portal offers.
- The old shims and `epi` remain working during migration; retirement is a
  staged replacement, not a break.
