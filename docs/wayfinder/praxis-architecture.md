# Wayfinder map: Agent praxis, SkillSet packages and World-facing cards

`wayfinder:map` — local-markdown tracker (this repository has no Wayfinder
issue map for this effort). Oriented through the Wayfinder Methodology
(`skill/personal/wayfinder`, `METHODOLOGY:`) with the Documentation
Methodology (`skill/central/docs-methodology`) composing in parallel over the
representation field. Governing contract:
[`docs/PRAXIS-ARCHITECTURE.md`](../PRAXIS-ARCHITECTURE.md).

## Destination

A fresh Agent can answer, from real source / resolution / activity state:
why am I here, what can I do, how do I work, how do I orient, what do I
carry, what World am I in, what is my citizenship there, how does a human and
how does another Agent encounter me, what is operative now, what changed and
where does it go back — and a developer can package one native SkillSet for
Codex, Claude Code and Pi without re-authoring its praxis.

## Notes

- Execution is carried in this map (the Notes override "plan, don't do"):
  each ticket names its owning product, its acceptance evidence and the map
  revision its Return causes.
- Owners: AIKit (praxis form, SkillSet nesting/registry sets, disclosure,
  package SDK, A2A card builder); Central (Documentation Methodology and
  Skills/Methods, `central:documentation`, `central:core-development`, matrix
  `extensions.documentation`, AgentProfile comments); Control personal ground
  (adopted Wayfinder / grilling / prototype); O:I (participation +
  citizenship, human card, Guardian reconciliation, Cradle surface); Factory
  (Methodology composition in its development Skill).
- Skills every session consults: `skill/aikit/skill-authoring`,
  `skill/aikit/profile-skillset`, `skill/aikit/verification`,
  `skill/central/docs-methodology`.
- Disk is constrained (~15 GB free on 2026-09-24): no new worktrees; one
  target dir per checkout.

## Decisions so far

- **Praxis form is classification only** — `PraxisForm {Skill, Method,
  Methodology}` read from the ordinary description; `METHODOLOGY:` never
  detects as `METHOD:`; Methodology adds horizon `@3`; no ResourceKind/store.
  (`crates/aikit-core/src/method.rs`)
- **Referenced children finish nesting** — `set.toml children` /
  registry `child_refs` resolve at load, shared not copied, cycles and
  dangling refs refused before write; `aikit set add --child`.
  (`aikit-store/src/skillsets.rs`, `registry_skillsets.rs`)
- **Registry SkillSets are portable source** — discovered from
  `<home>/registries/*` and the sibling `skillsets/` of each directory skill
  source; addressed by semantic ref.
- **`aikit:account-authoring` is the shared child** — carried by reference by
  `aikit:project-author` and `central:documentation`; the verifier refuses a
  parent repeating its child's members.
- **Disclosure is a read model with an honest ladder** — `aikit praxis
  disclose` → `aikit.agent-praxis-disclosure/v1`; rungs are true / false /
  not-observed; carried ≠ loaded ≠ invoked; `source_rewritten: false`.
- **`method_refs` stays a compatibility relation**; no `methodology_refs`.
- **Core repertoire restored, not renamed** — `central:core-development`:
  Wayfinder (Methodology), grilling and prototype (Methods, adopted into
  Control with upstream attribution), research / domain-modeling /
  writing-great-skills (Skills, upstream bodies).
- **Documentation field** — `docs-methodology` promoted to `METHODOLOGY:`;
  five form Skills; seven Methods; maintained templates; compact inventory
  for Jev attention; `extensions.documentation` in `ql-capability-matrix/1`.
- **A2A card is a projection** — AIKit `a2a_card.rs` builds v1.0.1 cards
  from `oi.agent-world-participation/v1`, public capabilities only, validated
  by the same consumer-side check.
- **Guardian set is three members** — owner ruling 2026-09-17; operative O:I
  prose reconciled to the manifest.

## Not yet specified

- A production source for **public capability disclosure**: today the
  participation reading only publishes capabilities an explicit disclosure
  source names; the profile has no public-disclosure field. Whether that
  belongs on the AgentProfile (Central) or on an Actuation publication
  decision is an owner decision.
- **Serving** the A2A card at `/.well-known/agent-card.json` from a live
  Agent endpoint (needs an exposed A2A server binding; O:I's A2A work is
  consumer-side today).
- **Skill `invoked` evidence from hooks**: the activity contract exists; a
  hook reading that emits it from harness Skill-tool events does not.
- Cradle creator: progressive card derivation *before* acceptance (from the
  expressed-but-unaccepted profile).

## Out of scope

- A second Method/Methodology store, a documentation graph, a second matrix
  protocol, or a scalar citizenship score — ruled out by the governing
  contract.
