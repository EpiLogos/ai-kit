# Worktree projection — project checkouts onto `origin/main`

A dev environment or a machine holds a *set* of repository checkouts (worktrees).
Keeping each one at the canonical branch, `origin/main`, is otherwise a manual
loop — `git fetch` + `git reset --hard origin/main` + `git clean`, per repo, per
machine. Worktree projection makes that loop a native, declarative, one-command
capability with drift surfaced and safely repaired.

This is the *git-repository* sense of "projection" — the material state of a
working checkout against the canonical branch it tracks. It is deliberately
distinct from **skill-source projection** (`aikit diff`'s harness-visible skill
copies) and from **Workcell material projection** (execution bindings). The three
never share a code path.

## The one law

> A projection never discards uncommitted or unmerged work to force the target.

Only a **clean** checkout that is **strictly behind** the target (its `HEAD` an
ancestor of `origin/main`) is fast-forwarded, and only with `--apply`. Everything
else is *surfaced* — reported as a delta a human resolves — never reset, cleaned,
or rebased:

| State | Meaning | With `--apply` |
| --- | --- | --- |
| `projected` | `HEAD` is the target (clean, or with local uncommitted edits) | nothing to do |
| `behind` | clean, strictly behind the target | fast-forwarded to the target |
| `behind` + dirty | behind, but the working tree has uncommitted changes | **surfaced** — commit/stash first |
| `ahead` | local commits not on the target | **surfaced** — push/PR; never discarded |
| `diverged` | local *and* remote history moved | **surfaced** — reconcile by hand |
| `target-missing` | `origin/main` could not be resolved | **surfaced** — fetch / check the ref |
| `failed` | the checkout could not even be read | **surfaced**, other repos still processed |

A detached `HEAD` — the usual shape of a dev worktree — is fast-forwarded in
place and stays detached. `git merge --ff-only` is the only mutation used, so a
change that lands between the read and the apply cannot cause a clobber: the
fast-forward simply refuses.

## The command

```sh
aikit worktree project \
  --repo <key>=<path> [--repo …] \
  [--target origin/main] [--apply] [--no-fetch] [--json]
```

- `--repo <key>=<path>` (repeatable, required): a checkout to project. `<key>` is
  the report key (e.g. the dev-world project key); `<path>` is the checkout root.
  A bare `<path>` derives the key from the directory name.
- default is **observe** — read-only; `--apply` performs the safe fast-forwards.
- default **fetches** the remote first; `--no-fetch` compares against the
  last-fetched target.
- `--target` overrides `origin/main` (a bare ref resolves against `origin`).

The JSON envelope carries a structured `SuiteProjection`
(`aikit.worktree-projection/v1`) — one `RepoProjection` per checkout with its
divergence, cleanliness, and action — plus a plain `summary`. Checkouts still
needing a human (surfaced / failed) also come back as reply warnings.

## Ownership

The capability sits exactly where the suite's ownership law puts it:

- **AIKit** owns *repository / worktree / branch* state, so it owns the git
  projection: fetch, the ahead/behind ancestry read, the classification, and the
  one safe fast-forward. Implemented on the existing native-Git provider
  (`NativeGitProvider`, `aikit-adapters`) over the pure model in
  `aikit-core::resource::worktree_projection`.
- **Central** owns the *desired-state / machine-role* declaration — "this
  machine/environment projects `main` for the whole suite" — as authored intent
  on the `central.machine` role (`Control/machines/current.json`). *(Next
  increment.)*
- **Workcell** owns *material lifecycle*, not git (its `ExecutionDemand` carries
  no branch/worktree fields, by contract). A projected worktree can be carried as
  an opaque correlation subject in a Workcell world, but Workcell never performs
  or re-derives the git projection. *(Correlation is a later increment.)*
- **O-I** composes the one-command operator surface: it resolves the machine's
  checkout roots (the dev-world `[projects]` table) and delegates to
  `aikit worktree project`, mirroring how `oi dev world` resolves tokens and
  delegates to `aikit session up`. *(Next increment.)*

## Remaining increments

1. **Central desired state** — a "projects `main`" policy on the `central.machine`
   declaration, surfaced through `machine.plan` / `machine.verify` so the
   projection has an authored target to reconcile toward and a drift verdict.
2. **O-I operator surface** — `oi …` resolves the dev-world checkout roots and
   delegates to this command for a true whole-suite one-liner, plus remote
   machines over the existing ssh/dev-world path.
3. **Workcell correlation** — surface the AIKit projection as a correlated
   reconcile observation on a Workcell material world (opaque subject only), so a
   "this environment projects main" desired material state reads through
   `workcell` without Workcell owning git.
