# Task atom contract

Use this reference when generating or reviewing a batch of task atoms.

## Atomicity

One atom = one independently verifiable difference.

An atom is too broad when it contains multiple owner-level outcomes that could fail independently.

An atom is too narrow when completing it creates no meaningful verifiable state without another atom's simultaneous edits.

## Batch table

Before emitting full prompts, build a compact batch table:

| Atom | Difference | Owner | Read set | Write set | Depends on | Parallel with | Verification | Releases |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |

Use this table to detect write collisions and hidden dependencies.

## Shared worktree checks

For every pair of atoms intended to run concurrently:

1. Compare mutable write sets.
2. Compare generated outputs/registries/lockfiles they may indirectly mutate.
3. Compare service/runtime effects.
4. If any shared mutable subject exists, serialise or create a parent integration atom.
5. Read-only overlap is fine.

Do not treat Git's ability to merge text as proof that concurrent semantic mutation is safe.

## Prompt strength

For weak/cheap models prefer:

- imperative title;
- 1–3 governing source refs;
- exact files or public operations to inspect;
- explicit allowed files/effects;
- literal acceptance command/readback;
- one clear return format.

Avoid architecture essays, multiple optional strategies, open-ended "improve/refine" verbs, hidden product choices, and whole-programme completion language.

## Parent coverage

Maintain a mapping:

```text
parent obligation
  -> atom(s)
  -> returned evidence
  -> integrated/verified standing
```

A parent obligation is not complete merely because all child atoms returned success if their joined relation has not been tested.

## UI split rule

If a UI state requires a native semantic relation that does not exist:

```text
NATIVE atom
  publish/repair real owner read/Action
      ↓
UI atom
  consume real relation
      ↓
INTEGRATION atom
  disconnect native producer and prove truthful degradation
```

Never let the UI atom invent the missing semantic contract.

## Repair rule

A defect discovered while executing an atom belongs to one of three classes:

1. **inside the atom's owner/effect boundary** — repair it now and verify;
2. **inside the parent commission but outside this atom's write claim** — return an exact dependent atom request to the parent; do not ask the human to route it;
3. **real external authority/material/human boundary** — stop the consequential effect and return the exact blocker.

"Different repository" is not class 3 by itself.

## Dispatch packet rule

Atoms are planning/evidence units. Sessions are economic execution units.

Bundle several atoms when they share source/context, semantic owner, code neighbourhood and verification environment. Keep one primary local intent and preserve per-atom evidence.

Prefer a coherent 20–60 minute bounded packet over five tiny sessions that each reload the same architecture. The exact duration is not a contract; context locality and boundedness are.

Do not bundle merely to keep an Agent busy. Split when independent failure, conflicting writes, owner boundaries or verifier independence matter.

## Factory / NOW swarm

When native Factory/NOW is available:

```text
Run / parent NOW
  -> packet P1 -> child NOW A -> Agent A
  -> packet P2 -> child NOW B -> Agent B
  -> packet P3 -> child NOW C -> Agent C
  -> barrier / integration packet
  -> verifier packet
```

Packets share the parent worktree only when their mutable claims are disjoint. Redis carries the hot packet/child-NOW view; governing Wayfinder/specification remains source-addressed.

## Discovered repairs

A child owns small necessary repairs within its packet authority/write neighbourhood. If the repair would collide with a sibling claim, preserve the failure and coordinate through the parent instead of racing the sibling or deferring to the human.

## Two dispatch modes

### Prompt export

Render packet prompts for manual/external harness use. The prompt is the portable execution carrier, so it must be self-contained enough to run from ordinary source/native-tool access.

Required properties:

- no hidden dependency on internal Factory/NOW objects;
- exact parent/source refs retained;
- explicit worktree/write claim;
- clear verification and Return format;
- parent dependencies/barriers stated in human-readable terms;
- suitable for copy/paste into Zcode or another harness.

### Native Factory

Materialise the same packet graph through native Factory/NOW custody. In this mode:

- Factory/Run/workflow-unit refs are canonical work identity;
- child NOW is the packet continuation address;
- AIKit projects only the necessary harness-facing body/context;
- generated prompt text is derivative and must not become a second queue/task record;
- Activity/Evidence/Return flow back through native owners.

The two modes should preserve the same atom IDs and acceptance meaning so an externally executed packet can later be compared with or admitted into Factory evidence without identity fiction.
