# AIKit agent protocol

AIKit-managed capability trees are generated state. Agents must not edit
`.agents/skills`, `.claude/skills`, or files below an AIKit generation directly.
Change the source, Skill Set, Project Specification, profile, or scope overlay,
then let `aikit apply` publish a new generation.

Use `aikit skill overlay set/show/clear` for user- or project-specific skill
orientation. Treat the emitted overlay as additive, user-authoritative guidance;
it does not alter the upstream skill's trust, permissions, identity, or
invocation policy.

When AIKit variables are present, preserve `AIKIT_HOME`, `AIKIT_CONTEXT_ID`,
and `AIKIT_ISOLATION` when launching child processes or handing work to another
harness. Inspect `aikit project show`, `aikit status --all`, and `aikit explain`
when capability availability is relevant to the task.

Availability, activation, and in-process loading are distinct. A generation
swap changes the filesystem view immediately, but an already-running agent may
have cached its skill catalogue. Never claim that a live harness reloaded a
skill unless that harness reports it; start a fresh agent task when in doubt.
