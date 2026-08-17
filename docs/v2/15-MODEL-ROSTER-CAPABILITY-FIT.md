# Model roster and capability-fit read model

Status: implementation receipt for #64. This is an AIKit application/read-model surface over canonical Resources and contextual evidence; it is **not** a second Model registry.

## Ownership

- **Actuation** defines what situated model-bearing conditions mean, including the inference/control/interior distinctions represented by its `ModelAccessProfile` contract.
- **AIKit** resolves canonical Model/resource/provider/Contract/Harness/Profile/SkillSet state and owns this derived roster/ranking application view.
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

A denied, unavailable, incompatible or under-capable model cannot win on price or historical fitness.

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
- availability/provider state;
- catalog price estimate;
- exact execution spend;
- latency/reliability/context characteristics;
- inference/control/interior/local access.

Authored preference and frecency are intentionally visible in explanations but do not silently become task fitness. Exact spend is retained separately from catalog price and learned fitness.

## Provider observation fixture

`crates/aikit-core/tests/fixtures/openai-gpt-5.4-2026-08-17.json` records one real provider catalog observation made on 2026-08-17 from:

`https://developers.openai.com/api/docs/models/gpt-5.4`

The observation includes source, observation time, variant/snapshot, token pricing, context characteristics, modalities, structured-output support and tool support. It is explicitly point-in-time and must be refreshed before being treated as current provider truth.

It is not authored preference, learned fitness, account-specific availability or authorisation, or a canonical quality score.

## Human and agent projections

The semantic operation is `rank_model_roster` in `aikit-core`.

- TUI: `aikit_tui::model_roster_matrix` renders the core roster and explanation only; it does no ranking.
- CLI/headless: `aikit_cli::model_roster_text` uses the same terminal matrix and `aikit_cli::model_roster_json` serialises the same core `ModelRoster` object.
- Agent/future O:I: consume the serialisable `ModelRoster` / `ModelRankingExplanation` directly rather than scraping terminal text.

This keeps ranking logic below all presentation surfaces.

## Acceptance coverage

Tests exercise:

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
