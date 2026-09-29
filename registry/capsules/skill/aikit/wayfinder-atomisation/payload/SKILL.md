---
name: aikit-wayfinder-atomisation
description: "METHOD: Decompose a Wayfinder, protocol or large development plan into source-bound atoms and context-efficient packets, then either export paste-ready harness prompts or compile the same packets into native Factory/NOW execution; use when a parent scope needs focused parallel or chained work."
---

# Wayfinder atomisation

Semantic ref: `skill/aikit/wayfinder-atomisation`. Native owner: AIKit.

Use this Method when a strong parent/orchestrator needs to turn a large Wayfinder, implementation protocol, UX refinement or cross-product plan into small executable tasks for weaker/cheaper subagents.

It does **not** execute the work, spawn Agents, create worktrees, replace Factory work custody, or become another workflow language. Factory's `factory-bounded-work` Method remains the execution discipline when commissioned developmental work is being carried. This Method prepares the bounded units that such work can consume.

## 0. Choose the execution mode

This is **one Method with two execution branches**. The decomposition semantics stay the same; the carrier changes.

### A. Prompt-export mode

Use when the parent is ChatGPT Web, another external orchestrator, or any environment where the user/operator will paste or send work into a harness such as Zcode.

The Method returns:

```text
parent coverage / atom graph
        ↓
context-efficient dispatch packets
        ↓
portable self-contained harness prompts
```

Each packet prompt must contain enough source identity, local purpose, write/effect bounds, dependencies, verification and Return shape for a fresh harness worker to execute it without access to the parent conversation.

Do **not** require Factory refs, child NOWs or internal runtime objects that the pasted harness cannot resolve. Where useful, include those refs as provenance, but make the prompt executable from ordinary repository/native-tool access.

The external parent retains the packet board, dependencies and barriers. Returned child results are reconciled back into that parent manually or through whatever orchestration surface owns the conversation.

### B. Native Factory mode

Use when the work is already commissioned into Factory or the caller explicitly asks the native Factory/NOW system to execute the decomposition.

The Method returns/feeds:

```text
Factory Run / parent NOW
        ↓
atom graph
        ↓
dispatch packets / workflow-unit relation
        ↓
bounded child NOW per active packet
        ↓
AIKit body + Agent/Agency + Workcell placement
        ↓
Activity / Evidence / Return
```

In this mode, the **native refs and custody are primary**. A generated model prompt is only the harness-facing projection of the packet. It must not become a second task record, second queue, or substitute for Factory/NOW identity.

Factory owns work custody/barriers/Return; Central owns NOW; AIKit resolves praxis/context/body; Actuation owns Agency/authority; Workcell owns material placement.

### Mode-selection law

Choose prompt-export when the requested deliverable is prompts/task packets for external/manual harness dispatch.

Choose native Factory mode when the caller asks Factory to carry the work or when execution is already inside an admitted Factory Run.

Do not silently switch a prompt-export request into Factory mutation, and do not reduce native Factory execution to a pile of pasted prompts.

The same atom IDs, parent obligations, write claims and evidence expectations should remain comparable across both modes.

The governing law is:

> **An atom is one independently verifiable difference. A dispatch packet may carry several tightly related atoms when shared context makes that cheaper and clearer.**

If the child still has to decide what the product means, invent an architecture, choose between materially different user outcomes, discover hidden dependencies, or coordinate conflicting writers, the parent has not atomised far enough.

## 1. Recover the parent whole before splitting it

Start from the actual Wayfinder/protocol source and its current revision. Do not decompose an Agent summary when the governing source is available.

Recover only what is needed to preserve the parent meaning:

```text
purpose / intended difference
authoritative source refs + revisions
required outcomes
invariants / non-goals
owner boundaries
dependencies / ordering
branch / failure / re-entry conditions
verification / evidence obligations
current implementation state where relevant
```

Keep authored intent, current implementation and inference distinguishable.

A task atom may carry a concise interpretation, but it must retain the exact parent source refs so the child or verifier can reopen the governing source when needed.

## 2. Build an obligation graph, not a prose checklist

Translate the parent into obligations and dependency relations before writing child prompts.

For each obligation ask:

```text
what exact difference must become true?
who owns the semantic operation?
what input/basis is required?
what can be changed independently?
what output/evidence proves it?
what blocks another obligation?
what can run in parallel?
```

Do not create a new permanent obligation ontology. Use the Wayfinder's own IDs, story/UX/capability refs, issue refs or stable source headings where they exist.

The decomposition should reveal:

- **independent atoms** — may run concurrently because their write/effect sets do not collide;
- **ordered atoms** — one produces a contract, source or implementation consumed by another;
- **barriers** — several independent atoms must all return before synthesis/integration;
- **integration atoms** — intentionally join already-implemented pieces and prove the relation;
- **verification atoms** — independently replay or pressure a completed difference;
- **decision residues** — true unresolved human/product choices which should stay with the parent rather than be pushed onto a weak child.

## 3. Atomicity test

A good atom satisfies all of these:

```text
one bounded intended difference
one clear semantic/native owner
small source basis
bounded read set
bounded write/effect set
explicit dependencies
one primary verification condition
clear stop/escalation condition
return format small enough to compare mechanically
```

An atom is a planning/coverage unit, not a command to create one model session per atom.

## 3a. Context amortisation and dispatch packets

Do not spend a full Agent startup/context load on a trivial atom when several adjacent atoms share the same source basis, owner, code neighbourhood, write locality and verification environment.

Bundle compatible atoms into a **dispatch packet** when all of these hold:

```text
one primary local intent / concern
same ProjectWorld / product owner
mostly shared source/context
compatible write/effect claims
short bounded execution horizon
no hidden architecture or human decision
results can still be reported per atom
```

A packet may therefore contain several small changes, tests or cleanup repairs. The child remains focused because the packet has one coherent local purpose.

Split a packet when:

- it crosses independent semantic owners with little shared context;
- it requires unrelated code regions or competing mutable files;
- one leg can fail independently and should release downstream work separately;
- an integration or independent-verifier boundary would be weakened by bundling;
- the worker would need to keep too much of the parent architecture in mind.

Optimise for **useful work per context load**, not minimum task count and not maximum parallelism.

The parent should prefer a few coherent packets over dozens of tiny sessions when the latter merely repeat source loading.

Atomic does **not** mean one file. A coherent change may touch implementation + test + local documentation when those are one inseparable owner-owned difference.

Do not split until each atom becomes meaningless mechanical busywork. The smallest useful atom still returns a real verified change.

## 4. Weak-model law

Assume the child is competent but not good at maintaining a large architectural field.

The parent therefore settles before dispatch:

- what is being changed;
- why this atom exists;
- exact authoritative source;
- which owner/repository applies;
- allowed files/effects;
- required operation;
- acceptance;
- what not to decide;
- dependencies;
- return shape.

The child should **execute and verify**, not reconstruct the whole programme.

Do not hide facts the child genuinely needs. Do hide unrelated future work, synthesis conclusions and verifier canaries when exposing them would encourage premature completion.

A task that says only "implement the Wayfinder section for X" is not atomised.

## 5. Shared-worktree law

This Method is designed for many threads/subagents working in **one existing worktree**.

Default rules:

```text
one worktree
no child creates another worktree or branch
one writer per shared mutable subject
parallel writes require disjoint write sets
shared files are serialised through a named parent/barrier
read-only inspection may run in parallel
commits/integration follow the parent campaign's existing Git policy
```

Every atom declares a `read_set` and `write_set` at the narrowest useful level. A dispatch packet carries the union of its member atoms' claims and must remain conflict-free against concurrently running packets.

If two atoms need the same mutable file/registry/schema, either merge them into one atom, order them, or introduce a parent-owned integration/barrier atom.

Do not solve write conflicts with optimistic hope.

## 6. Native-owner and UI law

Decompose by semantic ownership, not by where the user sees the result.

For a UI requirement:

```text
required native owner read/Action missing?
    -> native-owner atom first
    -> UI consumer atom depends on it
native contract already real?
    -> UI atom consumes it
```

A UI atom must not invent semantic state simply to make the screen work.

Likewise a cross-product feature should normally split at published owner seams, then rejoin through an integration atom.

"Native owner" is routing, not deferral. If the commissioned parent owns the connected work and authority permits it, the corresponding owner atom is part of the decomposition.

## 6a. Own discovered repairs relative to the packet intent

Bounded execution is not permission to ignore defects encountered on the path.

When the child discovers a fault:

```text
necessary to achieve the packet intent
+ inside current authority
+ inside the same local owner/write neighbourhood
+ small enough not to destroy the packet boundary
    -> own it, repair it, verify it, and report which atom it affected

inside the parent commission but outside this packet's claimed mutable boundary
    -> preserve the evidence, communicate cheaply to the parent/affected Position,
       and request/emit a bounded repair atom or claim expansion; do not defer to the human

real human / authority / material boundary
    -> stop only the consequential effect, report the exact boundary, continue independent work
```

Do not interpret `allowed files` as “ignore every bug outside these files.” It is a concurrency/write-claim boundary. The agent still owns discovered faults relative to the commissioned intent and routes them through the swarm correctly.

## 7. Task atom contract

Each emitted atom should use this compact shape. Omit fields that are genuinely inapplicable; do not replace them with vague prose.

```text
ATOM <stable local id> — <imperative title>

PARENT
- Wayfinder/protocol:
- source refs/revisions:
- parent obligation(s):

DIFFERENCE
- make this one condition true:
- why it matters to the parent:

OWNER / PLACE
- repository/product:
- existing worktree:
- semantic owner:
- current basis/revision:

READ
- exact files/refs/operations to inspect first:

WRITE / EFFECT
- allowed files/subjects:
- prohibited overlap:
- no new worktree/branch unless parent explicitly says otherwise

DEPENDENCIES
- requires:
- may run parallel with:
- barrier / integration successor:

EXECUTE
- concrete implementation/repair/research action
- use existing native public operations
- do not redesign parent architecture

VERIFY
- command/test/readback:
- exact expected evidence:
- negative/stale/disconnect case if material:

STOP / ESCALATE
- stop only for:
- do not ask the human for routine recoverable facts:
- if a discovered fault is inside scope/authority, repair it at the native owner

RETURN
- changed files/refs
- checks actually run
- result/evidence
- unresolved real blocker
- next dependency released
```

For a weaker model such as a bounded Zcode worker, make `EXECUTE` literal and reduce optional interpretation, but give it enough coherent neighbouring work to justify the context load.

When rendering a packet for **prompt-export mode**, prepend:

```text
PACKET <id> — <one local purpose>
member atoms: <ids>
shared worktree: <path/ref>
combined write/effect claim: <subjects>
budget/stop condition: <bounded>
return-to-parent: <how/where to report>
```

For **native Factory mode**, retain the same packet fields in native Factory/NOW/workflow-unit custody and project only the smallest harness-facing instructions required by the selected body. Do not duplicate the native task into another prompt ledger.

Prompt-export packets may include `child NOW` as informational provenance when one already exists, but must not require an unresolved internal ref in order to execute.

Then include the member atom contracts.

Do not copy the whole Wayfinder.

Legacy packet prefix example:

```text
PACKET <id> — <one local purpose>
member atoms: <ids>
shared worktree: <path/ref>
combined write/effect claim: <subjects>
budget/stop condition: <bounded>
return-to-parent: <how/where to report>
```


Open `references/task-atom-contract.md` when generating a large batch or when file-claim/barrier details matter.

## 7a. Factory / NOW swarm execution

When Factory and the NOW field are available, this Method should compile naturally into a small swarm rather than a pile of unrelated prompts.

Recommended relation:

```text
parent Wayfinder / Factory Run
        ↓
atom graph
        ↓ group by context + write locality
dispatch packets
        ↓
one bounded child NOW per active packet
        ↓
Agent / Agency / Zcode or other harness body
        ↓
shared worktree with disjoint mutable claims
        ↓
Activity / evidence / packet Return
        ↓
barrier / integration / verifier
```

The child NOW is the continuation/address for the packet, not a copy of the Wayfinder. Redis/Jev can prepare the packet from exact parent refs, relevant Skills/METHODS, current code/Wiki/GitNexus relations and sibling changes.

The parent/orchestrator should keep:

- packet membership and atom coverage;
- active write/effect claims;
- dependencies/barriers;
- sibling Returns and newly discovered repair atoms;
- current source revisions;
- total concurrency/context/model budget.

Siblings may use cheap Gateway Communiques for coordination. A Communique does not alter write ownership or create Factory work by itself.

Do not create one child NOW per trivial atom unless the atoms genuinely require separate execution/verification.

## 8. Dependency and fan-out strategy

Prefer breadth only where the work is truly independent.

A useful pattern is:

```text
contract/source atoms
        ↓
owner-native implementation atoms
        ↓
parallel consumer/UI/test atoms
        ↓
barrier
        ↓
integration atom
        ↓
independent verifier atom
```

Do not start consumer atoms against an unpinned contract when their implementation would otherwise invent it.

Do not serialize unrelated work merely because it belongs to one Wayfinder.

For large sweeps, group atoms first by **write-conflict lane**, then bundle adjacent atoms into dispatch packets where context locality earns it.

## 9. Verification inheritance

Every atom inherits the relevant parent evidence law.

The parent should never accept child prose, file existence, component rendering or one convenient test as proof of a larger parent obligation.

An atom may close only its declared difference. The parent later recomposes atom evidence against the full Wayfinder.

Where a feature crosses owner boundaries, include an integration/consumer-binding atom and, where material, an independent negative/disconnection check.

## 10. Return economy

Weak child Returns should be terse and machine/comparison friendly. A packet Return reports each member atom plus any owned repair discovered along the way.

Good:

```text
PACKET UI-P2 complete
atoms: UI-07 complete; UI-08 complete; UI-09 blocked
owned repair: UI-R1 fixed stale selector in src/state.ts
changed: src/foo.ts, src/state.ts, tests/foo.spec.ts
verified: pnpm test foo.spec.ts (7/7)
evidence: native unavailable state now renders degraded; disconnected-provider case passes
released: INT-02
blocker: UI-09 requires native read NAT-04
```

Bad:

- restating the architecture;
- narrating every command;
- dumping speculative future work;
- declaring the parent Wayfinder complete;
- asking the human to route a small native-owner defect.

The parent/orchestrator owns synthesis and human-facing curation.

## 11. When not to atomise

Do not use this Method when:

- the task is already one bounded verified difference;
- the main unresolved work is a product/design decision;
- the work requires one person's continuous local reasoning across tightly coupled code;
- splitting would increase coordination more than it reduces model/context load;
- the only outcome would be many tiny mechanical prompts with no independent evidence;
- several small atoms clearly share one context neighbourhood and should be dispatched together.

Atomisation is useful when it reduces context burden and allows safe parallel or chained execution.

## 12. Completion

The decomposition is complete when:

- every parent obligation is mapped to one or more atoms or an explicit retained parent decision;
- no atom silently owns an unresolved architectural choice;
- parallel atoms have disjoint mutable write sets;
- shared mutations have an ordering/barrier;
- each atom has source basis, exact difference, execution boundary and proof;
- integration and independent verification are present where the parent requires them;
- a weak fresh subagent can execute any atom or coherent dispatch packet without being taught the whole Wayfinder;
- prompt-export packets are executable without hidden Factory-only state, while native Factory packets preserve canonical work/NOW custody rather than creating a prompt-side duplicate;
- the parent can reconstruct parent-level coverage from the returned atom IDs/evidence.

This Method grants no authority to spawn Agents, allocate worktrees, mutate repositories or approve product choices. It prepares executable task packets; the current parent/Factory/Actuation/Git authority still governs execution.
