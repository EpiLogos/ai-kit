---
name: harness-plug-in
description: "METHOD: Plug an installed technology (an agent harness) into the platform's capability account — confirm the declared gap, census the harness from its own surface with bounded probes, author a truthful actuation.harness-capability/v1 descriptor, validate it, and mint a contribution receipt. From 'it is installed here' to 'receipt minted' in minutes, with zero repository exploration."
---

# Plugging a harness into the platform

Use this Skill when an agent harness (claude-code, codex, gemini, openclaw,
...) is installed here and the platform's capability catalog declares only a
**gap** for it. The work: account for what that harness accepts — lifecycle
events, context injection, blocking, wake, install seams, model dispatch —
by observing the harness itself, never by reading platform source code.

## What you can extend today

Two artifacts have intake faces:

```text
harness capability descriptor   what a harness accepts; JSON, schema
                                actuation.harness-capability/v1. Lands in the
                                Actuation catalog (catalog/targets.json);
                                you contribute, the owner lands.
harness profile                 how AIKit composes for a harness; TOML,
                                aikit.harness-profile/v1. AIKit-side; not
                                part of the catalog.
```

The whole loop, one line:

```text
author → validate → receipt → owner lands → rediscovery shows the account
```

Concretely:

1. **Identify the seam.** `actuation harness capability <slug>` — a declared
   capability gap means the slug is open for contribution; a full descriptor
   means the capability is already declared and correcting it is an owner
   edit, not a contribution.
2. **Census the harness from its own surface** (§ Probe discipline): version,
   help text, config structure, at most one bounded session-level probe.
3. **Author the descriptor** (§ Grammar) — every declared fact cites its source.
4. **Validate**: `actuation harness capability validate <file>` until it exits 0.
5. **Mint the receipt**: `actuation config-contribution capability <file>`.
6. **Stop.** The receipt states the exact landing edit. Landing is the
   Actuation owner's edit to the catalog; the next catalog revision shows
   your account at `actuation harness capability <slug>`. Never edit the catalog.

Useful read models: `actuation harness detect` (what is detected here),
`actuation harness capability` (declared list + catalog revision),
`actuation harness capability <slug> --json` (one account, machine readable).

## The descriptor grammar (one page)

Top level: `schema` = `actuation.harness-capability/v1`, `document` =
`capability`, `harness_slug` (must name a detection descriptor in the shipped
catalog), optional one-line `summary`.

Every section below is REQUIRED unless marked optional: the document must
carry them all. There is no way to omit a section you could not observe — the
validator refuses a missing section with a generic `expected a record
object`. The honest form for "unobserved" is the least-committal closed value
(`none`, an empty list) plus a note saying what was probed and what remains
unobserved. Only `model_dispatch` may be left out entirely.

`native_events[]` — one entry per lifecycle event the harness offers. Fields:

```text
event            closed vocabulary: session-start | user-prompt-submit |
                 pre-tool-use | post-tool-use | stop | session-end |
                 pre-compact | notification | custom
                 Known kinds are unique (declare each once). An event that
                 fits no known kind is `custom`; keep the harness's own name
                 in native_name and say what it does in notes.
native_name      the harness's own event name (required)
transport        how the event reaches a program: e.g. "settings-json-hooks-map"
can_block        boolean (required)
context_channel  closed vocabulary: stdout-additional-context |
                 stdout-plain-text | exit-code-payload | none
notes            optional; put version-specific behaviour here
```

`injection_channel` — how additional context actually travels:
`kind` (same closed vocabulary as context_channel) + `mechanism` (required:
name the exact field/pipe) + optional `notes`. No channel observed →
`kind: "none"` and say so.

`blocking_semantics` — `kind`: `deny-and-block` | `advisory-only` | `none`,
plus optional `notes`. Take the value the observed surface supports and cite
it in provenance; notes carry what was not observable.

`wake_capability` — `kind`: `immediate-wake` | `next-event` | `none`, plus
optional `notes`. `immediate-wake` REQUIRES notes naming the listener that
makes it true. No wake surface observed → `none`.

`install_seam` / `uninstall_seam` — identical shape:

```text
config_path                 where dispatch entries live (user/project paths)
format                      closed vocabulary: json | jsonc | toml | skill-tree
entry_shape                 what one entry looks like
ownership_marker            how to recognise our entries in that file
preserves_foreign_entries   must be exactly true — seams are reversible
```

`model_dispatch` (optional) — `kind`: `native-provider-binding` | `none`.
With a binding: `providers[]`, each `{provider_ref, selector: {kind:
config-key | cli-flag | env-var, name}, credential: {required: bool, hint}}`
(`hint` required exactly when `required` is true). No evidenced binding →
`kind: "none"` and no providers.

`provenance` — `authored_by` (who/what observed this, with date) and
`source_refs[]` (non-empty; § Probe discipline defines the classes).

## Probe discipline

Census the harness from its own surface, cheap-first:

```text
1. --version                      pin the exact version you observed
2. --help, subcommand --help      enumerate the real surface (hooks? config?
                                  mcp? flags?) — never from brand memory
3. config presence                the config file/directory exists; read
                                  STRUCTURE and KEY NAMES, never secret values
4. session-level probe            at most one; bounded (below); only if 1-3
                                  leave a question a cheap probe can answer
```

Hard rules:

- **Never let a probe hang.** Give every invocation a hard timeout — 10s is
  the default budget: `timeout 10 <cmd>` where a timeout binary exists, or
  `perl -e 'alarm shift; exec @ARGV' 10 <cmd>` on a stock Mac. A probe that
  outlives its timeout taught you one fact: the surface did not answer in
  time. If a legitimate help/read command times out, ONE escalation retry at
  a larger budget (30s) is legitimate — record both outcomes. Never infer
  absence from a timeout.
- **Never start a long-running server, daemon or REPL as a probe.** One-shot,
  exits-by-itself commands only (`--version`, `--help`, `config get`, a
  `--print`-style headless run). If the only way to answer a question is to
  keep a process alive, that question stays open — say so.
- **Never read secret contents.** API keys, tokens, passwords: presence of the
  config key is a fact; its value is not yours.
- **Credential-gated means declare the gate.** A surface that stops at an
  auth wall is `credential-gated`: name the condition in notes. Do not work
  around it, do not guess past it.

Every probe outcome maps to the shared vocabulary, and each outcome licenses
different declarations:

```text
ok                observed working (cite the command in source_refs)
                  → licenses positive declared facts about this machine's surface
credential-gated  present but behind auth (name the condition)
                  → licenses declaring the gate and anything upstream docs
                    add, marked as upstream
unreachable       present but would not answer (port closed, service down)
unsupported       the feature does not exist on this surface
                  → licenses declared absence (injection_channel none,
                    wake none, model_dispatch none) — absence is truth
timed-out         hit the probe budget; you did NOT observe absence
                  → licenses only "unobserved"; the fact stays open
refused           the tool refused (non-zero, error); record the refusal text
                  → licenses only "unobserved" plus the refusal itself
```

`timed-out`/`refused`/`credential-gated` never justify declaring absence —
only `unsupported` (positive evidence of absence) does.

Every declared fact cites its source in `provenance.source_refs`, prefixed by
class:

```text
local:<cmd or file>    live observation on this machine, this version line
detection:<receipt>    the platform's own detection record for this harness
upstream:<repo/docs>   the harness's shipped documentation or schema for the
                       version line — states the documented behaviour, and
                       notes must say which facts rest on it
```

`local` licenses what you saw. `upstream` licenses documented behaviour of the
named version line, always marked. Brand similarity ("it works like X")
licenses nothing. When live probes cannot settle a documented behaviour, the
harness's shipped documentation for the installed version line is the
licensed source: cite `upstream:` and name in notes which facts rest on it
(the gemini specimen below settles hook semantics exactly this way). No web
access in your environment? Say so in provenance and declare only what the
local surface showed.

## Validate-first loop

```bash
actuation harness capability validate my-descriptor.json   # or: ... validate - < file
```

Exit codes: `0` valid; `1` refused with named checks on stdout; `2` handler
refusal (unreadable or non-JSON input). Three checks run in order:

```text
schema-admission   the closed vocabularies, reversible seams, wake honesty,
                   provenance — diagnostics name the exact field and the
                   accepted values ("expected one of json, jsonc, toml, ...")
slug-alignment     harness_slug names a detection descriptor in the catalog
coverage-closure   the slug currently carries a declared capability gap
```

A refused specimen is progress: the diagnostic names the field and the
vocabulary; fix that field and re-run. Two diagnostic shapes exist: value
errors name the accepted set (`expected one of json, jsonc, toml, ...`);
`expected a record object` / `expected an array` mean a required section is
missing or has the wrong JSON type — audit your document against the grammar
table, top to bottom. Typical value refusals: an event outside the vocabulary,
a seam format outside {json, jsonc, toml, skill-tree},
`preserves_foreign_entries` not true, `immediate-wake` without notes, or
missing provenance.

For the AIKit-side artifact: `aikit harness-profile validate <file.toml>`
answers `admit` or `refuse` with named, coded diagnostics (refusal exits
non-zero). Same loop: read, fix, re-run.

## Worked example: gemini (condensed from the real case)

1. **Gap.** `actuation harness capability gemini` answered: declared
   capability gap ("AIKit admissions ... no observed event/blocking grammar").
2. **Census.** `gemini --version` → 0.29.5. `gemini --help` → hooks
   subcommand, `--approval-mode`, `--experimental-acp`. `~/.gemini/settings.json`
   present (structure read, values not). `NO_BROWSER=1 gemini --list-sessions`
   → interactive OAuth prompt: **credential-gated**, declared, not worked
   around. A scratch-project hook probe did not fire — the auth wall precedes
   session start — so event semantics rest on the upstream hooks reference,
   marked `upstream:`.
3. **Author.** Gemini's event names mapped by function onto the boundary
   vocabulary: BeforeTool → `pre-tool-use`, AfterTool → `post-tool-use`,
   PreCompress → `pre-compact`, the rest → `custom` (native_name keeps the
   harness's names).
4. **Validate → receipt.** `config-contribution capability` minted
   `capability-contribution:gemini:f1af4031414a`, stating the landing edit.
5. **Landed.** The owner applied the edit verbatim as catalog r15;
   `actuation harness capability gemini` now returns the full account.
   Re-running intake on the landed specimen now refuses coverage-closure —
   the same law working.

## Honesty rules

- **Unsupported stays unsupported.** A surface observed absent is declared
  absent (`none`) and named in notes. Never invent grammar to fill a field.
- **Declared absence is truth.** `injection_channel: none`, `wake_capability:
  none`, `model_dispatch: kind none` are first-class facts, not failures —
  with notes carrying what was probed and what stays unobserved.
- **Brand similarity proves nothing.** "It is Claude-Code-like" is not
  evidence. Read this tool's `--help`, config, and shipped docs for the
  installed version.
- **Upstream-doc facts are marked as such** (`upstream:` prefix, and notes
  naming what rests on them), especially where a credential gate blocked live
  observation.
- A descriptor is corrected against the real surface, never defended: if your
  own probe disproves one of your fields, fix the field and say so in notes.
