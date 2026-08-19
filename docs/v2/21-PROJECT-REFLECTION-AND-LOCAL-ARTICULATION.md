# AIKit V2 — Project Reflection and Local Structural Articulation

**Status:** current implementation contract  
**Date:** 2026-08-19  
**Implementation owner:** existing Knowledge Navigation / `ProjectMap`; `project_reflection.rs`; `local_source_discovery.rs`; `project_recovery.rs`  
**Coordinates with:** Central local source contracts, Factory structural-source fidelity, O:I Project/world integration, Actuation world-binding boundary

## 0. The engineering problem

A living Project is represented in several ways at once:

```text
human-authored Ground / Canon
        ↓
SemanticWiki / Project language
        ↕
local structural descriptions / scoped contracts
        ↕
exact code
        ↕
derived code-index structural intelligence
        ↓
verification / evidence / developmental history
        ↺
returned difference
```

These are related representations of one Project world, but they are **not equally authoritative for the same question**.

The purpose of Project reflection is to make those representations mutually traversable without collapsing their authority or creating another universal graph.

## 1. Authority stays differentiated

The current architecture preserves at least these distinctions:

```text
human-authored Ground / Canon
    strongest evidence for authored purpose, intended experience and recognised position

SemanticWiki
    Agent-maintained semantic knowledge / Project vocabulary

native local structural description
    source or contract about a local implementation region

exact CodeReference
    stable address into implementation

CodeIndex / GitNexus
    derived structural observation about code

verification / observed evidence
    bounded evidence for implementation/behaviour claims

Run / Decision / current development history
    records of development and returned reality
```

Current code is implementation truth for what is real now; it does not retroactively define why the Project exists. Authored Ground states intended meaning; it does not prove implementation. A local description describes the implementation region at its own revision; it is not implementation truth. GitNexus can reveal code structure; it is not semantic authority.

## 2. ProjectMap remains the explicit federation seam

The existing `ProjectMap` is sufficient for cross-representation bindings. It remains a bounded explicit federation index, not a universal graph database.

Current hard lenses remain exactly:

```text
Git
Code
SemanticWiki
SourcePool
Canon
Run
Decision
Verification
Evolution
```

The implementation audit rejected adding `Description`, `Temporal` and `Praxis` lenses. Those are source roles or application relations which can be represented through stable refs + source metadata + explicit ProjectMap bindings without acquiring hard lens identity.

This matters because lens proliferation would turn useful distinctions about source role into a second ontology of the entire Project.

## 3. Local structural description is an ordinary source role

A local structural description may be expressed by many native Project forms:

```text
module/file header summary
AGENTS.md
CLAUDE.md
CONTEXT.md
package/module README
ADR
architecture/interface note
structural manifest
native Project contract
another existing local form
```

The filename does **not** establish the role. AIKit can use names/paths as discovery hints, but operational Project recovery treats hint-only classification as partial/unresolved until stronger source evidence exists.

A source role can include:

```text
HumanProjectGround
AgentGovernance
AgentMaintainedWiki
LocalStructuralDescription
OrdinarySource
DerivedDocumentation
CodeIndexObservation
TemporalWorkingMaterial
Praxis
Unresolved
```

Role is separate from source identity. A source retains its stable `SourceRef` even when AIKit is not yet sure what role it plays.

## 4. Bounded native discovery

`local_source_discovery` provides the filesystem adapter for heterogeneous existing Projects.

It deliberately does not recursively ingest a repository. Default discovery is bounded by:

```text
maximum files visited
maximum traversal depth
maximum sampled body bytes
```

It:

- skips symlinks and obvious generated/dependency build trees;
- respects recursive `.no-agent-retrieval` boundaries;
- examines conventional source/contract locations as candidates;
- samples only bounded content needed for classification hints;
- preserves exact owner/adoption relations when supplied;
- retains compatible native sources in place;
- gives ordinary unowned discovered material a stable observed source identity;
- marks generated material as generated/derived rather than promoting copied wording.

The point is to recover enough local articulation to navigate the act, not to build a shadow document store.

## 5. The reflection read model

`project_reflection(map, subject, max_hops, limit)` constructs a bounded read model from explicit ProjectMap routes only.

It discloses the selected subject and reachable representations grouped as:

```text
meaning
    Canon / SemanticWiki

descriptions
    SourcePool-bound local/source representation

code
    Code / Git

verification
    Verification

other
    Run / Decision / Evolution and other explicit mapped resources
```

Every relation remains an actual ProjectMap route. Provider-native graphs are not copied into the read model.

Human-facing surfaces can therefore render the same underlying refs pithily:

```text
this is …
part of …
implements …
relates to …
described by …
verified by …
```

Agent-facing consumers retain the same stable identities and route evidence. CLI/TUI/desktop presentation may differ without creating separate semantics.

## 6. Bidirectional navigation

The target human/Agent navigation relation is symmetrical.

From a semantic concept/WikiNode:

```text
WHAT IS THIS?
    SemanticWiki / Project language

WHY DOES IT EXIST?
    human Ground / design source

WHERE / HOW IS IT REALISED?
    local description + exact CodeReference

WHAT IS ITS STRUCTURE NOW?
    CodeIndex/GitNexus context/impact/trace

WHAT PROVES IT?
    tests / verification / evidence

WHAT CHANGED?
    Run / Decision / development history / return
```

From an exact CodeReference, traverse the reverse explicit bindings to any known:

```text
Project concept
local description / ownership
human/design source
verification/evidence
development history
```

The ability to travel both ways is the practical reason for Project reflection: semantic articulation can lead to exact executable articulation, and implementation reality can return pressure to the semantic/source world.

## 7. Staleness is evidence, not automatic mutation

If implementation moves or changes while a description or semantic binding still points at an older revision, the system should report a discrepancy.

Examples:

```text
semantic concept has no declared implementation binding
local description is stale relative to moved code
code graph contradicts a structural assertion
verification falsifies an implementation relation
stable name survives while constitutive parentage/relation has changed
```

These are evidence. They do not grant AIKit permission to rewrite human Ground, Agent governance, SemanticWiki or native local source automatically.

Return pressure should be routed to the nearest actual owner:

```text
implementation difference
    -> exact semantic/source/code refs
    -> discrepancy/evidence
    -> owner of Wiki / description / governance / human Ground
    -> update proposal or Recognition where required
```

## 8. Strong target-owned reflection laws

Ordinary Projects do not need a formal coordinate system.

When a target **does** own stable coordinate identity and declares a reflection law, AIKit can verify that law without understanding the target's domain semantics.

`ReflectionLaw` carries opaque target-owned coordinates and explicit expected relations between semantic and implementation refs, optional description relation, exact implementation revision and constitutive relations.

`verify_reflection_law` can detect:

```text
missing mapping
wrong relation
multiple implementation targets where uniqueness is required
stale implementation revision
missing declared description relation
constitutive flattening
stale constitutive relation
```

Label equality is never parity. A coordinate name can survive while the relation that makes it what it is has disappeared.

## 9. Epi/QL conformance and its boundary

The first strong repository-owned conformance fixture uses the QL-MEF holographic kernel manifest and exact C primitive `ql_position_invert` at pinned source revisions. It proves:

```text
target-owned formal semantic subject
    ↔ manifest/source articulation
    ↔ exact CodeReference
    ↔ verification evidence
```

and verifies an explicit strong reflection law through ProjectMap in both directions.

This fixture intentionally does **not** fabricate a live Bimba `Mx/Mx′` node. The exact Bimba graph identity required for full semantic-coordinate proof lives in the Epi source world and cannot honestly be inferred from repository labels alone. Full Bimba Mx/Mx′ conformance remains an owner-level/local-source acceptance boundary until the live Bimba source/graph is inspectable in the execution environment.

That boundary is evidence of the architecture working correctly: AIKit refuses to promote a similarly named repository artifact into source-owned semantic identity.

## 10. Project recovery

`project_recovery` composes ProjectCentral orientation, native source bindings/classification, reflection, Method/praxis state and ContextResolution into a single bootstrap receipt.

It reports stages as:

```text
Available
Partial
OptionalAbsent
Unresolved
```

A Project with no Central, no SemanticWiki, no local-description convention and no Method tree is still valid. Rich Project articulation increases what can be recovered/navigated; it is not the new minimum ontology of software.

`act_authority_inferred = false` is deliberate. Project recovery resolves articulated context; Actuation owns situated authority.

## 11. Filesystem/document-governance consequence

Reflection changes how an Agent should edit a repository.

Before changing a region:

```text
recover applicable broad source
    ↓
recover closest local contract/description
    ↓
follow stable semantic/code refs when present
    ↓
inspect exact implementation
    ↓
act
    ↓
verify
    ↓
return any changed relation to the nearest owner
```

The closest contract supplies local specificity, not global semantic supremacy. Stable reference material remains distinct from temporal/current working material. Generated indexes remain rebuildable. One durable fact should have one authoritative home and be linked rather than copied.

## 12. Actuation boundary

The reflection architecture must not become an agency topology.

```text
filesystem path / local source != WorldBinding identity
resolved source/guidance != Agency authority
ProjectMap route != Agentic Determination
ProjectRecovery != Actuation
```

A situated Agent may receive an operative articulated world through AIKit resolution. When reality falsifies that articulation, AIKit preserves enough refs/evidence for the correct owner and Actuation Return relation to handle the difference.

## 13. Accepted and rejected primitives

Accepted:

- existing SourceRef / ResourceRef identity;
- existing ProjectMap bindings;
- bounded ProjectReflection read model;
- local source-role classification separate from source identity;
- bounded heterogeneous filesystem discovery;
- target-owned optional strong reflection laws;
- ProjectRecovery as a read/receipt composition over existing owners.

Rejected:

- `CodeWiki`;
- universal `SelfDescription` root;
- universal graph store;
- GitNexus as semantic authority;
- local description as implementation truth;
- new Description/Temporal/Praxis ProjectLens variants;
- filename-based authorship/authority;
- hard-coded Epi/QL coordinate semantics in generic AIKit;
- ProjectMap as Actuation topology.

## 14. Acceptance shape

A rich acceptance subject should prove:

```text
recognised human Ground
    -> SemanticWiki
    -> native local description
    -> exact CodeReference
    -> real CodeIndex provider
    -> bidirectional reflection
    -> Method / ContextResolution
    -> target-native Skill projection
    -> real use / verification / evidence
    -> Explain / History / returned discrepancy
```

The contrasting minimal subject must still prove ordinary Knowledge Navigation and native Skill operation without special Project substrate.

The deeper result is a Project whose meaning and executable reality can answer one another precisely **because their representations remain differentiated**, not because they have been collapsed into one graph or one source of truth.
