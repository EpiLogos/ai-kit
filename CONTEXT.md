# AIKit

AIKit resolves and projects the agent capabilities appropriate to one working
context. This language keeps reusable project configuration separate from the
live checkout and session in which an agent is operating.

## Language

**Project Specification**:
A stable, reusable project declaration containing identity matchers and ordered
profile and skill-set bindings.
_Avoid_: Project profile, project template, project config

**Project Instance**:
One concrete working directory matched to a Project Specification, with its own
context and session state.
_Avoid_: Checkout, workspace, project

**Project Binding**:
The ordered association from a Project Specification to a profile or skill set.
_Avoid_: Assignment, attachment

**Repository Identity**:
A normalized Git remote name used to recognize clones and worktrees of the same
repository without contacting a network service.
_Avoid_: Repository URL, origin URL, local repository

**Project Boundary**:
The directory at which project matching and inherited project scope begin or
reset.
_Avoid_: Repository root, workspace root

**Skill Set**:
A reusable, ordered collection of skill sources that can be bound to any number
of Project Specifications.
_Avoid_: Skill pack, plugin, profile

**User Baseline Profile**:
The lowest-precedence persistent profile through which one user activates and
configures capabilities across AIKit contexts.
_Avoid_: Global settings, default set, system profile

**Skill Usage Overlay**:
Scoped, additive, user-authoritative guidance that orients how an immutable
upstream skill should be selected and applied without changing its invocation
policy, trust, payload, or source identity.
_Avoid_: Skill patch, prompt override, fork

**Effective Skill**:
The harness-facing skill produced from one immutable upstream skill plus its
ordered Skill Usage Overlays for the current context.
_Avoid_: Modified skill, patched source, custom skill
