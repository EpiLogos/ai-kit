# Part XXI — Verification as resolved evidence-bearing operation

## 71. Verification is a feature of the operating world, not a universal workflow

AIKit should treat verification as a first-class resolvable feature without imposing one engineering ceremony on every Project.

The maximal assurance field is useful because it gives a Project and actor a complete vocabulary of possible evidence-producing operations. Resolution determines which subset matters here and now.

A small library, exploratory prototype, production service, package release, and protected deployment may therefore resolve very different verification demands while using the same underlying primitives.

The governing question is:

> **What evidence is required for the transition or completion claim at hand, and what available powers can produce it?**

AIKit does not own the meaning of Project correctness and does not become a CI provider.

---

## 72. CI flows are Runs

A local verification command, GitHub Actions workflow, release verification sequence, or post-deployment smoke sequence should not introduce a rival Pipeline ontology merely because it has a graph of steps.

At the shared semantic level these are naturally **verification-oriented Runs**, or nested Runs within a larger developmental Run.

```text
Verification requirement
        ↓
verification-oriented Run
        ↓
Actions / Executions / Assessments
        ↓
Results + Evidence
        ↓
closure determination where relevant
        ↓
Gate permits / refuses transition
```

Provider concepts such as workflow, job, step, runner, matrix, status check, environment, and merge queue describe how a provider materialises or enforces part of that Run. They do not redefine `Run`, `Evidence`, `Assessment`, or `Gate`.

The same semantic Run may be visible through local CLI, an agent Action, GitHub, AIKit UI, or Project history.

---

## 73. Run, Closure, and Gate remain distinct

The wider Factory architecture distinguishes three non-identical things:

```text
Run
    chronological execution / observation of an intended transformation

Closure
    sufficient determinacy relative to the operative whole and what opened it

Gate
    rule or assessment controlling whether a state transition is permitted
```

A Run can end without Closure. Closure may depend on evidence accumulated across several Runs. A Gate can require Closure, a narrower verified condition, a human determination, or a composite of these.

AIKit should therefore resolve the powers, sources, provider bindings, and evidence horizon needed to support closure or satisfy gates without treating a green provider workflow as definitionally equivalent to completion.

This relation is especially important for agentic engineering:

```text
implementation Run
    + verification Run
    + independent Review Run where required
            ↓
current Evidence / Assessments
            ↓
causal disclosure of whether the operative conditions hold
            ↓
Closure determination
            ↓
Gate
```

---

## 74. Verification requirements and the effective Verification Plan

The architecture should preserve the difference between **what must be true** and **how/where it will be established now**.

Project-owned source may therefore declare a Verification Contract or equivalent structure describing obligations such as:

```text
what must be verified
which public or architectural seams carry the obligation
what kinds of Evidence may satisfy it
which lifecycle boundaries require satisfaction
which failures block advancement
which review independence is required
what evidence must persist
```

The exact source syntax is Project-owned and should not be constitutionalised by AIKit.

AIKit resolves an effective `VerificationPlan` from:

```text
Project verification requirements
+ durable user verification disposition
+ current Subject / change / risk
+ Project architecture and standards
+ available verification Capabilities / Actions
+ Host and execution conditions
+ provider state
+ current qualifying Evidence
        ↓
effective VerificationPlan
```

This plan answers the actor-facing question:

> **What verification applies to the change I am making right now?**

It is an inspectable resolution product, not a new source of Project truth.

---

## 75. Ownership

The intended ownership boundary is:

```text
human-owned source
    durable verification ideals and collaboration dispositions

Project-owned source
    Project-specific requirements, seams, canonical verification commands,
    architecture-specific assurance, release/deployment obligations

AIKit
    discovery, indexing, capability/provider resolution,
    effective VerificationPlan, explanation and projection

Agent / Agency
    executes checks, interprets failures, repairs work,
    requests or performs review, grounds completion claims in Evidence

CI / execution provider
    materialises remote execution, status publication, artifacts,
    platform matrices and enforcement mechanisms
```

Observed provider state never silently becomes authored Project intent. Authored intent never pretends that a provider currently enforces something it does not.

---

## 76. Checks, Reviews, and human judgement

The implementation should resist creating unnecessary root primitives where existing semantics suffice.

A **deterministic Check** is an evidence-producing verification operation expected to be reproducible under declared conditions.

An **automated Review** is normally an `Assessment` produced by an Agent/Agency against explicit criteria such as Spec fidelity, engineering Standards, architecture, security judgement, scope, or test adequacy.

A **human Review** is an attributable human Assessment or Recognition where human authorship/judgement is part of the requirement.

These should not collapse to one boolean.

```text
Check
    executable proposition → Result / Evidence

Automated Review
    attributable judgement → Assessment + findings

Human Review
    human judgement / Recognition → Assessment / Decision / Closure input
```

Fresh-context review can be expressed by resolving a reviewer Agency whose context is deliberately independent of the authoring AgentSession or implementation assumptions. The choice of same/different model is a provider/Agency disposition, not part of the abstract Review meaning.

---

## 77. Evidence is subject-bound and freshness matters

A completion claim is warranted only by evidence applicable to the state being claimed complete.

A verification result should retain enough identity and provenance to answer:

```text
which verification operation?
which Subject / commit / tree / artifact / deployment state?
which command or procedure version?
which environment / Host / provider?
when did it run?
what was the outcome?
what structured findings were produced?
where are logs or retained artifacts?
which source/configuration version governed the run?
```

Large logs remain in provider or artifact storage. AIKit/Factory retain the durable result envelope and references needed for explanation and Claims/Evidence relations.

When the relevant Subject changes, prior results may become superseded for a particular Gate or Closure claim even though they remain historical Evidence.

This supports the strong engineering relation:

> **A completion Claim must cite current qualifying Verification Results whose Subject/state matches the state being claimed complete.**

---

## 78. Verification bootstrap and assurance audit

Project Bootstrap should be able to discover the verification horizon before asking the human to restate information the repository already carries.

Discovery may include:

- language/toolchain and test frameworks;
- canonical project scripts/commands;
- build and package systems;
- CI/provider configuration;
- branch/ruleset requirements;
- review/CODEOWNERS expectations;
- security and dependency workflows;
- release/deployment environments;
- test, coverage and generated-artifact configuration;
- recent provider observations where available.

The audit should preserve four distinct readings:

```text
DECLARED
    what Project-owned source says should be true

EXECUTABLE
    what local commands/tests/build graph actually exercise

PROVIDER
    what the remote provider currently runs/enforces

OBSERVED
    what recent executions show actually happened
```

Differences are surfaced as verification drift rather than silently reconciled.

---

## 79. Assurance-changing change

A normal implementation change should preserve or strengthen the declared assurance regime unless changing that regime is part of the intended work.

The useful concept is semantic **AssuranceImpact**:

> a change capable of altering what evidence is produced, what evidence counts, or which state transition that evidence controls.

This can include obvious edits to tests, assertions, CI configuration, security scanning, coverage thresholds, required checks, or deployment gates, but the determination must not be a path blacklist. Changes to shared test helpers, package graphs, skip logic, verification commands, or generated configuration may have the same effect.

Where material AssuranceImpact is detected, the effective VerificationPlan may require explicit independent review of the assurance change itself.

AIKit resolves and explains that requirement; it does not invent Project policy unilaterally.

---

## 80. Stability and flakiness

A single verification execution has an observed outcome such as:

```text
pass
fail
error
cancelled
unavailable
not-applicable
```

`superseded` is better treated as evidence lifecycle/relevance than as the outcome of the execution itself.

Flakiness is normally a derived property across comparable Results, not a result that one run can truthfully declare about itself.

Contradictory outcomes for the same meaningful Subject and materially equivalent conditions create evidence of instability. A later green rerun must not erase the contradictory history.

This allows a Project to resolve policies such as requiring stability investigation rather than permitting repeated reruns until a passing result appears.

---

## 81. Local and remote verification

Fast feedback and independent enforcement serve different purposes.

The same Project verification semantics should be projectable across surfaces where possible:

```text
canonical Project verification operation
        ├── local CLI
        ├── agent Action
        ├── AIKit resolved operation
        └── remote provider workflow
```

Projects should generally prefer thin provider orchestration around Project-owned verification semantics rather than duplicating the meaning of verification in provider YAML.

Provider-only assurance remains legitimate where the property inherently depends on the provider or environment: OS/runtime matrices, hosted security services, protected deployments, merge-group integration states, artifact attestations, and similar concerns.

A possible resolved cadence is:

```text
edit / slice        fastest relevant focused evidence
implementation      affected-domain verification
completion boundary broader Project verification
review-ready        independent Assessment where required
PR                   remote reproducible checks
merge candidate      target-integrated Subject verification
release              package/build/security/provenance
 deployment          environment protection + smoke/runtime evidence
scheduled            drift / broad assurance where warranted
```

This is a vocabulary of boundaries, not a globally mandated ladder.

---

## 82. GitHub-first provider projection

GitHub is the first rich provider model because it exposes mature execution and enforcement surfaces.

Provider projection should be able to represent or discover, as appropriate:

```text
workflow / reusable workflow
job / step / runner
matrix / concurrency
check run / commit status
artifact
ruleset / branch protection
review requirement / CODEOWNERS relation
merge queue / merge-group subject
release
artifact attestation
 deployment / environment protection
```

These remain GitHub provider concepts.

The provider-neutral seam should expose semantic operations such as:

```text
execute verification work
observe verification status/results
retrieve evidence/artifact refs
inspect enforcement/gates
inspect provider configuration
materialise eligible remote verification
```

Exact provider traits/interfaces should be derived during implementation from real GitHub integration rather than pre-named into a speculative universal CI framework.

---

## 83. Interaction with ContextResolution

Verification extends the world AIKit can resolve without changing the fundamental resolver model.

A `ContextResolution` may expose:

```text
verification requirements relevant to present Focus
available verification Capabilities / Actions
canonical Project verification surfaces
current qualifying Evidence
unresolved assurance drift
resolved VerificationPlan
provider availability / enforcement observations
```

The actor-facing context seed need not preload all verification details. It should make the faculty addressable, for example:

> **What verification applies to what I am changing now?**

---

## 84. Architectural acceptance

The design is correctly implemented only if tests can eventually show that:

- a verification-oriented GitHub workflow can be represented as a Run/provider projection without inventing a second pipeline ontology;
- a Run can terminate without the surrounding work achieving Closure;
- Closure can cite Evidence produced across multiple Runs;
- a Gate can require deterministic Evidence, automated Assessment, human judgement, or a composite without conflating them;
- Project-authored requirements and observed provider enforcement can disagree visibly;
- identical verification-resolution inputs produce an equivalent effective VerificationPlan;
- a provider outage does not rewrite Project verification intent;
- a stale passing Result does not satisfy a Gate for a materially changed Subject;
- repeated contradictory Results can surface instability rather than being hidden by the latest pass;
- AssuranceImpact can trigger explicit review without encoding a brittle filename blacklist;
- local and GitHub execution can exercise one canonical Project verification operation where provider-specific behaviour is not required;
- AIKit remains usable for Projects choosing a deliberately lightweight verification posture.
