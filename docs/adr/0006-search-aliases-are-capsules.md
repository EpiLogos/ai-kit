# ADR 0006 — Search aliases are capsules (`alias` with `syntax = "operative"`)

Status: accepted. Implemented 2026-10-18 (ai-kit#513: `aikit alias define|expand`,
expansion in `resource/operative.rs`).

## Context

The device manual's Search aliases (`@name = search`, with `?` as a gap) had no owner.
The operative grammar's `=` is already the Express relation and `@name` already lexes
as an ordinary word, so `@gs = x` parses as a valid expression: a definition cannot be
told from a query by the parser.

## Decision

1. **A definition is a statement made on purpose**, `aikit alias define '@name =
   expression'`, never inferred from a query.
2. **Expansion is one step on the query text before it is parsed**, in
   `resource/operative.rs` (the grammar's owner), so the CLI, agents and the Search
   device share it. A query that names no alias is passed on untouched; quoted words
   are never expanded; a body that combines operands keeps its grouping among others.
   A lone `?` takes the next word; aliases nest to depth eight; a loop is refused
   naming the chain, at definition and at expansion.
3. **Storage is the capsule envelope.** `alias` gains `syntax = "shell" | "operative"`
   (shell by default, so every alias written before keeps its meaning), identity
   `alias/personal/<name>`. Discovery, provenance, scope and the personal registry are
   not duplicated.

## Consequences

- Three runtime seams: an operative alias exports no command (no shim); `aikit run`
  refuses it without asking for review (it is inert text); `aikit z` opens it.
- Expansion reads the whole catalogue, active or not. A name a person gave a query
  should not vanish from search because a profile changed.
- `aikit search` reports `alias_expansion {input, expanded, aliases}` when one was used.
- The shell alias runtime (`plan_alias`) runs a one-line body followed by the caller's
  words, in the directory it was typed in. An alias that must change the caller's own
  shell belongs in `aikit shell init`.
