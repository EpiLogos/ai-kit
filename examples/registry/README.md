# AIKit seed registry

A small, genuinely useful registry that doubles as documentation of *what a real
capsule looks like*. Every capsule here is exercised by the release-blocking
acceptance suite in `crates/aikit-cli/tests/acceptance.rs`, so these files are
guaranteed to load, resolve and project — they are not illustrative pseudocode.

Point AIKit at it by copying the tree under a name in your home:

```sh
cp -R examples/registry "$AIKIT_HOME/registries/seed"
```

The layout is exactly what `aikit-store` expects of any registry
(`ARCHITECTURE.md` §5):

```
registries/<name>/
  capsules/<kind>/<group>/<name>/manifest.toml   # the envelope
                                 /payload/...     # the typed payload
```

The capsule's directory path **must** equal its declared id, and the id's kind
prefix **must** equal the `kind` field — the store refuses a capsule whose path
and id disagree, so a capsule can never masquerade as one you already reviewed.

## What is here

| Capsule | Kind | What it is |
|---|---|---|
| `script/rust/cargo-nextest` | script | Runs the suite with `cargo-nextest`, falling back to `cargo test`. Exports `nt`. |
| `skill/rust/rust-review` | skill | A reviewer's checklist for a Rust diff, as a native Agent Skill. |
| `skill/rust/unsafe-audit` | skill | A focused audit of `unsafe` blocks and their invariants. |
| `hook/guard/project-boundary` | hook | A `PreToolUse` gate that denies tool calls reaching outside the project. |
| `guidance/research/deep-research` | guidance | Injected method for multi-source, source-verified research. |
| `session/dev/rust-dev` | session | A portable three-pane Rust session (editor · tests · agent). |

`unsafe-audit` is a second skill beyond the five the brief names: the acceptance
suite's *"two tmux sessions carry different skill sets"* case is a comparison of
two **non-empty** skill sets, which is a stronger demonstration than one session
having a skill and the other having none.

## Trust is not included, on purpose

Nothing here ships a trust decision. A manifest **may not** declare its own trust
(`manifest.trust_not_self_declarable`), and being present in a registry is not
being reviewed. Skills, hooks and guidance stay inert until a human reviews the
exact `(source, capsule, revision)` — which is what the acceptance suite does
explicitly before it expects any of them to activate.
