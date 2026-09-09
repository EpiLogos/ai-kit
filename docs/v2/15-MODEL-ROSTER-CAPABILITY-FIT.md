---
register: episteme
---

# Model field, roster and capability-fit

Status: canonical architecture/design home for the Model field after #265. The #64 ranking receipt remains a layer of that field; this is an AIKit application/read-model surface over canonical Resources and contextual evidence. It is **not** a second Model registry.

## The Model field

Direction, as shipped:

```text
catalogue → availability → selection → actualisation
```

Usage returns afterward, and only afterward. The join that produces availability takes a catalogue, observed routes and credential bindings; there is no parameter through which model-usage telemetry can enter. Usage never feeds availability.

### Identity grammar

A Model is named by a canonical `ModelRef` of the form `model:<stable-id>` — one colon, then a stable id. Identity is stable across provider renames because it does not come from a provider. Provider-native ids (`llama3.2:latest`, `anthropic/claude-sonnet-4`) are opaque route metadata and are never promoted to identity.

The stale in-tree spelling `model/...` is refused at the canonical parser and migrated with disclosure at an ingestion boundary. Silently accepting both spellings would let two identities coexist for one Model.

The catalogue that supplies identity has three layers, weakest first: the first-party seed, whatever Provider Sources have published, and the owner's own entries. Later layers win by canonical `ModelRef`. Detection never writes here: a Model that exists only because something is installed today would not be a stable identity. A provider-native id no entry claims stays an unmatched offer until an owner or a Provider Source admits or maps it.

`aikit model-catalogue refresh|show` is the Provider Source plumbing. The catalogue-to-route join, selection and actualisation surface through `aikit compose --json [--realise --model <ref> [--provider <ref>]]`. There is deliberately no roster listing command.

### Three route-evidence kinds

Availability is a join, not a mint. Three independent kinds of evidence can prove a route, and none stands in for another:

1. **Provider inventory.** A detected entry classified `native_kind: "model-provider"` may carry a typed models-facet inventory (ids plus an inventory receipt). A directory count without named ids is presence, not identity, and supplies no route. A failed detection run leaves catalogued Models known-but-unproven, never absent.
2. **Router listing.** A Provider Source reads what a router or provider actually publishes. A listing proves the *router* offers an id. It proves nothing about the vendor's own API; the router route may be observed while the provider-native route stays unobserved with a reason.
3. **Detected-harness dispatch.** A harness that is genuinely detected here, and whose Actuation capability descriptor declares `model_dispatch` (catalog r6 as consumed by #265), is evidence that the named provider is reachable from this machine. Both halves are required: a declaration without detection proves nothing, and `kind: "none"` is a declared absence rather than a missing declaration.

Route shape (`provider-native`, `router-route`, `harness-native`, `local-serving`) is not identity. The same `ModelRef` may be reachable through several of these at once.

### Three standings

A route carries three distinct standings. Collapsing any two is how a catalogue starts lying:

| Standing | Meaning |
| --- | --- |
| **unproven** | Declared or catalogued; nothing has been observed that proves the route is there. Known-but-unavailable, not absent. |
| **observed** | Live evidence named the provider-native id, or a detected harness declared it can dispatch to that provider. |
| **presently usable** | Observed, and the credential the route needs is bound. A missing key makes a route unusable, not absent and not a different Model. |

Credentials record that a binding exists. They never materialise, read or carry the secret.

### Selection and actualisation

`select_model` collapses the roster's `(ModelRef, route)` pairs back onto Model identity with every viable route intact. A provider pin constrains the route, never the identity. Route loss re-resolves under the same `ModelRef`.

Actualisation composes a selected `(ModelRef, route)` into an `actuation.instantiation/v1` receipt and hands it to Actuation, which re-runs live detection and applies its own evidence gate. Workcell is asked for a material body only where a local route needs one that is not already there; it is told a model subject and an engine, and it never learns what a `ModelRef` means.

Ranking, below, is the policy layer inside selection. It does not mint Models, prove routes, or record usage as availability.

## Ownership

- **Actuation** defines what situated model-bearing conditions mean, including the inference/control/interior distinctions represented by its `ModelAccessProfile` contract, and owns the instantiation evidence gate. The harness catalog r6 `model_dispatch` descriptor is Actuation's; AIKit consumes it as route evidence.
- **AIKit** resolves canonical Model/resource/provider/Contract/Harness/Profile/SkillSet state, owns the catalogue → availability join, and owns this derived roster/ranking application view.
- **Factory** owns developmental `ExecutionDemand`, `ExecutionDisposition`, Run truth and P5 fitness/cost/return observations. AIKit may consume scoped observations without taking ownership of the Run.
- **Workcell** remains the authority for material process/service/storage/GPU/lifecycle reality. Placement/materialisation observations can affect eligibility or inspectability without becoming Model identity.

## Core invariant

A ranking is always:

```text
For demand D, under policy R, among currently eligible candidates C,
why did this candidate rank ahead of the others?
```

It is never:

```text
Model X = 94
```

`policy_score` is derived per request and exists only to explain ordering under the named policy.

## Hard gates before ranking

The implementation excludes candidates before policy scoring when any required condition fails:

- current availability;
- authorisation;
- provider usability;
- policy allowance;
- Contract compatibility;
- Harness compatibility;
- required capabilities;
- required modalities;
- required tools;
- required Contracts;
- policy-specific independence;
- for `QUALITY_UNDER_BUDGET`, a known estimate within the supplied ceiling.

A denied, unavailable, incompatible or under-capable model cannot win on price or historical fitness. An unproven route is not a candidate.

## Separate capability/body layers

Each roster row retains separate sets for:

```text
native model capabilities
harness-provided capabilities
profile/skill-supplied capabilities
observed fitness of the resulting execution body
```

The effective capability set is a derived union used only for hard capability satisfaction. The source sets remain inspectable.

## Ranking policies

The first deterministic policy set is:

- `CHEAPEST_ELIGIBLE`
- `TASK_FIT`
- `ROLE_FIT`
- `PROFILE_FIT`
- `QUALITY_UNDER_BUDGET`
- `BALANCED`
- `INDEPENDENT_REVIEWER`
- `LOCAL_INSPECTABILITY`

Where a policy uses weights, the weights are written back into `ModelRankingExplanation.components`; the weighted number is not persisted as a Model property.

## Signal separation

The read model keeps these distinct:

- hard eligibility/trust/policy gates;
- native/harness/profile capability facts;
- task, role and profile fitness;
- scoped learned/observed fitness;
- authored preference;
- frecency;
- availability/provider state, including the three route standings;
- catalog price estimate;
- exact execution spend;
- latency/reliability/context characteristics;
- inference/control/interior/local access.

Authored preference and frecency are intentionally visible in explanations but do not silently become task fitness. Exact spend is retained separately from catalog price and learned fitness. Observed fitness and usage telemetry do not become route availability.

## Provider observation fixture

`crates/aikit-core/tests/fixtures/openai-gpt-5.4-2026-08-17.json` records one real provider catalog observation made on 2026-08-17 from:

`https://developers.openai.com/api/docs/models/gpt-5.4`

The observation includes source, observation time, variant/snapshot, token pricing, context characteristics, modalities, structured-output support and tool support. It is explicitly point-in-time and must be refreshed before being treated as current provider truth.

It is not authored preference, learned fitness, account-specific availability or authorisation, or a canonical quality score.

## Human and agent projections

The semantic ranking operation remains `rank_model_roster` in `aikit-core`. Ranking logic stays below all presentation surfaces.

- Owner surfaces after #265: `aikit model-catalogue refresh|show` reads and discloses the catalogue; `aikit compose --json [--realise --model <ref> [--provider <ref>]]` runs the join, selection and (when asked) actualisation. There is deliberately no roster listing command.
- TUI: `aikit_tui::model_roster_matrix` renders a core roster and explanation only; it does no ranking.
- Library/headless: `aikit_cli::model_roster_text` uses the same terminal matrix and `aikit_cli::model_roster_json` serialises the same core `ModelRoster` object.
- Agent/future O:I: consume the serialisable `ModelRoster` / `ModelRankingExplanation` directly rather than scraping terminal text.

## Acceptance coverage

The #64 ranking suite still exercises:

1. cheapest eligible and task-fit can select different Models;
2. different use types rank the same candidate set differently;
3. Profile/Agency/body fitness remains contextual rather than becoming Model identity;
4. missing required capability is ineligible;
5. denial/availability are hard gates;
6. unknown price is explicit and never free;
7. observed fitness is used only when its provenance scope matches the demand;
8. authored preference remains separate;
9. frecency remains separate;
10. provider replacement preserves Model identity;
11. inference/control/interior/local access axes remain inspectable;
12. explanation retains gates/components/weights/missing data/provenance and loss reason;
13. Factory consumes opaque AIKit Model/provider refs rather than owning a registry;
14. Factory P5 observations carry Run provenance back to AIKit without transferring Run ownership;
15. repository CI is the final conformance gate for this change.

The #265 field suites sit beside that receipt: `model_route_availability_acceptance` carries one test per availability row; `model_realisation_end_to_end` walks Agent → selection → route → Actuation → Workcell → usage and holds that usage cannot feed availability.
