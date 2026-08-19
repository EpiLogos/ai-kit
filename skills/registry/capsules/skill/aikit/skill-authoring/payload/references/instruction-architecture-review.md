# Instruction architecture deep review

Open this reference only when the authoring task involves a substantial guidance corpus, repeated behavioural failures, trigger ambiguity, progressive disclosure, phase separation or historical instruction audit.

It is a review procedure behind `aikit:skill-authoring`, not always-present guidance.

## 1. Inventory source and execution roles

For each relevant instruction source, record:

```text
owner
source ref / revision
scope
human-authored vs generated/derived standing
operative projection / harness target
```

Where Central governance is involved, preserve `Control/agents/governance/**`, `ProjectCentral/agents/governance/**` or accepted `central.agent-governance-relations/v1` identity. Do not invent an AIKit shadow copy as canon.

Keep source scope distinct from operational precedence. AIKit ContextResolution explains which eligible fragments were selected/composed for the act.

## 2. Trigger audit

For each Skill description, ask:

- What concrete situation should make this Skill enter consideration?
- What nearby task should not select it?
- Does the description name the relation clearly enough without carrying procedure that belongs in the body?
- Is a short natural-language trigger more discriminative than accumulated keywords?

Add at least one positive and one nearby negative fixture for consequential routing.

## 3. Guidance versus procedure audit

Classify each paragraph or rule:

```text
stable fact / vocabulary / care / boundary / collaboration stance
    → persistent governance/guidance may be appropriate

situational multi-step procedure
    → Skill

rare template / provider recipe / deep example
    → context pointer/reference

current Project fact
    → Project source / Wiki / evidence owner

one-off workaround
    → likely no durable instruction unless repeated evidence supports it
```

Do not move human-authored Central source automatically. Produce a proposal when the owning source should change.

## 4. Invocation/disclosure trade-off

Compare:

```text
context load
    cost of making language automatically visible/selectable

human invocation load
    cost of requiring the person to remember/call a capability
```

Choose among always-present compact guidance, model-invoked Skill, user-invoked Skill and conditional reference. There is no universal threshold; explain the local reason.

## 5. Vocabulary audit

For a leading term, record:

```text
term
relation it compresses
what distinction would be lost if treated as a slogan
example of correct use
nearby incorrect interpretation
```

Retain domain vocabulary when it genuinely improves compression and action. Do not replace the relation with branding.

## 6. Positive specification audit

For each important prohibition, ask what the Agent should do instead.

Example:

```text
boundary
    Do not silently rewrite human-authored source.

positive operation
    Update Agent-maintained knowledge when authorised; when returned reality pressures human source, prepare a provenance-bearing proposal for human adoption.
```

A prohibition can stay. The positive transition prevents the boundary from becoming operational silence.

## 7. Progressive disclosure audit

For each large block, ask:

- Is this needed in the common invocation path?
- Does it apply only to a provider, rare failure mode, artifact type or advanced branch?
- Can the common Skill state the condition and point to this reference instead?

Test that the common path does not load the rare reference when the condition is absent.

## 8. Contrast-example audit

Use a compact bad/good or alternatives specimen when the targeted distinction remains ambiguous in prose.

Test both:

1. the target case changes in the intended direction;
2. unrelated output does not inherit the specimen's incidental phrasing or structure.

## 9. Phase-separation audit

Use hidden/future-phase separation only when evidence suggests later artifacts become premature completion targets.

Record:

```text
phase
closure condition
what later artifact remains unavailable
why early visibility would damage inquiry or authority
```

Do not phase-gate direct work without a reason.

## 10. Vertical-slice audit

When recommending a tracer bullet / vertical slice, state:

```text
risk being tested
layers crossed
observable evidence returned earlier
why horizontal work would delay that evidence
```

If those cannot be named, the phrase is probably ritual rather than useful build-order guidance.

## 11. Historical evidence audit

Sample real sessions/Runs/PRs and classify repeated waste or failure before changing instructions:

- tool misuse;
- redundant questions;
- excessive ceremony;
- overbuilding;
- failure to verify;
- scope creep;
- communication failure;
- stale environment command;
- repeated PR/review mistake;
- inappropriate delegation/parallelism.

Then identify the actual owner:

```text
missing source/context      → source/discovery problem
bad reusable procedure     → Skill
ambiguous stable relation   → governance proposal
provider-specific quirk     → provider/reference layer
product ambiguity           → human/product authorship
```

Historical frequency increases evidential weight; it does not confer authorship.

## 12. No-op / deletion test

For each accumulated instruction, name the conformance or recurring evidence it changes.

Remove or consolidate candidates whose deletion does not materially regress desired behaviour. Re-run the targeted fixtures after deletion.

The aim is not minimal character count. The aim is the smallest instruction architecture that still changes the desired acts reliably.

## 13. Return quality

For a substantial authoring change, return:

```text
problem observed
owning source/capability
change made/proposed
trigger/disclosure impact
targeted regression fixture
broader checks
what was deliberately deleted/not added
source/authority boundary retained
```

Where the human needs to adopt a Central governance change, keep the proposed source diff separate from generated evidence supporting it.
