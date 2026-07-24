# The skills distribution ecosystem

`docs/PRIOR-ART.md` §Tier 3 covers how agent hosts **discover and load** skills at
runtime. This document covers the layer above that: how skills get **onto the
machine** in the first place — the CLIs, marketplaces, lockfiles and registries
that centralise and distribute them.

The two documents are deliberately separate. Discovery is a filesystem contract
the hosts define. Distribution is a tooling market, and it moved fast enough in
2026 that a de facto standard emerged without anyone specifying it.

Survey date: 2026-07-23. Machine surveyed: this one.

---

## Contents

1. [The tool on this machine: the Skills CLI](#1-the-tool-on-this-machine-the-skills-cli)
2. [The other centralisers present locally](#2-the-other-centralisers-present-locally)
3. [The ecosystem, compared](#3-the-ecosystem-compared)
4. [What we must be compatible with](#4-what-we-must-be-compatible-with)
5. [What we should not copy](#5-what-we-should-not-copy)

---

# 1. The tool on this machine: the Skills CLI

**`skills` — the npm package `skills`, by vercel-labs, v1.5.10.**
Repo <https://github.com/vercel-labs/skills>. Registry/leaderboard
<https://skills.sh>. Formerly published as `add-skill` (v1.0.29 is still in the
bun cache at `~/.bun/install/cache/add-skill@1.0.29@@@1/`).

It is **not installed as a binary**. `which -a skills` finds nothing; there is no
entry in `~/.cargo/bin`, `~/.local/bin`, `/usr/local/bin`, `~/.bun/bin`, npm or
pnpm globals. It runs via `npx skills`, and the resolved package sits in the npx
cache at:

```
/Users/admin/.npm/_npx/ac0ed6aa23b37c1e/node_modules/skills/   # v1.5.10
```

That invisibility matters: **the thing that owns the skill tree on this machine
is not on the PATH and has no persistent installation.** Nothing about the local
state announces which tool produced it except the lockfile.

## 1.1 State it owns

| Path | Role |
|---|---|
| `~/.agents/.skill-lock.json` | Global lockfile, `version: 3`. 18 entries. |
| `~/.agents/skills/<name>/` | **Canonical store** for globally installed skills. |
| `~/<agent-dir>/skills/<name>` | Relative symlink into the canonical store. |
| `<cwd>/skills-lock.json` | Project lockfile, `version: 1`. Different schema. |
| `<cwd>/.agents/skills/<name>/` | Canonical store for project-scope installs. |

Lock path is `$XDG_STATE_HOME/skills/.skill-lock.json` if `XDG_STATE_HOME` is set,
else `~/.agents/.skill-lock.json`.

Global lock entry (all seven keys observed locally):

```json
"ralph-tui-prd": {
  "source": "subsy/ralph-tui",
  "sourceType": "github",
  "sourceUrl": "https://github.com/subsy/ralph-tui.git",
  "skillPath": "skills/ralph-tui-prd/SKILL.md",
  "skillFolderHash": "bf73ca88685816787c9822388db5b6519dd0848e",
  "installedAt": "2026-01-24T19:47:34.231Z",
  "updatedAt":   "2026-02-20T16:12:38.926Z"
}
```

`sourceType` ∈ `github | git | local | well-known | node_modules`. An optional
`ref` and `pluginName` may also appear.

The 18 entries came from three sources: `Leonxlnx/taste-skill` (13),
`subsy/ralph-tui` (4), `vercel-labs/skills` (1, the `find-skills` skill — which is
how the CLI advertises itself to agents).

## 1.2 The actual mechanism

**Install** (`skills add <source>`):

1. Resolve the source. GitHub shorthand, full GitHub/GitLab URL, `tree/<ref>/<path>`
   deep link, any git URL, a local path, or an HTTPS origin serving
   `.well-known/agent-skills/index.json`.
2. Fetch. Public GitHub → direct archive download via the API; on auth failure →
   `git` clone (HTTPS then SSH), using `GITHUB_TOKEN`/`GH_TOKEN` or `gh auth token`.
3. Discover skills in the payload. Walks a fixed list of container dirs
   (`skills/`, `skills/.curated/`, `skills/.experimental/`, `skills/.system/`,
   `.claude/skills/`, `.agents/skills/`, `.hermes/skills/`, ~60 more) one level
   deep, plus one extra level for `skills/<category>/<name>/SKILL.md` catalog
   layouts. A shallower `SKILL.md` shadows anything nested under it.
4. **Copy** the skill directory into the canonical store —
   `cleanAndCreateDirectory(canonicalDir)` then `copyDirectory`. `metadata.json`,
   `.git/`, `__pycache__/` are excluded; symlinks are dereferenced.
5. **Symlink** each target agent's dir at the canonical copy, as a *relative*
   link: `~/.claude/skills/brandkit -> ../../.agents/skills/brandkit`. If
   `symlink()` fails (Windows without privilege, exotic FS) it silently falls back
   to a full copy and reports `symlinkFailed`. `--copy` forces copies everywhere
   and **writes no canonical copy at all**.
6. Write the lock entry.

Agents whose skills dir *is* `.agents/skills` (Codex, Cursor, OpenCode, Gemini
CLI, Copilot, Cline, Zed, Warp, Amp…) are "universal": for them the canonical
store *is* the install location, no symlink needed. Everyone else gets a link.

This is exactly what is on disk here: `~/.claude/skills` contains 14 relative
symlinks into `~/.agents/skills` interleaved with 25 hand-made real directories
that the CLI has never touched and does not know about.

**Provenance**: recorded in the lockfile, per skill, and reasonably complete —
source, resolved URL, path within the repo, folder hash, install and update
timestamps. Better than anything else in the ecosystem at this layer.

**Integrity**: *recorded but never verified.* `skillFolderHash` for a GitHub
source is the **git tree SHA-1 of the skill directory**, read from the GitHub tree
API (`getSkillFolderHashFromTree`). For non-GitHub sources it is a home-grown
sha256 over `(relpath, bytes)` pairs in sorted order (`computeSkillFolderHash`).
It exists solely so `skills update` can diff. Nothing is checked on the way in;
there is no `--frozen`, no verify-on-load. The project-scope
`skills-lock.json` stores a `computedHash` of the *installed* tree instead, so the
two lockfiles hash different objects with different algorithms.

**Update** (`skills update [names…]`): for each source, fetch the tree (or clone),
recompute the folder hash for each locked `skillPath`, and offer to reinstall
where `latestHash !== entry.skillFolderHash`. Skills with no hash or no path are
listed as unfixable with a reason: `Local path`, `Git URL`, `Well-known skill`,
`Private or deleted repo`, `No version tracking`. Skills whose `skillPath` has
vanished upstream are reported as deleted and offered for removal (declined
automatically under `-y`).

**Versioning**: there is none. No semver, no ref pinning on install, no
`resolved` commit recorded. `ref` is stored only if the user typed one. "Has this
changed" is a content-hash diff and nothing more.

**Conflict handling**: none. Install is `clean-and-copy` into
`~/.agents/skills/<sanitized-name>`. Two sources providing `pdf` means the second
install silently destroys the first, and the lockfile — keyed by bare skill name —
silently reassigns provenance. The only guards are `sanitizeName()` and a
`isPathSafe()` traversal check.

**Trust**: essentially none, with one hardcoded exception. Any source under the
`openclaw/*` owner is **blocked** with a warning that the skills "run with full
agent permissions and could be malicious", and requires
`--dangerously-accept-openclaw-risks`. For everything else, `skills add` may print
an advisory security table (Snyk / Socket / ATH risk labels) fetched from
`https://add-skill.vercel.sh/audit` — **display only, never a gate**, and dropped
after a 3 s timeout. Then it installs.

**Telemetry**: `https://add-skill.vercel.sh/t` receives event, source, skill
names, target agents, and the skill→path map on every install/find, unless
`DISABLE_TELEMETRY` or `DO_NOT_TRACK` is set. This is what powers the skills.sh
install-count leaderboard.

## 1.3 Command surface

`skills --help` is flat — subcommands take no `--help` of their own
(`skills add --help` → `Unknown command`).

```
add <package>          (a)   add a skill package
use <package>@<skill>        print a prompt for one skill without installing
remove [skills]        (rm)  remove installed skills
list                   (ls)  list installed skills            [--json]
find [query]                 search skills.sh interactively
update [skills…]       (upgrade)
experimental_install         restore skills from skills-lock.json
experimental_sync            sync skills out of node_modules
init [name]                  scaffold <name>/SKILL.md
```

Flags: `-g/--global`, `-a/--agent <…>`, `-s/--skill <…>`, `-l/--list`,
`-y/--yes`, `--copy`, `--all`, `--full-depth`,
`--dangerously-accept-openclaw-risks`, `-p/--project` (update only).

Note `skills check` **does not exist** in 1.5.10, despite the installed
`find-skills` SKILL.md advertising it. The vendored skill is stale relative to
its own CLI — a small illustration of why AIKit treats a skill's claims as data,
not as truth.

## 1.4 Two more findings worth carrying into the design

* **Lockfile downgrade discards everything.** `readSkillLock()` returns an *empty*
  lock when `parsed.version < CURRENT_VERSION`. A v3→v4 bump means every machine's
  provenance record is silently dropped on first run of the new CLI, not migrated.
  AIKit must migrate, or refuse and say so.
* **The lockfile is keyed by bare skill name in a single flat namespace**, with no
  registry, group or source qualifier. That is the same design mistake as a global
  mutable install set, one level up.

---

# 2. The other centralisers present locally

## 2.1 Codex `skill-installer` (`~/.codex/skills/.system/skill-installer`)

Not a CLI — a **skill that shells out to two Python scripts**. Preinstalled by
Codex alongside `skill-creator`, `imagegen`, `openai-docs`, `plugin-creator`,
`review-agent`, all fingerprinted by
`~/.codex/skills/.system/.codex-system-skills.marker` (`6fac8acc0c6abb7b`).

* `scripts/list-skills.py` — lists `openai/skills/skills/.curated` (or
  `.experimental`) via the GitHub API, annotating what is already installed.
* `scripts/install-skill-from-github.py --repo <o>/<r> --path <p>…` — direct
  download, falling back to git sparse checkout on auth failure.
* Installs to `$CODEX_HOME/skills/<name>`, default `~/.codex/skills`.
* **Aborts if the destination already exists.** Fail-on-conflict — the opposite
  of the Skills CLI, and the better default.
* No lockfile, no hash, no provenance record, no update path. Reinstall is the
  update mechanism. `--ref` defaults to `main`.

## 2.2 Codex plugins and marketplaces

Codex has a full plugin system layered over skills. From `~/.codex/config.toml`:

```toml
[plugins."visualize@openai-bundled"]
enabled = true

[marketplaces.openai-bundled]
last_updated = "2026-07-21T10:41:20Z"
source_type  = "local"
source       = "/Users/admin/.codex/.tmp/bundled-marketplaces/openai-bundled"

[[skills.config]]
path = "/Users/admin/.codex/skills/telegram-squad/SKILL.md"
enabled = false
```

Four marketplaces are materialised under `~/.codex/plugins/cache/`:
`openai-bundled`, `openai-primary-runtime`, `openai-curated`,
`openai-curated-remote` — cached as `<marketplace>/<plugin>/<version>/`, e.g.
`openai-bundled/sites/0.1.30`, `openai-curated/github/d6169bef` (a content id, not
a semver). Remote installs drop a `.codex-remote-plugin-install.json`:

```json
{ "schema_version": 1, "remote_plugin_id": "plugin_connector_1p_1a69035c238881919c4190932b2df699" }
```

Marketplace manifest lives at `.agents/plugins/marketplace.json`; each plugin
carries `.codex-plugin/plugin.json` with `name`, `version`, `skills: "./skills/"`
and a rich `interface` block (display name, category, logos, privacy/ToS URLs,
default prompts). Marketplace entries carry a `policy` block —
`{"installation": "AVAILABLE", "authentication": "ON_INSTALL"}`.

Two things Codex gets right that nothing else does:

* **Per-skill disable by absolute path** in config (`[[skills.config]]`), which is
  the closest thing in the ecosystem to a pool patch.
* **`[hooks.state]` records a `trusted_hash = "sha256:…"` per hook file and
  event**, keyed `<path>:<event>:<idx>:<idx>`. Codex already implements
  trust-on-first-use content approval — for hooks. Not for skills. AIKit's §7
  trust gate is the generalisation of a mechanism the vendor has already shipped
  next door.

## 2.3 Claude Code plugins

`~/.claude/plugins/`:

* `installed_plugins.json` (`version: 2`) — `"<plugin>@<marketplace>"` →
  `[{scope, installPath, version, installedAt, lastUpdated, gitCommitSha}]`.
  **An array**, because scope (`user`/`project`/`local`) is part of the identity.
  Locally: `superpowers@claude-plugins-official` 6.1.1, `clangd-lsp` 1.0.0,
  `frontend-design` version `"unknown"` — the manifest had no `version`, so the
  field degrades to a string literal rather than falling back cleanly.
* `known_marketplaces.json` — name → `{source: {source: "github", repo}, installLocation, lastUpdated}`.
* `marketplaces/<name>/` — a plain git clone of the marketplace repo.
* `cache/<marketplace>/<plugin>/<version>/` — the installed plugin.
* `blocklist.json` — fetched remotely, a **kill switch**: plugin id, `added_at`,
  `reason` (`security`), free text.

This is the most complete provenance model in the ecosystem: marketplace source +
plugin version + `gitCommitSha` + scope + timestamps, plus a revocation channel.
Per the docs, a plugin `source` may be `github` / `url` / `git-subdir` (each with
`ref?` and `sha?`, where **`sha` wins**) or `npm` (`package`, `version?`,
`registry?`). Marketplace sources support `ref` but **not** `sha`.

## 2.4 `obra/superpowers` and superpowers-marketplace

`~/.claude/plugins/marketplaces/superpowers-marketplace/.claude-plugin/marketplace.json`
is a catalog of seven plugins, each pointing at its **own separate git repo**
(`obra/superpowers`, `obra/superpowers-chrome`, `obra/episodic-memory`, …) with a
declared `version` and `"strict": true`.

The distribution unit is the **plugin**; skills are its cargo. `superpowers` 6.1.1
contributes the 14 `superpowers:*` skills. Notably the catalog pins **versions but
not SHAs** — `{"source": "url", "url": "https://github.com/obra/superpowers.git"}`
with no `ref`/`sha` — so what you get is whatever the default branch holds when
the version string last changed. The marketplace is curated by one person; that
*is* the trust model.

## 2.5 Hermes

`~/.hermes/skills/` is a **two-level catalog**: `skills/<category>/<name>/SKILL.md`
across 26 categories (`research/`, `nara/`, `software-development/`, …). Frontmatter
is richer than the spec requires: `version`, `author`, `license`,
`platforms: [linux, macos, windows]`, `metadata.hermes.{tags, related_skills, homepage}`,
`prerequisites`.

* `~/.hermes/skills/.bundled_manifest` — a flat `name:md5` list of every bundled
  skill. Its job is **third-party modification detection**: it lets the curator
  tell "shipped and untouched" from "user edited", exactly the state chezmoi
  tracks (PRIOR-ART §chezmoi).
* `curator:` in `config.yaml` — `enabled: true`, `interval_hours: 168`,
  `min_idle_hours: 2`, `stale_after_days: 30`, `archive_after_days: 90`,
  `prune_builtins: true`. A background process that **archives and prunes skills
  by disuse**. It is frecency, inverted, and applied destructively rather than to
  ranking.
* `skills.external_dirs: []` — the point-me-at-a-directory seam SPEC-III §2 wants,
  present and unused.

## 2.6 Nothing else

There is no `skills` binary, no Rust skill manager, and no other skill-managing
tool on this machine. `~/Documents/quaternal-logic-plugin/epi-logos/skills/` is a
hand-authored source tree that `~/.agents/skills` symlinks into by hand (13 links,
all dated 2025-05-07, none in the lockfile) — a manually maintained observed set,
in SPEC-III §1.3 terms.

---

# 3. The ecosystem, compared

## 3.1 The specification layer

**Agent Skills** (<https://agentskills.io/specification>) is the format, released
by Anthropic 2025-12-18 and now implemented by 30+ tools. It specifies a
directory with `SKILL.md`; required `name` (≤64 chars, `[a-z0-9-]`, no leading/
trailing/consecutive hyphens, **must match the parent directory name**) and
`description` (≤1024); optional `license`, `compatibility` (≤500),
`metadata` (arbitrary map), `allowed-tools` (experimental, space-separated);
conventional `scripts/`, `references/`, `assets/`. Unrecognised keys must be
ignored. Reference validator: `skills-ref validate ./my-skill`
(<https://github.com/agentskills/agentskills>).

**The spec says nothing about distribution.** No packaging, no versioning, no
registry, no integrity. That vacuum is what every tool below fills differently.

**There is no first-party Anthropic skills CLI or registry.** `anthropics/skills`
is a plain repo of ~17 skills that you consume as a *plugin marketplace*
(`/plugin marketplace add anthropics/skills`). The official distribution story is
plugins-and-marketplaces; skills ride inside plugins.

**Cloudflare's discovery RFC** (<https://github.com/cloudflare/agent-skills-discovery-rfc>)
is the one serious attempt at integrity. Publishers serve
`/.well-known/agent-skills/index.json` with `$schema`
(`https://schemas.agentskills.io/discovery/0.2.0/schema.json`) and a `skills[]`
array of `{name, type: "skill-md"|"archive", description, url, digest}` where
`digest` is `sha256:<64hex>`. Clients **MUST** verify the digest and reject on
mismatch, maintain origin allowlists, reject scripts by default pending explicit
approval, and validate archives for traversal / escaping symlinks / zip bombs.
The Skills CLI already implements the fetch half of this (`WellKnownProvider`,
50 MB / 1000-file archive caps, mandatory root `SKILL.md`).

## 3.2 The seven questions

| | Skills CLI (vercel-labs) | Codex skill-installer | Codex plugins | Claude Code plugins | superpowers | Hermes | skills-lock / skillpm | tech-leads-club |
|---|---|---|---|---|---|---|---|---|
| **1. Unit** | single skill (multi-select from a repo) | single skill | plugin (skills+MCP+hooks) | plugin (skills+agents+hooks+MCP+LSP) | plugin, one git repo each | bundled category tree | single skill | single skill |
| **2. Install** | copy → canonical store, then relative symlink per agent (`--copy` opts out) | download or sparse checkout → `$CODEX_HOME/skills/<n>` | fetch → `cache/<mkt>/<plugin>/<ver>/` | clone marketplace, copy plugin → `cache/<mkt>/<plugin>/<ver>/` | git clone per plugin repo | shipped with the binary | git clone at pinned commit → agent dirs | CDN catalog → copy or symlink |
| **3. Provenance / integrity** | lockfile w/ source+path+folder hash; **hash never verified** | none | `remote_plugin_id`, version dir | marketplace + `version` + `gitCommitSha` + scope | marketplace entry version only | `.bundled_manifest` md5 (tamper detect, not provenance) | `resolved`+`commit`+sha256 tree `integrity`, **verified** under `--frozen` | lockfile + content hash + audit trail |
| **4. Versioning / update** | none; content-hash diff on `update` | none; reinstall | version dirs, `last_updated` | `version` string or commit SHA; `sha` beats `ref` | `version` in catalog, no SHA pin | curator prunes by age | explicit commit pin, held until you ask | lockfile |
| **5. Scoping** | global **or** project (`-g`); per-agent target selection | global only | global; repo-scoped config is an open issue | user / project / local scope in the entry key | global | global | project | both |
| **6. Conflicts** | **silent clobber**, flat name key | **aborts** if dir exists | marketplace-qualified id | marketplace-qualified id, plus `renames` map | n/a (one owner) | n/a | project-local | atomic lockfile |
| **7. Trust** | none, except a hardcoded `openclaw/*` block; advisory Snyk/Socket table, display-only | none | install policy + host sandbox/approval | remote `blocklist.json`; `strictKnownMarketplaces` for orgs | one curator's judgement | none | hash verification only | CI static analysis + Snyk pre-publish + curation |

**Scoping — the expectation was global-only. Refuted, but only just.** The Skills
CLI has a real project scope (`.agents/skills` + `skills-lock.json` in the repo),
Claude Code makes scope part of the installed-plugin identity, and `skills-lock`
is project-first. What none of them have is **session or task scope**, or any
scope that is not a directory on disk. Every one of them answers "which skills are
active" with "look at the filesystem" — so two agents in one checkout always see
the same set. That part of the thesis stands intact.

## 3.3 Registries and the rest

| Thing | What it is |
|---|---|
| [skills.sh](https://skills.sh) | The de facto registry. Search API + install-count leaderboard, fed by CLI telemetry. Install counts are the only quality signal. |
| [agentskills.io](https://agentskills.io) | The spec, plus `skills-ref` validator and the `schemas.agentskills.io` discovery schemas. Not a package registry. |
| [claude-plugins-official](https://github.com/anthropics/claude-plugins-official) | Anthropic's curated plugin marketplace. CI pins each approved plugin to a commit; the pin moves only after re-review. |
| [obra/superpowers-marketplace](https://github.com/obra/superpowers-marketplace) | Seven plugins, one curator. |
| [skills-lock](https://github.com/luisalima/skills-lock) | Manifest in `package.json`, `skills-lock.json` with `{spec, resolved, ref, commit, path, integrity}`. `install --frozen` recomputes and fails on mismatch. **The best integrity model in the ecosystem.** |
| [skillpm](https://github.com/sbroenne/skillpm) | Skills as npm packages: real semver, real lockfile, real registry, thin wiring layer into agent dirs. |
| [tech-leads-club/agent-skills](https://github.com/tech-leads-club/agent-skills) | Curated registry whose pitch is trust: CI static analysis, Snyk pre-publish scanning, no binaries, content hashing, audit log. Claims 13% of marketplace skills carry critical vulnerabilities. |
| crates.io | `skillshub`, `skill-manager`, `agent-skills`, `agent-skills-cli`, `agent-skills-rs`, `oh-my-agent-skills`, `skillset`, `agent-kit`. All small, all reimplementing `skills add`. **None is a capability router; the niche AIKit occupies is empty in Rust.** |
| Directories | Agensi, ClaudeSkills.info, awesomeskills.dev, addyosmani/agent-skills, assorted awesome-lists. Aggregators over GitHub; no install mechanism of their own. |

---

# 4. What we must be compatible with

These are settled. AIKit reads and emits them without argument.

**1. The skill directory shape.** `<name>/SKILL.md`, frontmatter `name` +
`description` required, `name` matching the parent directory, `[a-z0-9-]`, ≤64 and
≤1024 chars, `scripts/` `references/` `assets/` beside it. Unknown frontmatter keys
must be **preserved and ignored**, never stripped — Hermes' `platforms:`, Codex's
`metadata.openai`, and our own `metadata.aikit` all coexist under that rule. Emit
`metadata` sub-keys namespaced (`metadata.aikit.*`); the spec explicitly asks for
this.

**2. The agent skill directories, as *outputs*.** `~/.claude/skills/`,
`~/.agents/skills/`, `~/.codex/skills/`, `.claude/skills/`, `.agents/skills/`,
`~/.hermes/skills/<category>/`. A projection must be indistinguishable from a
hand-made tree, because that is all any host will ever look at.

**3. Both container layouts when indexing.** Flat `skills/<name>/SKILL.md` **and**
catalog `skills/<category>/<name>/SKILL.md`, with the shallower `SKILL.md`
shadowing anything nested beneath it. Hermes is a two-level tree; the Skills CLI
already handles both; an indexer that only walks one level will silently miss 26
categories on this machine.

**4. `~/.agents/.skill-lock.json` (v3) as a provenance source.** Read it. It is the
only record on this machine of where 18 skills came from. Import it as `Foreign`
ownership, `TrustState::Unseen`, mapping `sourceUrl` + `skillPath` → origin and
`skillFolderHash` → the last-seen upstream digest. Emitting it is optional; if we
do, emit v3 exactly, since a lower version number causes the vendor tool to
**discard the entire file**.

**5. `skills-lock.json` semantics for project scope.** `{spec, resolved, ref,
commit, path, integrity}` (luisalima) is the shape the ecosystem is converging on
and is a strict superset of what vercel-labs writes. Our `resolution.lock.toml`
should carry the same facts under our own names so a translation is mechanical.

**6. Claude Code's plugin state.** `installed_plugins.json` v2 and
`known_marketplaces.json` are readable, stable, and describe capabilities already
live on the machine. `installPath` → `cache/<marketplace>/<plugin>/<version>/`
gives us the payload; `gitCommitSha` gives us provenance for free. A plugin's
`skills/` directory is an **observed set** (SPEC-III §1.3), addressable as
`@superpowers` without re-authoring anything.

**7. Codex plugin and marketplace shapes.** `.codex-plugin/plugin.json`
(`skills: "./skills/"`), `.agents/plugins/marketplace.json`, and in `config.toml`
the `[marketplaces.<name>]` / `[plugins."<p>@<m>"]` / `[[skills.config]]` blocks.
`[[skills.config]] path=… enabled=false` is Codex's own per-skill disable and our
Codex projection should write it rather than invent a parallel mechanism.

**8. Symlink-into-a-canonical-store as the wire format.** This is what
`~/.claude/skills` already looks like. Our projections must be able to *coexist*
with those symlinks — recognise a link into `~/.agents/skills` as foreign-owned,
never clobber it, and resolve through it when indexing.

**9. The `.well-known/agent-skills/index.json` discovery format.** `$schema`,
`skills[] = {name, type, description, url, digest}`, `digest = sha256:<64hex>`.
If AIKit ever publishes a registry over HTTP, this is the format — and it is the
only ecosystem format that carries a verifiable digest at all.

---

# 5. What we should not copy

Each of these is a live choice by a shipping tool that we have rejected on
purpose. Contributors should be able to see the divergence is deliberate.

### 5.1 A single global mutable canonical store

`skills add` copies into `~/.agents/skills/<name>` and every agent on the machine
symlinks at it. One store, one name per skill, every session and project sharing
it. Changing what a project sees means mutating what everything else sees.

AIKit: no global mutable active set (ARCHITECTURE §2). Content-addressed
generations per context, `current`/`previous` symlinks flipped atomically.
Two tmux sessions on one repo can carry different skills. We accept a heavier
apply path for that.

### 5.2 Recording a hash and never checking it

The Skills CLI computes `skillFolderHash` at install and uses it **only** to
detect upstream drift later. It never verifies what it just wrote. `skills-lock`
proves this is a choice, not a constraint: `install --frozen` recomputes the tree
hash and fails on mismatch, and the Cloudflare RFC makes verification a MUST.

AIKit: content hashes participate in resolution identity and are verified.
A generation whose payload does not hash to its name is not a generation.

### 5.3 No gate between download and live

Universal across the ecosystem, and PRIOR-ART §Tier 3 already documents the
runtime half. At the distribution layer it is worse: `skills add … -y` puts a
directory of model-executed instructions on disk, symlinked into four agents,
live at the next session start, with the only interruption being a hardcoded
`openclaw/*` deny and a security table that renders after a 3-second timeout and
gates nothing. `git pull` on a repo with `.claude/skills/` is the same event with
no CLI involved at all.

AIKit: inbox → `ready`/`quarantine`/`rejected`, `TrustState`, and
`UnavailableReason::TrustRequired`. An imported capsule is *catalogued* and
*unavailable* until adoption. **Codex has already shipped the mechanism for hooks**
(`[hooks.state] trusted_hash = "sha256:…"`), which is the strongest available
evidence that this is implementable and not paranoid.

### 5.4 Silent clobber on name collision

Two sources with a `pdf` skill: the Skills CLI's second install
`cleanAndCreateDirectory`s the first out of existence and rewrites the lock entry.
No warning, no rename, no record that a swap happened. Codex's `skill-installer`
gets this right by aborting; Claude Code and Codex plugins avoid it structurally
with marketplace-qualified ids.

AIKit: rule 5 — conflicts and export-name collisions **fail visibly by default**,
naming both sides (the Guix behaviour, PRIOR-ART §Guix). Ids are
`<kind>/<group>/<name>` and registry-qualified, so a bare name is never the key.

### 5.5 Discarding provenance on a schema bump

`readSkillLock()` returns an empty lock when `version < CURRENT_VERSION`. A v4
release silently erases every user's install history and every skill becomes
un-updatable.

AIKit: migrate, or refuse and say what is needed. Canonical files are the source
of truth; the SQLite index is rebuildable from them; neither is quietly dropped.

### 5.6 Install counts as the trust signal

skills.sh ranks by install count, which is derived from CLI telemetry, which is
sent on install — so popularity measures installs, not safety, and the vendored
`find-skills` skill instructs agents to treat that number as a quality proxy
("prefer 1K+ installs"). tech-leads-club's claim that 13% of marketplace skills
carry critical vulnerabilities is the counter-evidence.

AIKit: trust is local, explicit and per-capsule. Popularity is at most a search
facet. Frecency ranks **your own** usage and may never make anything active
(SPEC-III §3.4).

### 5.7 Aggregation that launders trust

A superpowers-style plugin is one trust decision covering 14 skills, several
hooks and an MCP server. Accepting the bundle accepts everything inside it,
forever, including whatever the next version adds.

AIKit: SPEC-III §1.2 — a skill-set has no trust of its own and cannot acquire
any. Projecting a set projects only members that pass their own gates, and the set
reports what it withheld and why.

### 5.8 Destructive automatic curation

Hermes' curator archives skills after 90 idle days and prunes builtins on a
7-day timer. Disuse becomes deletion, in the background, unprompted.

AIKit: the same signal, non-destructively. Disuse lowers a frecency rank and may
raise an inbox item; it never removes a capability. Nothing becomes active — or
inactive — without a decision that is recorded and explainable (`aikit explain`).

### 5.9 Silent fallback that changes the mechanism

When `symlink()` fails the Skills CLI copies instead and reports `symlinkFailed`
in a struct the user never sees. The install now has different update semantics —
the canonical store no longer governs it — and nothing on disk says so.

AIKit: `ActivationEffect` is a first-class result. When the Codex adapter cannot
write a per-task `.agents/skills` into a shared tree it reports *which* fallback
it took (ARCHITECTURE §3). "Active in AIKit" must never imply "loaded by every
client".

---

## Sources

Local: `~/.agents/.skill-lock.json`, `~/.agents/skills/`,
`~/.npm/_npx/ac0ed6aa23b37c1e/node_modules/skills/{package.json,README.md,dist/cli.mjs}`,
`~/.claude/skills/`, `~/.claude/plugins/{installed_plugins.json,known_marketplaces.json,blocklist.json,marketplaces/}`,
`~/.codex/{config.toml,skills/.system/,plugins/cache/,.tmp/bundled-marketplaces/}`,
`~/.hermes/{config.yaml,skills/.bundled_manifest,skills/}`.

Web: <https://agentskills.io/specification> ·
<https://github.com/vercel-labs/skills> · <https://skills.sh> ·
<https://code.claude.com/docs/en/plugin-marketplaces> ·
<https://github.com/anthropics/skills> ·
<https://github.com/anthropics/claude-plugins-official> ·
<https://github.com/obra/superpowers-marketplace> ·
<https://github.com/obra/superpowers> ·
<https://learn.chatgpt.com/docs/plugins> ·
<https://github.com/cloudflare/agent-skills-discovery-rfc> ·
<https://github.com/luisalima/skills-lock> ·
<https://github.com/sbroenne/skillpm> ·
<https://github.com/tech-leads-club/agent-skills> ·
<https://vercel.com/changelog/introducing-skills-the-open-agent-skills-ecosystem>
