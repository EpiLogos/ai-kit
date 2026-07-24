# bkmr

`bkmr` is a single-binary knowledge base: bookmarks, snippets, shell scripts and
markdown files in one SQLite database, with full-text search and vector search
over the same rows. It is a good fit for AIKit because it is exactly the shape
AIKit routes well — an external tool, cheap to invoke, whose *only* notion of
scope is which file it opens.

This document argues one thing: **the correct integration deletes the existing
wrapper rather than repairing it**, because that wrapper's entire mechanism is a
global mutable active set, and a global mutable active set is the specific thing
AIKit exists to replace (`docs/ARCHITECTURE.md` §2, §14).

Everything asserted here about bkmr was measured on this machine or read from
upstream source at a named tag. §9 separates the two.

---

## 1. What "project scoping" is, mechanically

There is no project column, no `--project` flag, and no multi-tenancy in bkmr.
A "project" is one SQLite file. Selecting a project means pointing the binary at
a different file. In 6.5.0 the only lever is the `BKMR_DB_URL` environment
variable, falling back to `db_url` in `~/.config/bkmr/config.toml`. From 7.6.0
there is also a global `--db <FILE>` flag which overrides both.

Verified in `bkmr/src/config.rs` at `v7.6.7`:

```rust
if let Ok(db_url) = std::env::var("BKMR_DB_URL") { ... }
```

and in `bkmr/src/cli/args.rs` at `v7.6.7`:

```rust
/// Path to database file (overrides BKMR_DB_URL and config.toml)
#[arg(long = "db", value_name = "FILE", global = true)]
pub db: Option<PathBuf>,
```

So the whole of "project scoping" is: **export one environment variable**. Hold
that thought.

---

## 2. The observed failure

Three entry points exist on this machine. Asked which project is active, they
give three different answers:

| Entry point | Answers | Because |
|---|---|---|
| bare `bkmr` | `agent-payment-protocol` | `db_url` in `~/.config/bkmr/config.toml` points at `~/.config/bkmr/agent-payment-protocol.db` |
| `kbase.sh current` | `next-words-blog` | `~/.config/bkmr/projects/.current` contains `next-words-blog` |
| `epi vimarsa …` | `epi-logos` | the `epi` binary injects a hardcoded `BKMR_PROJECT=epi-logos`, and `BKMR_PROJECT` outranks `.current` in `kbase.sh` |

These are not three bugs. They are one design: a mutable file (`.current`) that
claims to describe a global "current project", plus two other layers that
override it out of band. Note also that `agent-payment-protocol` names *two*
different databases — `~/.config/bkmr/agent-payment-protocol.db` (31 rows) and
`~/.config/bkmr/projects/agent-payment-protocol.db` (3 rows) — and which one you
get depends on which entry point you used.

A fourth consumer,
`~/Documents/Epi-Logos/Idea/epi-claw/extensions/khora/src/bkmr.ts`, is simply
broken: it shells out to `epi kbase sem-search --project … --top … --query …`.
`epi` has no `kbase` subcommand at all (`error: unrecognized subcommand
'kbase'`), its `vimarsa sem-search` accepts none of those three flags, and the
parser splits on `|` where bkmr emits tab-separated fields. It has three
independent faults and cannot ever have worked.

`.current` is the root. Two tmux sessions on two projects share it. Whoever ran
`kbase use` last wins, for everyone, until someone else runs it. That is the
same shape as the release-blocking cases AIKit lists at `ARCHITECTURE.md`
§15.1 ("two tmux sessions for the same project carry different skill sets") and
§15.3 ("a project profile change does not mutate another project's context"),
applied to a knowledge base instead of a skill set.

---

## 3. Why `kbase.sh` is deleted, not fixed

`kbase.sh` is 903 lines of bash, present three times
(`…/S5/epi-kbase-core/scripts/kbase.sh`, `…/S0/epi-cli/scripts/kbase.sh`,
`~/.epi-claw/workspace/skills/kbase/scripts/kbase.sh`; the latter two are
byte-identical, the first differs only in trailing whitespace on 108 lines).

Its default branch is this:

```sh
*)
    # Pass through to bkmr with resolved DB
    resolve_db
    bkmr "$@"
    ;;
```

`resolve_db` reads `$BKMR_PROJECT`, else `.current`, else `config.toml`, and
exports `BKMR_DB_URL`. That is the *whole* mechanism. Five of the verbs its own
help text advertises — `search`, `tags`, `show`, `open`, and everything else not
explicitly cased — reach bkmr through that branch and nothing else. They are 903
lines of indirection around `BKMR_DB_URL=… bkmr "$@"`.

Three arguments for deletion rather than repair:

1. **Its job is already AIKit's job.** Setting environment variables per context,
   deterministically, from a declaration, with an explanation of where each value
   came from, is the shell projection. A second, independent, undocumented
   resolver competing with it is not a feature.

2. **The part that is not env-setting mostly does not survive inspection.** §5
   enumerates all twenty-five distinct behaviours. Four survive as capsules
   because they do something real that upstream does not (`search-all`,
   `snapshot`, project creation, the inventory). Six carry defects that range
   from merely wrong to actively harmful: `find` is a bash syntax error that
   fails on every successful match; `info` reports the `embeddable` flag and
   calls it "Embedded"; `list-all` gates itself on `bkmr show`, which *writes*
   to every database it claims to list; `snapshot` gzips a live SQLite file;
   `search-all` parses a `--limit` it never uses and leaks `BKMR_DB_URL` into
   the caller; `auto_tag` interpolates an unescaped project name into a regex.
   The remainder is `.current` manipulation, or thin pass-throughs to upstream
   verbs that have since improved.

3. **`.current` cannot be made correct.** Any fix that keeps a single global file
   reproduces the three-way disagreement the moment a second consumer appears —
   which it already has, twice.

Repairing it would mean keeping the file, keeping the precedence chain, keeping
the three copies in sync, and still not being able to run two projects in two
panes.

---

## 4. The replacement

A project declares which database it uses. Nothing writes a global file.

```toml
# <repo>/.aikit/project.toml
schema = 1

[integrations.bkmr]
db  = "epi-logos"                       # resolved under the database directory
dir = "~/.config/bkmr/projects"         # optional; default shown
also = ["books", "4-2-techne"]          # optional; the declared cross-search set
```

Resolution produces, in the context's shell projection:

| Variable | Value |
|---|---|
| `AIKIT_BKMR_DB` | `/Users/admin/.config/bkmr/projects/epi-logos.db` |
| `BKMR_DB_URL` | the same path, so bare `bkmr` in that context is already correct |
| `AIKIT_BKMR_DB_SET` | colon-separated absolute paths for `db` + `also` |
| `AIKIT_BKMR_DB_DIR` | the resolved database directory |

Two `AIKIT_*` names plus the tool's own `BKMR_DB_URL` is deliberate. The
`AIKIT_*` names are what the capsules read, so a capsule keeps working if the
tool renames its variable; `BKMR_DB_URL` is exported as well so that a person
typing bare `bkmr` in that pane gets the same database the capsules do. That is
the only way to make §2's disagreement structurally impossible: there is no
second answer to give.

Two tmux sessions on two projects now hold two values of one environment
variable. They do not race, because there is nothing shared to race over.
`aikit explain` answers "which database, and why" with a scope, not a guess.

Post-upgrade, the same block also carries the embedding provider, since bkmr 7.x
selects the model from `~/.config/bkmr/config.toml` and a wrong model silently
mismatches stored dimensions:

```toml
[integrations.bkmr]
db = "epi-logos"
embedding_model = "NomicEmbedTextV15"   # 768 dims; must match what was backfilled
```

### 4.1 What does not exist yet

Honesty about the gap: **`aikit-core` has no `project.toml` and no environment
projection today.** `ProjectProfileFile` reads `<repo>/.aikit/profile.toml`, is
`#[serde(deny_unknown_fields)]`, and carries only `schema` plus a `PoolPatch`
(`profiles` / `enable` / `disable` / `[config.<capsule-id>]`). An
`[integrations.bkmr]` table in that file is rejected today, and `ProjectionItem`
has no environment-variable variant.

Until that lands, the same declaration desugars onto the mechanism that does
exist:

```toml
# <repo>/.aikit/profile.toml — works with today's schema
schema = 1
enable = [
  "tool/search/bkmr",
  "script/search/project-text",
  "guidance/tools/bkmr",
]

[config."tool/search/bkmr"]
db  = "epi-logos"
dir = "~/.config/bkmr/projects"
also = ["books"]
```

The capsules read `AIKIT_BKMR_DB` with a `BKMR_DB_URL` fallback and fail loudly
when neither is set, so they are correct under either spelling. The three things
core would need are small and general, not bkmr-specific:

1. an `env` section on the project declaration (or a `ProjectionItem::Env`),
   resolved and hashed like any other projection input;
2. `[integrations.<name>]` as sugar that a capsule can claim, so a declaration
   reads as the domain noun rather than as a capsule id;
3. `${…}` interpolation of resolved values in `ScriptSection::env`.

None of them are on the critical path for the capsules in this directory.

---

## 5. Behaviour ledger

Every distinct behaviour in the 903 lines, and where it goes. Nothing here is
dropped without a stated reason.

### Database resolution

| # | `kbase.sh` behaviour | Disposition |
|---|---|---|
| 1 | `resolve_db()` — `$BKMR_PROJECT` → `.current` → `config.toml` `db_url` → `~/.config/bkmr/bkmr.db`, then `export BKMR_DB_URL` | **AIKit built-in.** This is the shell projection (§4). The `.current` link in the chain is deleted, not ported. |
| 2 | `get_current_project()` — same chain, name only | **AIKit built-in.** `aikit explain` names the value and the scope that set it. `tool/search/bkmr`'s `doctor.sh` prints the effective binary + database for a shell that has no AIKit. |
| 3 | `ensure_projects_dir()` — `mkdir -p` | **Capsule.** Folded into `script/bkmr/project-init`. |

### Project lifecycle

| # | Behaviour | Disposition |
|---|---|---|
| 4 | `init <name>` — `bkmr create-db`, then **auto-switch** by writing `.current` | **Capsule** `script/bkmr/project-init`, minus the auto-switch. Creating a database and choosing which shell sees it are separate acts; fusing them is what produced §2. The capsule prints the `project.toml` stanza instead. |
| 5 | `use <name>` / `use --global` — write `.current` | **Dropped.** This *is* the global mutable active set. Its replacement is editing a declaration, or a session overlay for a one-session change. |
| 6 | `list` — glob `*.db`, count rows via `sqlite3`, mark the active one | **Capsule**, as `project-init --list`. The `[active]` marker is dropped (nothing is globally active) and `*_backup_*` files are excluded — see #22. |
| 7 | `current` — print the active project | **AIKit built-in** (`aikit explain`), plus `doctor.sh`. |
| 8 | `info [project]` — rows, size, mtime, "Embedded" | **Capsule**, as `project-init --info`, with the bug fixed: `kbase info` counts `embeddable = 1`, which means *eligible to be embedded*, not *embedded*. It reports 37 for `books.db` and 0 for `next-words-blog.db` and happens to be right in both cases only because those two agree. The capsule counts `embedding IS NOT NULL` on 6.x and the presence of `vec_bookmarks` on 7.x. |
| 9 | `delete <name>` — prompt, then `rm` | **Dropped.** An unrecoverable delete behind a `[y/N]` prompt, with no snapshot first, in a tool that has a snapshot verb. `rm` is not worth 30 lines, and wrapping it makes it look safer than it is. |
| 10 | `rename <old> <new>` — `mv`, then rewrite `.current` | **Dropped.** `mv` plus editing the declaration. The only thing the wrapper added was keeping `.current` consistent, which no longer exists. |
| 11 | `find <partial>` — exact + substring match over names | **Dropped.** The palette is the fuzzy finder (`ARCHITECTURE.md` §13: search keystroke < 16 ms). Also broken: line 365 builds `$(( … + ( [ -n "$exact_match" ] && echo 1 || echo 0 ) ))`, which is not valid arithmetic — bash reports `syntax error: operand expected` and, under `set -e`, `kbase find` exits non-zero on every successful match. |
| 12 | `switch <partial>` — fuzzy match, then `use` | **Dropped** with #5. |

### Content

| # | Behaviour | Disposition |
|---|---|---|
| 13 | `add <url> [tags]` — auto-tag, `bkmr add`, auto-backfill | **Dropped as a wrapper**; with `BKMR_DB_URL` projected, `bkmr add` already targets the right database. The two things it added are handled separately: auto-tagging at #17, auto-backfill at #18. |
| 14 | `add-file <path> [tags]` — store `file://<abs>` with `_md_` and a title from the basename | **Superseded upstream.** 7.x `bkmr import-files` does this properly: it stores content, tracks the source path, mtime and hash, and reads YAML frontmatter for title and tags. The *pattern* — a row is a pointer into a vault, not a copy of it — is preserved and taught in `guidance/tools/bkmr`. (The wrapper also creates and immediately deletes a `mktemp` file it never uses.) |
| 15 | `fetch <url> [tags]` — `curl` the URL, `pandoc` it to text, store up to 10 000 bytes | **Dropped.** This is a web archiver, orthogonal to capability routing, and it stores the fetched body *in the URL column*, which is `not null unique` — so two pages that strip to the same text collide. If it is wanted, it belongs in its own capsule with `network = true` declared, not smuggled into a search integration. |
| 16 | `update <id>` / `refresh <id>` — update, force-enable embedding, re-embed; `refresh` also re-fetches and tags `_updated-YYYYMMDD` | **Superseded upstream.** 7.6.0 made embedding automatic on write with a `--no-embed` opt-out, and `update --embed/--no-embed` replaced `set-embeddable`. `refresh` is dropped with #15. |
| 17 | `auto_tag()` — append `_<project>` to the tag list | **Kept, as a convention rather than a mechanism.** It is genuinely useful: it is the only thing that makes a row attributable if two databases are ever merged. `project-init` prints the tag to use; `guidance/tools/bkmr` teaches the `_`-wrapped system-tag vocabulary. Not automated, because the implementation was `[[ ! "$tags" =~ _$project ]]` — an unanchored regex interpolating an unescaped project name, so a project called `4-2-technè` or anything with a regex metacharacter matches wrongly. |
| 18 | `auto_backfill()` — `bkmr --gemini backfill` after every write, `2>/dev/null`, warning on failure | **Dropped as automatic.** A network call and an API charge on every add, with the error swallowed. On 7.6.0+ upstream embeds on write anyway, locally and for free. Explicit backfill is step 3 of `contrib/bkmr/UPGRADE.md`. |

### Cross-database

| # | Behaviour | Disposition |
|---|---|---|
| 19 | `search-all <q> [--gemini] [--limit]` — loop every `*.db`, search each, print with a header | **Capsule.** This is the one behaviour that is neither env-setting nor available upstream, so it survives as `bkmr-text --all`, over the **declared** `also = [...]` set rather than a directory glob. Two bugs not carried over: `--limit` is parsed and never used, and `export BKMR_DB_URL="$db"` leaks the last database into the caller's environment after the loop. |
| 20 | `list-all` — per-database dump of the newest 20 rows | **Partly kept, partly a bug.** The inventory is `project-init --list`. The row dump bypasses bkmr and reads `sqlite3` directly, and — worse — gates itself on `bkmr show 1`, which **mutates**: measured on 6.5.0, `bkmr show --json 2` bumped `flags` 2 → 3 and rewrote `last_update_ts`. So `kbase list-all` silently writes to every database it "lists". Nothing in the capsule set calls `bkmr show`. |

### Versioning

| # | Behaviour | Disposition |
|---|---|---|
| 21 | `snapshot [message]` — `gzip -c live.db > .snapshots/<p>-<ts>.db.gz`, keep 10 | **Capsule** `script/bkmr/project-snapshot`, reimplemented. `gzip` of a live SQLite file is not a backup: it reads pages while another process may be writing, and from 7.1.0 bkmr runs in WAL mode, so committed data can be sitting in a `-wal` file `gzip` never opens. The capsule uses `VACUUM INTO`, which takes the read lock SQLite requires, then reads the result back to prove it opens. The `message` argument is dropped because the original accepted it and stored it nowhere. |
| 22 | `log` — list snapshots | **Capsule**, as `project-snapshot --list`. |

### Ambient

| # | Behaviour | Disposition |
|---|---|---|
| 23 | `*)` pass-through — `resolve_db; bkmr "$@"` | **Dropped, and this is the point.** With `BKMR_DB_URL` in the context's environment, plain `bkmr` *is* the pass-through, with upstream's own help, completions and error messages. |
| 24 | `BKMR_AUTO_BACKFILL`, `BKMR_AUTO_TAG` env switches | **Dropped** with #17 and #18. |
| 25 | `sem-search` case — `resolve_db; bkmr --gemini sem-search "$@"` | **Capsule** `script/search/project-semantic` (bkmr 7.x local; `network = false`). See §7. |

One more hazard, which is new rather than inherited: bkmr 7.x's automatic
migration writes `<name>_backup_YYYYMMDD.db` **beside** each database. Every
`*.db` glob in `kbase.sh` — `list`, `search-all`, `list-all` — would then treat
those backups as additional projects and search them. The capsules skip
`*_backup_*` explicitly, and `--all` reads a declared list rather than globbing
at all.

---

## 6. Migration table

| Today | Replacement | Notes |
|---|---|---|
| bare `bkmr <verb>` reading `config.toml` | bare `bkmr <verb>` reading the context's `BKMR_DB_URL` | Same command. The only change is that the answer now depends on which project you are in, which is what you wanted. |
| `kbase.sh search` / `sem-search` | `bkmr-text` / `bkmr-semantic` | JSON out; effects declared; `bkmr show` never called. |
| `kbase.sh search-all` | `bkmr-text --all` | Over `also = [...]`, not a glob. |
| `kbase.sh init` | `bkmr-project <name>` | Creates the database; prints the declaration; does not switch. |
| `kbase.sh use` / `switch` | edit `.aikit/project.toml`, or a session overlay for one session | The global switch is gone by design. |
| `kbase.sh list` / `current` / `info` | `bkmr-project --list` / `aikit explain` / `bkmr-project --info` | `--info` counts real embeddings. |
| `kbase.sh snapshot` / `log` | `bkmr-snapshot` / `bkmr-snapshot --list` | `VACUUM INTO` instead of `gzip`. |
| `kbase.sh add` / `add-file` / `fetch` / `update` / `refresh` | `bkmr add` / `bkmr import-files` / — / `bkmr update` / — | `fetch` and `refresh` are not replaced (§5 #15). |
| `kbase.sh delete` / `rename` / `find` | `rm` / `mv` / the palette | See §5 #9–#11. |
| `epi vimarsa …` | drop the hardcoded `BKMR_PROJECT=epi-logos`; call `bkmr-text` or `bkmr` and inherit the context | `epi`'s subcommand list can stay; what must go is the injected override, which is the reason it disagrees with everything else. |
| `khora/src/bkmr.ts` | rewrite against `bkmr-text --json`, or delete | It calls a subcommand that does not exist, passes flags that do not exist, and parses a delimiter that is not emitted. There is nothing to preserve. |
| `~/.config/bkmr/projects/.current` | nothing | Delete the file. Its function is now a declaration resolved per context. |
| the three copies of `kbase.sh` | nothing | Delete all three. |

---

## 7. The capsule set

Under `contrib/bkmr/capsules/`. The baseline is bkmr 7.x with local fastembed —
this is a new integration and it targets the good version, not a legacy path.

| Capsule | Exports | Declared effects |
|---|---|---|
| `tool/search/bkmr` | — | read outside project, subprocess |
| `script/install/bkmr` | `bkmr-install` | read/write outside project, network, subprocess |
| `script/search/project-text` | `bkmr-text` | read outside project, subprocess |
| `script/search/project-semantic` | `bkmr-semantic` | read outside project, subprocess |
| `script/bkmr/project-init` | `bkmr-project` | read/write outside project, subprocess |
| `script/bkmr/project-snapshot` | `bkmr-snapshot` | read/write outside project, subprocess |
| `guidance/tools/bkmr` | — | none |

**One semantic capsule, and why its effect declaration is honest.** `[effects]`
is a static claim the palette shows *before* activation, and it drives the
confirmation prompt (`aikit-core::effects`). Local fastembed makes the claim
simple and true: `network = false`, `credentials = []`. `project-semantic`
enforces the same precondition at runtime — it refuses on bkmr < 7 with a pointer
to `script/install/bkmr` — so the declaration and the behaviour cannot drift.
There is no remote variant: a Gemini-backed 6.x path would have a *different*
effect declaration, and rather than ship a second capsule to carry it, the
integration simply requires the version where the honest declaration is the
cheap one.

**Why installation is a capsule you run, not a resolution step.** AIKit is
explicitly not a package manager (`ARCHITECTURE.md` §14). `tool/search/bkmr`
therefore carries `install_hint = "script/install/bkmr"` and nothing more: an
unsatisfied tool renders as unavailable with a pointer, never as a background
`cargo install`. `script/install/bkmr` prints a plan and exits; it needs
`--apply` to act, and it refuses outright — before touching anything — when the
upgrade would discard existing embeddings, unless `--allow-reembed` is passed.
Run today on this machine it reports **504** embeddings at risk across six
databases and stops.

**Effects caveat, stated rather than hidden.** The search capsules declare
`filesystem = ["read:outside"]` and not `write:outside`. Measured on 6.5.0,
`bkmr search --json` leaves the database byte-identical and creates no journal
files (`PRAGMA journal_mode` is `delete`). From 7.1.0 bkmr opens databases in
WAL mode, which does create `<db>-wal` and `<db>-shm` sidecars even for a read.
`WriteOutsideProject` is an *elevated* effect that demands confirmation on every
invocation; declaring it for a command meant to be run dozens of times a session
would teach the user to dismiss the prompt that should matter. The trade is
recorded here and in the manifest comment rather than being silently made.

---

## 8. Guidance, and what it must not say

`guidance/tools/bkmr` is 354 estimated tokens against a declared budget of 400
(`aikit_core::guidance::estimate_tokens`, `ceil(normalized_chars / 4)`). It
teaches four things and nothing else:

1. one database, bound per project; do not set `BKMR_DB_URL`, do not read
   `.current`;
2. the cost gradient — text search is tens of milliseconds and offline, semantic
   search runs an embedding pass and on 6.x is a network call;
3. results are `file://` pointers into a vault, not content: read the file;
4. never run `bkmr show`, because it writes.

It carries `dedup_key = "tool:bkmr"`, so a project- or session-scoped fragment
describing a different bkmr setup replaces it instead of stacking with it.

---

## 9. Verified versus inferred

**Measured on this machine (bkmr 6.5.0, read-only, or against scratch copies):**

- `bkmr --version` → `6.5.0`; `/usr/local/bin/bkmr` is stock upstream.
- `search --json` and `show --json` exist; `sem-search` has no `--json`.
- `bkmr show --json 2` on a scratch copy bumped `flags` 2 → 3 and rewrote
  `last_update_ts`. `bkmr search --json` left the file byte-identical.
- With stdout piped, `bkmr search --np` prints the formatted listing to
  **stderr** and only a comma-separated id list to **stdout**. Any wrapper that
  captures stdout alone gets `31,37` and looks broken.
- Embedding BLOBs are 6152 bytes uniformly = 8-byte prefix + 1536 × f32. The
  schema records no model and no version.
- Row / embedding counts: `epi-logos` 410/410, `books` 37/37,
  `projects/agent-payment-protocol` 3/3, `next-words-blog` 0/12,
  `4-2-technè` 0/7, `~/.config/bkmr/epi-bimba.db` 47/47,
  `~/.config/bkmr/agent-payment-protocol.db` 6/31, `~/.config/bkmr/bkmr.db` 1/1.
  Total embedded: **504**.
- `~/Documents/Nara-Personal/Antykathera-Essay-Work/.bkmr/db/*.sqlite`:
  six collections, 426 rows, **0** embedded.
- `epi kbase` → `error: unrecognized subcommand 'kbase'`. `epi vimarsa
  sem-search` has no `--project`, `--top` or `--query`. `strings ~/.cargo/bin/epi`
  contains `BKMR_PROJECT` adjacent to `epi-logos`.
- `kbase find`'s arithmetic on line 365 is a bash syntax error.
- The three `kbase.sh` copies differ only in trailing whitespace (108 lines);
  two are byte-identical, one is not. The background's "three byte-identical
  copies" is very slightly wrong; functionally it is right.

**Read from upstream source at `v7.6.7` / `v6.5.0`:**

- 7.x default model is `NomicEmbedTextV15`, **768** dimensions
  (`infrastructure/embeddings/fastembed_provider.rs`); alternatives are
  `NomicEmbedTextV15Q` 768, `AllMiniLML6V2(Q)` 384, `BGESmallENV15(Q)` 384,
  `BGEM3` 1024. Configured as `[embeddings] model = …` in `config.toml`.
- `--openai` and `--gemini` are gone from 7.x; `args.rs` has neither.
- `sem-search` still has **no** `--json` in 7.6.7 — only `--limit` and `--np`.
  The JSON-capable semantic path is `hsearch --json`, whose objects are
  `{id, url, title, description, tags, rrf_score}` with `tags` as a **string**,
  not the array `search --json` emits.
- The `_mem_` system tag arrived in **7.1.0**, together with a one-system-tag
  invariant and unified WAL mode.
- 7.6.0 added the global `--db` flag and auto-embed-on-write with `--no-embed`.
- The 7.0 migration's `up.sql` is literally
  `UPDATE bookmarks SET embedding = NULL WHERE embedding IS NOT NULL;`, and its
  `down.sql` is a no-op comment: *the downgrade cannot restore embeddings*.
- The automatic pre-migration backup is `<name>_backup_YYYYMMDD.<ext>` beside
  the database, taken only when the file exceeds 16 KB and contains user data.
  The name carries a date but no time, so a second migration the same day
  overwrites it.
- bkmr depends on `fastembed 5` with the `ort-download-binaries` feature on by
  default, so `cargo install` fetches an ONNX Runtime binary at build time.

**Corrections to the working assumptions this integration started from:**

1. 6.5.0 does **not** silently fall back to `DummyEmbedding` for search. Both
   `sem-search` and `backfill` check the embedder type and abort with a red
   error. The real defect is that the message says *"Use --openai flag"* even
   when you meant `--gemini`.
2. `sem-search` gaining `--json` never happened; `hsearch` is the answer, and it
   is new in 7.0.0.
3. `BKMR_DB_URL` is not the only lever any more — 7.6.0's global `--db` flag
   outranks it. The projection still uses the environment variable, because that
   is what a bare `bkmr` in the pane will read.
4. The three `kbase.sh` copies are not byte-identical (whitespace only).

**Inferred, not verified:** the ~130 MB figure for the downloaded ONNX model is
upstream's own number from the v7.0.0 release notes and was not measured here;
check `du -sh` on the bkmr model cache after the first backfill. The ~0.4–1.0 s
latency for a 6.x Gemini round trip is inherited from the earlier investigation
and was not re-measured, because doing so would have required a live API call.
