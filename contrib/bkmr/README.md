# bkmr capsules

Project-scoped knowledge-base search for AIKit, wrapping
[`bkmr`](https://github.com/sysid/bkmr).

`bkmr` searches exactly one SQLite database, chosen by one environment variable.
These capsules let a *project* declare which database that is, so two sessions
on two projects get two databases instead of racing over a shared
`~/.config/bkmr/projects/.current`. The reasoning is in
[`docs/integrations/bkmr.md`](../../docs/integrations/bkmr.md); the 6.5.0 → 7.x
upgrade is in [`UPGRADE.md`](UPGRADE.md).

## What is here

| Capsule | Command | What it does |
|---|---|---|
| `tool/search/bkmr` | — | Checked dependency: `bkmr --version`, floor 6.5.0 (text search); semantic needs 7.x. |
| `script/search/project-text` | `bkmr-text` | FTS + tag search, JSON out. Offline, no key, tens of ms. |
| `script/search/project-semantic` | `bkmr-semantic` | Semantic / hybrid search on bkmr **7.x**. Local vectors: `network = false`, no key. |
| `script/bkmr/project-init` | `bkmr-project` | Create a database; list or report on existing ones. Switches nothing. |
| `script/bkmr/project-snapshot` | `bkmr-snapshot` | `VACUUM INTO` backup with retention and a read-back check. |
| `script/install/bkmr` | `bkmr-install` | Explicit upgrade to 7.x. Prints a plan; needs `--apply`. |
| `guidance/tools/bkmr` | — | 354-token brief teaching an agent which verb is cheap and what a result *is*. |

The two semantic capsules share an export name and declare a mutual conflict, so
exactly one can be enabled. That is deliberate: `[effects]` is a claim shown
before activation, and "network on 6.x, none on 7.x" is not one honest claim.

## Adopt it

**1. Install the capsules.** Copy them into a registry, or point a project-local
registry at this directory:

```sh
cp -R contrib/bkmr/capsules/* ~/.aikit/registries/personal/capsules/
```

**2. Create or pick a database.**

```sh
sh contrib/bkmr/capsules/script/bkmr/project-init/payload/project-init.sh --list
sh contrib/bkmr/capsules/script/bkmr/project-init/payload/project-init.sh my-project
```

**3. Declare it in the project.** The target schema:

```toml
# <repo>/.aikit/project.toml
schema = 1

[integrations.bkmr]
db   = "my-project"
also = ["books"]            # optional, the set that `bkmr-text --all` sweeps
```

Until `project.toml` and environment projection land in `aikit-core`, the same
thing on today's schema:

```toml
# <repo>/.aikit/profile.toml
schema = 1
enable = [
  "tool/search/bkmr",
  "script/search/project-text",
  "script/search/project-semantic",
  "script/bkmr/project-snapshot",
  "guidance/tools/bkmr",
]

[config."tool/search/bkmr"]
db  = "my-project"
dir = "~/.config/bkmr/projects"
```

**4. Review and trust.** `guidance/tools/bkmr` changes agent behaviour and
cannot activate until its revision is reviewed. The script capsules can be run
while inactive; activation only puts them on the contextual `PATH`.

**5. Use it.**

```sh
bkmr-text "quaternal logic"                     # JSON
bkmr-text --tags _md_ --limit 5 "bergson"
bkmr-text --all "session keys"                  # across the declared set
bkmr-text "vector" | jq -r '.[].url'            # feed Read / grep
bkmr-semantic "what makes a memory persistent"  # the expensive verb
```

## Environment contract

The capsules read these; AIKit's shell projection sets them. Every one has an
explicit failure message when it is missing — nothing falls back to a global
default.

| Variable | Meaning |
|---|---|
| `AIKIT_BKMR_DB` | Absolute path to the bound database. Falls back to `BKMR_DB_URL`. |
| `AIKIT_BKMR_DB_SET` | Colon-separated paths for `bkmr-text --all`. Required by `--all`; never a glob. |
| `AIKIT_BKMR_DB_DIR` | Where `project-init` creates databases. Default `~/.config/bkmr/projects`. |
| `AIKIT_BKMR_SNAPSHOT_DIR` | Where `bkmr-snapshot` writes. Default `<db dir>/.snapshots`. |

## Two things not to do

- **Do not run `bkmr show`.** It bumps `access_count` and the update timestamp —
  measured on 6.5.0, `bkmr show --json 2` changed the row it displayed. Reads go
  through `bkmr search --json`, which does not. Nothing here calls `show`.
- **Do not set `BKMR_DB_URL` by hand, or resurrect `.current`.** That is the
  global mutable active set that made three entry points on this machine
  disagree about which project was open. Declare it per project instead.

## Checking a context

```sh
sh contrib/bkmr/capsules/tool/search/bkmr/payload/doctor.sh
```

Prints the effective binary, version, bound database, row and embedding counts,
and warns when semantic search would silently return nothing because the
database has no vectors. Read-only.
