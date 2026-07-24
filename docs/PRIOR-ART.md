# Prior art

A survey of systems that solve some part of what AIKit calls the *omni-harness*:
a context-scoped capability router that gives a whole machine a uniform,
explainable, safely-mutable description.

This document is written for implementers. It records **how these systems
actually work internally**, with file and function references where they were
verifiable from source, so that decisions in
[`ARCHITECTURE.md`](./ARCHITECTURE.md) can be defended or revised against real
engineering experience rather than marketing copy.

Every system is assessed against the same five questions:

1. **Scope.** Is state a global mutable set, or per-context? If per-context, how
   is context identified?
2. **Generations.** Is there an immutable/content-addressed generation concept?
   Atomic switch? Rollback?
3. **Trust.** What is the trust/review model, if any?
4. **Explanation.** How does it explain *why* something is active?
5. **Contention.** What is the failure mode when two contexts want different
   things simultaneously?

The two closing sections — [What none of them do](#what-none-of-them-do) and
[What we should steal](#what-we-should-steal) — are the payload.

---

## Contents

- [Tier 1 — the generation model](#tier-1--the-generation-model)
  - [Nix, NixOS and home-manager](#nix-nixos-and-home-manager)
  - [GNU Guix profiles](#gnu-guix-profiles)
  - [chezmoi](#chezmoi)
- [Tier 2 — per-directory environment and tool resolution](#tier-2--per-directory-environment-and-tool-resolution)
  - [mise (formerly rtx) and asdf](#mise-formerly-rtx-and-asdf)
  - [direnv](#direnv)
  - [devenv, devbox, flox](#devenv-devbox-flox)
- [Tier 3 — agent capability systems](#tier-3--agent-capability-systems)
  - [Agent Skills (Anthropic, Codex, agentskills.io)](#agent-skills-anthropic-codex-agentskillsio)
  - [Block Goose](#block-goose)
  - [MCP and Claude Code MCP scopes](#mcp-and-claude-code-mcp-scopes)
  - [Registries: official MCP registry, Smithery](#registries-official-mcp-registry-smithery)
  - [Continue.dev, aider, open-interpreter](#continuedev-aider-open-interpreter)
- [Tier 4 — capture and search](#tier-4--capture-and-search)
  - [atuin](#atuin)
  - [fzf, nucleo, skim](#fzf-nucleo-skim)
- [Cross-cutting comparison](#cross-cutting-comparison)
- [What none of them do](#what-none-of-them-do)
- [What we should steal](#what-we-should-steal)
- [Sources](#sources)

---

# Tier 1 — the generation model

## Nix, NixOS and home-manager

This is the closest existing thing to the AIKit generation design, and the one
with the most accumulated operational experience. It is worth being precise
about which layer does what, because "Nix generations" is actually three
mechanisms stacked.

### Layer 1: the store — content addressing that isn't content addressing

`/nix/store/<hash>-<name>` looks content-addressed but by default is **input**-
addressed: the hash is over the derivation (its inputs, builder, and
environment), not over the produced output. Nix has a true content-addressed
mode (`ca-derivations`) but it is still experimental. Practically: the hash is a
*deterministic function of the recipe*, which is exactly what AIKit's resolution
hash is — a function of `(context, catalog revision, overlay, isolation)` rather
than of the bytes produced.

That is the right choice for AIKit too, and for the same reason: you want the
hash **before** you build, so you can short-circuit a no-op apply. AIKit's
`ARCHITECTURE.md` §6 says "rename to content hash"; in practice you almost
certainly want the *resolution* hash (pre-build, from the lock) as the directory
name, and the materialized-tree hash only as a validation field in
`metadata.json`. Nix learned this the hard way — see `createGeneration` below,
which compares store paths rather than tree contents.

### Layer 2: profiles — the generation chain

Source: [`src/libstore/profiles.cc`](https://github.com/NixOS/nix/blob/master/src/libstore/profiles.cc).

A *profile* is a symlink (`/nix/var/nix/profiles/system`) pointing at a
*generation link* (`system-1054-link`), which in turn points into the store.

```
system            -> system-1054-link
system-1054-link  -> /nix/store/xxxx-nixos-system-host-25.11
system-1053-link  -> /nix/store/yyyy-nixos-system-host-25.11
```

Key implementation facts:

| Function | Behaviour worth copying |
|---|---|
| `makeName(profile, num)` | Generation naming is `<profile>-<N>-link`, a *monotonic integer*, not a hash. The hash lives at the far end of the link. |
| `createGeneration()` | **Deduplication before allocation.** If `readLink(last.path) == store.printStorePath(outPath)`, it returns the existing generation instead of allocating N+1. The comment: *"This helps keeping gratuitous installs/rebuilds from piling up uncontrolled numbers of generations, cluttering up the UI like grub."* |
| `createGeneration()` | Calls `store.addPermRoot()`, which **blocks if the GC is running**, so a generation cannot be built into a window where GC has a stale root view. |
| `switchLink(link, target)` | Rewrites an absolute target as relative when target and link share a parent — the generation chain is relocatable. Then calls `replaceSymlink`. |
| `replaceSymlink()` ([`src/libutil/file-system.cc:557`](https://github.com/NixOS/nix/blob/master/src/libutil/file-system.cc)) | The atomic switch: create `.<N>_<name>` sibling symlink, retrying with incremented N on `EEXIST`, then `rename()` over the target. `rename(2)` on the same filesystem is atomic; this is the whole trick. |
| `lockProfile(lock, profile)` | Per-profile `PathLocks` with `setDeletion(true)`. All mutation (`switchGeneration`, `deleteGenerations*`) takes it. |
| `optimisticLockProfile()` | Returns the current `readLink()` value as a token. Callers re-read before committing and abort if it changed — a **compare-and-swap on the symlink**. This is Nix's answer to concurrent `nix-env` invocations. |
| `deleteGeneration2(..., dryRun)` | Dry-run is threaded through the *deletion* path, not just the build path, and logs "would remove profile version N". |

`deleteGenerationsOlderThan` / `deleteGenerationsGreaterThan` / `--delete-older-than 30d`
are the GC policy surface. Note that deleting a *generation link* only removes a
GC root; store paths survive until `nix-collect-garbage` runs. **Retention policy
and reclamation are separated.**

### Layer 3: activation — the stateful part

The store and the profile symlink are pure. Everything that has to touch the
outside world lives in an `activate` script inside the generation.

**NixOS** ships `bin/switch-to-configuration` in each system generation. As of
NixOS 25.05 the Perl implementation is gone and the Rust rewrite
(`switch-to-configuration-ng`, [PR #308801](https://github.com/NixOS/nixpkgs/pull/308801))
is used everywhere; it talks to systemd over D-Bus rather than shelling out to
`systemctl`, which lets it reason about unit lifecycle (start/restart/reload/
`.mount` reload-vs-restart) precisely. Crucially, `switch-to-configuration` is
*part of the generation*, so rolling back runs the **old** switch script, not the
new one. AIKit should note this: the activation logic must be versioned with the
generation, not with the binary.

**home-manager** is the closer analogue, because it manages a user's dotfiles —
a symlink farm into `$HOME` — which is structurally what AIKit's projections are.

The activation script is assembled in
[`modules/home-environment.nix`](https://github.com/nix-community/home-manager/blob/master/modules/home-environment.nix)
(`home.activationPackage`). Its structure:

1. **A DAG, not a list.** `home.activation` is `lib.hm.types.dagOf types.str`.
   Blocks declare `entryAfter [...]` / `entryBefore [...]` and are topologically
   sorted by `lib.hm.dag.topoSort`
   ([`modules/lib/dag.nix`](https://github.com/nix-community/home-manager/blob/master/modules/lib/dag.nix),
   a generalisation of nixpkgs' `strings-with-deps.nix` that adds a "wanted by"
   edge). A cycle is a **build-time abort**, not a runtime error:
   `abort ("Dependency cycle in activation script: " + builtins.toJSON sortedCommands)`.

2. **A `writeBoundary` sentinel node.** Every block that has an observable side
   effect must be `entryAfter [ "writeBoundary" ]`. Everything before the
   boundary may *verify but not mutate*. This is the single most valuable idea in
   the whole system: **the plan/verify phase and the mutate phase are separated
   by a node in the same ordering graph**, so a third-party module cannot
   accidentally interleave a write into the checking phase.

3. **`DRY_RUN` is a first-class contract.** Blocks are documented to respect
   `DRY_RUN`, and a `run` helper (`run`, `run --quiet`, `run --silence`) is
   provided that echoes instead of executing. See `modules/lib-bash/activation-init.sh`.

4. **A GC root taken with a `trap`:**
   ```bash
   trap 'run rm -f $VERBOSE_ARG "$newGenGcPath"' EXIT
   run --silence nix-store --realise "$newGenPath" --add-root "$newGenGcPath"
   ...activation...
   run --silence nix-store --realise "$newGenPath" --add-root "$currentGenGcPath"
   ```
   The *new* generation is pinned before activation and the pin is released on
   exit; the *current* pin is only moved after all activation blocks succeeded.
   A crash mid-activation leaves `current-home` pointing at the old generation.

5. **Sanity checks before anything:** `checkStringEq USER`, `checkPathEq HOME`,
   optional UID check, skippable via `SKIP_SANITY_CHECKS`. A generation refuses
   to activate into the wrong identity.

### The collision problem — "the file already exists and differs"

This is the hardest real problem in the whole space and home-manager's answer is
worth reading in full:
[`modules/files/check-link-targets.sh`](https://github.com/nix-community/home-manager/blob/master/modules/files/check-link-targets.sh).

Ownership is decided by a **path pattern on the symlink target**:

```bash
homeFilePattern="$(readlink -e @storeDir@)/*-home-manager-files/*"
```

If `$targetPath` exists and `readlink "$targetPath"` does not match that glob,
the file is not ours. The decision cascade is then:

| Condition | Outcome |
|---|---|
| target is under a `force = true` path | skip collision check entirely |
| contents are byte-identical (`cmp -s`) | warn, skip — *not* an error |
| `HOME_MANAGER_BACKUP_COMMAND` set | delegate to user command, assume success |
| `HOME_MANAGER_BACKUP_EXT` set, no existing backup | warn, will `mv` to `$target.$ext` |
| backup exists and `HOME_MANAGER_BACKUP_OVERWRITE` set | warn, clobber backup |
| backup exists, no overwrite flag | **collision error** |
| otherwise | **collision error** |

Collisions are **accumulated into an array and reported together** with a
remediation menu, then `exit 1`. It does not fail on the first collision.

### The link/clean ordering proof

From `modules/files.nix`, the comment on `home.activation.linkGeneration`:

> 1. Remove files from the old generation that are not in the new generation.
> 2. Symlink files from the new generation into `$HOME`.
>
> This order is needed to ensure that we always know which links belong to which
> generation. Specifically, if we're moving from generation A to generation B
> having sets of home file links FA and FB, respectively then cleaning before
> linking produces state transitions similar to
>
> `FA → FA ∩ FB → (FA ∩ FB) ∪ FB = FB`
>
> and a failure during the intermediate state `FA ∩ FB` will not result in lost
> links because this set of links are in both the source and target generation.

This is a **crash-safety argument written into the source**. The intermediate
state is a subset of both endpoints, so an interrupted activation is recoverable
by re-running either direction. AIKit's projections need the same argument, and
should carry it in the same place — a comment on the function that establishes
the invariant.

The cleanup script also refuses to delete anything whose readlink does not match
`homeFilePattern`: *"Path '$targetPath' does not link into a Home Manager
generation. Skipping delete."* Unmanaged files are never removed, even if they
occupy a managed path.

### Profile migration

`modules/lib-bash/activation-init.sh`'s `migrateProfile()` is a small lesson in
storage-location churn. Profiles moved from `/nix/var/nix/profiles/per-user/$USER`
to `$XDG_STATE_HOME/nix/profiles` in Nix 2.14. The migration re-realises each old
generation link into the new directory with `nix-store --realise "$p" --add-root
"$newProfilesDir/$name"` — it does not `mv`, because the links are GC roots and
must be re-registered — then `cp -P` the current pointer and deletes the old ones.

Related: each generation writes `gen-version` (currently `1`) into its output,
which "indicates the format of the generation package itself. It allows us to
make backwards incompatible changes in the package output and have surrounding
tooling adapt." **A generation declares its own format version.** AIKit's
`metadata.json` must do this from day one.

### Explanation

Nix's answer to "why is this active" is assembled from several tools, none of
which is a single `explain`:

- **`nix why-depends A B`** — traces a dependency path through the closure. This
  answers "why is this package here" but not "which config file turned it on".
- **`nix store diff-closures` / `nix profile diff-closures` / `nvd` / `nix-diff`** —
  what changed between two generations. Preview-before-switch is a community
  practice, not built in.
- **The module system's option priorities** — `lib/modules.nix`:
  `mkOptionDefault = mkOverride 1500`, `mkDefault = mkOverride 1000`,
  `mkForce = mkOverride 50`. All definitions of an option are gathered, the
  lowest-priority-number set wins, and they are merged. Conflicting definitions
  produce an error naming the *files*: `In 'moduleB.nix': true` /
  `In 'moduleC.nix': false`. This works because every definition carries a
  `_file` location through `definitionsWithLocations`.

That last mechanism is exactly AIKit's `explain`, and NixOS proves it is viable
at scale — but note that it only surfaces on *conflict*. There is no ergonomic
"show me the layer chain for this option in the success case" (`nixos-option`
approximates it and is widely considered poor). **AIKit's differentiator is
making the success-case explanation as good as the failure-case one.**

### Assessment

| Question | Answer |
|---|---|
| Scope | **Global per profile.** Profiles are per-user or per-system, not per-directory or per-session. `nix develop` / `nix-shell` give per-directory *ephemeral* environments but they are outside the generation model entirely — no generation, no rollback, no explanation. |
| Generations | Yes, canonical. Integer-numbered symlink chain into an input-addressed store, deduplicated on identical target, atomic `symlink`+`rename` switch, `switchGeneration` for rollback, `deleteGenerations*` + `nix-collect-garbage` for GC. |
| Trust | **None.** Nix has no trust model for configuration. Trust is at the *substituter* level (binary cache public keys, `trusted-users`). Any expression you evaluate can run arbitrary code at build time. Flakes' `--accept-flake-config` prompt is the closest thing and is widely disliked. |
| Explanation | Partial: dependency tracing (`why-depends`), closure diffing (`nvd`), and module option priority with file locations — but only reported on conflict. |
| Contention | Serialised by `lockProfile` + `optimisticLockProfile` CAS. Two contexts **cannot** want different things: there is one profile. `nix-shell` layering is the escape hatch and it is unmanaged. |

---

## GNU Guix profiles

Source: [`guix/profiles.scm`](https://git.savannah.gnu.org/cgit/guix.git/tree/guix/profiles.scm).

Guix inherits the Nix store and generation model and then makes four decisions
differently. All four matter to AIKit.

### 1. The generation carries its own manifest

```scheme
(define (profile-manifest profile)
  "Return the PROFILE's manifest."
  (let ((file (string-append profile "/manifest")))
    (if (file-exists? file)
        (call-with-input-file file read-manifest)
        (manifest '()))))
```

Every built profile contains a `manifest` file describing exactly the entries
that produced it, in a **versioned serialisation format**
(`%manifest-format-version`, threaded through `profile-derivation` as
`#:format-version`). `generation-profile` even uses the presence of
`<profile>/manifest` as the *test* for whether a directory is a real profile
generation.

Consequences Guix gets for free that Nix does not:

- `guix package --export-manifest` reconstructs a declarative `manifest.scm`
  from an imperatively-built profile.
- `guix package --list-generations` can diff *semantic entries*, not just store
  paths.
- A profile is self-describing when detached from the machine that built it.

**This validates AIKit's `resolution.lock.toml` inside `generations/<hash>/`
completely.** Guix's experience says: version the format, and make its presence
the definition of "this directory is a generation".

### 2. Manifest entries carry provenance

```scheme
(define-record-type* <manifest-entry> manifest-entry
  (name ...) (version ...) (output ...) (item ...)
  (dependencies manifest-entry-dependencies  ; <manifest-entry>*
                (default '()))
  (search-paths manifest-entry-search-paths  ; search-path-specification*
                (default '()))
  (parent       manifest-entry-parent        ; promise (#f | <manifest-entry>)
                (default (delay #f)))
  (properties   manifest-entry-properties
                (default '())))
```

`parent` is a **lazy back-pointer to the entry that pulled this one in**. That is
AIKit's `required_by` field, already proven in production. `properties` is an
open key/value bag for metadata that does not participate in equality —
`manifest-entry=?` explicitly ignores it. AIKit should copy both: a `required_by`
chain *and* a non-semantic `properties` bag so that UI hints, timestamps and
provenance annotations don't perturb the resolution hash.

### 3. Collisions fail by default, with both sides named

```scheme
(define-condition-type &profile-collision-error &error
  profile-collision-error?
  (entry    profile-collision-error-entry)      ; <manifest-entry>
  (conflict profile-collision-error-conflict))  ; <manifest-entry>
```

`profile-derivation` calls `check-for-collisions` unless
`allow-collisions? = #t`. The error carries **both colliding entries as
structured data**, so the reporter can walk each one's `parent` chain and explain
how each arrived. Guix also supports per-entry `properties` for package
*transformations*, so `guix package --list-generations` can show that an entry is
present *because of* a `--with-input` rewrite.

AIKit rule 5 ("conflicts fail visibly by default") should be implemented as a
structured error carrying both `CapabilityRef`s and both origin chains — not a
formatted string.

### 4. Generation 0 is synthesised, not stored

```scheme
(define (link-to-empty-profile store generation)
  "Link GENERATION, a string, to the empty profile."
  (let* ((drv (run-with-store store (profile-derivation (manifest '()) ...)))
         (prof (derivation->output-path drv "out")))
    (build-derivations store (list drv))
    (switch-symlinks generation prof)))
```

`roll-back` and `delete-generation` call this when the previous generation is
missing or is number 0. **Rolling back from the first real generation always
works**, because "nothing" is a buildable, materialisable state rather than a
special case. Compare Nix, where rolling back past generation 1 errors.

This is directly applicable: AIKit should be able to materialise an **empty
generation** — a valid `current/` with an empty lock, empty `bin/`, empty
`hooks/` — so that "turn everything off in this session" and "roll back to before
this context existed" are the same code path, not an unimplemented edge.

### The atomic switch

```scheme
(define (switch-symlinks link target)
  "Atomically switch LINK, a symbolic link, to point to TARGET.  Works
both when LINK already exists and when it does not."
  (let ((pivot (string-append link ".new")))
    (let symlink/remove-old ()
      (catch 'system-error
        (lambda () (symlink target pivot))
        (lambda args
          (if (= (system-error-errno args) EEXIST)
              (begin (delete-file pivot) (symlink/remove-old))
              (apply throw args)))))
    (rename-file pivot link)))
```

Same technique as Nix, but note the difference in the collision strategy: Guix
uses a **fixed** pivot name (`link.new`) and deletes a stale one, explicitly
justified by "This can happen if a previous switch-symlinks was interrupted".
Nix uses an **incrementing** pivot name and never deletes. Guix's version is
simpler and self-healing after a crash; Nix's is safer under genuine concurrency.
AIKit takes the per-context file lock anyway (`state/locks/`), so the Guix
approach is sufficient and its crash-recovery property is worth having.

### Profile hooks — the projection layer

`%default-profile-hooks` is a list of monadic procedures run at profile build
time to produce **derived, unified artefacts** from the union of entries:
`info-dir-file`, `manual-database`, `ca-certificate-bundle`, `glib-schemas`,
`gtk-icon-themes`, `xdg-desktop-database`, `fonts-dir-file`, and others. Each is
tagged with `#:properties '((type . profile-hook) (hook . <name>))` so build
progress can report which hook is running.

This is precisely AIKit's `projections/{claude,codex,shell}/`: a set of
target-specific views derived from one resolved manifest, built inside the
generation, atomic with it. Guix's lesson: **tag each projection with its
producer** so a slow or failing projection is attributable.

The profile also emits `etc/profile` containing `search-path-specification`
exports — the shell projection — computed as a union across entries. AIKit's
shell projection should likewise be *derived*, not authored.

### Assessment

| Question | Answer |
|---|---|
| Scope | Global per profile, same as Nix. `guix shell` (per-project, via `guix.scm`/`manifest.scm`) is a separate, non-generational mechanism, though it does have a profile cache and `--check` to verify the shell wasn't clobbered. |
| Generations | Yes, plus **self-describing manifests**, a **versioned manifest format**, and a **synthesised empty generation** so rollback is total. |
| Trust | None for local configuration. Substitute servers are keyed. `guix challenge` verifies reproducibility across servers — a *provenance* mechanism with no analogue elsewhere in this survey. |
| Explanation | Best in class of the Nix family: `manifest-entry-parent` gives a "required by" chain; `guix describe` + `channels.scm` + `guix time-machine` pin and replay the *entire* config source; collision errors carry both structured entries; `--export-manifest` reconstructs declarative source from state. |
| Contention | Same as Nix — one profile, serialised. Guix's answer to "two contexts want different things" is `guix shell`, i.e. leave the generation model. |

---

## chezmoi

Sources: [`www.chezmoi.io/developer-guide/architecture/`](https://www.chezmoi.io/developer-guide/architecture/),
[`www.chezmoi.io/reference/concepts/`](https://www.chezmoi.io/reference/concepts/).

chezmoi is the only Tier 1 system with **no generation concept at all**, and it
is instructive precisely because of what it does instead.

### The four states

| State | Meaning | Where |
|---|---|---|
| **Source state** | Declared desired state. Regular files and directories only; attributes are encoded in *filenames* (`private_`, `executable_`, `dot_`, `.tmpl`, `encrypted_`, `run_once_`, `run_onchange_`, `create_`, `modify_`, `symlink_`). | `~/.local/share/chezmoi` |
| **Target state** | Computed from source state + config file + destination state. Template execution happens here. | in-memory |
| **Destination/actual state** | What is really on disk. | `$HOME` |
| **Persistent state** | A two-level `map[Bucket]map[Key]Value` BoltDB store recording **SHA256 checksums, not contents**. | `~/.config/chezmoi/chezmoistate.boltdb` |

Interfaces: `SourceStateEntry` (`SourceStateFile`, `SourceStateDir`) →
`TargetStateEntry` (`TargetStateFile`, `TargetStateDir`, `TargetStateSymlink`,
`TargetStateRemove`, `TargetStateScript`) compared against `ActualStateEntry`
(`ActualStateAbsent`, `ActualStateFile`, `ActualStateDir`, `ActualStateSymlink`).

### The `System` interface — dry-run as a decorator

All OS interaction goes through a `System` interface. `RealSystem` is the base;
`DryRunSystem` and `DebugSystem` **wrap** it. Dry-run is therefore not a flag
checked at a hundred call sites (contrast home-manager's `DRY_RUN`/`run` shell
convention, which relies on every module author remembering) — it is a
**substituted implementation of the effect boundary**. `chezmoi diff` is
`chezmoi apply` against a diff-emitting system.

This is the single cleanest engineering idea in chezmoi and it maps directly onto
AIKit's crate boundary: `aikit-core` is already I/O-free, so `aikit-store`'s
materialiser should be a trait with `RealFs`, `DryRunFs`, `DiffFs` implementations
rather than an `if dry_run` ladder. `aikit apply --dry-run`, `aikit diff`, and the
palette's "what will this toggle do?" preview then share one code path by
construction.

### Third-party modification detection

The persistent state records the checksum of each entry as chezmoi last wrote it.
On apply, chezmoi compares *persistent state* against *actual state*: if they
differ, the file was changed by something other than chezmoi since the last apply.
This is a three-way comparison — desired / last-known-written / actual — where
home-manager only does two (desired / actual, plus an ownership heuristic on the
symlink target).

The three-way comparison is strictly better and AIKit should use it. AIKit's
`state/aikit.sqlite3` can hold `(context_id, projection_target, path, sha256,
generation)` rows written at materialisation time; a subsequent apply that finds
a different hash knows the difference between "user edited our projection"
(preserve or prompt) and "the previous generation wrote something different"
(safe to replace).

### Script execution state

- `run_once_<name>` — SHA256 of *contents* stored in the `scriptState` bucket.
  Re-runs only if the content changes.
- `run_onchange_<name>` — target name and content hash stored in `entryState`.
- Ordering by `before_` / `after_` prefixes and numeric name prefixes.

Content-hash-keyed idempotence is exactly what AIKit needs for capsule install /
postinstall steps and for `once`-semantics hooks.

### Encryption

An `Encryption` interface with `AGEEncryption` and `GPGEncryption`
implementations. Encrypted source files are `encrypted_<name>.age`; they are
decrypted into the target state at apply time and **never** written to the source
directory in plaintext. There is a separate mechanism for one-off secrets: template
functions calling out to a password manager (`onepasswordRead`, `bitwarden`,
`pass`, `keepassxc`, `vault`, `secret`), which means secrets are *referenced* in
the committed source, never *stored* in it.

That distinction — **encrypted-in-repo vs referenced-from-an-external-agent** —
is the right frame for AIKit's `inbox/` and the "a captured secret never enters
the ordinary registry" acceptance case (§15.10). Capsules should be able to
declare a secret *reference* (`keychain:`, `op://`, `env:`) resolved at
projection time, in addition to whatever encrypted-payload support exists.

### Diff-first discipline

The documented workflow is `chezmoi diff` → `chezmoi apply`, with
`chezmoi apply --dry-run --verbose` as the belt-and-braces variant, plus
`chezmoi status` (a `git status`-shaped two-column summary) and `chezmoi
verify` (exit non-zero if any target differs — the CI form). `chezmoi apply` on
a single path is supported, so review-then-apply can be incremental.

Notably, chezmoi's `diff` uses `diff.pager` and can be routed to an external
difftool, and `chezmoi merge` opens a three-way merge (`vimdiff` by default)
between source, destination and target when the answer is "both changed".
**"Both changed" is treated as a first-class outcome with a dedicated verb**,
not an error.

### Assessment

| Question | Answer |
|---|---|
| Scope | **One global target: `$HOME`.** Per-machine variation is handled by *templating* (`.chezmoi.hostname`, `.chezmoi.os`, `.chezmoi.arch`, custom `data`) rather than by scoping. There is exactly one applied state per machine. |
| Generations | **None.** No immutable snapshot, no atomic switch, no rollback. Rollback means `git revert` in the source repo then re-apply — and re-apply is not transactional. |
| Trust | Minimal. `chezmoi init <repo>` warns about executing scripts from untrusted repos; `--no-tty`/`--force` bypass prompts. No content-hash approval database. Encryption is confidentiality, not integrity or authorisation. |
| Explanation | `chezmoi source-path <target>` maps a target file back to the source file that produced it; `chezmoi data` dumps the template variables; `chezmoi execute-template` evaluates a template against them; `chezmoi cat` shows the computed target contents. This is a genuinely good **provenance-by-inversion** answer: *given an output, show me the input.* |
| Contention | Not modelled. Two chezmoi processes applying concurrently will race. Two *contexts* wanting different things is unrepresentable. |

---

# Tier 2 — per-directory environment and tool resolution

## mise (formerly rtx) and asdf

### asdf

Resolution is a simple upward walk. `asdf` looks for `.tool-versions` in the
current directory, then each parent, then `$HOME/.tool-versions`. The **first**
file containing an entry for the requested tool wins for that tool — resolution
is per-tool, not per-file, so a nested `.tool-versions` naming only `node` still
inherits `python` from a parent. `ASDF_<TOOL>_VERSION` overrides everything.
With `legacy_version_file = yes` in `.asdfrc`, plugins may additionally read
foreign files (`.ruby-version`, `.nvmrc`).

Injection is via **shims**: `$ASDF_DATA_DIR/shims` on `$PATH`, one tiny
executable per binary of every installed version. Running `node` runs the shim,
which resolves and `exec`s the real binary. Consequences: correct in any process
(not just interactive shells), but a `$PATH` scan sees shim names rather than
real tools, `which node` lies, and every invocation pays a resolution cost.

### mise

Sources: [`docs/configuration.md`](https://github.com/jdx/mise/blob/main/docs/configuration.md),
[`src/config/config_file/mod.rs`](https://github.com/jdx/mise/blob/main/src/config/config_file/mod.rs),
[`src/hook_env.rs`](https://github.com/jdx/mise/blob/main/src/hook_env.rs).

**The config chain.** Within one directory, in decreasing precedence:

```
mise.local.toml
mise.toml
mise/config.toml
.mise/config.toml
.config/mise.toml
.config/mise/config.toml
.config/mise/conf.d/*.toml      (alphabetical)
```

plus `mise.<env>.toml` variants selected by `MISE_ENV` (which is a *list*:
`MISE_ENV=ci,test`), and `mise.<os>.toml`. mise then walks the directory tree
upward, collecting the whole chain, so unlike direnv **every ancestor
contributes**. Above that sit `~/.config/mise/config.toml` and
`/etc/mise/config.toml`.

**Section-specific merge semantics** — this is the interesting part, and it is
the thing AIKit's §4 "seven rules" is a more rigorous version of:

| Section | Merge |
|---|---|
| `[tools]` | additive with override — a parent's `python` survives a child that only names `node` |
| `[env]` | additive with override, order-sensitive |
| `[settings]` | additive with override |
| `[tasks]` | **whole-task replacement** — a child redefining `tasks.build` replaces it entirely, no key-level merge |

Different sections having different merge algebras is not a wart; it is a
recognition that "a set of tools" and "a named procedure" are different kinds of
thing. AIKit already distinguishes `enable`/`disable` lists from `[config.*]`
tables; the mise experience says be explicit and documented about the algebra of
each, because users *will* discover it by surprise otherwise.

**Write-target selection.** `mise use`, `mise set`, `mise unset` write to *"the
lowest precedence file in the highest precedence directory"* — i.e. `mise.toml`
in the nearest configured directory, deliberately **not** `mise.local.toml`, so
that the common case produces a shareable, committable change. `--env local`
explicitly targets the ignored file. AIKit's palette needs the same rule and the
same explicit escape hatch: a toggle must state *which file it will edit* before
it edits it.

**Trust** — `src/config/config_file/mod.rs`. Substantially more nuanced than
direnv:

- `trust(path)` writes a symlink at `$MISE_DATA/trusted-configs/<canonical-path-hash>`
  pointing at the real file. Default trust is therefore **path-keyed, not
  content-keyed** — editing a trusted `mise.toml` does not revoke trust.
- With the `paranoid` setting, `trust()` additionally writes a `.hash` sidecar
  containing `file_hash_sha256(path)`, and `trust_file_hash()` re-verifies it on
  every load. This is direnv's model, opt-in.
- **`safe` mode** is the most interesting idea in the file. From the source
  comment: *"In safe mode, config is inert (no code execution, no env injection
  — see MISE_SAFE / the `safe` setting), so loading an untrusted config is
  harmless and no trust is required. `safe` is global-only, so a project config
  cannot disable it for itself."* Untrusted configuration is **degraded to a
  data-only reading** instead of being refused.
- **Trust follows git worktrees.** If there is no trust record for a path, mise
  calls `git::main_checkout_equivalent()` and inherits trust from the same
  relative path in the repo's main checkout — explicitly disabled in paranoid
  mode "where trust is tied to file contents that can differ between worktree
  branches". Given AIKit's `Isolation::Worktree`, this is a direct requirement:
  spawning a worktree task must not re-prompt for trust on every capsule.
- **Monorepo markers.** Trusting a directory marked `monorepo_root = true` writes
  a `.monorepo` sidecar; `is_trusted` walks ancestors looking for one and trusts
  all descendants.
- **Three states, not two.** `TRUSTED_CONFIGS` and `IGNORED_CONFIGS` are separate
  directories. Declining a trust prompt writes to `IGNORED_CONFIGS`, which
  suppresses future prompting without being a permanent block — a distinct state
  from "never seen" and from "blocked". The precedence in `is_trusted()` is
  carefully ordered and commented: `ignored_config_paths` setting (hard block) >
  in-process cache > `trusted_config_paths` setting > persisted ignore list >
  global-config exemption > monorepo marker > paranoid hash check.
- **`hook-env` never prompts.** `trust_check()` explicitly excludes `cmd ==
  "hook-env"` from the prompt path. A `cd` must never block on a dialog.

**Shell injection** — `src/hook_env.rs`. mise installs a `chpwd`/`precmd` hook
that runs `mise hook-env`, which emits shell code. Two environment variables
carry state across invocations:

- `__MISE_DIFF` — a serialised `EnvDiff` of everything mise added/changed/removed.
  On the next directory change, `mise` computes `env::__MISE_DIFF.reverse().to_patches()`
  and applies the inverse before applying the new diff. **The undo instructions
  live in the environment they modified.**
- `__MISE_SESSION` — records the previous directory and the resolved
  `watch_files` list with mtimes. `should_exit_early()` returns immediately if
  the directory hasn't changed and no watched file's mtime moved, which is what
  keeps a per-prompt hook cheap.

`should_exit_early_fast()` is a further pre-check that avoids even loading the
full config. AIKit's <60 ms warm palette budget and <20 ms hook dispatcher
startup need exactly this two-tier early-exit structure.

### Assessment (mise/asdf)

| Question | Answer |
|---|---|
| Scope | **Per-directory, hierarchical.** Context is the cwd, resolved by upward walk with per-section merge. Plus `MISE_ENV` as an orthogonal named-variant axis. Not per-session: two shells in the same directory get the same answer. |
| Generations | **None.** No snapshot, no atomic switch, no rollback. `mise.lock` (newer) pins resolved versions but is not a materialised generation. Undo exists only as the in-band `__MISE_DIFF` inverse patch. |
| Trust | The richest of any system surveyed: path-keyed by default, content-keyed in paranoid mode, three-state (trusted / ignored / untrusted), monorepo inheritance, git-worktree inheritance, `trusted_config_paths` and `ignored_config_paths` settings, and a `safe` mode that renders untrusted config inert rather than refused. |
| Explanation | `mise doctor`, `mise config ls` (lists every config file in the chain and its precedence), `mise env`, `mise ls --json` (shows source file per resolved tool), `mise settings --all`. Good but per-question, not a unified `explain`. |
| Contention | Not modelled. Two shells in one directory get identical environments. Two shells in *different* directories are independent by construction because state lives in each shell's environment — which is also why there is nothing to roll back. |

---

## direnv

Source: [`internal/cmd/rc.go`](https://github.com/direnv/direnv/blob/master/internal/cmd/rc.go).

direnv is the trust model AIKit is closest to, and its implementation is ~400
lines.

### The trust database

```go
func fileHash(path string) (hash string, err error) {
	if path, err = filepath.Abs(path); err != nil { return }
	fd, err := os.Open(path)
	hasher := sha256.New()
	_, err = hasher.Write([]byte(path + "\n"))   // path is part of the hash
	if _, err = io.Copy(hasher, fd); err != nil { return }
	return fmt.Sprintf("%x", hasher.Sum(nil)), nil
}

func allow(path string, allowPath string) (err error) {
	return os.WriteFile(allowPath, []byte(path+"\n"), 0644)
}
```

Design points, each deliberate:

1. **The key is `SHA256(absolute_path + "\n" + contents)`.** Including the path
   means the same content at a different path is a different grant — you cannot
   launder an approval by copying an approved `.envrc` elsewhere. AIKit's trust
   key is `(registry source, capsule id, content revision)`, which is the same
   idea with a stronger identity component. Keep the source in the key.
2. **The grant is a *file named by the hash*, whose contents are the path.**
   Grants are content-addressed (so an edit silently revokes) but
   *human-enumerable* (so `direnv status`/`direnv prune` can list them and garbage
   collect grants whose path no longer exists). AIKit's trust table should
   likewise store the human-readable identity alongside the hash key so the trust
   ledger is auditable and prunable.
3. **Deny is keyed by path hash only** (`pathHash`, no contents). A denial
   survives edits; an approval does not. **Asymmetric key granularity is
   correct**: fail-safe on the deny side, fail-closed on the allow side. AIKit
   should apply this to `blocked` — blocking a capsule should block it at
   `(source, capsule id)` granularity, not `(source, capsule id, revision)`,
   otherwise a version bump silently unblocks.
4. **Whitelist prefixes** (`direnv.toml` `[whitelist] prefix = [...]`, `exact =
   [...]`) bypass the database entirely for trusted trees. Checked *after*
   `os.Stat(allowPath)` and *after* an `EvalSymlinks` — the comment says *"when
   whitelisting we want to be (path) absolutely sure we've not been duped with a
   symlink"*. Symlink resolution before prefix matching is a real attack surface;
   AIKit's project scope chain has the same exposure.

### Discovery — deliberately non-hierarchical

```go
func findEnvUp(searchDir string, loadDotenv bool) (path string) {
	if loadDotenv { return findUp(searchDir, ".envrc", ".env") }
	return findUp(searchDir, ".envrc")
}
```

`findUp` returns the **first** match walking upward and stops. There is no
merge. If you want the parent's environment you write `source_up` explicitly in
your `.envrc`. This is the opposite of mise's implicit hierarchical merge.

The rationale is trust: with a merge, an approved child `.envrc` would silently
inherit an unapproved parent's effects. With explicit `source_up`, the
inheritance is *in the file you approved*, so the hash covers it. **Implicit
hierarchy and content-hash trust are in tension**, and direnv resolves it in
favour of trust while mise resolves it in favour of ergonomics (and pays for it
with path-keyed rather than content-keyed trust).

AIKit has *both* an implicit scope chain (repo root → cwd, `depth` increasing)
and content-revision trust. This is the exact tension. The resolution should be:
**trust is keyed on capsules, not on profile files** — a profile layer can only
*select* capabilities, never define behaviour, so an unapproved parent profile
can enable nothing that isn't itself individually trusted. This is already
implied by §7 ("unreviewed hooks / skills / guidance cannot activate") and is
worth stating as the explicit security argument for why implicit layering is safe
here and is not safe in direnv.

### Reversibility and watches

```go
newEnv[DIRENV_WATCHES] = rc.times.Marshal()
defer func() {
	newEnv[DIRENV_DIR]  = "-" + filepath.Dir(rc.path)
	newEnv[DIRENV_FILE] = rc.path
	newEnv[DIRENV_DIFF] = previousEnv.Diff(newEnv).Serialize()
}()
```

Three things, all set in a `defer` so they are **recorded even when the load is
disallowed or fails**:

- `DIRENV_DIFF` — the reversible patch (this is where mise's `__MISE_DIFF` comes
  from).
- `DIRENV_WATCHES` — the file→mtime map, extendable at runtime by `watch_file`
  in the `.envrc`, so a `.envrc` that reads `package.json` can declare that
  dependency and get reloaded when it changes.
- `DIRENV_DIR` / `DIRENV_FILE` — *which* file produced the current state.

The "recorded even on failure" detail matters: after a blocked load, the shell
still knows it left the previous directory and still un-applies the previous
environment. AIKit's `ActivationEffect` reporting needs the same property — a
failed projection must still leave the client in a *known* state, and §15.6
("a failed projection leaves the previous generation active") is the stronger
version of this.

The `.envrc` is executed by `bash -c 'eval "$(direnv stdlib)" && __main__
source_env <path>'`, output captured as JSON, with `set -euo pipefail` prepended
when `strict_env` is on and the child cancellable via SIGINT. The *evaluator is
not the shell you are sitting in* — direnv never sources anything into the
interactive shell; it computes a diff in a subprocess and emits export
statements. AIKit's shell projection should hold the same line.

### Assessment

| Question | Answer |
|---|---|
| Scope | Per-directory. Context = nearest ancestor directory containing `.envrc`. Explicitly **not** hierarchical; composition is opt-in via `source_up`. |
| Generations | None. But `DIRENV_DIFF` is a genuine inverse-patch mechanism, which is generation-*like* in that leaving a directory exactly restores the prior state. No history beyond one step, no naming, no rollback-to-arbitrary-point. |
| Trust | `SHA256(path + contents)` allow-list, path-only deny-list, symlink-hardened prefix whitelists, mandatory re-approval on every edit. The strictest and simplest model surveyed. |
| Explanation | `direnv status` prints the loaded RC, its allow path, allow status, and the found config. That is roughly the level of explanation AIKit's `explain` needs to beat comprehensively. |
| Contention | Per-shell by construction (state lives in each shell's environment), so two shells in different directories genuinely differ. Two shells in the *same* directory are identical. There is no notion of "this shell wants a variant". |

---

## devenv, devbox, flox

All three wrap Nix to give per-project declarative environments. Their relevance
to AIKit is narrower, but flox has one mechanism worth the trip.

**devbox** — `devbox.json` (declared packages, env, scripts) + `devbox.lock`
(resolved versions, pinned to a nixpkgs commit per package). `devbox shell`
enters a subshell with the resolved packages on `$PATH`; `devbox run <script>`
executes without an interactive shell; `devbox generate direnv` emits an `.envrc`
so the environment activates on `cd`. The two-file declare/lock split is the
convention AIKit already follows (`profile.toml` → `resolution.lock.toml`).

**devenv** — `devenv.nix` + `devenv.lock` + `devenv.yaml` (inputs). Adds
first-class `processes`, `services` and `tasks` with dependency ordering, plus
`git-hooks.nix` integration. Its interesting contribution is that it makes the
*module system* (options, `mkDefault`/`mkForce`, `imports`) available per-project,
so composition and override priority work the same way as NixOS. `devenv` also
ships `devenv test` — the environment definition is testable.

**flox** — `.flox/env/manifest.toml` + `manifest.lock` in the project, with three
mechanisms that matter here:

1. **Generations, but for a *project* environment.** `flox generations list`,
   `flox generations switch <n>`, `flox generations rollback`, `flox generations
   history`. Notably, **rollback does not create a new generation but does append
   a history entry** — a clean separation of *state* from *audit log*. AIKit
   should copy this exactly: `previous`/`current` pointer moves are state; every
   pointer move is an event in `logs/events.jsonl` and in the SQLite event table.
2. **Composition via `[include]`.** A manifest may include other environments;
   they are merged into a single **merged manifest**, which is then locked. The
   documented semantics: *manifests are merged (not lockfiles — "as opposed to
   building the environments and merging their lockfiles"); later entries in
   `include.environments` beat earlier ones; the composing environment's own
   manifest is applied last and therefore wins; the `include.environments` array
   itself is stripped during the merge; `install` is a union of package
   descriptors; **and when one manifest overrides another, a warning is
   displayed***. `flox include upgrade` re-pulls included environments explicitly.

   The two takeaways: (a) merge **declarations**, then resolve once — do not
   resolve each layer and merge results; AIKit's §4 rule 3 ("dependencies are
   expanded after explicit selection") is the same insight. (b) **A silent
   override is a bug; every override emits a warning.** AIKit's `explain` should
   have a "shadowed" concept surfaced proactively in the palette, not only on
   request.
3. **Layering** — `flox activate` inside an already-activated environment stacks
   as a subshell rather than merging. Composition (declarative, merged, locked)
   and layering (imperative, stacked, runtime) are **named differently and behave
   differently**, and flox's docs are explicit that these are distinct. AIKit's
   scope chain is composition; its session/task overlays are closer to layering.
   Naming them distinctly in the UI is a lesson worth taking.

| Question | devbox | devenv | flox |
|---|---|---|---|
| Scope | per-project dir | per-project dir | per-project dir, plus remote/FloxHub environments consumable by many projects |
| Generations | no | no | **yes** — numbered, switchable, rollback, separate history log |
| Trust | none | none | none beyond Nix's |
| Explanation | `devbox info`, lock inspection | `devenv info` | merged-manifest display + **override warnings** |
| Contention | subshell isolation | subshell isolation | layering as subshells; composition conflicts are resolved by documented precedence + warning |

---

# Tier 3 — agent capability systems

This is where the field is weakest and where AIKit's thesis lives.

## Agent Skills (Anthropic, Codex, agentskills.io)

### The on-disk contract

A skill is a **directory** whose name matches the `name` frontmatter field,
containing a required `SKILL.md`:

```yaml
---
name: pdf-processing            # ≤64 chars, [a-z0-9-], no leading/trailing hyphen,
                                # must match the parent folder name,
                                # may not contain "anthropic" or "claude"
description: Extract text ...   # ≤1024 chars, must say WHAT and WHEN
---
```

Only `name` and `description` are required; the spec says conforming runtimes
**ignore unrecognised frontmatter keys**. Optional conventional fields include
`version`, `license`, `allowed-tools`, and host-specific `metadata`. Optional
subdirectories by convention: `scripts/`, `references/`, `assets/`.

### The discovery algorithm

**Claude Code** scans, in order: built-in skills, plugin-provided skills, project
`.claude/skills/`, personal `~/.claude/skills/`. Plugins can contribute skills as
a bundled component.

**Codex** scans four scopes, per OpenAI's documentation: *"Codex reads skills from
repository, user, admin, and system locations. For repositories, Codex scans
`.agents/skills` in every directory from your current working directory up to the
repository root."*

| Scope | Location |
|---|---|
| Repository | `.agents/skills` in **every directory from cwd up to repo root** |
| User | `$HOME/.agents/skills` |
| Admin | `/etc/codex/skills` |
| System | bundled with Codex |

Codex additionally supports a sidecar `agents/openai.yaml` for host-specific
metadata: `interface.display_name`, `interface.short_description`,
`interface.icon_small`/`icon_large`, `interface.brand_color`,
`policy.allow_implicit_invocation`, `dependencies.tools`.

Two contract details worth noting because AIKit must interoperate:

- **Codex uses the same upward directory walk that mise/asdf use**, which means
  Codex already has a notion of nested project scope for skills. Claude Code does
  not — it is project-root or personal.
- **Codex does not merge same-named skills**: *"Codex doesn't merge them; both
  can appear in skill selectors."* Claude Code resolves same-named skills by
  precedence. AIKit's rule 5 (export-name collisions fail visibly) is stricter
  than either, and correctly so — but the projection layer must be aware that the
  two hosts behave differently when a collision reaches them.

### Progressive disclosure

Three levels, and this is the entire capability-routing model in the ecosystem:

| Level | When loaded | Cost | Content |
|---|---|---|---|
| 1 — metadata | always, at startup | ~100 tokens/skill (measured range ~55–235) | `name` + `description` in the system prompt |
| 2 — instructions | when the model decides the skill is relevant | target <5k tokens | `SKILL.md` body, read via bash |
| 3 — resources | as needed | 0 until read | bundled files; scripts run via bash, only stdout enters context |

The routing decision at level 1→2 is made by **the model, matching the request
against `description` strings**. There is no deterministic resolver, no
precedence, no policy check, no per-session scoping. "Which skills are available"
is a filesystem scan; "which skill is used" is an LLM judgement over a flat list.

### Trust

Anthropic's documentation is explicit and unambiguous that there is **no
mechanism**, only advice:

> Use Skills only from trusted sources... a malicious Skill can direct Claude to
> invoke tools or execute code in ways that don't match the Skill's stated
> purpose. ... **Treat like installing software.**

with a checklist of manual audit steps (review all bundled files; beware skills
that fetch external URLs, because "fetched content may contain malicious
instructions"). There is no signing, no content-hash approval database, no
quarantine, no review state, no provenance record. Codex's only control is
disabling a skill by name in `~/.codex/config.toml`.

**A skill dropped into `.claude/skills/` by `git pull` is live on the next
session start.** That is the single largest gap in the ecosystem and the clearest
justification for AIKit's §7.

### Assessment

| Question | Answer |
|---|---|
| Scope | Filesystem scan of a fixed set of directories. Claude Code: personal + project-root + plugins. Codex: nested walk to repo root + user + admin + system. **No session scope, no task scope, no host scope, no runtime scope change.** Changing what is available means editing the filesystem, which changes it for everything sharing that filesystem. |
| Generations | None. No snapshot, no hash, no atomic switch, no rollback, no lock file. |
| Trust | **None.** Documentation-level advice only. |
| Explanation | None. There is no way to ask "why is this skill available" or "which of the four scopes did this come from". Codex's non-merging of duplicates at least makes shadowing visible in the selector. |
| Contention | Undefined for Claude Code (last-writer-wins on the filesystem). Codex shows both and lets the selector disambiguate. Two agent sessions in one checkout **cannot** have different skill sets. |

## Block Goose

Goose is the closest thing in the agent world to a declarative capability
manifest, and its **recipe** concept is genuinely good prior art.

**Extensions** are Goose's capability unit — mostly MCP servers, configured in
`~/.config/goose/config.yaml` under `extensions:`, keyed by id, with
`type` (`builtin` | `stdio` | `sse` | `streamable_http` | `platform` |
`frontend` | `inline_python`), `enabled`, `name`, `timeout`, `env_keys`, and
`bundled`. Secrets named in `env_keys` are resolved from the **system keyring**,
prompting on first use rather than living in the config file. This is a good
pattern and directly relevant to AIKit's secret handling.

**Recipes** are a YAML document bundling `title`, `description`, `version`,
`instructions`/`prompt`, `parameters` (with `{{ }}` substitution and typed
inputs), `extensions`, `settings` (model/provider), `retry` (with success
validation), `response` (JSON output schema), `sub_recipes`, and `activities`.

The important semantic, and the one thing in Tier 3 that meaningfully anticipates
AIKit:

> When a recipe explicitly defines an `extensions` block, **only those extensions
> load** — the system does not automatically include your default configuration.

That is a **per-invocation capability set that replaces rather than merges with
global state**. It is the single closest thing in the agent ecosystem to a
context-scoped capability view. The design note in Goose's own materials is
telling: *"The agent doesn't decide which tools to load — the recipe does."*

Limits: the scope is the *recipe invocation*, not a session, project, host, or
directory. There is no layering — it is replace-all, so there is no precedence
question and no `explain` need. Sub-recipes get their own extension sets, which
is closer to AIKit's task overlays, but again by replacement.

**Trust.** Recipe distribution is the weak point: with `GOOSE_RECIPE_GITHUB_REPO`
set, a recipe not found locally is **downloaded from GitHub and run**, with no
checksum or approval step. The enterprise control is separate and coarse: a
`GOOSE_ALLOWLIST` environment variable pointing at a YAML file (ideally over
HTTPS from an internal host) listing permitted extensions by `id` and `command`;
installation of anything unlisted is blocked. See
[`crates/goose-server/ALLOWLIST.md`](https://github.com/block/goose/blob/main/crates/goose-server/ALLOWLIST.md).
That is an **admin denylist/allowlist**, structurally similar to AIKit's "managed
policy constraints" layer — and note that Goose, like AIKit, places it *outside*
the ordinary precedence chain.

| Question | Answer |
|---|---|
| Scope | Global `config.yaml` for interactive use; **per-recipe replacement** for recipe runs; sub-recipes get their own sets. No project or directory scope. |
| Generations | None. |
| Trust | Admin allowlist by extension id + command, via `GOOSE_ALLOWLIST`. Keyring-backed secrets via `env_keys`. **No trust model for recipes themselves**, which are fetchable from GitHub and executed. |
| Explanation | The recipe file *is* the explanation, because it is a total specification. This is real, but it works only because there is no layering to explain. |
| Contention | Not applicable — replacement, not merge. |

## MCP and Claude Code MCP scopes

The MCP specification itself says essentially nothing about discovery, scoping,
or trust — it is a client/server wire protocol. Two protocol features are
relevant:

- **Roots** (`roots/list`, `notifications/roots/list_changed`): the *client* tells
  the *server* which filesystem roots are in play. Claude Code answers with the
  session's launch directory plus every `--add-dir`/`/add-dir`/`additionalDirectories`
  entry, and pushes `list_changed` when that set changes. This is the protocol's
  only per-session scoping primitive, and it constrains a server's *reach*, not
  its *availability*.
- **Sessions** exist in the transport (Streamable HTTP `Mcp-Session-Id`), but
  carry no capability semantics.

Everything else is host policy. Claude Code's is the most developed:

| Scope | Loads in | Shared | Stored in |
|---|---|---|---|
| **Local** (default) | current project only | no | `~/.claude.json`, under that project's path key |
| **Project** | current project only | yes, via VCS | `.mcp.json` in project root |
| **User** | all your projects | no | `~/.claude.json` |

Precedence when a name appears more than once: **Local > Project > User >
plugin-provided > claude.ai connectors**, and — importantly —
*"The entire server entry from that source is used; fields are not merged across
scopes."* Whole-record replacement, not field merge. This matches flox's `[tasks]`
and mise's `[tasks]` semantics: a *definition* is replaced wholesale, only a *set*
is merged.

**The trust model is the most sophisticated in Tier 3**, and it is worth reading
closely because it is the same problem AIKit's §7 solves:

- Project-scoped servers from `.mcp.json` are **pending approval** until the user
  approves them interactively. `claude mcp list` shows `⏸ Pending approval (run
  claude to approve)`; rejected servers show `✘ Rejected (see
  disabledMcpjsonServers in settings)`.
- `claude mcp reset-project-choices` clears the approvals for re-review.
- **A repository cannot approve its own servers.** `enableAllProjectMcpServers`
  or `enabledMcpjsonServers` committed into `.claude/settings.json` is *ignored*
  in an untrusted folder. Approvals are only honoured from settings files that
  are not checked into the repo, and only after the workspace trust dialog is
  accepted — Claude Code literally runs `git` to check whether
  `.claude/settings.local.json` is tracked, and runs that check only in an
  already-trusted folder.
- `disabledMcpjsonServers` in **any** settings file rejects a server — denial is
  unconditional and non-overridable, in the same asymmetric way as direnv's deny
  list and AIKit's managed denials.

This is a direct, independent confirmation of AIKit's §7 principle that *"a
manifest may not declare its own trust"* (`manifest.trust_not_self_declarable`).
It is the right rule and someone else has already shipped it.

Related host mechanisms: tools are namespaced `mcp__<server>__<tool>` (plugin
servers register as `plugin:<plugin>:<server>`), and that full name is the
addressing unit for permission rules, a skill's `allowed-tools`, a subagent's
`tools` field, and hook matchers. **The permission surface is tool-granular, not
server-granular**, which AIKit's projections must preserve.

| Question | Answer |
|---|---|
| Scope | Three named scopes plus plugins and connectors, resolved by whole-record precedence. Local and Project are **per-project**; nothing is per-session or per-task. Two Claude Code sessions in one repo get identical MCP servers. |
| Generations | None. Config is read at startup; `/mcp` reconnects; no snapshot or rollback. |
| Trust | Interactive approval for project-scoped servers, keyed on **workspace trust + server name** (not content hash — editing `.mcp.json` does not revoke). Repo-committed settings cannot self-approve. `disabledMcpjsonServers` is an unconditional block. Managed/enterprise configuration is a separate, higher layer. |
| Explanation | `claude mcp get <name>` shows the source scope and status; `/mcp` shows connection state and tool counts. Better than the skills story, still not a resolver explanation. |
| Contention | Undefined — one config per project. Two agents wanting different servers in one repo is unrepresentable. |

## Registries: official MCP registry, Smithery

**The official MCP registry** (`registry.modelcontextprotocol.io`,
[modelcontextprotocol/registry](https://github.com/modelcontextprotocol/registry))
is a *metadata* registry: it hosts `server.json` records, not code. Its
contribution is **namespace authentication**:

- Names are reverse-DNS: `io.github.<user>/<server>`, `com.example/<server>`.
- GitHub OAuth/OIDC grants `io.github.<user>/*` and `io.github.<org>/*`.
- **DNS verification** grants `com.domain/*` **and all subdomains**
  `com.domain.*/*`.
- **HTTP verification** grants `com.domain/*` **only** — exact domain, no
  subdomains.

The DNS-vs-HTTP asymmetry is a deliberate, well-reasoned capability grant: proving
control of DNS proves control of the subdomain space; proving control of one HTTP
endpoint does not. AIKit's registry identity should encode the same distinction if
it ever accepts third-party registries — **the strength of the identity proof
should determine the breadth of the namespace it can claim.**

`server.json` records carry version, repository URL, transport, environment
variables, headers and secret placeholders. The registry is explicitly designed
to be consumed by **subregistries** (Smithery, PulseMCP, Docker Hub, GitHub,
Anthropic) which layer their own curation. That federated shape — one canonical
identity/namespace layer, many opinionated curation layers — is a good model for
AIKit's `registries/<name>/` directory: identity is registry-scoped, curation and
trust are local.

**Smithery** adds scanning and scoring on top: partnership with Invariant Labs for
continuous vulnerability/prompt-injection scanning of listed servers, verified
repositories, and an embeddable trust badge. Comparable efforts (mpak's L1–L4
trust tiers over 25 supply-chain controls; MCP Skills' pre-install trust layer)
are converging on the same shape: **a numeric or tiered trust score attached to a
(server, version) pair, computed by a third party.**

The gap common to all of them: **the score is a property of the artefact in the
registry, not of the decision to activate it on this machine, in this context.**
Nobody records "I, the user, reviewed revision X and permitted it here." That is
exactly AIKit's trust ledger, and none of these systems has one.

| Question | Answer |
|---|---|
| Scope | Registry-global. No context concept at all. |
| Generations | Version records per server; no local generation. |
| Trust | Namespace ownership proof (GitHub OIDC / DNS / HTTP, with breadth proportional to proof strength) + third-party scanning and scoring. **No local review ledger.** |
| Explanation | Provenance to a verified namespace; scan findings. |
| Contention | N/A. |

## Continue.dev, aider, open-interpreter

**Continue.dev** has the most developed tool-policy model of the three, and it is
worth noting because it is *permission* scoping rather than *availability*
scoping.

Three tiers: `allow` (run silently), `ask` (prompt; TUI only), `exclude`
(**hidden from the agent entirely** — the tool is not in the model's tool list).
`exclude` is the important one: it is availability, not just permission.

Defaults: read-only tools (`Read`, `List`, `Search`, `Fetch`, `Diff`,
`AskQuestion`, `Checklist`, `Status`, `CheckBackgroundJob`, `ReportFailure`,
`UploadArtifact`) default to `allow`; writes (`Edit`, `MultiEdit`, `Write`) and
`Bash` default to `ask`, and **switch to `exclude` in headless mode** — a
non-interactive context downgrades ask-tier tools to unavailable rather than
auto-approving them. That is the correct default and AIKit's `requires_run_confirmation`
needs the same rule for non-interactive contexts.

Precedence: mode policies (`--auto`, `--readonly`) > CLI flags (`--allow`,
`--ask`, `--exclude`) > `~/.continue/permissions.yaml` > built-in defaults. Rules
support argument globbing: `Write(**/*.ts)`, `Bash(npm install*)`. Modes
**completely override** individual flags rather than merging.

The gap: `permissions.yaml` is `~/.continue/permissions.yaml` — **user-global**.
Continue's rules and blocks (`.continue/rules/*.md`, with `globs:` frontmatter)
*are* project-scoped and even file-glob-scoped, but that governs **prompt
content**, not tool availability. So Continue scopes *what the model is told*
per-file, and *what the model may do* per-session-global. Those are the wrong way
round for AIKit's purposes, and it is a clean illustration of the gap.

**aider** has no tool-scoping model at all. Its context mechanisms are the
tree-sitter repo map (`--map-tokens`, a PageRank-weighted selection of the most
relevant symbol signatures), the chat file set (`/add`, `/drop`, `/read-only`,
`--read`), and `CONVENTIONS.md` conventionally loaded as a read-only file.
Configuration comes from `.aider.conf.yml` discovered in the home directory, the
git repo root, and the cwd, with later overriding earlier — a per-directory
config chain, but for *settings*, not capabilities. aider's capability surface is
fixed at compile time.

**open-interpreter** has **profiles** (`~/.config/open-interpreter/profiles/*.yaml`,
selected with `--profile`) that set model, system message, `auto_run`,
`custom_instructions` and loop settings. A profile is a named preset, selected
explicitly at launch — closest to Goose recipes, with no directory binding, no
layering and no trust model. `auto_run` (`-y`) is a global on/off for
confirmation, with no per-tool or per-argument granularity.

| System | Scope of tool availability | Generations | Trust | Explain |
|---|---|---|---|---|
| Continue.dev | user-global permissions; project-scoped *rules* (prompt content) with glob targeting | none | none | policy source is inspectable, precedence documented |
| aider | fixed | none | none | `--show-repo-map`, `/tokens` |
| open-interpreter | per named profile, chosen at launch | none | `auto_run` global switch only | none |

---

# Tier 4 — capture and search

## atuin

Sources: [`crates/atuin-client/src/settings.rs`](https://github.com/atuinsh/atuin/blob/main/crates/atuin-client/src/settings.rs),
[`crates/atuin-client/src/secrets.rs`](https://github.com/atuinsh/atuin/blob/main/crates/atuin-client/src/secrets.rs).

### Capture

`preexec`/`precmd` shell hooks capture, per command: the command text, cwd,
hostname, **session id**, duration, and exit code, into a local SQLite database.
The session id is generated per shell instance, which is what makes session-scoped
search possible.

### Filter modes — the model AIKit should generalise

```rust
pub enum FilterMode {
    Global = 0,          // everything
    Host = 1,            // this machine
    Session = 2,         // this shell session
    Directory = 3,       // this directory
    Workspace = 4,       // this git repo (requires `workspaces = true`)
    SessionPreload = 5,  // session + preloaded history
}
```

`Ctrl-R` **cycles the filter mode**, and the current mode is rendered in the UI as
`GLOBAL` / `HOST` / `SESSION` / `DIRECTORY` / `WORKSPACE` / `SESSION+`.

The default is computed, not fixed:

```rust
pub fn default_filter_mode(&self, git_root: bool) -> FilterMode {
    self.filter_mode
        .filter(|x| self.search.filters.contains(x))
        .or_else(|| self.search.filters.iter()
            .find(|x| match (x, git_root, self.workspaces) {
                (FilterMode::Workspace, true, true) => true,
                (FilterMode::Workspace, _, _) => false,
                (_, _, _) => true,
            }).copied())
        .unwrap_or(FilterMode::Global)
}
```

If `workspaces` is enabled *and* you are inside a git repo, `Workspace` becomes
the default; otherwise it is skipped and the next configured filter wins. **The
scope defaults to the narrowest context that actually exists.** AIKit's palette
should behave identically: default the search scope to the narrowest live scope
(task → session → project → host → user) and let one keystroke widen it, with the
active scope always rendered.

Search modes are an orthogonal axis, also cycled:

```rust
pub enum SearchMode { Prefix, FullText, Fuzzy, Skim, DaemonFuzzy }
```

with `SearchMode::next()` implementing a cycle whose third position depends on the
user's *configured* mode — cycling from `FullText` goes to `Skim` if you
configured Skim, `DaemonFuzzy` if you configured that, otherwise `Fuzzy`. Small
detail, good UX principle: **the cycle returns you to your own preference, not to
a canonical one.**

### What is worth keeping

atuin's answer is a set of explicit, inspectable filters rather than a heuristic:

| Setting | Effect |
|---|---|
| `store_failed` (default `true`) | keep commands with non-zero exit |
| `history_filter` | `RegexSet` — commands matching are **not stored** |
| `cwd_filter` | `RegexSet` — commands run in matching directories are **not stored** |
| `secrets_filter` (default `true`) | commands matching known secret patterns are not stored |

`secrets.rs` is a hand-maintained `SECRET_PATTERNS: &[(&str, &str, TestValue)]`
table — `(name, regex, test value)` — covering AWS keys and env-var names, Azure,
GCP, GitHub PATs old and new, GitHub OAuth tokens, `atuin login` itself, and
more. **Every pattern ships with a test value that must match**, so the table is
self-testing and cannot silently rot. The file opens with `// This file will
probably trigger a lot of scanners. Sorry.`

This is directly reusable for AIKit's §15.10 acceptance case ("a captured secret
never enters the ordinary registry"): a named, regex-based, self-tested denylist
applied at **capture** time, defaulting to on, with the pattern *name* available
so a rejection can be explained ("not captured: matches `GitHub PAT (new)`")
rather than silently dropped.

Note also the negative space: atuin does **not** try to score "importance". It
keeps almost everything and filters by explicit rule and by scope. Curation is a
search-time concern, not a capture-time one. AIKit's §14 "no automatic promotion
from usage count" is the same instinct and is validated here.

### Sync

Optional, end-to-end encrypted client-side before upload, so the server stores
ciphertext. The newer record-sync (v2) is an append-only, per-host record store
with a Merkle-ish index designed to sync *multiple* record types (history, kv,
aliases) rather than history alone — i.e. atuin generalised from "sync history"
to "sync an append-only log of typed records" once it needed a second data type.
Worth knowing before AIKit's event log grows a sync story.

| Question | Answer |
|---|---|
| Scope | **Per-context at query time, global at storage time.** One database; context (session id, cwd, hostname, git root) is captured as *columns* and used as *filters*. Context is identified by shell session id, cwd, hostname, and git repo root. |
| Generations | None; append-only history with optional deletion (`atuin search --delete`). |
| Trust | E2E encryption for sync; capture-time secret filtering. No trust model for what is executed. |
| Explanation | The active filter mode and search mode are always rendered in the UI. That *is* the explanation, and it is enough because the model is simple. |
| Contention | None — read-only queries over shared data. |

## fzf, nucleo, skim

Sources: [`fzf/src/algo/algo.go`](https://github.com/junegunn/fzf/blob/master/src/algo/algo.go),
[helix-editor/nucleo](https://github.com/helix-editor/nucleo).

**fzf's `FuzzyMatchV2`** is a modified Smith–Waterman with affine gaps, O(nm),
with a documented fallback to the greedy `FuzzyMatchV1` when the input is too
large. The scoring constants are the actual design:

```go
scoreMatch        = 16
scoreGapStart     = -3
scoreGapExtension = -1

bonusBoundary            = scoreMatch / 2                     // 8
bonusNonWord             = scoreMatch / 2                     // 8
bonusCamel123            = bonusBoundary + scoreGapExtension  // 7
bonusConsecutive         = -(scoreGapStart + scoreGapExtension) // 4
bonusFirstCharMultiplier = 2

bonusBoundaryWhite     = bonusBoundary + 2   // after whitespace / start of string
bonusBoundaryDelimiter = bonusBoundary + 1   // after / : ; ,
```

Three things to note:

1. **`bonusConsecutive` is defined as exactly the gap penalty it cancels.** The
   constants are expressed as relationships, not magic numbers, so the scoring
   system stays coherent when one is tuned.
2. **First-character bonus is doubled** (`scoreMatch + bonus*bonusFirstCharMultiplier`),
   which is why prefix matches dominate.
3. **`bonusBoundaryDelimiter` treats `/ : ; ,` as boundaries**, and the delimiter
   set is scheme-adjusted (`--scheme=path` weights `/` more, `--scheme=history`
   flattens boundary bonuses). **The ranking has a domain scheme.** For AIKit,
   capability ids look like `group/name` and export names look like paths;
   `--scheme=path`-style weighting is the right default, and a distinct scheme for
   history/recent-commands search is worth having.

Beyond score, fzf applies configurable **tiebreaks** in order: `length` (shorter
wins), `chunk`, `begin` (earlier match position), `end`, `index` (input order,
i.e. stable). `--tiebreak=begin,length` and friends compose. AIKit should
similarly separate *score* from *tiebreak* so that "usage recency" can be a
tiebreak (which is defensible) rather than a score component (which makes ranking
unstable and unexplainable).

**nucleo** (Rust, by the Helix editor authors) uses **the same scoring system as
fzf** but implements Smith–Waterman with **two matrices instead of fzf's one**,
which finds the optimal alignment more often. The canonical example from its
README: matching `foo` against `xf foo`, nucleo selects `x__foo` while fzf selects
`xf_oo`; the former is more intuitive and scores higher. nucleo also provides
multi-threaded matching, incremental streaming of items, and a high-level `Nucleo`
type with a worker pool — designed exactly for a TUI that re-queries on every
keystroke.

**skim** uses a different, weaker bonus system (per nucleo's comparison: the fzf/
nucleo bonus is *stateful* — adjacent matches inherit the previous character's
bonus if it is larger — while skim's is not), and is generally slower.

For AIKit's <16 ms per-keystroke budget with in-process fuzzy matching in Rust,
**`nucleo-matcher` is the correct dependency**: same scoring semantics as the tool
everyone's fingers already know, better optimality, native Rust, no subprocess.

---

# Cross-cutting comparison

| System | 1. Scope | 2. Generations | 3. Trust | 4. Explain | 5. Contention |
|---|---|---|---|---|---|
| **Nix / NixOS** | global per profile | ✅ integer chain, dedup, atomic `rename`, rollback, GC policy | ❌ | partial (`why-depends`, `nvd`, module priorities *on conflict*) | serialised by lock + symlink CAS; unrepresentable |
| **home-manager** | global per user | ✅ (inherits Nix) | ❌ | ❌ beyond Nix | unrepresentable |
| **Guix** | global per profile | ✅ + **self-describing versioned manifest**, **synthesised empty gen** | ❌ (but `guix challenge` for build provenance) | ✅ best-in-family: `parent` chains, `--export-manifest`, `time-machine` | unrepresentable |
| **chezmoi** | one target (`$HOME`), varied by template | ❌ | ⚠️ prompts only | ✅ `source-path`, `data`, `execute-template` (provenance by inversion) | unmodelled race |
| **mise** | per-directory, hierarchical merge | ❌ (inverse patch only) | ✅ richest: path/content keyed, 3-state, monorepo + worktree inheritance, `safe` mode | ⚠️ `config ls`, `doctor`, per-question | independent per shell; no per-shell variants |
| **asdf** | per-directory, per-tool first-match | ❌ | ❌ | ⚠️ `asdf current` shows source file | independent per shell |
| **direnv** | per-directory, **nearest only** | ❌ (one-step inverse patch) | ✅ `SHA256(path+contents)` allow, path-only deny, prefix whitelist | ⚠️ `direnv status` | independent per shell |
| **devbox / devenv** | per-project | ❌ | ❌ | ⚠️ lock inspection | subshell isolation |
| **flox** | per-project + remote envs | ✅ numbered, switch, rollback, **separate history log** | ❌ | ✅ merged manifest + **override warnings** | composition precedence + warning; layering via subshells |
| **Agent Skills** | fixed directory scan | ❌ | ❌ **none** | ❌ | unrepresentable |
| **Goose** | global config, **per-recipe replacement** | ❌ | ⚠️ admin allowlist by id+command; none for recipes | ✅ trivially (recipe is total) | N/A (replacement) |
| **Claude Code MCP** | 3 named scopes, whole-record precedence | ❌ | ✅ approval + **repo can't self-approve** + unconditional deny | ⚠️ `mcp get` shows source scope | unrepresentable |
| **MCP registry** | global | version records | ✅ namespace proof scaled to proof strength | ⚠️ provenance | N/A |
| **atuin** | **captured as columns, filtered at query** | ❌ | ⚠️ capture-time secret denylist; E2E for sync | ✅ mode always rendered | N/A |
| **fzf / nucleo** | N/A | N/A | N/A | ⚠️ score is opaque; tiebreaks are declared | N/A |

---

# What none of them do

Seven genuine gaps. Each is a thing AIKit would be the first to do, and each is
load-bearing for the product thesis.

### 1. Nobody has generations *per context*

This is the central gap and the whole reason to build AIKit.

Nix, Guix and flox have excellent generation machinery bolted to a **single
mutable pointer** — one system profile, one user profile, one project
environment. mise, asdf and direnv have excellent **per-context resolution** with
no generation, no snapshot, no rollback, and no materialised artefact. Nobody has
put the two together.

The consequence is that in every existing system, "give this session a different
capability set" is either impossible (Nix family) or is achieved by leaving the
managed model entirely (`nix-shell`, `guix shell`, `flox activate` layering) into
an unmanaged, unexplainable, un-rollbackable subshell. AIKit's
`state/contexts/<ctx>/current` — *a generation chain per context* — has no
precedent. Acceptance cases §15.1 and §15.2 (two tmux sessions, two cmux
workspaces, same project, different capability sets, neither mutating the other)
are not achievable in any system surveyed.

### 2. No agent capability system has any trust model at all

This bears repeating in the harshest terms available. Anthropic's own
documentation for Agent Skills says to treat a skill like installing software and
then provides **no mechanism whatsoever** — no signing, no hash pinning, no
review state, no quarantine, no provenance record, no diff-on-update. A skill
that arrives via `git pull` into `.claude/skills/` is live at the next session
start. Codex is the same, minus a per-name disable switch in `config.toml`.
Goose will download a recipe from a configured GitHub repo and run it.

The one exception in the agent world is **Claude Code's MCP project-scope
approval**, which independently arrived at AIKit's core rule (a repo cannot
approve itself) — but it is name-keyed, not content-keyed, so editing an approved
`.mcp.json` server entry does not revoke the approval.

Meanwhile the *registries* (official MCP registry, Smithery, mpak) have built
namespace proofs and third-party scanning, but the score is a property of the
artefact in the registry — **nobody records the local decision**: "I reviewed
revision X of capsule Y from source Z and permitted it, here, at this time."
There is no trust *ledger* anywhere in the ecosystem.

### 3. Nobody separates *available*, *enabled* and *loaded*

Every system surveyed collapses at least two of these three. In Agent Skills,
present-on-disk *is* available *is* eligible-for-loading, and whether it was
actually loaded is invisible. In MCP, configured is connected (or silently
failed). In mise, declared is installed is on-`$PATH`.

AIKit's vocabulary table and its `UnavailableReason` enum
(`NotInCatalog`, `DeniedByPolicy`, `PlatformUnsupported`, `NoSupportedTarget`,
`TrustRequired`, `Quarantined`, `Blocked`, `DependencyUnavailable`) with
"declared but unavailable is not an error, it is a different rendering" is
without precedent. The closest analogue is Claude Code's `⏸ Pending approval` /
`✘ Rejected` / connected states for MCP servers — three states where everyone
else has one.

### 4. Nobody explains the success case

NixOS explains conflicts beautifully (both files named, priorities compared) and
explains success poorly. Guix's `manifest-entry-parent` is the best success-case
provenance in the survey and is not surfaced in a first-class command. mise has
`config ls` and `doctor` but no single "why is this the answer" verb. chezmoi's
`source-path` is genuinely excellent but works by inversion (output → input) and
covers only file provenance, not selection logic.

**No system has a first-class `explain <thing>` that returns the full layer
chain, the winning layer, the shadowed layers, the dependency chain that pulled
it in, the policy checks it passed, and the trust decision that permitted it.**
AIKit's rule 7 is, as far as this survey goes, novel as a *design commitment*
rather than a diagnostic afterthought.

### 5. Nobody models "two contexts want different things" as a normal condition

In the Nix family, contention is a *lock*: the second writer waits and then wins.
In the per-directory family, contention is *impossible* because state is
per-process and unmanaged. In the agent family, contention is *undefined*.

Nobody treats concurrent divergent demand as a first-class, expected, correct
state. AIKit's per-context generation chain with `base_generation` compare-and-swap
on the session overlay makes divergence the normal case and makes *convergence*
(promotion) the explicit act. That inversion is the product.

### 6. Nobody is honest about partial application

When a system cannot fully deliver what was asked for, the field has two
responses: fail, or lie. Continue.dev's headless downgrade (ask-tier tools become
`exclude` rather than auto-approve) is the only example found of a system that
*degrades deliberately and says so*. mise's `safe` mode is a close second
(untrusted config becomes inert data rather than being refused).

AIKit's `ActivationEffect` (`Immediate | LiveReloadExpected | RestartClient |
NextSessionOnly | Brokered | Unsupported`) and the §3 Codex shared-tree fallback
ladder — project-stable native → brokered → explicitly accepted shared, *with the
chosen rung reported* — has no equivalent. "Active in AIKit must never imply
already loaded by every client" is a genuinely novel honesty commitment.

### 7. Nobody has capture and curation in the same system as the router

atuin captures beautifully and routes nothing. Goose routes (per recipe) and
captures nothing. The path from "I ran this command and it worked" to "this is a
reviewed capsule in a registry, enabled for this project" does not exist anywhere.
AIKit's `inbox/{ready,quarantine,rejected}/` plus §15.11 ("promotion can be
completed without hand-writing a manifest") is the only design in the survey that
closes that loop, and it needs atuin's capture discipline (see below) to do it
safely.

---

# What we should steal

Ordered by value. Each item is specific enough to implement.

### 1. home-manager's `writeBoundary` — a sentinel node in the activation DAG

**What it is.** `home.activation` is a DAG, topologically sorted at build time,
containing a no-op node named `writeBoundary`. Every block with an observable
side effect must declare `entryAfter [ "writeBoundary" ]`. Blocks before the
boundary may verify and abort but may not mutate. A dependency cycle is a
build-time `abort`, not a runtime failure.

**Why.** AIKit's §8 hook chain already has phases (`gate → transform → verify →
inject → observe → capture`) ordered by `(phase, order, capsule id)`. Phases are
weaker than a DAG in one specific way: they cannot express "this capsule's
verifier must run after that capsule's verifier" without abusing the numeric
order field, and they cannot express the write boundary as a *node* that
third-party capsules attach to.

**How.** In `aikit-core::hooks`, keep phases as the coarse ordering but add
optional `after: Vec<HookRef>` / `before: Vec<HookRef>` edges within a phase,
topologically sorted at **generation build time** with a cycle producing
`hooks.dependency_cycle` and failing the build — so a cyclic hook chain can never
be written into a generation, let alone activated. Then apply the same structure
to *materialisation*: `aikit-store`'s generation builder gets an explicit
`WriteBoundary` between "validate the temp generation" and "rename into place",
and no validation step may touch anything outside the temp directory. The
architecture already says "build a temp generation → materialize → validate →
rename"; the boundary makes that a type-level property rather than a convention.

### 2. chezmoi's `System` interface — dry-run as a substituted implementation

**What it is.** All OS interaction goes through a `System` interface;
`DryRunSystem` and `DebugSystem` wrap `RealSystem`. `chezmoi diff` is `apply`
against a diff-emitting system.

**Why.** home-manager's `DRY_RUN` + `run` shell convention is the alternative, and
it fails the moment one module author forgets. AIKit promises a diff-first palette
("the palette shows the real consequence of a toggle per client"), which means the
preview and the apply *must* be the same code path or the preview will drift.

**How.** `aikit-store` defines

```rust
trait Materializer {
    fn write(&mut self, path: &Path, contents: &[u8], mode: u32) -> Result<()>;
    fn symlink(&mut self, target: &Path, link: &Path) -> Result<()>;
    fn remove(&mut self, path: &Path) -> Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> Result<()>;
    fn run(&mut self, cmd: &Command) -> Result<Output>;
}
```

with `RealFs`, `DryRun(Vec<Op>)`, and `Diff(against: &Generation)` implementations.
Then `aikit apply`, `aikit apply --dry-run`, `aikit diff`, and the palette's
toggle preview are one function parameterised by a `Materializer`. The
`ActivationEffect` a toggle would produce is computed by running the same
resolution and materialisation against `DryRun`, which means the palette cannot
promise something apply won't deliver.

### 3. direnv's trust key, with mise's state machine and Claude Code's self-approval ban

Three systems each got one third of this right. Combined:

**From direnv:** the trust key is `SHA256(identity || content)` where identity is
part of the hash, so an approval cannot be relocated. **Deny is keyed more
coarsely than allow** — direnv's deny is path-only, surviving edits, while allow
is content-keyed and dies on edit. Apply this to AIKit directly:

- `trusted` / `reviewed` are keyed on `(registry source, capsule id, content revision)` — already the design. Correct.
- `blocked` must be keyed on `(registry source, capsule id)` **without** the
  revision, or a version bump silently unblocks a capsule the user rejected.
  This is currently ambiguous in §7 and should be pinned down.

Also from direnv: the grant record stores the human-readable identity alongside
the hash key, which is what makes `direnv status` and `direnv prune` possible.
AIKit's trust table should carry `(key_hash, source, capsule_id, revision,
reviewed_at, reviewed_by, note)` so the ledger is auditable and prunable — and
so `aikit trust list` can show grants whose capsule no longer exists.

**From mise (`src/config/config_file/mod.rs`):**

- **Three states, not two.** `TRUSTED_CONFIGS` and `IGNORED_CONFIGS` are separate.
  Declining a prompt is *not* blocking; it suppresses re-prompting without
  becoming a permanent denial. AIKit's `unseen | quarantined | reviewed | trusted
  | blocked | superseded` covers this if `quarantined` is understood as
  "user declined, don't re-prompt" — but that is not what §7 currently says
  (`quarantined` reads as an intake state). **Add a distinct `dismissed` state**,
  or document `quarantined` as covering both.
- **Trust inherits across git worktrees.** `git::main_checkout_equivalent()` maps
  a path in a linked worktree to the same relative path in the main checkout and
  inherits its trust. Given `Isolation::Worktree`, AIKit **must** do this or every
  `--worktree` task re-prompts for every capsule. Note mise disables the
  inheritance in paranoid (content-keyed) mode because contents can differ per
  branch — for AIKit, whose trust key is `(source, capsule id, revision)` rather
  than a path, the problem does not arise: the same revision is the same grant
  regardless of which worktree observes it. Worth stating explicitly as a
  property of the key design.
- **`safe` mode.** Untrusted configuration is loaded *inert* — no code execution,
  no environment injection — rather than refused, and *"`safe` is global-only, so
  a project config cannot disable it for itself"*. AIKit's §7 already refuses to
  activate unreviewed hooks/skills/guidance; a `safe` mode generalises this so
  that an untrusted capsule still appears in the catalog, still shows its
  metadata, and is still searchable — it simply cannot project. That is a much
  better UX than invisibility, and it makes `TrustRequired` in `UnavailableReason`
  actionable from the palette.
- **Never prompt from a non-interactive entry point.** `trust_check()` explicitly
  excludes `cmd == "hook-env"`. AIKit's `aikit hook dispatch` must never prompt,
  and neither must any shell-integration path.

**From Claude Code MCP:** a repository cannot approve its own capabilities.
`enableAllProjectMcpServers` committed to `.claude/settings.json` is ignored in an
untrusted folder; approvals only count from files that are not tracked by git, and
that check only runs in an already-trusted folder. AIKit's
`manifest.trust_not_self_declarable` is the same rule at the capsule level; extend
it to the **profile** level: `<repo>/.aikit/profile.toml` (committed) may enable
capabilities but may never contribute trust, and `profile.local.toml` (ignored)
may — verified by asking git whether the file is tracked.

### 4. Guix's self-describing, version-stamped generation

**What it is.** Every Guix profile contains a `manifest` file in a versioned
serialisation (`%manifest-format-version`), and the *presence* of
`<dir>/manifest` is the test for "this is a profile generation"
(`generation-profile`). home-manager independently does the same with
`gen-version` ("allows us to make backwards incompatible changes in the package
output and have surrounding tooling adapt").

**How.** `generations/<hash>/metadata.json` carries `generation_format: 1` from
the first commit, and `aikit` refuses to activate a generation whose format it
does not understand — with a message naming the version it found and the versions
it supports, and offering a rebuild. Detection of "is this directory a
generation" is `metadata.json` present and parseable, never a name pattern. This
is cheap now and impossible to retrofit.

Also from Guix: `manifest-entry-properties` is explicitly excluded from
`manifest-entry=?`. AIKit's `resolution.lock.toml` entries should have a
`[properties]` table that does **not** participate in the resolution hash, for
timestamps, UI hints, and provenance annotations. Without it, every cosmetic
metadata change invalidates every generation.

### 5. Guix's synthesised empty generation

**What it is.** `link-to-empty-profile` builds and links an empty profile on
demand, so `roll-back` from generation 1 goes to a *real, materialised* empty
generation rather than erroring.

**How.** `aikit` can materialise an empty generation: valid `metadata.json`, empty
lock, empty `bin/`, `hooks/`, `guidance/`, `projections/`. Then:

- "roll back past the first generation" works;
- `aikit session reset` / "turn everything off here" is the *same code path* as
  any other apply, not a special case;
- the "no capabilities" state is testable and materialised rather than being the
  absence of a directory;
- `AIKIT_VIEW` is always valid, which removes a whole class of "does `current`
  exist yet" branches from every consumer.

### 6. atuin's filter-mode model, defaulted to the narrowest live scope

**What it is.** `FilterMode { Global, Host, Session, Directory, Workspace,
SessionPreload }`, cycled by `Ctrl-R`, **always rendered in the UI**, with
`default_filter_mode(git_root)` selecting the narrowest configured filter that is
actually applicable — `Workspace` only if `workspaces` is on *and* you are in a
git repo, otherwise fall through.

**How.** AIKit's palette gets `SearchScope { Task, Session, Project, Host, User,
All }`, cycled by one key, rendered as a persistent badge, and defaulted by the
same "narrowest applicable" rule: `Task` if a task overlay exists, else `Session`
if a session space is bound, else `Project` if inside a project scope chain, else
`Host`. Widening is one keystroke and the widening is visible. This gives AIKit's
"any agent — or human — can operate it" claim a concrete UI primitive that users
already know from `Ctrl-R`.

Steal the search-mode cycle detail too: `SearchMode::next()` returns to the
*user's configured* mode, not a canonical one.

### 7. atuin's self-testing secret pattern table

**What it is.** `SECRET_PATTERNS: &[(&str, &str, TestValue)]` — `(name, regex,
test value)` where the test value **must** match its own regex, so the table is
covered by construction and cannot rot. Applied at capture, defaulting to on,
alongside `history_filter` and `cwd_filter` `RegexSet`s.

**How.** Port the table into `aikit-core` (it is MIT-licensed; attribute it) and
apply it in the capture path before anything reaches `inbox/`. Because each
pattern is *named*, the rejection is explainable: "not captured — matches
`GitHub PAT (new)`", which satisfies §15.10 with a user-visible reason rather than
a silent drop. Add `capture_filter` / `cwd_filter` `RegexSet`s with the same
shape. Use `regex::RegexSet` specifically — it matches all patterns in a single
pass, which matters at capture-time latency.

### 8. mise's two-tier early exit and reversible env diff

**What it is.** `should_exit_early_fast()` checks the cheap conditions (directory
unchanged) before loading config at all; `should_exit_early()` then checks
watched-file mtimes. `__MISE_DIFF` holds a serialised env diff whose `.reverse()`
un-applies it; `__MISE_SESSION` holds the previous directory and the resolved
watch list. direnv's `DIRENV_DIFF`/`DIRENV_WATCHES` are the same idea, and
direnv sets them **in a `defer`, so they are recorded even when the load is
disallowed or fails**.

**How.** AIKit's hook dispatcher (§13: <20 ms before capsule work) needs exactly
this shape:

1. `AIKIT_VIEW` + `AIKIT_GENERATION` in the environment; if the pointed-at
   generation hash is unchanged and no watched input's mtime moved, exit before
   opening SQLite.
2. `AIKIT_DIFF` carries the reversible patch for the shell projection, so leaving
   a context restores the prior environment exactly rather than approximately.
3. Set these in a `defer`-equivalent so a failed or denied resolution still leaves
   the shell in a known state — the environment-level analogue of §15.6.

### 9. flox's separation of state changes from history, and its override warnings

**What it is.** `flox generations rollback` changes which generation is live but
**does not create a generation**; it appends to a separate history log. Separately:
when composing manifests, *"if one manifest overrides another, a warning is
displayed"* — silent shadowing is treated as a defect.

**How.** Two things:

- Pointer moves (`current` → new hash, rollback, promotion) are **events** in
  `logs/events.jsonl` and the SQLite event table; they never mint generations.
  `aikit history` shows what was live when, distinct from `aikit generations`
  showing what exists. Rollback-then-forward must not create hash churn.
- The resolver emits a `Shadowed` warning whenever a later layer overrides an
  earlier layer's decision, carried in the `warnings[]` array of the JSON
  envelope and rendered in the palette as a dimmed annotation on the affected
  row. Do not wait for the user to run `explain`.

### 10. nucleo for matching, with fzf's constant relationships and separated tiebreaks

**What it is.** nucleo implements fzf's exact scoring with a two-matrix
Smith–Waterman, in Rust, multithreaded, with incremental streaming — designed for
per-keystroke TUI re-query. fzf's constants are expressed as relationships
(`bonusConsecutive = -(scoreGapStart + scoreGapExtension)`), it doubles the
first-character bonus, it has domain **schemes** (`path`, `history`) that reweight
delimiter bonuses, and it separates **score** from ordered **tiebreaks**
(`length`, `chunk`, `begin`, `end`, `index`).

**How.** Depend on `nucleo-matcher` (or `nucleo` for the worker-pool API) in
`aikit-tui`. Use a path-flavoured scheme for capability ids and export names.
Critically: put **usage recency and frequency in the tiebreak, never in the
score.** A score that mixes match quality with usage statistics is unstable
(results reorder as you use them) and unexplainable (you cannot show the user why
row 3 beat row 2). Ordered tiebreaks are both stable and describable — and they
keep faith with §14's "no automatic promotion from usage count".

### 11. Codex's nested skill walk and Claude Code's whole-record precedence

Two interoperability facts to encode in the adapters rather than discover later:

- **Codex scans `.agents/skills` in every directory from cwd to repo root**, so it
  already has nested project scope; Claude Code does not. AIKit's projections must
  be built against each host's actual discovery algorithm, and the palette's
  per-client `ActivationEffect` must reflect that a nested project scope is native
  for Codex and must be flattened for Claude Code.
- **Codex does not merge same-named skills** ("both can appear in skill
  selectors"); Claude Code resolves by precedence. AIKit fails export-name
  collisions at resolution (rule 5) and therefore never emits an ambiguous
  projection — but the error message should say *which host would have done what*,
  because that is the thing the user actually needs to understand.
- Claude Code's MCP precedence uses **whole-record replacement, not field merge**
  ("the entire server entry from that source is used; fields are not merged across
  scopes"). AIKit's `[config.*]` tables must pick a rule per section and document
  it, exactly as mise does — sets merge, *definitions* replace. Getting this
  wrong is the single most common source of "why is my config not taking effect"
  in every system surveyed.

---

# Sources

**Tier 1**

- Nix profiles: [`src/libstore/profiles.cc`](https://github.com/NixOS/nix/blob/master/src/libstore/profiles.cc), [`src/libutil/file-system.cc`](https://github.com/NixOS/nix/blob/master/src/libutil/file-system.cc) (`replaceSymlink`)
- NixOS activation: [`nixos/modules/system/activation/activation-script.nix`](https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/system/activation/activation-script.nix); switch-to-configuration Rust rewrite [PR #308801](https://github.com/NixOS/nixpkgs/pull/308801)
- NixOS module priorities: [`lib/modules.nix`](https://github.com/NixOS/nixpkgs/blob/master/lib/modules.nix), [`lib/options.nix`](https://github.com/NixOS/nixpkgs/blob/master/lib/options.nix)
- home-manager: [`modules/home-environment.nix`](https://github.com/nix-community/home-manager/blob/master/modules/home-environment.nix), [`modules/files.nix`](https://github.com/nix-community/home-manager/blob/master/modules/files.nix), [`modules/files/check-link-targets.sh`](https://github.com/nix-community/home-manager/blob/master/modules/files/check-link-targets.sh), [`modules/lib-bash/activation-init.sh`](https://github.com/nix-community/home-manager/blob/master/modules/lib-bash/activation-init.sh), [`modules/lib/dag.nix`](https://github.com/nix-community/home-manager/blob/master/modules/lib/dag.nix); [generation and profile management](https://deepwiki.com/nix-community/home-manager/2.5-generation-and-profile-management)
- Guix: [`guix/profiles.scm`](https://git.savannah.gnu.org/cgit/guix.git/tree/guix/profiles.scm), [`guix/build/utils.scm`](https://git.savannah.gnu.org/cgit/guix.git/tree/guix/build/utils.scm) (`switch-symlinks`), [Writing Manifests](https://guix.gnu.org/manual/en/html_node/Writing-Manifests.html), [Reproducible profiles](https://guix.gnu.org/cookbook/en/html_node/Reproducible-profiles.html)
- Nix diagnostics: [`nix why-depends`](https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix3-why-depends), [`nix store diff-closures`](https://nix.dev/manual/nix/2.18/command-ref/new-cli/nix3-store-diff-closures), [nvd](https://khumba.net/projects/nvd/)
- chezmoi: [Architecture](https://www.chezmoi.io/developer-guide/architecture/), [Concepts](https://www.chezmoi.io/reference/concepts/), [Target types](https://www.chezmoi.io/reference/target-types/), [Source state attributes](https://www.chezmoi.io/reference/source-state-attributes/)

**Tier 2**

- mise: [`docs/configuration.md`](https://github.com/jdx/mise/blob/main/docs/configuration.md), [`src/config/config_file/mod.rs`](https://github.com/jdx/mise/blob/main/src/config/config_file/mod.rs), [`src/hook_env.rs`](https://github.com/jdx/mise/blob/main/src/hook_env.rs), [configuration system](https://deepwiki.com/jdx/mise/3.2-configuration-system)
- asdf: [Configuration](https://asdf-vm.com/manage/configuration.html), [Versions](https://asdf-vm.com/manage/versions.html), [version selection](https://deepwiki.com/asdf-vm/asdf/5.2-version-selection)
- direnv: [`internal/cmd/rc.go`](https://github.com/direnv/direnv/blob/master/internal/cmd/rc.go), [`direnv.toml(1)`](https://direnv.net/man/direnv.toml.1.html), [`direnv(1)`](https://direnv.net/man/direnv.1.html)
- flox: [Generations](https://flox.dev/docs/concepts/generations/), [Composing environments](https://flox.dev/docs/concepts/composition), [Environments](https://flox.dev/docs/concepts/environments), [Layering and composition](https://flox.dev/blog/layering-and-composing-flox-environments/)
- devbox: [jetify-com/devbox](https://github.com/jetify-com/devbox); devenv: [devenv.sh](https://devenv.sh/)

**Tier 3**

- Agent Skills: [Anthropic overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview), [Equipping agents for the real world](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills), [Skills in Claude Code](https://code.claude.com/docs/en/skills), [agentskills.io specification](https://agentskills.io/specification)
- Codex skills: [Build skills](https://learn.chatgpt.com/docs/build-skills)
- Goose: [Configuration file](https://block.github.io/goose/docs/guides/config-file/), [Recipe reference](https://goose-docs.ai/docs/guides/recipes/recipe-reference/), [`crates/goose-server/ALLOWLIST.md`](https://github.com/block/goose/blob/main/crates/goose-server/ALLOWLIST.md)
- MCP in Claude Code: [Connect Claude Code to tools via MCP](https://code.claude.com/docs/en/mcp)
- MCP registry: [About](https://modelcontextprotocol.io/registry/about), [modelcontextprotocol/registry](https://github.com/modelcontextprotocol/registry), [official registry API](https://github.com/modelcontextprotocol/registry/blob/main/docs/reference/api/official-registry-api.md)
- Smithery / scanning: [Invariant × Smithery](https://invariantlabs.ai/blog/smithery-mcp-scan); [mpak trust tiers](https://github.com/NimbleBrainInc/mpak)
- Continue.dev: [Tool permissions](https://docs.continue.dev/cli/tool-permissions), [Rules](https://docs.continue.dev/customize/deep-dives/rules), [config.yaml reference](https://docs.continue.dev/reference)
- aider: [Repository map](https://aider.chat/docs/repomap.html), [Options reference](https://aider.chat/docs/config/options.html)

**Tier 4**

- atuin: [`crates/atuin-client/src/settings.rs`](https://github.com/atuinsh/atuin/blob/main/crates/atuin-client/src/settings.rs), [`crates/atuin-client/src/secrets.rs`](https://github.com/atuinsh/atuin/blob/main/crates/atuin-client/src/secrets.rs), [atuin.sh](https://atuin.sh/)
- fzf: [`src/algo/algo.go`](https://github.com/junegunn/fzf/blob/master/src/algo/algo.go), [fzf(1)](https://man.archlinux.org/man/fzf.1.en), [fuzzy matching algorithm](https://deepwiki.com/junegunn/fzf/2.2-fuzzy-matching-algorithm)
- nucleo: [helix-editor/nucleo](https://github.com/helix-editor/nucleo), [nucleo-matcher](https://crates.io/crates/nucleo-matcher)
