# Manage skill sources separately from project activation

AIKit will ingest Agent Skills from pinned Git revisions and host-local
directories without projecting mutable source trees directly into an agent
harness. Source lifecycle, reusable Skill Set composition, project identity,
and live scope activation are separate layers.

## Source lifecycle

A source has a stable AIKit id and is either:

- a Git repository, exact revision, and relative skill root; or
- a canonical host-local directory.

`source sync` creates an immutable, content-addressed candidate. The identity
includes each relative path, file bytes, and relevant file permissions. The
complete validated skill directory is copied, including `references/`,
`scripts/`, `assets/`, `agents/`, and invocation policy in `SKILL.md`. Git
sources are read from a detached checkout of the resolved commit, not from a
mutable working tree, and the checkout is discarded after snapshot creation.

Sync does not change the active catalogue. `source promote` makes the candidate
active; `--trust` records explicit trust for every capsule revision in that
snapshot. Promotion retains the previous active digest as a rollback point.
Source ids qualify capsule ids, so identically named skills from two packs do
not silently overwrite each other.

Local-directory paths remain machine-private in `AIKIT_HOME`. Git source
coordinates are portable. Updates create candidates and never mutate an active
snapshot in place.

## Composition and project binding

Skill Sets are reusable named projections over source-qualified skill ids.
They do not own provenance, trust, or update state, and they do not alter an
upstream skill's invocation policy. A Project Specification may select several
Skill Sets, and a Skill Set may be selected by several projects.

`default_skill_sets` in `AIKIT_HOME/config.toml` supplies configurable defaults.
Projects inherit them unless bound with `--no-default-skill-sets`; project-local
sets append to inherited sets. The default recommended foundation is
`mattpocock/wayfinder-foundation`, containing:

- `wayfinder`
- `setup-matt-pocock-skills`
- `grilling`
- `domain-modeling`
- `prototype`
- `research`
- the local `writing-guidance-tools` skill

Project matching follows ADR 0001. Directory and normalized Git repository
bindings identify a project; sessions, tasks, and worktrees retain distinct
context state.

## Harness projection and hot swapping

One resolved generation materializes both native layouts:

- Codex: `projections/codex/.agents/skills`
- Claude Code: `projections/claude/.claude/skills`

For an isolated, bound project, `.agents/skills` is an AIKit-owned link to the
stable current Codex projection. AIKit refuses to replace a user-owned tree or
foreign link. Generation publication is atomic, so new harness processes see
one complete view and projects that do not match the specification receive
none of its skills.

This is filesystem-level hot swapping, not a promise of in-process harness
reload. Codex, Claude Code, and future adapters may cache discovery state. A mux
can preserve the AIKit context across panes, but the safest boundary for a
changed skill catalogue is a new agent task. AIKit reports activation effects
and never equates "active in AIKit" with "already loaded by this process."
