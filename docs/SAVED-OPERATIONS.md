# Saved operations: the stable fields a device or agent can bind to

A saved operation is a capsule of kind `script` or `alias`, reached by capturing an
example, reviewing it and promoting it. There is no second store and no per-instrument
copy: the Search device, the Terminal, the Capture skill device and agents all read and
write through the commands below. Everything is a field of an owner command's `--json`
reply. Additions are additive; no existing field is renamed or removed.

Design: [SPEC-II §2](SPEC-II-PROCEDURES-AND-INBOX.md) (capture, the inbox, the recurrence
observer), [SPEC-III §3](SPEC-III-SKILLSETS-AND-FRECENCY.md) (use is recorded) and
[ADR 0006](adr/0006-search-aliases-are-capsules.md) (search aliases).

## `aikit capture`

Reply (the Capture skill device already reads `candidate`, `kind`, `state`, `quarantined`,
`findings`, `duplicate_of`, `similar`). Added: `confidence` (`declared|high|medium|low`),
`kind_reason`, `exports` [string], `scope` (`home|project|session`), `source_ref`, `by`,
`session`, `proposed_id` (what accepting would save it as), `inbox_item` (the message it
put on the inbox), `proposal` (the planned Procedure; null when held back).
Flags: `--kind`, `--export` (repeatable), `--scope`, `--source-ref`, `--by`.
Prose is not filed as a script: it is `guidance` at `confidence: low`, which a UI should
show as a question.

## `aikit promote` and `aikit inbox`

`promote` reply: `id, root, manifest` plus `kind`, `scope`, `exports`. Without `--id` it
proposes `<kind>/<personal|project>/<name>`. It refuses a candidate already saved
(`inbox.already_promoted`) or rejected (`inbox.already_rejected`).

`inbox [--all]` rows keep `id, kind, title, body, state, project, evidence, proposal,
created`. A `capture-candidate` row's evidence is `[{evidence:"hash",label:"candidate"},
{evidence:"hash",label:"proposed-id"}, {evidence:"file",path}, {evidence:"summary",text}]`
and its `proposal` is the planned Procedure.
`inbox accept <item|candidate>` returns `{candidate, item, id, kind, exports, procedure,
applied, undo}`; `inbox reject <item|candidate> [--reason]` returns `{candidate, state,
reason}`. Both stores resolve together.

## `aikit observe run | status`

Feed contract: one run by flags (`--command --cwd --by --at --session --exit-code --pane`), many
as JSON lines on stdin (`--stdin-jsonl`: `command, cwd, by, at, session, exit_code, pane|panes`),
or one harness event (`--from-hook`). `--threshold N` (3), `--window SPAN` (7d).
Reply: `{runs:[{command, by, recorded, skipped, detail, count, threshold, window_seconds,
offered:{candidate, inbox_item, proposal, new}|null, panes}], recorded, skipped, offered, ignored}`.
A run names the terminal panes it ran in as `tmux:%3@<socket>,<server pid>`,
`herdr:w1:p1@<socket>` or `cmux:surface:20`; a hook names them itself from `TMUX_PANE`/`TMUX`,
`HERDR_PANE_ID`/`HERDR_SOCKET_PATH` and `CMUX_SURFACE_ID`. A secret-free run is kept for its
panes even when it does not count (a staple, a failed run), and a working Surface's capture
reads that pane history as `observed` (`session-space working-surface capture`).
`skipped` is one of `empty, too-long, failed-run, noise, secret, decided`; a command holding
a secret is never stored. `status` returns `{repeating:[{command, by, count, first, last,
offered, waiting}], count, window_seconds}`.
A Terminal pane or Factory frame only has to push JSON lines. Attribution is the feed's:
`human` for typed runs, an agent ref for agents; an unattributed run is counted as `unknown`.

## `aikit alias define | expand`

`alias define '@name = expression' [--replace --description --by]` returns `{id:"alias/personal/<name>",
alias, expression, takes, manifest, replaced, try}`. `alias expand '<query>'` returns
`{input, expanded, aliases}`. `aikit search` expands aliases itself and adds
`alias_expansion:{input, expanded, aliases}` only when one was used.

## `aikit saved [--kind script|alias] [--limit N]`

The one reading of what is saved; it stores nothing.
`{saved:[{id, kind, syntax, name, command, exports, description, registry, scope, manifest,
revision, maturity, trust, reviewed, active, by, saved_at, origin, source_ref, session,
candidate, use:{successful_runs, failed_runs, last_success_seconds_ago}}],
waiting:[{candidate, kind, title, state, confidence, exports, by, source_ref, scope,
proposed_id, accept}], summary:{saved, scripts, aliases:{shell, operative}, ever_used,
never_used, waiting}, standing}`. Most used first.

## Where a saved operation shows

In the browser and on Search's `@5` horizon (rows from `aikit saved`), not in the skillset
rack: scripts and aliases are not practice kinds. `aikit act discover` lists Actions only,
by design; a saved script is found by `search`, `z` and `saved`, and acted on through
`act describe|invoke <capsule id>`.

## Existing readings that now tell the truth

`recent` and `failures` show `run` events (success, failure with `exit-N`, denied with
`trust.required`, skipped for a background start). `stats` has `by_kind`/`by_source` and a
real `ever_used`; `unused` lists; `z` learns (`candidates[].successful_runs`).

## Not offered yet

Capturing an operative alias through the inbox (`--syntax operative`); aliases are defined
directly with `alias define`. The Terminal pane and Factory frames have no producer for the
`--stdin-jsonl` feed yet.
