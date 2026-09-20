# Authoring harness profiles

A harness profile is one declarative `aikit.harness-profile/v1` document that
says how AIKit handles one harness: which layers it has (skills, guidance,
hooks, tools, models, sessions, settings), who owns each layer, and how AIKit
may observe, project into, and disclose it. AIKit carries an embedded profile
for every harness its censuses have evidenced; this document is the public
route for everything else.

## The intake

```sh
aikit harness-profile validate my-harness.toml   # field-by-field check
aikit harness-profile show claude-code           # an embedded document, exactly as shipped
aikit harness-profile list                       # what resolves + what failed to load
aikit harness-profile register my-harness.toml   # validate + install into the AIKit home
```

- **validate** checks a document against the schema and reports the first
  failure with the field that caused it. It never writes anything.
- **show** prints an embedded document verbatim — the published grammar by
  example — or summarizes a registered external one.
- **register** copies a validated document into
  `$AIKIT_HOME/harness-profiles/<slug>.toml` (without `AIKIT_HOME`,
  `~/.aikit/harness-profiles/`). The profile registry reads that directory at
  process start, so commands launched afterwards resolve the document wherever
  a profile joins by slug — posture gates, model key delivery, launch argv
  joins, the settings disclosure.
- **list** discloses every profile that resolves (embedded and external) and
  every external document that failed to load, with the reason. A broken
  document is never silently dropped.

## Rules

1. **Embedded profiles are never overridden.** A document whose `slug`
   collides with an embedded profile is refused at `register` and reported as
   a load problem at read time. An external document earns its place under a
   new slug.
2. **Unknown fields are errors.** Every layer uses `deny_unknown_fields`: a
   typo is a validation failure, not a silently ignored key.
3. **Declare only what you evidenced.** An absent layer says nothing; a
   posture you cannot honour (`managed` means AIKit projects and retracts the
   layer) must not be claimed. The authoring Skill
   (`skill/aikit/harness-adapter-authoring`) and the SDK contract
   (`docs/v2/HARNESS-ADMISSION-AND-ADAPTER-SDK.md`) govern the census behind
   a document.
4. **Machine-specific paths are home-relative** (`~/...`), so a document
   survives machines.

## The grammar

Top level:

| field | type | notes |
|---|---|---|
| `schema` | string | exactly `"aikit.harness-profile/v1"` |
| `slug` | string | the Actuation catalog slug this document is joined by |
| `edition` | enum | `cli`, `desktop`, `ide`, `hosted`, `embedded`, `custom` |
| `<layer>` | table | zero or more of the layers below |

Each layer is optional and carries a `posture` plus layer-specific fields:

- `posture`: `managed` (AIKit projects and retracts), `brokered` (composes
  through disclosure only), or `observed` (detection and UI; writes nothing).
- `presence`: `executables` (argv programs that join to this profile) and
  `config-dir`.
- `skills`: observe `paths` and, when managed, a projection `prefix`.
- `guidance`: observed instruction files.
- `hooks`: when managed, the `format` grammar of the projection (for example
  `claude-hook-map`, `zcode-hook-wrapper`, `pi-extensions-record`) and the
  merge behaviour; a harness with no hook faculty records none rather than
  inventing a config key.
- `tools`: how tool-protocol capsules (MCP servers) project, when supported.
- `models`: `none` with a reason, or `dispatch` declarations plus per-provider
  `key-delivery` records (an env var the native launch reads, or the
  own-login fact).
- `sessions`: `protocol` (`process`, `acp`, …) and `open-modes`.
- `settings`: harness-owned `trust_settings` disclosures for the
  configuration plane.

The authoritative field set is the schema in this repository
(`crates/aikit-core/src/harness_profile.rs`); `aikit harness-profile show`
prints a complete document that validates against it.

## A worked specimen

The first external specimen this intake admitted was authored from public
surface only (the authoring Skill, opencode's own docs and `--help`, and the
published grammar above). Its original form failed validation because the
grammar was not published; this is the corrected document, which `validate`
accepts:

```toml
schema = "aikit.harness-profile/v1"
slug = "opencode"
edition = "cli"

[presence]
executables = ["opencode"]
config-dir = "~/.config/opencode"

[skills]
posture = "observed"
observe = { paths = ["~/.config/opencode/skill", "~/.config/opencode/skills"] }

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["attach"]
```

Registering it under that slug makes `aikit` resolve the document wherever a
profile joins by slug; teaching AIKit to *project* into opencode's skill
directories remains adapter work under the SDK contract, not a profile
document's job.
