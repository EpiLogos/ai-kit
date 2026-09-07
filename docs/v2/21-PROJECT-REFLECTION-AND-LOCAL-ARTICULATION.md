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

## 15. Documentation population contract

**Protocol addition, 2026-09-06.** The procedures below specify documentation integration and its acceptance. The implementation boundary in section 18 distinguishes available operations from work still required.

The shared artifact contract is Central’s `docs/CAPABILITY-MATRIX-PROTOCOL.md`, `ql-capability-matrix/1`, with templates owned by `skills/capability-matrices/assets`. Its manifest declares ordered axes, views, anchor and default view; uniform CSV distinguishes stable capability records from relation placements referencing zero or more capabilities. The Wiki consumes that contract rather than defining another matrix format.

The human account states the product's purpose, intended experience and commitments. Its capability matrix makes the capabilities and their relations addressable. The Wiki discloses both alongside ordinary notes, code evidence and eligible execution material. All participate in the existing source pool and SemanticWiki index with their own provenance; population adds attributable knowledge and traversable relations around those sources.

A source item can remain an ordinary `1`. A `0/1` discloses a whole with an explicit anchor, actual members and Return. The material and question determine the grain. Preserve partial, twofold, threefold, fourfold, sixfold and conjugated forms when present. General matrix axes retain their declared ordered member identities. They do not extend the native QL position range or acquire direct/conjugate semantics from their dimensions. The product default relates six account seed questions to S and the other five products; the native QL direct/conjugate 6×6 is a distinct declared view within the same matrix protocol.

Population proceeds over a selected, bounded source set:

1. Establish Project and canonical Wiki identity, accepted source relations, provider availability and retrieval exclusions. Record source refs and exact revisions before interpreting content.
2. Inventory account whole/seed/expanded-unit locators, stable capability IDs and matrix axis/record locators. Include ordinary ad hoc notes and eligible NOW/Run passages pertinent to the question. Retain their owners, standing, producer refs and lifecycle. Path alone establishes neither human authorship nor QL meaning.
3. Resolve explicit `[[links]]`, resource maps, memberships and declared tags against that inventory. Preserve pending and ambiguous targets. Tags aid discovery; they do not establish semantic edges or authority by themselves.
4. Read the selected source units and recover the materially relevant whole. Generate only the knowledge required for this population task. Attribute inferred relations separately from source-explicit relations. An empty matrix address remains empty unless there is an attributable determination to record.
5. Validate the proposed Wiki as a whole and persist through its current atomic write path. Record exact basis revisions and affected dependencies. Rerunning population at the same basis must not duplicate subjects, capabilities or edges; changed revisions invalidate dependent readings rather than silently refreshing their claims.
6. Exercise the read faculty against the populated sources: search, exact read, relations, route, frame, sources, explain, history and status. Inspect returned provenance and exact source passages, not only matching titles or node counts.

NOW is optional. A local session is available only through an eligible source/provider disclosure. Factory retains Run and RunThought lifecycle; AIKit consumes their owner-issued identities, revisions and passage anchors. This procedure does not clone execution transcripts into a second canonical session store or imply that every running session is already projected.

## 16. Capability-specific contemplation and source return

Start with a stable capability identity and a concrete question. Recover its account seed, expanded units, matrix records, and relevant code, note or Run evidence at exact revisions. A view address answers where the question is being considered; a capability identity names the capability through changes of view. Multiple determinations at an address remain separately attributable.

Explicit contemplation returns either an attributable reading, a tension/absence, or a bounded change proposal. A source-change proposal identifies:

- exact source refs/revisions and affected account units, capability IDs and matrix records;
- the intended functional or experiential change and its evidence;
- coordinated proposed edits to the HTML account and matrix CSV/Markdown, including changed links and seed dependencies;
- unresolved or conflicting material, affected dependent Wiki objects and the source owner's reconciliation basis.

Apply edits under the existing user task authorisation and owner/standing rules. Agent inference does not become ratified intention through generation or placement. Preserve meaningful differences between intended behaviour, implementation observation and accepted commitment. Validate all companion artifacts and references together before declaring the change complete. If any basis revision changes, re-read and reconcile it. The required operation must either complete the coherent source set or retain an inspectable incomplete state with recoverable originals; individual atomic Wiki writes do not supply multi-source atomicity.

Refresh affected Wiki dependencies after the source result is known. Retain the original reading and exact basis for explanation rather than retargeting its old evidence to the new text. Documentation consolidation maps the content of an ad hoc file to specific successor units or capability records; meaningful unresolved content remains outstanding until accounted for.

## 17. Retirement operations and ownership

Central owns the durable filesystem structure and contract for source identity, lifecycle, successor relations and any archive locations. AIKit owns the actual assessment, consolidation, reconciliation, retirement, archive and restore operations over that structure, including dependency and retrieval updates. Factory supplies relevant development decisions and Run evidence through its own interfaces. Central's existing migration of compatible Wiki collections does not constitute this document-retirement operation.

Lifecycle is independent of provenance and standing: retiring an agent-derived source does not make its successor human-adopted, and archiving a recognised source does not erase its attribution.

The operation sequence is:

1. **Assess:** read exact selected revisions, owners, inbound references and source contributions. Produce an inspectable preview with the intended successor mapping and filesystem effect.
2. **Consolidate:** account for each materially distinct source unit as retained, transferred, superseded, conflicting or unresolved. A successor filename alone is insufficient. Partial consolidation leaves the unaccounted material active and visible.
3. **Reconcile:** use existing user authorisation and source-owner rules. Check the exact basis immediately before mutation. No blanket second approval is required merely because an agent-maintained draft is being consolidated within the authorised task.
4. **Retire in place:** preserve source bytes, identity and exact revision access while recording lifecycle, successor, actor, time and rationale in Central's structure. Invalidate affected Wiki dependencies. Current-state retrieval should disclose and prefer the recognised successor; exact historical retrieval continues to resolve the original.
5. **Archive or restore when requested:** apply the same identity and revision guards to moves and inverse changes. Preserve inbound refs through stable identity/redirect resolution. An implementation using path-derived refs must establish continuity before moving a file. Restore must recover content, lifecycle and traversability without duplicating identities.

Deletion is a separate explicitly scoped action. Retirement previews and consolidation proposals are useful before mutation support lands; they must be labelled proposals and must not change source lifecycle or claim retirement occurred.

## 18. Current operation boundary and real acceptance

At this protocol revision the following native surfaces are available:

| Surface | Available behaviour | Remaining acceptance work |
| --- | --- | --- |
| ProjectCentral authored adapter | Eligible Markdown, explicit links, source revisions, dependencies and existing SemanticWiki index | HTML whole/unit/resource-map and capability CSV record interpretation through the same identity/policy/standing contract |
| `wiki stage` | Explicit Markdown `ql:` alignment with stable source/node refs; positions 0–5 relative to a unit | General documentation population and coherent HTML/CSV/source change operation |
| QL shape and contemplation | Actual constellation grains, deterministic shaped preflight and attributable generated readings/source proposals | Matrix view declaration integration without coercing axes into native direct/conjugate coordinates |
| Factory RunThought adapter | Exact disclosed owner refs, revisions, passages and producer provenance | End-to-end selected local NOW/Run source availability in the documentation population scenario |
| Wiki writes and knowledge reads | Whole validation, atomic individual persistence and existing navigation faculty | Multi-source recovery, identity-preserving document lifecycle operations and successor-aware retrieval |

Relevant implementation seams are `projectcentral_authored_wiki.rs`, `authored_wiki_source.rs`, `factory_run_thought_authored_wiki.rs`, `knowledge_wiki_shape.rs`, `knowledge_living.rs` and the native `wiki`/`knowledge` CLI modules. Check these and the installed command help before executing a workflow; this specification does not introduce new command spellings.

Acceptance must use actual filesystem sources and native parsers, storage, operations and query results. Start with the Central account and matrix, an ordinary linked note, a genuinely partial QL whole and an eligible owner-issued Run passage. Use the same protocol for the other five products after this specimen passes. O:I desktop feature documentation is the subsequent scope.

Required assertions:

- Every source, HTML unit and matrix capability/record is addressable at its exact revision; cross-format links work in both directions without duplicate source copies. CSV quoted commas/newlines, reordered records and unknown extension fields survive the supported edit round trip.
- Ordinary notes work without QL frontmatter. A `1` remains ordinary unless a disclosed whole is actually authored or generated with attribution. Partial wholes retain their grain. General axes and native QL shapes preserve different declared semantics while sharing source and relation handling.
- Search finds both structured and ad hoc evidence. Exact reads recover the expected passage/record. Frames distinguish purpose, commitment, implementation evidence and agent inference. Duplicate titles, ambiguous links and absent providers stay inspectable.
- A seed or capability edit changes the relevant dependency revisions; old readings remain explainable at their original basis. Repeated population is idempotent. Restart and rebuild preserve identities and query results.
- Recursive privacy exclusions apply before disclosure, including through backlinks, frames and Run/source attachments. An unavailable Run remains explicitly unavailable rather than receiving fabricated content.
- A coordinated capability change with a concurrent source edit rejects the stale proposal and leaves recoverable source state. Failure between companion writes is observable and recoverable; completion is not reported for a partial update.
- Retirement preserves bytes and exact retrieval, prefers the recognised successor for current-state questions, and retains unresolved contributions. Archive preserves inbound links and history; restore recovers the original identity. Stale previews, offline edits and restart/rebuild exercise real lifecycle persistence.

Report each lane as executed/pass, executed/fail or unavailable with its concrete missing operation. Source inspection and tests of the existing Markdown path cannot establish HTML/CSV or retirement acceptance. Protocol conformance of authored files can be verified before these runtime lanes land; full Wiki population remains gated by the missing operations above.
