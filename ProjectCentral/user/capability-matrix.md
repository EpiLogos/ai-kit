---
uid: aikit-capability-matrix
status: draft-for-review
standing: agent-inference
tags: [aikit, capabilities, product-ground, ql]
---

# AIKit capability matrix

This companion to the [six-question account](aikit.html#whole) gives 24 functional families stable addresses. It distinguishes intended usefulness, current source scope and the evidence needed to rely on an operation. Human review of the recovered seed remains pending.


## Seed × field contribution

[View declarations](capability-matrix.json) · [Editable CSV](capability-matrix.csv). Select a populated cell for its source and capability links. Unassessed cells carry no assertion.

| Seed | O:I whole | Central | Actuation | Software Factory | Workcell | Quaternal Logic |
| --- | --- | --- | --- | --- | --- | --- |
| [Why?](aikit.html#whole/why) | [3 capabilities](#field-q0-S) | Unassessed | Unassessed | Unassessed | Unassessed | Unassessed |
| [What?](aikit.html#whole/what) | [3 capabilities](#field-q1-S) | Unassessed | Unassessed | Unassessed | Unassessed | Unassessed |
| [How?](aikit.html#whole/how) | [13 capabilities](#field-q2-S) | Unassessed | Unassessed | Unassessed | Unassessed | Unassessed |
| [Who / Whereby?](aikit.html#whole/whereby) | [4 capabilities](#field-q3-S) | [2 capabilities](#field-q3-S0) | [relation](#field-q3-S1) | [relation](#field-q3-S3) | [relation](#field-q3-S4) | [1 capabilities](#field-q3-S5) |
| [Where / When?](aikit.html#whole/context) | [2 capabilities](#field-q4-S) | Unassessed | Unassessed | Unassessed | Unassessed | Unassessed |
| [Why-For?](aikit.html#whole/purpose) | [relation](#field-q5-S) | Unassessed | Unassessed | Unassessed | Unassessed | Unassessed |

<details>
<summary>Read the field contributions and their capability links</summary>

<a id="field-q0-S"></a>

### Why? → O:I whole

Agentic work draws on tools, Skills, knowledge, models and working environments that come from many places. A person needs these resources to become a coherent working context without rebuilding their setup around each Agent. AIKit makes the relevant part of that world discoverable, explainable and usable for the work at hand.

[discovery](#cap-aikit-discovery) · [adoption](#cap-aikit-adoption) · [projection](#cap-aikit-projection)

[Source account passage](aikit.html#whole/why) · placement: agent-inference.

<a id="field-q1-S"></a>

### What? → O:I whole

AIKit is a Rust CLI, TUI and shared application toolkit for discovering agent resources, selecting them for a Project or task, and projecting the resulting configuration into supported harnesses. It also provides knowledge search and retrieval, semantic Wiki navigation, runtime composition and session integration. Its concrete product is an inspectable answer to what this actor can use and ask about here.

[resource index](#cap-aikit-resource-index) · [methods](#cap-aikit-methods) · [routines](#cap-aikit-routines)

[Source account passage](aikit.html#whole/what) · placement: agent-inference.

<a id="field-q2-S"></a>

### How? → O:I whole

AIKit registers sources, resolves scoped choices against trust and compatibility, and prepares a target-specific view. It keeps larger bodies of knowledge and instructions addressable so an Agent can retrieve what the task needs. Changes become reviewable plans or generations; explanation connects the effective environment to the choices and observations that produced it.

[context resolution](#cap-aikit-context-resolution) · [generation](#cap-aikit-generation) · [source horizon](#cap-aikit-source-horizon) · [knowledge navigation](#cap-aikit-knowledge-navigation) · [wiki write](#cap-aikit-wiki-write) · [projectcentral](#cap-aikit-projectcentral) · [living knowledge](#cap-aikit-living-knowledge) · [reflection](#cap-aikit-reflection) · [composition](#cap-aikit-composition) · [sessions](#cap-aikit-sessions) · [activation](#cap-aikit-activation) · [procedures](#cap-aikit-procedures) · [continuity](#cap-aikit-continuity)

[Source account passage](aikit.html#whole/how) · placement: agent-inference.

<a id="field-q3-S"></a>

### Who / Whereby? → O:I whole

People compose and inspect their working environment through the CLI and TUI. Agents discover powers and retrieve relevant knowledge through the same underlying services. Providers and harness adapters connect existing tools while preserving their identities. Central supplies durable source; AIKit makes permitted source and capabilities available in a particular context.

[authored relations](#cap-aikit-authored-relations) · [wiki shapes](#cap-aikit-wiki-shapes) · [explain](#cap-aikit-explain) · [familiarity](#cap-aikit-familiarity)

[Source account passage](aikit.html#whole/whereby) · placement: agent-inference.

<a id="field-q3-S0"></a>

### Who / Whereby? → Central

Central’s recognised source relations carry provenance, truth standing, roles and treatment. AIKit preserves that distinction through the resulting knowledge view.

[authored relations](#cap-aikit-authored-relations) · [projectcentral](#cap-aikit-projectcentral)

[Source account passage](aikit.html#q3/source-contract) · placement: agent-inference.

<a id="field-q3-S1"></a>

### Who / Whereby? → Actuation

AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.



[Source account passage](aikit.html#q3/identity) · placement: agent-inference.

<a id="field-q3-S3"></a>

### Who / Whereby? → Software Factory

AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.



[Source account passage](aikit.html#q3/identity) · placement: agent-inference.

<a id="field-q3-S4"></a>

### Who / Whereby? → Workcell

AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.



[Source account passage](aikit.html#q3/identity) · placement: agent-inference.

<a id="field-q3-S5"></a>

### Who / Whereby? → Quaternal Logic

The QL shape service supplies valid structural readings while semantic edges continue to require actual evidence.

[wiki shapes](#cap-aikit-wiki-shapes)

[Source account passage](aikit.html#q3/source-contract) · placement: agent-inference.

<a id="field-q4-S"></a>

### Where / When? → O:I whole

AIKit works where a Project, actor, task, client and available machine meet. A shared baseline can be refined for a Project, session or individual invocation. The resulting configuration is situated: a resource may be useful in one context and unavailable in another. Explanations and activation evidence make those differences inspectable.

[profiles](#cap-aikit-profiles) · [models](#cap-aikit-models)

[Source account passage](aikit.html#whole/context) · placement: agent-inference.

<a id="field-q5-S"></a>

### Why-For? → O:I whole

AIKit lets a person build an increasingly capable environment while keeping each encounter understandable. Skills and Methods make ways of working reusable; Wiki and source navigation make accumulated knowledge askable; explicit composition lets the same Project be met through different harnesses. Within O:I it supplies the operative resources through which authorised agency can work on durable human intention.



[Source account passage](aikit.html#whole/purpose) · placement: agent-inference.

</details>


| Capability | Useful result | Account |
|---|---|---|
| [Discover existing resources](#cap-aikit-discovery) | A provenance-bearing inventory before adoption | [Governing unit](aikit.html#q0/continuity) |
| [Adopt an existing Skill source](#cap-aikit-adoption) | Managed source with preserved originals and explicit conflicts | [Governing unit](aikit.html#q0/continuity) |
| [Index typed resources](#cap-aikit-resource-index) | Addressable resources with owners and revisions | [Governing unit](aikit.html#q1/product) |
| [Resolve a situated environment](#cap-aikit-context-resolution) | An explainable current capability and source view | [Governing unit](aikit.html#q2/start) |
| [Compose scoped Profiles](#cap-aikit-profiles) | Effective choices traceable to scope | [Governing unit](aikit.html#q4/contexts) |
| [Publish a complete resolved view](#cap-aikit-generation) | A coherent replaceable target view | [Governing unit](aikit.html#q2/start) |
| [Expose resources to a harness](#cap-aikit-projection) | Native client configuration with source lineage | [Governing unit](aikit.html#q0/continuity) |
| [Discover and retrieve source](#cap-aikit-source-horizon) | Exact source material with provenance and disclosure state | [Governing unit](aikit.html#q2/investigate) |
| [Search and traverse knowledge](#cap-aikit-knowledge-navigation) | Inspectable routes across distinct knowledge lenses | [Governing unit](aikit.html#q2/investigate) |
| [Maintain validated Wiki objects](#cap-aikit-wiki-write) | Atomic valid Wiki revision or unchanged refusal | [Governing unit](aikit.html#q2/maintain) |
| [Compile source relationships](#cap-aikit-authored-relations) | Resolved edges and retained ambiguous or unresolved evidence | [Governing unit](aikit.html#q3/source-contract) |
| [Encounter ProjectCentral source](#cap-aikit-projectcentral) | Source identity and Central-issued standing retained | [Governing unit](aikit.html#q2/investigate) |
| [Inspect change impact](#cap-aikit-living-knowledge) | A visible basis for deliberate refresh and return | [Governing unit](aikit.html#q2/maintain) |
| [Read explicit QL structures](#cap-aikit-wiki-shapes) | Inspectable structural reading with source basis | [Governing unit](aikit.html#q3/source-contract) |
| [Connect meaning and implementation](#cap-aikit-reflection) | A discrepancy or relationship grounded in both sources | [Governing unit](aikit.html#q2/investigate) |
| [Compose situated practice](#cap-aikit-methods) | Practice bound to purpose inputs and expected Return | [Governing unit](aikit.html#q1/praxis) |
| [Establish repeatable Method use](#cap-aikit-routines) | Explainable Routine eligibility over a Method | [Governing unit](aikit.html#q1/praxis) |
| [Compose a runtime body](#cap-aikit-composition) | A body reading with compatibility and limits | [Governing unit](aikit.html#q2/compose) |
| [Maintain portable session relations](#cap-aikit-sessions) | A traceable session with bounded ownership | [Governing unit](aikit.html#q2/compose) |
| [Inspect model and provider fit](#cap-aikit-models) | Candidate selection with explicit provider evidence | [Governing unit](aikit.html#q4/providers) |
| [Record effective instruction activation](#cap-aikit-activation) | Standing selection and activation remain independently inspectable | [Governing unit](aikit.html#q2/start) |
| [Review and reverse external changes](#cap-aikit-procedures) | Attributable effects with a recovery route | [Governing unit](aikit.html#q2/compose) |
| [Explain state and history](#cap-aikit-explain) | Selection and change traces useful for diagnosis | [Governing unit](aikit.html#q3/resolution) |
| [Improve access through use](#cap-aikit-familiarity) | Faster access with exact refs and authority preserved | [Governing unit](aikit.html#q3/resolution) |
| [Meet standing knowledge and participants on re-entry](#cap-aikit-continuity) | Durable facts injected once per scope; disclosure names trigger, horizon and source | [Governing unit](aikit.html#q0/continuity) |

## Native CLI command catalogue

The maintained command mapping was recovered from recursive `target/debug/aikit --help` after a local `cargo build -p aikit-cli --bin aikit`. Each value below is an executable leaf path without the `aikit` prefix; the exact, machine-readable mapping and its code basis are retained in the linked capability's CSV record. `aikit capabilities list --json` is a separate, context-dependent broker export list: it returned no commands in this inspected context and is not this static CLI catalogue.

<!-- cli-catalog:start -->
| CLI identity | Capability |
| --- | --- |
| `adopt` | [cap.aikit.adoption](#cap-aikit-adoption) |
| `apply` | [cap.aikit.generation](#cap-aikit-generation) |
| `bypass issue` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `bypass list` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `bypass revoke` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `bypasses` | [cap.aikit.explain](#cap-aikit-explain) |
| `capabilities list` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
| `capabilities read` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
| `capture` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `client install` | [cap.aikit.projection](#cap-aikit-projection) |
| `client launch` | [cap.aikit.projection](#cap-aikit-projection) |
| `client status` | [cap.aikit.projection](#cap-aikit-projection) |
| `collate` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `compose` | [cap.aikit.composition](#cap-aikit-composition) |
| `context bind` | [cap.aikit.activation](#cap-aikit-activation) |
| `context current` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `context env` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `context list` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `context reset` | [cap.aikit.activation](#cap-aikit-activation) |
| `credential explain` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `credential list` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `credential setup` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `diff` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `disable` | [cap.aikit.activation](#cap-aikit-activation) |
| `doctor` | [cap.aikit.explain](#cap-aikit-explain) |
| `enable` | [cap.aikit.activation](#cap-aikit-activation) |
| `explain` | [cap.aikit.explain](#cap-aikit-explain) |
| `failures` | [cap.aikit.explain](#cap-aikit-explain) |
| `gateway discover` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `gateway ecology` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `gateway protocol` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `gateway serve` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `gateway snapshot` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `gateway status` | [cap.aikit.gateway](#cap-aikit-gateway) |
| `history` | [cap.aikit.explain](#cap-aikit-explain) |
| `hook dispatch` | [cap.aikit.projection](#cap-aikit-projection) |
| `inbox` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `init` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `jobs` | [cap.aikit.explain](#cap-aikit-explain) |
| `knowledge explain` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) |
| `knowledge forget all` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `knowledge forget destination` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `knowledge forget project` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `knowledge forget route` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `knowledge frame` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.reflection](#cap-aikit-reflection) · [cap.aikit.wiki-shapes](#cap-aikit-wiki-shapes) |
| `knowledge history` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `knowledge read` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.source-horizon](#cap-aikit-source-horizon) |
| `knowledge relations` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) |
| `knowledge route` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.reflection](#cap-aikit-reflection) |
| `knowledge search` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.source-horizon](#cap-aikit-source-horizon) |
| `knowledge sources` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.source-horizon](#cap-aikit-source-horizon) |
| `knowledge status` | [cap.aikit.knowledge-navigation](#cap-aikit-knowledge-navigation) · [cap.aikit.living-knowledge](#cap-aikit-living-knowledge) |
| `log export` | [cap.aikit.explain](#cap-aikit-explain) |
| `method list` | [cap.aikit.methods](#cap-aikit-methods) |
| `mux detect` | [cap.aikit.projection](#cap-aikit-projection) |
| `mux install` | [cap.aikit.projection](#cap-aikit-projection) |
| `procedure diff` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `procedure list` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `procedure plan adopt` | [cap.aikit.adoption](#cap-aikit-adoption) |
| `procedure plan profile-fork` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `procedure run` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `procedure undo` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `profile diff` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `profile fork` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `project bind` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `project defaults` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `project show` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `promote` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `prune` | [cap.aikit.generation](#cap-aikit-generation) |
| `recent` | [cap.aikit.familiarity](#cap-aikit-familiarity) |
| `rollback` | [cap.aikit.generation](#cap-aikit-generation) |
| `run` | [cap.aikit.familiarity](#cap-aikit-familiarity) |
| `search` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
| `session attach` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `session diff` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `session down` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `session list` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `session reconcile` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `session up` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `set add` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `set create` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `set delete` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `set list` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `set remove` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `set rename` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `set show` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `shell init` | [cap.aikit.familiarity](#cap-aikit-familiarity) |
| `skill overlay clear` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `skill overlay set` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `skill overlay show` | [cap.aikit.profiles](#cap-aikit-profiles) |
| `source add-directory` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `source add-git` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `source promote` | [cap.aikit.adoption](#cap-aikit-adoption) |
| `source rollback` | [cap.aikit.adoption](#cap-aikit-adoption) |
| `source set-revision` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `source show` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `source sync` | [cap.aikit.discovery](#cap-aikit-discovery) |
| `stats` | [cap.aikit.familiarity](#cap-aikit-familiarity) |
| `status` | [cap.aikit.context-resolution](#cap-aikit-context-resolution) |
| `task close` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `task list` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `task spawn` | [cap.aikit.sessions](#cap-aikit-sessions) |
| `tree` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
| `trust record` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `trust show` | [cap.aikit.procedures](#cap-aikit-procedures) |
| `ui` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
| `unused` | [cap.aikit.familiarity](#cap-aikit-familiarity) |
| `use` | [cap.aikit.activation](#cap-aikit-activation) |
| `wiki edge add` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki node create` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki node update` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki root adopt` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki root anchor` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki root doctor` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki root prune` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki space create` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki space link` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki stage` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `wiki validate` | [cap.aikit.wiki-write](#cap-aikit-wiki-write) |
| `z` | [cap.aikit.resource-index](#cap-aikit-resource-index) |
<!-- cli-catalog:end -->

<a id="cap-aikit-discovery"></a>


## Discover existing resources

`cap.aikit.discovery` · #aikit #capability

**Need:** Find tools and Skills already present. **Operation:** Inspect registered and foreign source roots. **Outcome:** A provenance-bearing inventory before adoption.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q0/continuity) · [Intent/source](../../README.md) · [Implementation](../../crates/aikit-adapters/src/local_source_discovery.rs) · [Functional test source](../../crates/aikit-cli/tests/foreign_discover.rs)

<a id="cap-aikit-adoption"></a>
## Adopt an existing Skill source

`cap.aikit.adoption` · #aikit #capability

**Need:** Manage useful existing material deliberately. **Operation:** Preview adoption and apply a reviewed change. **Outcome:** Managed source with preserved originals and explicit conflicts.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q0/continuity) · [Intent/source](../../README.md) · [Implementation](../../crates/aikit-cli/src/main.rs) · [Functional test source](../../crates/aikit-cli/tests/adopt_command.rs)

<a id="cap-aikit-resource-index"></a>
## Index typed resources

`cap.aikit.resource-index` · #aikit #capability

**Need:** Find a capability by identity or description. **Operation:** Build and search typed provider descriptors. **Outcome:** Addressable resources with owners and revisions.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q1/product) · [Intent/source](../../docs/v2/01-PRODUCT-AND-OWNERSHIP.md) · [Implementation](../../crates/aikit-core/src/resource/index.rs) · [Functional test source](../../crates/aikit-core/tests/v2_resource_foundation.rs)

<a id="cap-aikit-context-resolution"></a>
## Resolve a situated environment

`cap.aikit.context-resolution` · #aikit #capability

**Need:** Know what applies to this Project and act. **Operation:** Resolve scoped declarations and eligibility. **Outcome:** An explainable current capability and source view.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/start) · [Intent/source](../../docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md) · [Implementation](../../crates/aikit-core/src/context_resolution.rs) · [Functional test source](../../crates/aikit-core/tests/context_resolution_v2.rs)

<a id="cap-aikit-profiles"></a>
## Compose scoped Profiles

`cap.aikit.profiles` · #aikit #capability

**Need:** Reuse a baseline while expressing local needs. **Operation:** Select and compose Profile contributions. **Outcome:** Effective choices traceable to scope.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q4/contexts) · [Intent/source](../../docs/v2/18-PROFILE-COMPOSITION-APPLICATION-PARITY.md) · [Implementation](../../crates/aikit-core/src/composition.rs) · [Functional test source](../../crates/aikit-core/tests/profile_composition_v2.rs)

<a id="cap-aikit-generation"></a>
## Publish a complete resolved view

`cap.aikit.generation` · #aikit #capability

**Need:** Apply a predictable capability selection. **Operation:** Build and publish generated state against its basis. **Outcome:** A coherent replaceable target view.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/start) · [Intent/source](../../docs/ARCHITECTURE.md) · [Implementation](../../crates/aikit-core/src/projection.rs) · [Functional test source](../../crates/aikit-core/tests/projection.rs)

<a id="cap-aikit-projection"></a>
## Expose resources to a harness

`cap.aikit.projection` · #aikit #capability

**Need:** Use the selected capabilities in a supported client. **Operation:** Render adapter-specific projection. **Outcome:** Native client configuration with source lineage.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q0/continuity) · [Intent/source](../../docs/AGENT-HARNESS-INTEGRATION.md) · [Implementation](../../crates/aikit-core/src/projection.rs) · [Functional test source](../../crates/aikit-adapters/tests/codex.rs)

<a id="cap-aikit-source-horizon"></a>
## Discover and retrieve source

`cap.aikit.source-horizon` · #aikit #capability

**Need:** Ask about a broad body of knowledge selectively. **Operation:** List eligible descriptors then explicitly retrieve. **Outcome:** Exact source material with provenance and disclosure state.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/investigate) · [Intent/source](../../docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md) · [Implementation](../../crates/aikit-core/src/context_source.rs) · [Functional test source](../../crates/aikit-core/tests/context_sources_v2.rs)

<a id="cap-aikit-knowledge-navigation"></a>
## Search and traverse knowledge

`cap.aikit.knowledge-navigation` · #aikit #capability

**Need:** Move from a question to related source. **Operation:** Search providers and expand a bounded neighbourhood. **Outcome:** Inspectable routes across distinct knowledge lenses.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/investigate) · [Intent/source](../../registry/capsules/skill/aikit/knowledge-navigation/payload/SKILL.md) · [Implementation](../../crates/aikit-core/src/knowledge_navigation.rs) · [Functional test source](../../crates/aikit-cli/tests/knowledge_application_v2.rs)

<a id="cap-aikit-wiki-write"></a>
## Maintain validated Wiki objects

`cap.aikit.wiki-write` · #aikit #capability

**Need:** Record navigational understanding durably. **Operation:** Explicit node edge space mutations and validation. **Outcome:** Atomic valid Wiki revision or unchanged refusal.

**Scope:** Implemented CLI; 13 isolated-file Wiki command tests passed in coordinating pass on 2026-09-06.

[Account](aikit.html#q2/maintain) · [Intent/source](../../registry/capsules/skill/aikit/wiki-inhabitation/payload/SKILL.md) · [Implementation](../../crates/aikit-cli/src/wiki.rs) · [Functional test source](../../crates/aikit-cli/tests/wiki_commands.rs)

<a id="cap-aikit-authored-relations"></a>
## Compile source relationships

`cap.aikit.authored-relations` · #aikit #capability

**Need:** Use links already present in supported text. **Operation:** Parse Markdown links and OKF properties with source spans. **Outcome:** Resolved edges and retained ambiguous or unresolved evidence.

**Scope:** Implemented Markdown and OKF relation path; 21 selected authored-Wiki adapter tests passed on 2026-09-06; HTML convention is not automatic staging input.

[Account](aikit.html#q3/source-contract) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/126.json) · [Implementation](../../crates/aikit-adapters/src/authored_wiki_source.rs) · [Functional test source](../../crates/aikit-adapters/src/authored_wiki_source.rs)

<a id="cap-aikit-projectcentral"></a>
## Encounter ProjectCentral source

`cap.aikit.projectcentral` · #aikit #capability

**Need:** Enter human and Agent project material faithfully. **Operation:** Discover descriptors and explicitly read permitted source. **Outcome:** Source identity and Central-issued standing retained.

**Scope:** Implemented source contract. The current knowledge service change can obtain the Central Wiki reading through the configured `ctrl` executable when the Central root is present; linked tests were not freshly executed in this documentation pass.

[Account](aikit.html#q2/investigate) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/102.comments.json) · [Implementation](../../crates/aikit-adapters/src/projectcentral.rs) · [Knowledge service](../../crates/aikit-cli/src/app/knowledge.rs) · [Functional test source](../../crates/aikit-adapters/src/projectcentral.rs)

<a id="cap-aikit-living-knowledge"></a>
## Inspect change impact

`cap.aikit.living-knowledge` · #aikit #capability

**Need:** Know which readings depend on changed source. **Operation:** Track dependencies and inspect affected readings. **Outcome:** A visible basis for deliberate refresh and return.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/maintain) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/118.json) · [Implementation](../../crates/aikit-core/src/knowledge_living_relations.rs) · [Functional test source](../../crates/aikit-core/tests/living_knowledge_acceptance.rs)

<a id="cap-aikit-wiki-shapes"></a>
## Read explicit QL structures

`cap.aikit.wiki-shapes` · #aikit #capability

**Need:** Navigate supplied whole and relation structure. **Operation:** Derive bounded shape addresses from the pinned contract. **Outcome:** Inspectable structural reading with source basis.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q3/source-contract) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/158.json) · [Implementation](../../crates/aikit-core/src/knowledge_wiki_shape.rs) · [Functional test source](../../crates/aikit-core/src/knowledge_wiki_shape.rs)

<a id="cap-aikit-reflection"></a>
## Connect meaning and implementation

`cap.aikit.reflection` · #aikit #capability

**Need:** Inspect whether product meaning matches code. **Operation:** Traverse explicit semantic and code bindings. **Outcome:** A discrepancy or relationship grounded in both sources.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/investigate) · [Intent/source](../../docs/v2/21-PROJECT-REFLECTION-AND-LOCAL-ARTICULATION.md) · [Implementation](../../crates/aikit-core/src/project_reflection.rs) · [Functional test source](../../crates/aikit-core/tests/project_reflection_roundtrip.rs)

<a id="cap-aikit-methods"></a>
## Compose situated practice

`cap.aikit.methods` · #aikit #capability

**Need:** Use reusable Skills for a particular purpose. **Operation:** Resolve Method and SkillSet relations. **Outcome:** Practice bound to purpose inputs and expected Return.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q1/praxis) · [Intent/source](../../docs/v2/20-PRAXIS-METHODS-AND-SKILL-COMPOSITION.md) · [Implementation](../../crates/aikit-core/src/method.rs) · [Functional test source](../../crates/aikit-core/src/method.rs)

<a id="cap-aikit-routines"></a>
## Establish repeatable Method use

`cap.aikit.routines` · #aikit #capability

**Need:** Repeat proven work under declared conditions. **Operation:** Validate proof basis trigger and authority relations. **Outcome:** Explainable Routine eligibility over a Method.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q1/praxis) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/164.json) · [Implementation](../../crates/aikit-core/src/routine.rs) · [Functional test source](../../crates/aikit-core/src/routine.rs)

<a id="cap-aikit-composition"></a>
## Compose a runtime body

`cap.aikit.composition` · #aikit #capability

**Need:** Select Components that fit the work. **Operation:** Resolve requirements contributions and provider bindings. **Outcome:** A body reading with compatibility and limits.

**Scope:** Implemented source contract. The current adapter change preserves an explicit failed connection result in the portable Actuation stream projection; linked tests were not freshly executed in this documentation pass.

[Account](aikit.html#q2/compose) · [Intent/source](../../docs/v2/09-COMPOSABLE-RUNTIME-ENVIRONMENTS.md) · [Implementation](../../crates/aikit-core/src/composition.rs) · [Adapter projection](../../crates/aikit-adapters/src/actuation_stream_projection.rs) · [Functional test source](../../crates/aikit-core/tests/composition_v2.rs)

<a id="cap-aikit-sessions"></a>
## Maintain portable session relations

`cap.aikit.sessions` · #aikit #capability

**Need:** Continue work through usable surfaces. **Operation:** Inspect declare compare and reconcile session topology. **Outcome:** A traceable session with bounded ownership.

**Scope:** Implemented source contract. The current adapter change preserves a Pi-RPC protocol identity when a connection becomes a SessionSpace reading, and represents an explicit failed turn as terminal; linked tests were not freshly executed in this documentation pass.

[Account](aikit.html#q2/compose) · [Intent/source](../../README.md) · [Implementation](../../crates/aikit-core/src/session_space.rs) · [Connection adapters](../../crates/aikit-adapters/src/agent_connection.rs) · [Session host](../../crates/aikit-adapters/src/agent_session_host.rs) · [Session projection](../../crates/aikit-adapters/src/session_space_connection.rs) · [Adapter surface registration](../../crates/aikit-adapters/src/lib.rs) · [Functional test source](../../crates/aikit-cli/tests/session_integration.rs)

<a id="cap-aikit-gateway"></a>
## Serve the agency contact plane

`cap.aikit.gateway` · #aikit #capability

**Need:** A persistent, owner-only contact plane for situated agency. **Operation:** Serve and query the Agency Gateway over its well-known carriers; probe presence through doctor. **Outcome:** Bootstrap-verified gateway presence; connector ingress; authorised ecology reading over situated sessions.

**Scope:** Implemented source contract; the bootstrap fold lands the well-known default endpoint, the CLI front door and the doctor probe, with live workcell acceptance pending as the sixth execution unit.

[Account](aikit.html#q0/continuity) · [Intent/source](../../docs/USING-AIKIT.md) · [Runtime](../../crates/aikit-adapters/src/gateway_runtime.rs) · [Service carriers](../../crates/aikit-adapters/src/gateway_service.rs) · [Client](../../crates/aikit-adapters/src/gateway_client.rs) · [Connector SDK](../../crates/aikit-adapters/src/gateway_connector.rs) · [CLI operations](../../crates/aikit-cli/src/gateway_ops.rs) · [Doctor probe](../../crates/aikit-cli/src/doctor.rs) · [Functional test source](../../crates/aikit-cli/tests/gateway_command.rs)

<a id="cap-aikit-continuity"></a>
## Meet standing knowledge and participants on re-entry

`cap.aikit.continuity` · #aikit #capability

**Need:** A person or Agent re-entering a project meets its standing knowledge and present participants without reconstructing them from chat history. **Operation:** Compose SessionStart disclosure from authored carriers and world binding; activate declared KnowledgeDomains on hook prompts with rendered-content dedup scoped to the session. **Outcome:** Durable facts enter context once per scope while standing rules reassert every time; volatile state stays in the temporal fields; injection names its trigger, horizon and source.

**Scope:** Implemented source contract; the SessionStart disclosure and domain-activation cycles were proven live end-to-end (entity disclosure W10 V6; domain activation CASE 03).

[Account](aikit.html#q0/continuity) · [Disclosure capsule](../../registry/capsules/hook/continuity/entity-disclosure/manifest.toml) · [Domain capsule](../../registry/capsules/hook/continuity/domain-activation/manifest.toml) · [Disclosure engine](../../crates/aikit-core/src/continuity.rs) · [Domain engine](../../crates/aikit-core/src/domain.rs) · [Hook activation](../../crates/aikit-cli/src/domain_activation.rs) · [Injection ledger](../../crates/aikit-store/src/index.rs) · [Functional test source](../../crates/aikit-cli/tests/domain_activation.rs)

<a id="cap-aikit-models"></a>
## Inspect model and provider fit

`cap.aikit.models` · #aikit #capability

**Need:** Choose an available model for actual requirements. **Operation:** Evaluate roster observations and capability fit. **Outcome:** Candidate selection with explicit provider evidence.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q4/providers) · [Intent/source](../../docs/v2/15-MODEL-ROSTER-CAPABILITY-FIT.md) · [Implementation](../../crates/aikit-core/src/model_roster.rs) · [Functional test source](../../crates/aikit-core/tests/model_roster_acceptance.rs)

<a id="cap-aikit-activation"></a>
## Record effective instruction activation

`cap.aikit.activation` · #aikit #capability

**Need:** Know what a target made operative. **Operation:** Carry evidence-bearing activation receipts in ContextResolution. **Outcome:** Standing selection and activation remain independently inspectable.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/start) · [Intent/source](../../../github-recovery-mirror/mirror/ai-kit/issues/153.comments.json) · [Implementation](../../crates/aikit-core/src/context_activation.rs) · [Functional test source](../../crates/aikit-core/tests/context_activation_v2.rs)

<a id="cap-aikit-procedures"></a>
## Review and reverse external changes

`cap.aikit.procedures` · #aikit #capability

**Need:** Change an environment with a visible boundary. **Operation:** Plan execute and undo a Procedure. **Outcome:** Attributable effects with a recovery route.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q2/compose) · [Intent/source](../../docs/SPEC-II-PROCEDURES-AND-INBOX.md) · [Implementation](../../crates/aikit-core/src/procedure.rs) · [Functional test source](../../crates/aikit-cli/tests/procedure_commands.rs)

<a id="cap-aikit-explain"></a>
## Explain state and history

`cap.aikit.explain` · #aikit #capability

**Need:** Understand how the effective world arose. **Operation:** Read causal explanation and recorded changes. **Outcome:** Selection and change traces useful for diagnosis.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q3/resolution) · [Intent/source](../../docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md) · [Implementation](../../crates/aikit-core/src/explain_history.rs) · [Functional test source](../../crates/aikit-cli/tests/explain_history_production_v2.rs)

<a id="cap-aikit-familiarity"></a>
## Improve access through use

`cap.aikit.familiarity` · #aikit #capability

**Need:** Reach familiar relevant resources efficiently. **Operation:** Rank eligible candidates from recorded use. **Outcome:** Faster access with exact refs and authority preserved.

**Scope:** Implemented source contract; linked tests not freshly executed in this documentation pass.

[Account](aikit.html#q3/resolution) · [Intent/source](../../docs/SPEC-III-SKILLSETS-AND-FRECENCY.md) · [Implementation](../../crates/aikit-core/src/frecency.rs) · [Functional test source](../../crates/aikit-core/tests/familiarity_v2.rs)

## Suite directed relation field

The `suite-relations` view uses the same CSV contract as `product-field`. Its H/A identifiers retain the human-facing and agent-facing orientations of the six products. Source-defined relation readings and annotations remain attached to each determination in `extensions`; coverage is independent of implementation status. The manifest declares the selected scope and axis meaning.

## Full lossless records

```csv
id,record_type,view_id,row_id,column_id,capability_refs,need,operation,outcome,implementation_status,standing,source_refs,code_refs,test_refs,account_ref,relation,coverage,extensions,question
cap.aikit.discovery,capability,,,,[],Find tools and Skills already present,Inspect registered and foreign source roots,A provenance-bearing inventory before adoption,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,README.md,crates/aikit-adapters/src/local_source_discovery.rs,crates/aikit-cli/tests/foreign_discover.rs,aikit.html#q0/continuity,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-adapters/src/local_source_discovery.rs"": ""5757fdbd6f089e4302c34a10d665a86d104d7f894be3189ca0a0550c6724aa04""}}, ""cli_commands"": [""init"", ""collate"", ""source add-directory"", ""source add-git"", ""source set-revision"", ""source sync"", ""source show""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.adoption,capability,,,,[],Manage useful existing material deliberately,Preview adoption and apply a reviewed change,Managed source with preserved originals and explicit conflicts,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,README.md,crates/aikit-cli/src/main.rs,crates/aikit-cli/tests/adopt_command.rs,aikit.html#q0/continuity,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97"", ""hermes-nara:session:2026-09-07-gateway-reconcile"", ""EpiLogos/ai-kit#192""], ""code_basis"": {""crates/aikit-cli/src/main.rs"": ""99c60e6ad973f15a764fe9f2719923ab6538c06ef95c1b8cf01e6a6d502ede1f""}}, ""cli_commands"": [""adopt"", ""source promote"", ""source rollback"", ""procedure plan adopt""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.resource-index,capability,,,,[],Find a capability by identity or description,Build and search typed provider descriptors,Addressable resources with owners and revisions,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/01-PRODUCT-AND-OWNERSHIP.md,crates/aikit-core/src/resource/index.rs,crates/aikit-core/tests/v2_resource_foundation.rs,aikit.html#q1/product,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/resource/index.rs"": ""ae9da277b48c956d648d30c3bfaaeafc71b3961a954d7a41e883bd0d5a772466""}}, ""cli_commands"": [""search"", ""z"", ""tree"", ""ui"", ""capabilities list"", ""capabilities read""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.context-resolution,capability,,,,[],Know what applies to this Project and act,Resolve scoped declarations and eligibility,An explainable current capability and source view,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md,crates/aikit-core/src/context_resolution.rs,crates/aikit-core/tests/context_resolution_v2.rs,aikit.html#q2/start,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/context_resolution.rs"": ""1276dad27000b3b118119158788a1e62e8b3d7258ea999d6d847b6c8d1526fdd""}}, ""cli_commands"": [""project bind"", ""project show"", ""project defaults"", ""status"", ""diff"", ""context current"", ""context list"", ""context env"", ""credential setup"", ""credential explain"", ""credential list""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.profiles,capability,,,,[],Reuse a baseline while expressing local needs,Select and compose Profile contributions,Effective choices traceable to scope,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/18-PROFILE-COMPOSITION-APPLICATION-PARITY.md,crates/aikit-core/src/composition.rs,crates/aikit-core/tests/profile_composition_v2.rs,aikit.html#q4/contexts,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/composition.rs"": ""d7f875b975656b956d3e70efb031d6ab649c601a4a28f65a9c542e666fd3cc88""}}, ""cli_commands"": [""profile fork"", ""profile diff"", ""set list"", ""set show"", ""set create"", ""set add"", ""set remove"", ""skill overlay set"", ""skill overlay show"", ""skill overlay clear""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.generation,capability,,,,[],Apply a predictable capability selection,Build and publish generated state against its basis,A coherent replaceable target view,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/ARCHITECTURE.md,crates/aikit-core/src/projection.rs,crates/aikit-core/tests/projection.rs,aikit.html#q2/start,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/projection.rs"": ""0bacae1454af8d349951c5c439136ab7048b2cf0b7d24f2fb492877c48ab17f7""}}, ""cli_commands"": [""apply"", ""rollback"", ""prune""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.projection,capability,,,,[],Use the selected capabilities in a supported client,Render adapter-specific projection,Native client configuration with source lineage,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/AGENT-HARNESS-INTEGRATION.md,crates/aikit-core/src/projection.rs,crates/aikit-adapters/tests/codex.rs,aikit.html#q0/continuity,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/projection.rs"": ""0bacae1454af8d349951c5c439136ab7048b2cf0b7d24f2fb492877c48ab17f7""}}, ""cli_commands"": [""client install"", ""client launch"", ""client status"", ""mux install"", ""mux detect"", ""hook dispatch""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.source-horizon,capability,,,,[],Ask about a broad body of knowledge selectively,List eligible descriptors then explicitly retrieve,Exact source material with provenance and disclosure state,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md,crates/aikit-core/src/context_source.rs,crates/aikit-core/tests/context_sources_v2.rs,aikit.html#q2/investigate,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/context_source.rs"": ""c2b25168412d98f12c6cd0411d871e1e503f26c3ef8fa560951e23c017387380""}}, ""cli_commands"": [""knowledge search"", ""knowledge read"", ""knowledge sources""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.knowledge-navigation,capability,,,,[],Move from a question to related source,Search providers and expand a bounded neighbourhood,Inspectable routes across distinct knowledge lenses,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,registry/capsules/skill/aikit/knowledge-navigation/payload/SKILL.md,crates/aikit-core/src/knowledge_navigation.rs,crates/aikit-cli/tests/knowledge_application_v2.rs,aikit.html#q2/investigate,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/knowledge_navigation.rs"": ""4cc20260d4ec8cb01d212fbc84bab906720540ca61b05162cd0ccbaeda097e64""}}, ""cli_commands"": [""knowledge search"", ""knowledge relations"", ""knowledge frame"", ""knowledge explain"", ""knowledge status"", ""knowledge read"", ""knowledge route"", ""knowledge sources"", ""knowledge history"", ""knowledge forget destination"", ""knowledge forget route"", ""knowledge forget project"", ""knowledge forget all""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.wiki-write,capability,,,,[],Record navigational understanding durably,Explicit node edge space mutations and validation,Atomic valid Wiki revision or unchanged refusal,Implemented CLI; 13 isolated-file Wiki command tests passed in coordinating pass on 2026-09-06,agent-inference,registry/capsules/skill/aikit/wiki-inhabitation/payload/SKILL.md,crates/aikit-cli/src/wiki.rs,crates/aikit-cli/tests/wiki_commands.rs,aikit.html#q2/maintain,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-cli/src/wiki.rs"": ""90b39c0190737a813352418df4a36484feb4d8d4041a13a355e22ade789b83f1""}}, ""cli_commands"": [""wiki validate"", ""wiki node create"", ""wiki node update"", ""wiki edge add"", ""wiki space create"", ""wiki space link"", ""wiki root doctor"", ""wiki root prune"", ""wiki root adopt"", ""wiki root anchor"", ""wiki stage""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.authored-relations,capability,,,,[],Use links already present in supported text,Parse Markdown links and OKF properties with source spans,Resolved edges and retained ambiguous or unresolved evidence,Implemented Markdown and OKF relation path; 21 selected authored-Wiki adapter tests passed on 2026-09-06; HTML convention is not automatic staging input,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/126.json,crates/aikit-adapters/src/authored_wiki_source.rs,crates/aikit-adapters/src/authored_wiki_source.rs,aikit.html#q3/source-contract,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-adapters/src/authored_wiki_source.rs"": ""eb64cef405fd085bc52a010410f910e11384a852fea60c8e6bbca10a3c0351e7""}}, ""cli_commands"": [], ""cli_exposure"": {""kind"": ""library"", ""reason"": ""The authored-relation compiler is implemented as an adapter; this CLI has no dedicated relation-compilation command.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.projectcentral,capability,,,,[],Enter human and Agent project material faithfully,Discover descriptors and explicitly read permitted source,Source identity and Central-issued standing retained,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/102.comments.json,crates/aikit-adapters/src/projectcentral.rs;crates/aikit-cli/src/app/knowledge.rs,crates/aikit-adapters/src/projectcentral.rs,aikit.html#q2/investigate,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97"", ""hermes-nara:session:2026-09-07-gateway-reconcile"", ""EpiLogos/ai-kit#192""], ""code_basis"": {""crates/aikit-adapters/src/projectcentral.rs"": ""9e7867648b213f65b3255a61ee8789d7d72ca46ab022d656a0c6db7612971ed3"", ""crates/aikit-cli/src/app/knowledge.rs"": ""5c8d7bfab0a6a061e7e88f9bc4131d9e4293cee43c771478b7331f119d13c9c0""}}, ""cli_commands"": [], ""cli_exposure"": {""kind"": ""composed"", ""reason"": ""ProjectCentral material is encountered through the shared knowledge faculty; this CLI has no dedicated ProjectCentral command.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.living-knowledge,capability,,,,[],Know which readings depend on changed source,Track dependencies and inspect affected readings,A visible basis for deliberate refresh and return,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/118.json,crates/aikit-core/src/knowledge_living_relations.rs,crates/aikit-core/tests/living_knowledge_acceptance.rs,aikit.html#q2/maintain,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/knowledge_living_relations.rs"": ""fe1951920d937715368e599dbe0e92ecbde0404467eb141e352e376eea371aa4""}}, ""cli_commands"": [""knowledge status"", ""knowledge history"", ""knowledge forget destination"", ""knowledge forget route"", ""knowledge forget project"", ""knowledge forget all""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.wiki-shapes,capability,,,,[],Navigate supplied whole and relation structure,Derive bounded shape addresses from the pinned contract,Inspectable structural reading with source basis,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/158.json,crates/aikit-core/src/knowledge_wiki_shape.rs,crates/aikit-core/src/knowledge_wiki_shape.rs,aikit.html#q3/source-contract,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97"", ""hermes-nara:session:2026-09-07-gateway-reconcile"", ""EpiLogos/ai-kit#192""], ""code_basis"": {""crates/aikit-core/src/knowledge_wiki_shape.rs"": ""53fe3f67d0c4d2a3857a25039ad000ed26be17888e401b1f3054fd2616955c62""}}, ""cli_commands"": [""knowledge frame""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.reflection,capability,,,,[],Inspect whether product meaning matches code,Traverse explicit semantic and code bindings,A discrepancy or relationship grounded in both sources,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/21-PROJECT-REFLECTION-AND-LOCAL-ARTICULATION.md,crates/aikit-core/src/project_reflection.rs,crates/aikit-core/tests/project_reflection_roundtrip.rs,aikit.html#q2/investigate,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/project_reflection.rs"": ""56b3e46f1705d71add0e0455b6ba04c79b7f6491203d805fd6ad5494017dda27""}}, ""cli_commands"": [""knowledge frame"", ""knowledge route""], ""cli_exposure"": {""kind"": ""composed"", ""reason"": ""The CLI composes reflection through knowledge routes and frames; it has no dedicated reflection verb.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.methods,capability,,,,[],Use reusable Skills for a particular purpose,Resolve Method and SkillSet relations,Practice bound to purpose inputs and expected Return,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/20-PRAXIS-METHODS-AND-SKILL-COMPOSITION.md,crates/aikit-core/src/method.rs,crates/aikit-core/src/method.rs,aikit.html#q1/praxis,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/method.rs"": ""e76a19eff12777ef0db8288259921a903d9dd81fb5183148599f89c4a0edfeaa""}}, ""cli_commands"": [""method list""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.routines,capability,,,,[],Repeat proven work under declared conditions,Validate proof basis trigger and authority relations,Explainable Routine eligibility over a Method,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/164.json,crates/aikit-core/src/routine.rs,crates/aikit-core/src/routine.rs,aikit.html#q1/praxis,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/routine.rs"": ""0ebba5132c3cfff2d999afc7bc7520da6a44daf597b986b7ef9b751dd7e5036d""}}, ""cli_commands"": [], ""cli_exposure"": {""kind"": ""library"", ""reason"": ""Routine eligibility is implemented in the library; this CLI has no Routine command family.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.composition,capability,,,,[],Select Components that fit the work,Resolve requirements contributions and provider bindings,A body reading with compatibility and limits,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/09-COMPOSABLE-RUNTIME-ENVIRONMENTS.md,crates/aikit-core/src/composition.rs;crates/aikit-adapters/src/actuation_stream_projection.rs,crates/aikit-core/tests/composition_v2.rs,aikit.html#q2/compose,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/composition.rs"": ""d7f875b975656b956d3e70efb031d6ab649c601a4a28f65a9c542e666fd3cc88"", ""crates/aikit-adapters/src/actuation_stream_projection.rs"": ""20c0500221852e94f90904cbeac123e1e9bd62cdeab2910ba714c60ae744da84""}}, ""cli_commands"": [""compose""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.sessions,capability,,,,[],Continue work through usable surfaces,Inspect declare compare and reconcile session topology,A traceable session with bounded ownership,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,README.md,crates/aikit-core/src/session_space.rs;crates/aikit-adapters/src/agent_connection.rs;crates/aikit-adapters/src/agent_session_host.rs;crates/aikit-adapters/src/session_space_connection.rs;crates/aikit-adapters/src/lib.rs,crates/aikit-cli/tests/session_integration.rs,aikit.html#q2/compose,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97"", ""hermes-nara:session:2026-09-07-gateway-reconcile"", ""EpiLogos/ai-kit#192""], ""code_basis"": {""crates/aikit-core/src/session_space.rs"": ""aa7bf295619d3e7d1ed2174c0f546f728812f5dbeeb6299a100a8a386bdcf5c3"", ""crates/aikit-adapters/src/agent_connection.rs"": ""43b833515d51c807ed0964223c6b87d1c5a626daa63896d1830ca16fed77b5fe"", ""crates/aikit-adapters/src/agent_session_host.rs"": ""c8decbef224508ceef0864ea3af0d4cb05132d3795a37562cbba9e3843247cf6"", ""crates/aikit-adapters/src/session_space_connection.rs"": ""bd766003243b076f8a94c5829029680c3423d6a6b54c957929ad991d71fa7639"", ""crates/aikit-adapters/src/lib.rs"": ""3d795cbfedcdc7261f71fc7f41ddcb2bdcfb9bd241a1c70f2a3a98e1a5e8506f""}}, ""cli_commands"": [""session up"", ""session attach"", ""session list"", ""session diff"", ""session reconcile"", ""session down"", ""task spawn"", ""task list"", ""task close""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build; the separately released aikit-session-space companion is accounted for in cli_companions.""}, ""last_reconciled_at"": ""2026-09-06T16:46:32.541584+00:00"", ""cli_companions"": [{""executable"": ""aikit-session-space"", ""commands"": [""discover""], ""protocol"": ""aikit.session-space-application/v1"", ""source_refs"": [""crates/aikit-cli/src/bin/aikit-session-space.rs"", ""docs/v2/13-WORKING-ENVIRONMENT-PROVIDERS.md""]}]}",
cap.aikit.models,capability,,,,[],Choose an available model for actual requirements,Evaluate roster observations and capability fit,Candidate selection with explicit provider evidence,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/15-MODEL-ROSTER-CAPABILITY-FIT.md,crates/aikit-core/src/model_roster.rs,crates/aikit-core/tests/model_roster_acceptance.rs,aikit.html#q4/providers,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/model_roster.rs"": ""48e517bf0da874dfb01f11ad54e63beae8ded745e5af7b7ce9bda7642d882f31""}}, ""cli_commands"": [], ""cli_exposure"": {""kind"": ""gap"", ""reason"": ""The model roster capability has implementation evidence, but this CLI exposes no model command family.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.activation,capability,,,,[],Know what a target made operative,Carry evidence-bearing activation receipts in ContextResolution,Standing selection and activation remain independently inspectable,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,../github-recovery-mirror/mirror/ai-kit/issues/153.comments.json,crates/aikit-core/src/context_activation.rs,crates/aikit-core/tests/context_activation_v2.rs,aikit.html#q2/start,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/context_activation.rs"": ""9f7fa3ee1360ff43fb7566b14c8da000e99f349fb104bb329fa1916af0d0d728""}}, ""cli_commands"": [""enable"", ""disable"", ""use"", ""context bind"", ""context reset""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.procedures,capability,,,,[],Change an environment with a visible boundary,Plan execute and undo a Procedure,Attributable effects with a recovery route,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/SPEC-II-PROCEDURES-AND-INBOX.md,crates/aikit-core/src/procedure.rs,crates/aikit-cli/tests/procedure_commands.rs,aikit.html#q2/compose,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/procedure.rs"": ""6673e16c1ae5141df33c04704b3b54175de4e2d0a541f8fb9cefc5773c10a2ff""}}, ""cli_commands"": [""procedure plan profile-fork"", ""procedure diff"", ""procedure run"", ""procedure undo"", ""procedure list"", ""set rename"", ""set delete"", ""inbox"", ""capture"", ""promote"", ""bypass issue"", ""bypass list"", ""bypass revoke"", ""trust record"", ""trust show""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.explain,capability,,,,[],Understand how the effective world arose,Read causal explanation and recorded changes,Selection and change traces useful for diagnosis,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/v2/02-RESOLUTION-AND-CONTEXT-COGNITION.md,crates/aikit-core/src/explain_history.rs,crates/aikit-cli/tests/explain_history_production_v2.rs,aikit.html#q3/resolution,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/explain_history.rs"": ""aa88d3f5a7d5f38687a32f04b7ee5ba7e20642934548d027c745ac200eb18589""}}, ""cli_commands"": [""explain"", ""history"", ""log export"", ""jobs"", ""failures"", ""bypasses"", ""doctor""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.familiarity,capability,,,,[],Reach familiar relevant resources efficiently,Rank eligible candidates from recorded use,Faster access with exact refs and authority preserved,Implemented source contract; linked tests not freshly executed in this documentation pass,agent-inference,docs/SPEC-III-SKILLSETS-AND-FRECENCY.md,crates/aikit-core/src/frecency.rs,crates/aikit-core/tests/familiarity_v2.rs,aikit.html#q3/resolution,,,"{""basis"": ""Source-recovered capability account; original verification limits retained in Markdown."", ""converted_from_sha256"": ""9f8f34f320e78a72402844f14f7cf0607485953264edee23e2ce2574c0027c0b"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/O-I#97""], ""code_basis"": {""crates/aikit-core/src/frecency.rs"": ""a3c97e53adf4b57208bcb95dcceef3d7c2de8f52e08c5411922289d8ba9f9826""}}, ""cli_commands"": [""recent"", ""stats"", ""unused"", ""run"", ""shell init""], ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""Native CLI leaf commands recovered recursively from target/debug/aikit --help after a local build.""}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H0->H2,relation,suite-relations,H0,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""Central"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H0->A2,relation,suite-relations,H0,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""Central"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H1->H2,relation,suite-relations,H1,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""Actuation"", ""dst_product"": ""AIKit"", ""ql"": ""B1"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H1->A2,relation,suite-relations,H1,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""Actuation"", ""dst_product"": ""AIKit"", ""ql"": ""D2-transform"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H0,relation,suite-relations,H2,H0,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""AIKit"", ""dst_product"": ""Central"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H1,relation,suite-relations,H2,H1,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Actuation"", ""ql"": ""B1"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H2,relation,suite-relations,H2,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,self:AIKit,I,"{""src_product"": ""AIKit"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""self:AIKit"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H3,relation,suite-relations,H2,H3,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Factory"", ""ql"": ""A2|C3"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H4,relation,suite-relations,H2,H4,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""AIKit"", ""dst_product"": ""Workcell"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->H5,relation,suite-relations,H2,H5,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""AIKit"", ""dst_product"": ""QL"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A0,relation,suite-relations,H2,A0,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""AIKit"", ""dst_product"": ""Central"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A1,relation,suite-relations,H2,A1,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Actuation"", ""ql"": ""D2-require"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A2,relation,suite-relations,H2,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,conjugation:AIKit,H,"{""src_product"": ""AIKit"", ""dst_product"": ""AIKit"", ""ql"": ""D1"", ""cf_view"": ""CF3"", ""seam"": ""conjugation:AIKit"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A3,relation,suite-relations,H2,A3,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Factory"", ""ql"": ""D2-transform|D2-complete"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A4,relation,suite-relations,H2,A4,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""AIKit"", ""dst_product"": ""Workcell"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H2->A5,relation,suite-relations,H2,A5,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""AIKit"", ""dst_product"": ""QL"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H3->H2,relation,suite-relations,H3,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""Factory"", ""dst_product"": ""AIKit"", ""ql"": ""A2|C3"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H3->A2,relation,suite-relations,H3,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""Factory"", ""dst_product"": ""AIKit"", ""ql"": ""D2-require|D2-complete"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H4->H2,relation,suite-relations,H4,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""Workcell"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H4->A2,relation,suite-relations,H4,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""Workcell"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H5->H2,relation,suite-relations,H5,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""QL"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
H5->A2,relation,suite-relations,H5,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""QL"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A0->H2,relation,suite-relations,A0,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""Central"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A0->A2,relation,suite-relations,A0,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""Central"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A1->H2,relation,suite-relations,A1,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""Actuation"", ""dst_product"": ""AIKit"", ""ql"": ""D2-require.inverse"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A1->A2,relation,suite-relations,A1,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""Actuation"", ""dst_product"": ""AIKit"", ""ql"": ""D3:B1"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H0,relation,suite-relations,A2,H0,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""AIKit"", ""dst_product"": ""Central"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H1,relation,suite-relations,A2,H1,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Actuation"", ""ql"": ""D2-transform.inverse"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H2,relation,suite-relations,A2,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,conjugation:AIKit,H,"{""src_product"": ""AIKit"", ""dst_product"": ""AIKit"", ""ql"": ""D1.inverse"", ""cf_view"": ""CF3"", ""seam"": ""conjugation:AIKit"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H3,relation,suite-relations,A2,H3,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Factory"", ""ql"": ""D2-require.inverse|D2-complete.inverse"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H4,relation,suite-relations,A2,H4,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""AIKit"", ""dst_product"": ""Workcell"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->H5,relation,suite-relations,A2,H5,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""AIKit"", ""dst_product"": ""QL"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A0,relation,suite-relations,A2,A0,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,02:ground-context,L,"{""src_product"": ""AIKit"", ""dst_product"": ""Central"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""02:ground-context"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Central#24(PR);EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A1,relation,suite-relations,A2,A1,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,12:agency-operative-body,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Actuation"", ""ql"": ""D3:B1"", ""cf_view"": ""CF3"", ""seam"": ""12:agency-operative-body"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/Actuation#1;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A2,relation,suite-relations,A2,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,self:AIKit,I,"{""src_product"": ""AIKit"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF3"", ""seam"": ""self:AIKit"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A3,relation,suite-relations,A2,A3,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""AIKit"", ""dst_product"": ""Factory"", ""ql"": ""D3:A2|D3:C3"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A4,relation,suite-relations,A2,A4,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""AIKit"", ""dst_product"": ""Workcell"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A2->A5,relation,suite-relations,A2,A5,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""AIKit"", ""dst_product"": ""QL"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A3->H2,relation,suite-relations,A3,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""Factory"", ""dst_product"": ""AIKit"", ""ql"": ""D2-transform.inverse|D2-complete.inverse"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A3->A2,relation,suite-relations,A3,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,23:possibility-development,H,"{""src_product"": ""Factory"", ""dst_product"": ""AIKit"", ""ql"": ""D3:A2|D3:C3"", ""cf_view"": ""CF4"", ""seam"": ""23:possibility-development"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/agent-system-design#142(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A4->H2,relation,suite-relations,A4,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""Workcell"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A4->A2,relation,suite-relations,A4,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,24:context-materialisation,S,"{""src_product"": ""Workcell"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": ""CF5-field"", ""seam"": ""24:context-materialisation"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/Workcell#18(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A5->H2,relation,suite-relations,A5,H2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""QL"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
A5->A2,relation,suite-relations,A5,A2,[],,,,,agent-inference,O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR),,,,25:context-ql,S,"{""src_product"": ""QL"", ""dst_product"": ""AIKit"", ""ql"": """", ""cf_view"": """", ""seam"": ""25:context-ql"", ""defined_in"": ""O-I:docs/CANONICAL-PRODUCT-FIELD.md|QL-MEF#19(PR)"", ""tracked_by"": ""EpiLogos/O-I#29;EpiLogos/ai-kit#58(PR);EpiLogos/QL-MEF#19(PR)"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
rel.aikit.q0.S,relation,product-field,q0,S,"[""cap.aikit.discovery"", ""cap.aikit.adoption"", ""cap.aikit.projection""]",,,,,agent-inference,ProjectCentral/user/aikit.html#whole-why,,,aikit.html#whole/why,"Agentic work draws on tools, Skills, knowledge, models and working environments that come from many places. A person needs these resources to become a coherent working context without rebuilding their setup around each Agent. AIKit makes the relevant part of that world discoverable, explainable and usable for the work at hand.",,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q0"", ""source_unit"": ""whole-why"", ""seed_sha256"": ""d57463a327bba62808845484fadc35bbcad99bea0a6ce40ee934a15f5435635e"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",Why?
rel.aikit.q1.S,relation,product-field,q1,S,"[""cap.aikit.resource-index"", ""cap.aikit.methods"", ""cap.aikit.routines""]",,,,,agent-inference,ProjectCentral/user/aikit.html#whole-what,,,aikit.html#whole/what,"AIKit is a Rust CLI, TUI and shared application toolkit for discovering agent resources, selecting them for a Project or task, and projecting the resulting configuration into supported harnesses. It also provides knowledge search and retrieval, semantic Wiki navigation, runtime composition and session integration. Its concrete product is an inspectable answer to what this actor can use and ask about here.",,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q1"", ""source_unit"": ""whole-what"", ""seed_sha256"": ""eeaa885aec8af8592b8027608fdf42172ec47b666f4d0ef40d21604e3fcab10a"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",What?
rel.aikit.q2.S,relation,product-field,q2,S,"[""cap.aikit.context-resolution"", ""cap.aikit.generation"", ""cap.aikit.source-horizon"", ""cap.aikit.knowledge-navigation"", ""cap.aikit.wiki-write"", ""cap.aikit.projectcentral"", ""cap.aikit.living-knowledge"", ""cap.aikit.reflection"", ""cap.aikit.composition"", ""cap.aikit.sessions"", ""cap.aikit.activation"", ""cap.aikit.procedures"", ""cap.aikit.continuity""]",,,,,agent-inference,ProjectCentral/user/aikit.html#whole-how,,,aikit.html#whole/how,"AIKit registers sources, resolves scoped choices against trust and compatibility, and prepares a target-specific view. It keeps larger bodies of knowledge and instructions addressable so an Agent can retrieve what the task needs. Changes become reviewable plans or generations; explanation connects the effective environment to the choices and observations that produced it.",,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q2"", ""source_unit"": ""whole-how"", ""seed_sha256"": ""9f4fef1b1fc842ccfe9bcd2d4a0b5ff5d3b0223cf3d9cf02db3646b020f51507"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""EpiLogos/ai-kit#195""]}, ""last_reconciled_at"": ""2026-09-07T23:16:22.160762+00:00""}",How?
rel.aikit.q3.S,relation,product-field,q3,S,"[""cap.aikit.authored-relations"", ""cap.aikit.wiki-shapes"", ""cap.aikit.explain"", ""cap.aikit.familiarity""]",,,,,agent-inference,ProjectCentral/user/aikit.html#whole-whereby,,,aikit.html#whole/whereby,People compose and inspect their working environment through the CLI and TUI. Agents discover powers and retrieve relevant knowledge through the same underlying services. Providers and harness adapters connect existing tools while preserving their identities. Central supplies durable source; AIKit makes permitted source and capabilities available in a particular context.,,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q3"", ""source_unit"": ""whole-whereby"", ""seed_sha256"": ""c02422f1ce8607ef8d2967ec065ebd8538a25bf36f61466538dc99a3f5794361"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",Who / Whereby?
rel.aikit.q4.S,relation,product-field,q4,S,"[""cap.aikit.profiles"", ""cap.aikit.models""]",,,,,agent-inference,ProjectCentral/user/aikit.html#whole-context,,,aikit.html#whole/context,"AIKit works where a Project, actor, task, client and available machine meet. A shared baseline can be refined for a Project, session or individual invocation. The resulting configuration is situated: a resource may be useful in one context and unavailable in another. Explanations and activation evidence make those differences inspectable.",,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q4"", ""source_unit"": ""whole-context"", ""seed_sha256"": ""a4305ec3f1ff58c8860e900da1a41662c6350678df62ad60abd3c2dc6b0d2312"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",Where / When?
rel.aikit.q5.S,relation,product-field,q5,S,[],,,,,agent-inference,ProjectCentral/user/aikit.html#whole-purpose,,,aikit.html#whole/purpose,AIKit lets a person build an increasingly capable environment while keeping each encounter understandable. Skills and Methods make ways of working reusable; Wiki and source navigation make accumulated knowledge askable; explicit composition lets the same Project be met through different harnesses. Within O:I it supplies the operative resources through which authorised agency can work on durable human intention.,,"{""basis"": ""Exact overview seed text. Capability links follow their existing governing expanded account units; cell placement is editorial inference."", ""seed_ref"": ""aikit:seed:q5"", ""source_unit"": ""whole-purpose"", ""seed_sha256"": ""f7d37ffa2b57ffeb31c8f4e9e87c858546fcfbd5deefa882cfeb5a3d0decb0b1"", ""source_standing"": ""agent-inference"", ""reconciled_change_ref"": ""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",Why-For?
rel.aikit.q3.S0,relation,product-field,q3,S0,"[""cap.aikit.authored-relations"", ""cap.aikit.projectcentral""]",,,,,agent-inference,ProjectCentral/user/aikit.html#q3-source-contract,,,aikit.html#q3/source-contract,"Central’s recognised source relations carry provenance, truth standing, roles and treatment. AIKit preserves that distinction through the resulting knowledge view.",,"{""basis"": ""Source-account relation, with capability placement attributed as editorial inference."", ""source_unit"": ""q3-source-contract"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
rel.aikit.q3.S1,relation,product-field,q3,S1,[],,,,,agent-inference,ProjectCentral/user/aikit.html#q3-identity,,,aikit.html#q3/identity,"AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.",,"{""basis"": ""Exact account sentence; cell placement is editorial inference."", ""source_unit"": ""q3-identity"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
rel.aikit.q3.S3,relation,product-field,q3,S3,[],,,,,agent-inference,ProjectCentral/user/aikit.html#q3-identity,,,aikit.html#q3/identity,"AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.",,"{""basis"": ""Exact account sentence; cell placement is editorial inference."", ""source_unit"": ""q3-identity"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
rel.aikit.q3.S4,relation,product-field,q3,S4,[],,,,,agent-inference,ProjectCentral/user/aikit.html#q3-identity,,,aikit.html#q3/identity,"AIKit develops the operational selection and disclosure relations; Central keeps durable authored ground, Actuation supplies authority and agency, Factory owns development and Workcell supplies material execution.",,"{""basis"": ""Exact account sentence; cell placement is editorial inference."", ""source_unit"": ""q3-identity"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
rel.aikit.q3.S5,relation,product-field,q3,S5,"[""cap.aikit.wiki-shapes""]",,,,,agent-inference,ProjectCentral/user/aikit.html#q3-source-contract,,,aikit.html#q3/source-contract,The QL shape service supplies valid structural readings while semantic edges continue to require actual evidence.,,"{""basis"": ""Exact account sentence; cell placement is editorial inference."", ""source_unit"": ""q3-source-contract"", ""maintenance"": {""updated_at"": ""2026-09-06"", ""change_refs"": [""codex:thread:01a07608-d2ec-7b10-9713-74c445adf8a5""]}, ""last_reconciled_at"": ""2026-09-06T14:30:23.997737+00:00""}",
cap.aikit.gateway,capability,,,,[],"A persistent, owner-only contact plane for situated agency",Serve and query the Agency Gateway over its well-known carriers; probe presence through doctor,Bootstrap-verified gateway presence; connector ingress; authorised ecology reading over situated sessions,Implemented source contract; live workcell acceptance pending (sixth execution unit),implementation-fact,docs/USING-AIKIT.md;docs/v2/10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md,crates/aikit-adapters/src/gateway_client.rs;crates/aikit-adapters/src/gateway_runtime.rs;crates/aikit-adapters/src/gateway_service.rs;crates/aikit-adapters/src/telegram_bot_api.rs;crates/aikit-cli/src/cli.rs;crates/aikit-cli/src/doctor.rs;crates/aikit-cli/src/gateway_ops.rs;crates/aikit-cli/src/lib.rs;crates/aikit-store/src/home.rs,crates/aikit-cli/tests/gateway_command.rs,aikit.html#q0/continuity,,,"{""basis"": ""Gateway capability reconciled with the landed bootstrap fold: well-known default endpoint, doctor probe, CLI front door (PR #192). The continuity module declaration rides the same front door (PR #195)."", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""hermes-nara:session:2026-09-07-gateway-reconcile"", ""EpiLogos/ai-kit#192"", ""EpiLogos/ai-kit#195""], ""code_basis"": {""crates/aikit-adapters/src/gateway_client.rs"": ""d6ced782bfc1ad38aab366837424f0b1a44452e2d45ce3b9a9184dfb3efcc1de"", ""crates/aikit-adapters/src/gateway_runtime.rs"": ""dc913d660cf634d3172f734a3e17719940aafc7e187bc4b69613bbc5afc041c9"", ""crates/aikit-adapters/src/gateway_service.rs"": ""7f0c9304d41bad22462f595293f742431d12bca31c07d6a627c2aa63e0a671b4"", ""crates/aikit-adapters/src/telegram_bot_api.rs"": ""f9d74aa702e5401f05dc6725b794c01d4f1348939d6a018df333089a89776a73"", ""crates/aikit-cli/src/cli.rs"": ""e31aad0770623a9a96aa72f225c87f9cc72e315e793cdec327c54918c99dc016"", ""crates/aikit-cli/src/doctor.rs"": ""b5c6f8437890b62bde4495ecd8bb34379331ae6605577eddb5452b8d12988ee4"", ""crates/aikit-cli/src/gateway_ops.rs"": ""b6e884d7872f7f594659c9fc1e76438de10dd8b5cee7b92bfea26f91ebf16ec2"", ""crates/aikit-cli/src/lib.rs"": ""d8abace40e0468c96a6bdca03571d92f1bf4c63a9bd3ed8bace6ba97737c4772"", ""crates/aikit-store/src/home.rs"": ""719c9d9261e296762f5a7fcc16c0148aae76a9e5db8c921190b85e530d99b429""}}, ""cli_exposure"": {""kind"": ""direct"", ""reason"": ""The gateway commands are the owner-facing CLI surface of the Agency Gateway contact plane: serve, query, ecology read, snapshot.""}, ""cli_commands"": [""gateway serve"", ""gateway protocol"", ""gateway discover"", ""gateway status"", ""gateway ecology"", ""gateway snapshot""], ""last_reconciled_at"": ""2026-09-07T23:16:22.160762+00:00""}",
cap.aikit.continuity,capability,,,,[],A person or agent re-entering a project meets its standing knowledge and present participants without reconstructing them from chat history,Compose SessionStart disclosure from authored carriers and world binding; activate declared KnowledgeDomains on hook prompts with rendered-content dedup scoped to the session,"Durable facts enter context once per scope while standing rules reassert every time; volatile state stays in the temporal fields; injection names its trigger, horizon and source",Implemented source contract; live SessionStart and domain-activation cycles proven end-to-end (W10 V6; CASE 03),implementation-fact,registry/capsules/hook/continuity/entity-disclosure/manifest.toml;registry/capsules/hook/continuity/domain-activation/manifest.toml,crates/aikit-core/src/continuity.rs;crates/aikit-core/src/domain.rs;crates/aikit-core/src/lib.rs;crates/aikit-store/src/index.rs;crates/aikit-cli/src/app/mod.rs;crates/aikit-cli/src/domain_activation.rs,crates/aikit-cli/tests/domain_activation.rs;crates/aikit-core/tests/continuity_tuning.rs;crates/aikit-store/tests/injection_ledger.rs,aikit.html#q0/continuity,,,"{""basis"": ""Continuity capability reconciled with the landed continuity slice: entity-aware SessionStart disclosure (PR #194) and KnowledgeDomain activation with content dedup (PR #195); live cycle proven end-to-end."", ""maintenance"": {""updated_at"": ""2026-09-07"", ""change_refs"": [""zcode-session-2026-09-07-continuity-pickup"", ""EpiLogos/ai-kit#194"", ""EpiLogos/ai-kit#195""], ""code_basis"": {""crates/aikit-core/src/continuity.rs"": ""4a095fd6fd243d3631a20c34a98a26b250f4301c5f4253ab10ced44ae66eff7e"", ""crates/aikit-core/src/domain.rs"": ""5260f8ff1fc6f3fd1ce38310284b26cf5fd9c36a8c8046910b4b86ae710edd0a"", ""crates/aikit-core/src/lib.rs"": ""d8a238d462714a32801973f672f717fcc1ee52e0a335435d3df3d163297e6be6"", ""crates/aikit-store/src/index.rs"": ""c0d8b92eec12acbb4cc3470c8d0fb792f549c10a02907907de7fc6158dd8b0b0"", ""crates/aikit-cli/src/app/mod.rs"": ""05c2f4eb03904f91c584e28931ac6e3b36ebece2869e52a57cbccebb238d3e18"", ""crates/aikit-cli/src/domain_activation.rs"": ""eb167a97043f8b9cc7f1cc5923c290ede533b4ff9b5e17557d44f9a5ef34bd97""}}, ""cli_exposure"": {""kind"": ""composed"", ""reason"": ""Continuity rides the hook carriers — SessionStart entity disclosure and prompt-scoped KnowledgeDomain activation through hook dispatch — so it owns no CLI commands of its own.""}, ""cli_commands"": [], ""last_reconciled_at"": ""2026-09-07T23:16:22.160762+00:00""}",
```
