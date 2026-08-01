# Separate reusable project identity from live project instances

AIKit will represent a project with a stable **Project Specification** and each
matching working directory with a separate **Project Instance**. A specification
may bind any number of ordered profiles and skill sets, and the same profile or
skill set may be bound to any number of specifications. This preserves reusable
defaults across clones and worktrees without allowing one live agent session to
mutate another.

## Matching contract

A Project Specification has a stable AIKit id and one or more explicit matchers:

- A directory matcher names a canonical physical directory. It matches that
  directory and its descendants until a nested Git repository or an explicit
  Project Boundary is reached.
- A repository matcher names a normalized Git remote. Matching is exact and
  offline: strip credentials, query and fragment data, an optional trailing
  `.git`, and transport-specific syntax, then compare the resulting
  `host/owner/repository` identity. `origin` is the default remote; additional
  remote names and repository aliases must be declared explicitly.
- A fork does not inherit an upstream repository's bindings unless its identity
  is an explicit alias. Renaming or removing a remote stops that repository
  matcher from applying; a directory matcher may still apply, and `doctor` must
  report the stale identity instead of silently migrating it.

Every clone and linked worktree is a distinct Project Instance identified by its
canonical physical root. Instances sharing a Repository Identity inherit the
same repository-scoped defaults, but never share session overlays, task state,
context ids, or path-local overrides. A linked worktree therefore gets the same
repository defaults as its main checkout while retaining independent live state.

A nested Git repository resets the outer repository and directory match. Within
one repository, `.aikit` declarations layer from repository root toward the
current directory unless a declaration explicitly starts a new Project
Boundary.

## Determinism and precedence

Matching and composition use this stable order, from least to most specific:

1. repository-identity bindings;
2. directory bindings, shallowest to deepest;
3. committed in-repository declarations, root to current directory;
4. machine-local private declarations;
5. the existing session, task, and one-shot overlays.

When repository and directory matchers name the same Project Specification,
they contribute to one specification rather than creating duplicate projects.
If distinct specifications tie at the same specificity, resolution fails with
`project.ambiguous_binding` and reports every candidate. AIKit never chooses by
discovery order, filesystem enumeration order, or whichever remote happens to
appear first.

Skill sets compose by stable member union with duplicate skill identities
reported once. Profiles keep the existing precedence algebra, so their declared
order remains explainable and a later scope can deliberately override an earlier
ordinary choice.

## Consequences

Repository bindings are portable and suitable for shared configuration;
directory bindings are host-local and must not leak machine paths into shared
state. Hot swapping changes a Project Instance's resolved generation and reports
the real activation effect for each harness; it does not redefine project
identity or imply that an already-running harness has reloaded its skill roots.
