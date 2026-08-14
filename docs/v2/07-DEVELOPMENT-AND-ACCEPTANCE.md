# Part XIX — Development programme

## 63. Recommended V2 sequence

### V2-A — Product and resource foundation

- ratify this specification;
- define typed resource provider contracts;
- introduce precise `ProjectBinding`;
- generalise indexes beyond Capsules/Capabilities;
- keep current resolver behaviour green during refactor.

### V2-B — Extended ContextResolution

- Agent/Agency resource views;
- ActionSet;
- ContextSource horizon;
- Model/Harness/Host descriptors;
- source ownership/provenance;
- preference-intent binding;
- richer `aikit explain`.

### V2-C — Context cognition

- disclosure state model;
- source discovery/retrieval APIs;
- structured absence explanations;
- visibility/egress policy seam;
- situated ContextDisclosure.

### V2-D — Agent bootstrap

- thin target-native AIKit seed;
- question-addressable orientation Skills/capabilities;
- harness-specific bootstrap adapters;
- managed/authored/generated authority modes.

### V2-I — Knowledge Navigation

V2-I may prove provider/domain contracts in parallel once V2-C seams exist, but full unified traversal depends on the distinct lenses being operational.

- `SemanticWiki` as an open OKF-based specialised ContextSource/KnowledgeProvider;
- `SourcePoolProvider` with native baseline and replaceable rich providers such as bkmr;
- `CodeIndexProvider` with current GitNexus as the first rich structural-code provider;
- `ProjectMap` as navigational federation/index rather than universal graph store;
- `KnowledgeRoute` as operational traversal history, not semantic truth;
- stable cross-lens refs with provider/revision/provenance retained;
- one low-latency search/traversal/context-pack application layer for humans and agents;
- base QL-native Wiki topology without a live QL/MEF dependency;
- optional QL/MEF refractive/meta-Space depth through V2-G.

### V2-E — TUI rework

V2-E may begin its unified-state/application-service migration after V2-B/V2-C provide the required resource/context seams. Full V2-E Closure requires the Knowledge Navigation and familiarity contracts it renders rather than inventing TUI-local substitutes.

- one authoritative `TuiState` / reducer/effect path;
- Quick and Workspace presentations over the same state;
- human Project-world composition as a primary surface;
- universal fzf-like resource/knowledge search and contextual actions;
- Context as persistent resolved-world disclosure;
- Compose with distinct capability, information, actor/runtime and projection horizons;
- Explain as ubiquitous provenance/reason/target-effect inspection;
- History/familiarity as recoverable routes and prior worlds without frequency-as-truth;
- list/tree/graph as projections of one provider-neutral relation read model;
- SemanticWiki/SourcePool/ProjectMap knowledge navigation through the same application state;
- readable, calm terminal visual hierarchy and semantic theme roles;
- keyboard/mouse parity through common `UiAction`s;
- predictable back/dismiss semantics and explicit staging/apply/discard;
- preserve fast resident-process interaction, honest host behaviour, terminal safety and machine/application-service parity.

### V2-F — Learned ease

- cross-resource usage signals;
- destination and multi-hop route familiarity;
- frecency;
- contextual fitness observations;
- explicit preference separation;
- model/harness evidence;
- reset/forget semantics that remove learned ease without rewriting canonical resources or provider relations.

### V2-G — QL/MEF interoperability

- passive shared refs;
- QL provider adapter;
- no-QL parity tests;
- optional refraction surfaces;
- QL-MEF meta-Space/cross-wiki portal readings without ownership transfer;
- support experimental runtime provider references.

### V2-H — Verification resolution and provider projection

- discover Project-owned verification requirements and canonical verification surfaces;
- resolve an inspectable effective `VerificationPlan` for the present Subject/Focus rather than impose one universal workflow;
- preserve deterministic Check, automated Assessment, and human judgement as distinct evidence forms;
- carry Subject-bound result provenance and evidence freshness;
- expose assurance drift across declared, executable, provider, and observed state;
- detect material `AssuranceImpact` as a semantic property of changes;
- implement GitHub as the first rich provider while keeping provider orchestration subordinate to shared Run/Evidence/Gate semantics;
- prove lightweight and maximal Project verification postures through the same abstraction.

---

# Part XX — Acceptance cases

## 64. Source and ownership

The V2 design is not complete unless tests can show:

- deleting AIKit derived state does not delete independently authored source meaning;
- re-indexing source material recreates equivalent logical resource identity where inputs are equivalent;
- observed Host state never silently rewrites authored machine meaning;
- learned usage never silently rewrites authored preference;
- a generated agent-facing file never silently becomes canonical authored source.

---

## 65. Resolution

- two sessions in the same Project can resolve different effective worlds;
- managed denial cannot be overridden by lower-scope preference;
- an unavailable preferred Capability remains unavailable;
- a disabled required dependency fails visibly;
- every final resource decision is explainable;
- identical explicit inputs produce an identical resolution hash.

---

## 66. Context cognition

- a source can be known/askable without payload loading;
- an agent can query what resources exist beyond current Focus;
- latent and unknown are distinguishable in explanation;
- bound and missing are distinguishable in diagnostics;
- retrieving a source does not automatically make it current Focus;
- loaded material can be traced to its source and eligibility decision.

---

## 67. Agent/harness

- the same resolved Agent/Agency world can be projected to multiple harnesses through honest target effects;
- a harness restart requirement is not rendered as live activation;
- the bootstrap seed remains small and can route to broader AIKit faculties;
- existing human-authored harness instruction files are not overwritten without an explicit managed Procedure.

---

## 68. Memory

- frecency improves discoverability without changing trust;
- destination and multi-hop `KnowledgeRoute` familiarity is contextual and explainable;
- fitness observations retain context/use-type provenance;
- familiar resource routes can be explained;
- usage does not silently activate a resource in a new scope;
- reset/forget removes learned accessibility while leaving canonical resource/provider relations intact.

---

## 69. Privacy

- a never-agent-visible source payload never enters agent-searchable indexes;
- local visibility does not imply external-provider egress;
- Generation provenance records disclosure source/policy;
- dedicated secret mechanisms remain outside ordinary event and source indexes.

---

## 70. QL/MEF modularity

- AIKit passes ordinary tests with no QL provider installed;
- base SemanticWiki/ProjectMap navigation remains correct with no QL provider installed;
- QL provider absence is explicit only when an explicitly requested QL capability is required;
- QL readings retain target identity and provenance;
- optional Wiki/MEF relation overlays remain derived and do not silently become canonical Wiki/provider edges;
- a QL-MEF meta-Space may navigate mapped Project wikis without taking ownership of their Nodes/Spaces;
- optional lens/refraction use cannot silently override hard trust/policy decisions;
- experimental QL runtime selection can be represented without adding QL loop semantics to the deterministic resolver.

---

## 71. Knowledge Navigation

- SemanticWiki and SourcePool are discoverable as first-class specialised ContextSources through the same resource/application state;
- ProjectMap federates SemanticWiki, SourcePool, code intelligence, source tree and other Project lenses without becoming a universal graph database;
- search can address Space, Node, Source, Frame, canonical resource refs and learned Routes while preserving provider/lens origin;
- an exact/leaf result can deliberately expand into a bounded local whole with typed relations, provenance and cross-lens bindings;
- WikiEdges, code-graph edges, source-provenance relations and KnowledgeRoute steps remain semantically distinct;
- list/tree/graph read models are projections over stable refs/provider relations rather than independent semantic stores;
- basic fuzzy/address lookup remains low-latency when semantic/deep providers are unavailable;
- provider absence/degradation yields explainable partial navigation without identity drift;
- no TUI-only or agent-only project-knowledge ontology is introduced.

---

## 72. TUI state and navigation

- Quick and Workspace presentations use one authoritative `TuiState` and preserve selection/query/staging while expanding or contracting;
- switching list/tree/graph presentation preserves canonical `ResourceRef` selection and does not synchronise between independent controllers;
- keyboard and mouse gestures for the same operation converge on the same semantic `UiAction` and application service;
- refresh/re-resolution reconciles state by stable refs rather than transient row indexes;
- Search, Context, Compose, Explain, History and relation navigation consume shared application/read-model services and do not shell out to the CLI;
- Esc/back never accidentally applies or discards staged state; apply, discard, query-clear and application exit are explicit;
- durable changes remain staged and explain target/reprojection/restart effects before application;
- Context remains inspectable while moving among Projects, Compose, Explore, Projection and History;
- a selected result can move search → explain → relation view → compose/history without losing canonical identity.

---

## 73. TUI human composition and visual quality

- the present Project world visibly distinguishes capability horizon, information horizon, actor/runtime binding and projection targets;
- Profile/Skill Set composition distinguishes declared from effective state without requiring cryptic internal sigils;
- ordinary rows and inspectors expose readable labels, type/state/scope/provenance at the appropriate depth;
- semantic state remains legible without colour alone and has ASCII/Unicode-safe fallbacks;
- narrow layouts preserve semantic access through progressive disclosure rather than silent state loss;
- inline/fullscreen/popup host behaviour remains honest and terminal restoration is proven;
- representative snapshots cover Quick/Workspace, narrow/medium/wide, staging, Context/Explain, relation list/tree/graph, History and provider-degraded states;
- reducer transition tests cover route/back-stack behaviour, refresh/resize preservation and no accidental apply/discard;
- large resource indexes preserve the low-latency fuzzy-search path and relation expansion is explicitly bounded;
- an end-to-end acceptance route proves Project → Search → Explain → Compose → Apply/Context → Knowledge relation expansion → History without a second semantic store.

---
