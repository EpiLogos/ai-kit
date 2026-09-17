# Harness Profiles — the general outline for detection and projection

Status: design proposal (2026-09-16), grounded in the full adapter census of
this date. Companion to `docs/v2/03` (agent/harness relation) and `docs/v2/24`
(harness adapters). Inputs: complete per-harness handling census, model-routing
grounding, composition-primitive grounding.

## The question this answers

Harnesses claim to be supersets — each owns its skills ecosystem, its MCP
servers, its model config, its session stores — but each is actually a
*subset* of the stack's ontology, lossy and differently shaped. Today AIKit
mirrors that accident: seventeen adapters each hand-roll their own survey,
projection, and admission prose (evidence below), so every new layer (MCP was
the latest) must be re-implemented per harness and rots differently in each.

The fix is the thing the harnesses only pretend to be: **one general harness
outline, declarative, built on the stack's own primitives**, of which each
supported harness is a *profile instance*. Detection becomes one engine that
observes each layer uniformly; projection becomes one engine that renders
composed capability sets into native configs uniformly; UI becomes one
layer-uniform read model.

## The evidence that bespoke handling has already rotted

- Three copies of the identical skills-projection shape: `SKILLS_PREFIX` +
  export-name rule + payload-root rule in `claude.rs:67,142-161`,
  `codex.rs:81,264-282`, `pi.rs:100,131-150`; broker has a fourth variant.
- Two hook-merge implementations: shared Claude grammar (`hook_map.rs`) vs
  zcode's bespoke `enabled`+`events` wrapper (`zcode.rs:344-460`).
- Two divergent registries: `client.rs:169-367` `REGISTRY` vs
  `app/mod.rs:2187-2264` `client_effects` — qwen-code and ollama exist only in
  the latter, a live drift bug.
- Admission evidence hardcoded per machine: `claude.rs:278`
  (`/Users/admin/Central/CLAUDE.md`), `zcode.rs:170` — surveys as frozen
  strings, never re-read.
- MCP: seventeen adapters, zero reads, zero writes; only static census prose.
- Meanwhile the Actuation catalog *already carries* the declarative data a
  general engine needs per harness: `CapabilitySeam{config_path, format,
  entry_shape, ownership_marker, preserves_foreign_entries}`, `native_events`
  with transports, `model_dispatch` (claude/codex config-key bindings, zcode/pi
  declared `none` with reasons), probes and typed facet inventories.

## The outline

`aikit.harness-profile/v1` — one declarative document per harness, joining the
Actuation catalog record (what the harness *is*) to AIKit handling (what we do
about it). Sections are **layers**; each layer declares three aspects:

```text
layer      observe (detection)         project (composition)        disclose (faculties/UI)
--------   -------------------------   --------------------------   -------------------------
presence   probes (executable,         —                            detection receipts
           config-dir, service, env)
skills     skill trees + counts        skill-tree render (prefix,   NativeSkills, LiveReload
           (typed inventory)           export/payload rules,        isolation posture
                                       shared-tree policy)
guidance   AGENTS.md / rules files     generated-doc render         StandingInstructions,
                                       (single rule file,           ProjectInstructions
                                       hints file)
hooks      hook seam census            record merge into seam       SessionStartHook, events,
           (events, transports)        (grammar + matchers +        blocking semantics
                                       sweep + ownership)
tools      mcp-config inventories      mcp-servers record merge     ToolProtocol,
           (per config file, per       (entries as records,         NativeToolContribution
           scope)                      preserve foreign, sweep      + per-session transport
                                       stale)                       (mcp_servers on open)
models     model_dispatch + model      config-key / policy          routing posture (native
           provider inventory          binding; roster facts        binding, provider-plural,
                                                                    declared-none) + roster
                                                                    scope facts
sessions   protocol family census      — (launch-time only)         ConnectionCapabilities,
           (ACP/RPC/process)                                        SessionOpenMode set,
                                                                    refusal boundaries
settings   config files census         (carried by layers above)    install seam, edition
```

### Ownership posture per layer

The superset/subset relation is made explicit as an ownership decision recorded
per layer, one of:

- **`managed`** — AIKit projects and retracts this layer (claude skills today).
- **`brokered`** — the harness owns native state; AIKit composes through
  disclosure only (openclaw workspace, gemini authored trees — postures that
  today exist only as Rust refusals).
- **`observed`** — read-only detection feeds context and UI; nothing writes
  (MCP on every harness today, until promoted).

A layer may be `managed` for projection only where the profile records a seam
the harness truthfully re-reads; `ActivationEffect` per layer
(Immediate/LiveReload/RestartClient/NextSessionOnly/Brokered/Unsupported)
carries the activation truth, reusing the existing enum and its
`verify_activation_truth` law.

## What each layer binds to

### Carrier sources (what gets projected)

- Skills: existing skill capsules (unchanged). The `[skill]` section's
  `export_name`/`root` config keys stay; the profile's skills layer replaces
  the three copied implementations with one render driven by
  `TargetCapabilities`.
- Tools/MCP: a new capsule kind **`tool-protocol`** (or a `[tool-protocol]`
  section) whose payload is one MCP server record `{command, args, env, cwd,
  url}` — the `[config.*]` merge algebra already declares the right semantics
  (`config_merge = "replace"` was designed for exactly this). A composed set
  of tool-protocol capsules is the *capability source*; the profile's tools
  layer says where the records land (e.g. `~/.openclaw/mcp.json` root key
  `mcpServers`; zcode `mcp.servers`; claude `~/.claude.json` key `mcpServers`;
  cursor `~/.cursor/mcp.json`). Merge grammar: preserve foreign entries, sweep
  stale AIKit-owned entries by ownership identity, never emit secret values —
  the same law `hook_map.rs` already implements for hooks.
- Models: no new registry. The profile's models layer binds the catalog's
  `model_dispatch` (native binding via config-key, provider-plural, or
  declared-none-with-reason) and contributes *facts* to the existing roster:
  `ModelRosterDemand.profile`, `FitnessScope.harness_composition`,
  `harness_compatible`/`harness_capabilities` candidate gates. Encounter-level
  selection stays `ModelPolicy` (`aikit.model-dispatch-policy/v1`) pinned per
  provider body; the profile records which selector surface the harness
  natively exposes (argv flags, config key, ACP model selectors).
- Sessions: the profile declares `ConnectionProtocolFamily`, the
  `SessionOpenMode` set, and the `ConnectionCapabilities` flags as data —
  pi's Attach-only, no-MCP, no-cwd posture becomes profile facts (today
  adapter prose and hardcoded refusals). For ACP harnesses whose capability
  negotiation advertises `mcpServers: true`, the composed tool-protocol set is
  what populates `SessionOpenRequest.mcp_servers` — the field has existed on
  the wire since v1 and is hardwired empty today
  (`encounter_service.rs:759-760`).
- Hooks: unchanged carriers (hook capsules), but the merge engine becomes one
  grammar-table keyed by the profile's declared `format` (claude-map,
  zcode-wrapper, …) with the `CapabilitySeam` fields already in the catalog.

### Projection engine

`plan` becomes layer-folded: for each layer with posture `managed`, the
profile's projection declaration + the resolved capability set yield
`ProjectionItem`s (existing enum suffices: `Write`, `Copy`, `Link`, `Shim`).
Items flow through the existing `plan_install` → `WorldEdit` + `Inverse` →
`Procedure` receipt pipeline unchanged — reversibility, ownership markers and
foreign-entry preservation become properties of the profile-declared merge
grammar, implemented once.

The `install`-only limitation of today (`client.rs` refuses non-Write items)
is retained at first; link/copy layers stay with the generation projection
path they already use.

### Detection engine

Actuation stays the owner of presence and native observation (its descriptor
probe/facet grammar — including the new typed file inventories — is the right
substrate, proven with `mcp-config`). The profile binds facet kinds to layers
so AIKit's intake (`DetectionFacet` is already generic) lands each observation
in its layer. Layer inventories become uniform: skills counts, MCP server
names + redacted commands, model rosters, session-store presence — one shape:
`{entries, receipt, unavailable_reason}`.

### Registry unification

The profile replaces the static `REGISTRY` and the `client_effects` match:
one roster, keyed by catalog slug (the cross-product join), each row a profile
reference. This retires the qwen/ollama registry divergence by construction.
`Reach` (Client/AdapterOnly/SelfOwned) becomes derived: a profile with no
managed projection layers and no launch composition is AdapterOnly; SelfOwned
is AIKit's own profile.

### Reused vs new primitives

Reused unchanged: `TargetAdapter`, `TargetCapabilities`, `ProjectionPlan`/
`ActivationEffect` (+ truth verification), `WorldEdit`/`Inverse`/`Procedure`,
capsule model and `[config.*]` merge algebra, model catalogue → routes →
roster → selection stack, `ConnectionCapabilities`/`SessionOpenRequest`,
Actuation catalog + detection grammar, `ContextDisclosure` (the profile's
layer read model is exactly the `harness` slice of disclosure).

New, deliberately small:

1. `aikit.harness-profile/v1` — the outline document (schema above).
2. `tool-protocol` capsule kind — MCP server records as capability sources.
3. Layer merge-grammar table — the one implementation behind
   `hook_map.rs`/zcode-merge/mcp-record merge.
4. Layer read model — per-harness `{layer → posture, native, composed,
   activation, drift}` for status/TUI/disclosure.

A harness profile is **not** a Contract and gets no new `SurfaceKind`; the
closest existing pattern is the `CompositionBody` (scope-neutral reusable
body) wrapped by a scope-carrying relation — the profile is catalog-anchored
data consumed by resolution, not a component in the body.

## Example instance (abbreviated)

```toml
schema = "aikit.harness-profile/v1"
slug = "openclaw"                      # catalog join key
edition = "cli"

[presence]
config-dir = "~/.openclaw"

[skills]
posture = "brokered"                   # workspace files are authored identity
observe.paths = ["~/.openclaw/workspace/AGENTS.md"]

[tools.mcp]
posture = "managed"
observe = [{ path = "~/.openclaw/mcp.json", collection = "mcpServers" }]
project = { file = "~/.openclaw/mcp.json", key = "mcpServers",
            format = "mcp-servers-record", merge = "preserve-foreign-sweep-owned" }
activation = "restart-client"          # gateway re-reads MCP at start

[models]
posture = "observed"
dispatch = "none"                      # catalog-declared: no native provider binding

[sessions]
protocol = "process"
```

## Verification

- Conformance: every profile's declared faculties must match its admission
  descriptor (existing `HarnessAdmissionDescriptor` validation); every
  managed layer must declare a seam; unsupported-harness gaps derive from the
  profile, retiring the per-adapter gap boilerplate.
- Parity: per-layer projection golden tests — the same composed set projected
  through profiles must byte-match today's hand-written outputs for claude
  hooks, codex hooks, and skills trees before any hand-written path is
  retired.
- Drift: the qwen/ollama class of registry divergence becomes unrepresentable
  (single roster derived from profiles).
- Live: the bimba end-to-end (detection facet → tool-protocol capsules →
  projected `mcpServers` entry → harness re-read → tool call through the
  encounter) is the standing acceptance scenario.

## Staging

1. Schema + one profile (claude) in parallel with existing code; parity
   golden for hooks + skills.
2. Profiles for codex, zcode, pi; retire the two hook-merge copies and the
   three skills copies behind the grammar table.
3. `tool-protocol` capsule kind + MCP record merge + `mcpServers` population
   for ACP-capable sessions; openclaw/zcode/claude as first managed targets.
4. Registry unification (retire `REGISTRY` + `client_effects` divergence).
5. Model layer binding into roster facts; disclosure read model into status
   and TUI.
