# Upgrading bkmr 6.5.0 → 7.x

A plan to read, not a script to run. Every command is given explicitly, in
order, with what it changes. Nothing here is executed for you, and
`script/install/bkmr` deliberately refuses to act until you have decided the
question this document exists to put in front of you.

Facts about 7.x below were read from upstream source at tag `v7.6.7` and from
the `v7.0.0` / `v7.1.0` / `v7.6.0` release notes. Facts about your machine were
measured read-only on 2026-07-23.

---

## 0. The one-sentence version

Upgrading discards **all 504** embeddings you currently have, because 7.0.0
replaces OpenAI/Gemini 1536-dimension API vectors with local 768-dimension
fastembed vectors and stores them in a different table; the migration runs
automatically, without a prompt, on the first bkmr command that touches each
database.

---

## 1. What actually changes

### 1.1 Embeddings are local, and different

| | 6.5.0 (yours) | 7.x |
|---|---|---|
| Provider | Gemini or OpenAI API, opt-in via a global `--gemini` / `--openai` flag | local `fastembed` (ONNX Runtime), always on |
| Default model | `text-embedding-ada-002`-shaped, **1536** dims | **`NomicEmbedTextV15`**, **768** dims |
| Storage | `bookmarks.embedding` BLOB, 6152 bytes = 8-byte prefix + 1536 × f32 | `vec_bookmarks` virtual table (`sqlite-vec`) |
| Network per query | yes | no |
| API key | required | none |

Verified in `bkmr/src/infrastructure/embeddings/fastembed_provider.rs` at
`v7.6.7`:

```rust
fn model_dimensions(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::NomicEmbedTextV15 | EmbeddingModel::NomicEmbedTextV15Q => 768,
        EmbeddingModel::AllMiniLML6V2 | EmbeddingModel::AllMiniLML6V2Q => 384,
        EmbeddingModel::BGESmallENV15 | EmbeddingModel::BGESmallENV15Q => 384,
        EmbeddingModel::BGEM3 => 1024,
```

Selectable in `~/.config/bkmr/config.toml`:

```toml
[embeddings]
model = "NomicEmbedTextV15"   # default, 768 dims
```

If you ever change that value without re-running `backfill --force`, 7.x
detects the dimension mismatch and tells you so — a guarantee 6.5.0 does not
have, since its schema records neither model nor dimension and would happily
mix providers in one table.

### 1.2 The migration is destructive and automatic

`bkmr/migrations/2026-04-04-100000_add_vec_bookmarks/up.sql`, in full:

```sql
-- vec_bookmarks virtual table is created at runtime by
-- SqliteVectorRepository::init_vec_table() with the correct dimensions ...

-- Clear legacy embedding blobs from bookmarks table.
UPDATE bookmarks SET embedding = NULL WHERE embedding IS NOT NULL;
```

The matching `down.sql`:

```sql
-- No-op: old embedding data cannot be restored, vec_bookmarks managed at runtime
SELECT 1;
```

**The downgrade cannot restore your vectors.** Rolling back the binary (§7)
gets you 6.5.0 back; it does not get the embeddings back. Only your own backup
(§3) does.

There is no prompt. `run_pending_migrations` in
`infrastructure/repositories/sqlite/connection.rs` prints the pending list to
stderr and proceeds. It applies **per database, on first touch** — so an
unmigrated database stays 6.x-shaped until some 7.x command opens it, which
means you can and should do this one file at a time.

bkmr does take its own backup first, but read the conditions: only if the file
is larger than 16 KB *and* contains user rows, written as
`<name>_backup_YYYYMMDD.<ext>` **beside** the database. The name carries a date
and no time, so a second migration on the same day overwrites the first backup.
All of your databases clear the 16 KB bar, but do not rely on this.

### 1.3 CLI changes that matter here

- `--openai` and `--gemini` are **gone** as global flags. Anything that passes
  them — including `kbase.sh`'s `auto_backfill`, `sem-search` and `update` paths
  — breaks with an unrecognised-argument error.
- `set-embeddable` is replaced by `update --embed` / `--no-embed`.
- New: `hsearch`, hybrid FTS + vector search with Reciprocal Rank Fusion. It is
  the **only** semantic path with `--json`; `sem-search` still has no `--json`
  in 7.6.7. `hsearch --json` emits `{id, url, title, description, tags,
  rrf_score}` where `tags` is a *string*, unlike `search --json`, where it is an
  array.
- New: `clear-embeddings`, `search --embeddable`, embedding stats in `bkmr info`.
- 7.1.0: the `_mem_` system tag for agent memory, a one-system-tag-per-bookmark
  invariant, and unified WAL mode.
- 7.6.0: a global `--db <FILE>` flag that overrides `BKMR_DB_URL` and
  `config.toml`, and auto-embed on write with a `--no-embed` opt-out.
- 7.x also drops the `UpdateLastTime` trigger and adds an `accessed_at` column
  (migration `2026-04-03-100000_add_accessed_at`), so access tracking moves off
  `last_update_ts`. `bkmr show` still writes; keep not using it as a read.

---

## 2. Pre-flight

Run all of this before deciding. None of it changes anything.

```sh
# 2.1 What you have, and where.
command -v bkmr && bkmr --version           # expect /usr/local/bin/bkmr, 6.5.0
file "$(command -v bkmr)"                   # expect Mach-O x86_64

# 2.2 PATH order decides how rollback works.
printf '%s\n' "$PATH" | tr ':' '\n' | grep -n -E 'cargo/bin|/usr/local/bin'
```

On this machine `~/.cargo/bin` appears **before** `/usr/local/bin`. That is
good news: `cargo install` writes `~/.cargo/bin/bkmr`, which *shadows* the
existing 6.5.0 binary without deleting it. Rollback is then a one-line `mv`
(§7). Confirm the ordering yourself; if `/usr/local/bin` came first you would
have to move the old binary aside instead, and the rollback would be less
reversible.

```sh
# 2.3 Toolchain architecture.
rustup show | head -5
```

Note, if it applies to you as it does here: rustup reports

```
warn: Rustup is not running natively. It's running under emulation of x86_64-apple-darwin.
Default host: x86_64-apple-darwin
```

on an Apple M4. `cargo install bkmr` under that toolchain produces an **x86_64**
binary, and `fastembed` will download the x86_64 ONNX Runtime to match. It will
work, under Rosetta, and embedding is the one thing 7.x does that is genuinely
CPU-bound. If you care about that, install a native toolchain first
(`rustup toolchain install stable-aarch64-apple-darwin` and make it default)
*before* §4. This is orthogonal to the upgrade and can be deferred.

```sh
# 2.4 Inventory: rows and real embeddings, read-only, no bkmr involved.
for f in ~/.config/bkmr/*.db ~/.config/bkmr/projects/*.db; do
  printf '%6s %6s  %s\n' \
    "$(sqlite3 -readonly "$f" 'SELECT COUNT(*) FROM bookmarks;')" \
    "$(sqlite3 -readonly "$f" 'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;')" \
    "$f"
done

# 2.5 The same thing, with the plan and the refusal attached.
aikit run script/install/bkmr        # or: sh .../script/install/bkmr/payload/install.sh
```

`install.sh` with no arguments prints the plan, counts the embeddings at risk,
and exits without installing anything.

---

## 3. Back up first

bkmr's own automatic backup is not enough (§1.2). Take timestamped ones.

```sh
for f in ~/.config/bkmr/*.db ~/.config/bkmr/projects/*.db; do
  AIKIT_BKMR_DB="$f" \
  AIKIT_BKMR_SNAPSHOT_DIR="$HOME/bkmr-pre7-backups" \
  sh contrib/bkmr/capsules/script/bkmr/project-snapshot/payload/project-snapshot.sh --keep 0
done
ls -la ~/bkmr-pre7-backups
```

This uses `VACUUM INTO`, then reads the copy back to prove it opens — unlike
`gzip -c live.db`, which is what the old wrapper's `snapshot` did and which is
not a valid backup of a database another process may be writing.

Also back up the config, since §6 edits it:

```sh
cp ~/.config/bkmr/config.toml ~/bkmr-pre7-backups/config.toml.6.5.0
```

---

## 4. Install

```sh
# Refuses while embeddings are at risk; --allow-reembed is you saying you
# have read §1.2 and taken §3.
aikit run script/install/bkmr -- --apply --allow-reembed

# equivalently, by hand:
cargo install bkmr --version '^7' --locked
```

The build fetches an ONNX Runtime binary, because bkmr enables `fastembed`'s
`ort-download-binaries` feature by default. Expect a long compile.

Verify **before** touching any database:

```sh
command -v bkmr        # expect ~/.cargo/bin/bkmr
bkmr --version         # expect 7.x
```

If `command -v bkmr` still says `/usr/local/bin/bkmr`, stop: your PATH order is
not what §2.2 assumed, and continuing would migrate databases with the old
binary's assumptions still in play.

---

## 5. Migrate and re-embed, one database at a time

Order matters only in that you should do a small one first and confirm it
before spending time on `epi-logos.db`.

For each database:

```sh
DB=~/.config/bkmr/projects/books.db

# 5a. Trigger the migration. This is the irreversible step: it applies
#     `UPDATE bookmarks SET embedding = NULL`. bkmr prints the pending
#     migration list and its own backup path to stderr.
bkmr --db "$DB" search --np --limit 1 ""

# 5b. Regenerate vectors locally. First run for the whole machine also
#     downloads the ONNX model (~130 MB, upstream's figure) into the bkmr
#     model cache. No network after that.
bkmr --db "$DB" backfill --force

# 5c. Confirm.
bkmr --db "$DB" info
sqlite3 -readonly "$DB" "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';"
bkmr --db "$DB" sem-search --np --limit 3 "a phrase you know is in there"
```

### 5.1 The re-embed workload

Counts measured 2026-07-23. "Embedded" is `embedding IS NOT NULL`, not the
`embeddable` flag — the old `kbase info` reported the latter and called it
"Embedded".

| Database | Rows | Embedded today | Rows to embed on 7.x |
|---|---:|---:|---:|
| `~/.config/bkmr/projects/epi-logos.db` | 410 | 410 | 410 |
| `~/.config/bkmr/projects/books.db` | 37 | 37 | 37 |
| `~/.config/bkmr/projects/agent-payment-protocol.db` | 3 | 3 | 3 |
| `~/.config/bkmr/projects/next-words-blog.db` | 12 | **0** | 12 |
| `~/.config/bkmr/projects/4-2-technè.db` | 7 | **0** | 7 |
| `~/.config/bkmr/epi-bimba.db` | 47 | 47 | 47 |
| `~/.config/bkmr/agent-payment-protocol.db` | 31 | 6 | 31 |
| `~/.config/bkmr/bkmr.db` | 1 | 1 | 1 |
| **subtotal** | **548** | **504** | **548** |

Plus, if you want semantic search over the Antykathera essay corpus — which has
never had it:

| Database | Rows | Embedded today |
|---|---:|---:|
| `…/Antykathera-Essay-Work/.bkmr/db/passages.sqlite` | 218 | **0** |
| `…/.bkmr/db/records.sqlite` | 107 | **0** |
| `…/.bkmr/db/sections.sqlite` | 48 | **0** |
| `…/.bkmr/db/concepts.sqlite` | 22 | **0** |
| `…/.bkmr/db/arguments.sqlite` | 20 | **0** |
| `…/.bkmr/db/rooms.sqlite` | 11 | **0** |
| **subtotal** | **426** | **0** |

**974 rows in total** would end up embedded if you do all of it.

### 5.2 Databases where semantic search never worked

Say this out loud, because it changes what "upgrade" means for them:

- `next-words-blog.db` — 12 rows, **0** embeddings
- `4-2-technè.db` — 7 rows, **0** embeddings
- all six `Antykathera-Essay-Work/.bkmr/db/*.sqlite` — 426 rows, **0** embeddings

Every `sem-search` ever run against these returned nothing, and nothing is
indistinguishable from "no match". For these databases 7.x is not a regression
to recover from — it is the first time the feature will actually exist, and it
will work without an API key. That is the strongest single argument for the
upgrade.

Note also that on 6.5.0 only rows with `embeddable = 1` get vectors at all, and
in `next-words-blog.db` and `4-2-technè.db` **no** row has the flag set. On
7.6.0+ embedding is automatic on write, so this class of silent gap stops
recurring — but `backfill --force` still only processes embeddable rows, so
check `bkmr --db "$DB" search --embeddable` afterwards and set the flag where it
is missing.

---

## 6. Point AIKit at the local path

The 6.x and 7.x semantic capsules declare different effects and must not both be
enabled (they share the `bkmr-semantic` export and declare a mutual conflict).

```toml
# <repo>/.aikit/profile.toml
enable = [
  "tool/search/bkmr",
  "script/search/project-text",
  "script/search/project-semantic",          # local, offline
  "guidance/tools/bkmr",
]
disable = ["script/search/project-semantic-remote"]
```

Optionally pin the model so a later config edit cannot silently invalidate what
you just spent an afternoon computing:

```toml
# ~/.config/bkmr/config.toml
[embeddings]
model = "NomicEmbedTextV15"
```

You can now unset `GEMINI_API_KEY` for these contexts. Nothing in the 7.x path
reads it.

---

## 7. Rollback

**Binary** — because `cargo install` shadowed rather than replaced:

```sh
mv ~/.cargo/bin/bkmr ~/.cargo/bin/bkmr.7.disabled
hash -r
command -v bkmr && bkmr --version     # expect /usr/local/bin/bkmr, 6.5.0
```

Keep `/usr/local/bin/bkmr` (16 MB) until you are certain. It is a stock upstream
6.5.0 build and there is no reason to remove it early.

**Databases** — a database that has been migrated is *not* usable by 6.5.0 in
the state you left it: its embeddings are `NULL`, so 6.x semantic search returns
nothing. Restore per database from §3:

```sh
cp ~/bkmr-pre7-backups/books-<timestamp>.db ~/.config/bkmr/projects/books.db
```

or from bkmr's own automatic backup, if it exists and has not been overwritten
by a same-day second run:

```sh
ls ~/.config/bkmr/projects/*_backup_*.db
```

**Clean up the backup files afterwards.** They sit beside the live databases and
any `*.db` glob — including the three in the old `kbase.sh` — will treat them as
additional projects and search them. The AIKit capsules skip `*_backup_*`
explicitly, and `bkmr-text --all` reads a declared list rather than globbing at
all, but the files are still clutter:

```sh
mkdir -p ~/bkmr-pre7-backups/auto
mv ~/.config/bkmr/projects/*_backup_*.db ~/bkmr-pre7-backups/auto/ 2>/dev/null
mv ~/.config/bkmr/*_backup_*.db          ~/bkmr-pre7-backups/auto/ 2>/dev/null
```

---

## 8. Checklist

- [ ] §2 pre-flight run; PATH order confirmed; toolchain architecture decided
- [ ] §3 snapshots taken and listed; `config.toml` copied
- [ ] §4 installed; `bkmr --version` reports 7.x from `~/.cargo/bin`
- [ ] §5 one small database migrated, backfilled and verified end to end
- [ ] §5 remaining databases done, `bkmr info` checked on each
- [ ] §5.2 zero-embedding databases backfilled — the ones that gain the most
- [ ] §6 profile switched to `script/search/project-semantic`
- [ ] §7 automatic `*_backup_*.db` files moved out of the database directories
