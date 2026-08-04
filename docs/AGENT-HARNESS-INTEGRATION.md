# Agent harness integration

This is the production workflow for making a repository-backed skill pack and
a machine-local skill available to Codex and Claude Code through the same AIKit
project profile.

```sh
aikit source add-git mattpocock https://github.com/mattpocock/skills.git \
  --revision <exact-commit> --root skills
aikit source sync mattpocock
aikit source promote mattpocock \
  --trust-skill skill/mattpocock/engineering/wayfinder \
  --trust-skill skill/mattpocock/engineering/setup-matt-pocock-skills \
  --trust-skill skill/mattpocock/productivity/grilling \
  --trust-skill skill/mattpocock/engineering/domain-modeling \
  --trust-skill skill/mattpocock/engineering/prototype \
  --trust-skill skill/mattpocock/engineering/research

aikit source add-directory writing-guidance-tools \
  /Users/admin/Documents/Books/writing-guidance-tools
aikit source sync writing-guidance-tools
aikit source promote writing-guidance-tools \
  --trust-skill skill/writing-guidance-tools/writing-guidance-tools
```

Create the reusable foundation and make it the default for project bindings:

```sh
aikit set create mattpocock/wayfinder-foundation \
  skill/mattpocock/engineering/wayfinder \
  skill/mattpocock/engineering/setup-matt-pocock-skills \
  skill/mattpocock/productivity/grilling \
  skill/mattpocock/engineering/domain-modeling \
  skill/mattpocock/engineering/prototype \
  skill/mattpocock/engineering/research \
  skill/writing-guidance-tools/writing-guidance-tools
aikit project defaults --set mattpocock/wayfinder-foundation
```

Project defaults choose reusable Skill Sets when a Project Specification is
created. They are not the persistent user scope. Put capabilities or profiles
that should resolve in every context in the User Baseline Profile instead:

```sh
aikit enable skill/mattpocock/engineering/wayfinder --scope user
aikit use <profile-id> --scope user
```

`user` and `global` name the same lowest-precedence scope, stored at
`~/.aikit/scopes/global/profile.toml`. Project, session, and task declarations
remain free to override it.

Bind by one or more canonical directories, Git repository identities, or both:

```sh
aikit project bind my-project --directory /absolute/project/path
aikit project bind shared-project \
  --repository https://github.com/owner/repository.git \
  --set another/optional-set
```

Directory bindings are private to the machine. Repository bindings match SSH
and HTTPS transports by normalized repository identity, including clones and
linked worktrees. Multiple projects may select the same Skill Set, and each
project may select multiple sets. Use `--no-default-skill-sets` when a project
must start without the configured foundation.

Enable desired capabilities in the project scope, then apply:

```sh
aikit enable skill/mattpocock/engineering/wayfinder --scope project
aikit apply
aikit project show
aikit status --all
```

Add user-specific orientation without modifying or copying the upstream skill:

```sh
aikit skill overlay set skill/mattpocock/engineering/wayfinder \
  --scope user \
  --description "Prefer for work spanning agent sessions." \
  --guidance-file /absolute/path/to/wayfinder-orientation.md \
  --reviewed-against <exact-64-character-source-revision>

aikit skill overlay show skill/mattpocock/engineering/wayfinder
```

More-specific project/session/task overlays append after the user orientation.
Use `--no-inherit` when a scope must discard lower-scope orientation, and
`aikit skill overlay clear <skill-id> --scope <scope>` to return that scope to
inheritance. The emitted section explicitly identifies itself as
user-authoritative orienting augmentation; it never changes trust, permissions,
payload identity, or upstream invocation policy.

AIKit publishes immutable Codex and Claude Code projections together. Codex
also receives a project-native `.agents/skills` link when the context is
isolated. Preserve `AIKIT_CONTEXT_ID` across mux panes and child processes so
they resolve the same generation. After changing the selected skill catalogue
or changing Skill Usage Overlays, start a new harness task unless that harness
explicitly confirms live reload. A generation swap updates the filesystem
immediately; it cannot force an already-running harness to forget its cached
skill catalogue or prompt.

The full Matt Pocock pack is catalogued, but this workflow trusts only the six
selected Matt skills. Additional pack members remain unavailable until they are
reviewed and named explicitly with another `--trust-skill` promotion.

To update safely, run `aikit source set-revision <source> <exact-commit>`, then
`source sync`, inspect the candidate shown by `source show`, and promote it
explicitly. `source rollback` returns to the prior promoted snapshot without
fetching or rebuilding it.
