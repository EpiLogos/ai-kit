---
name: rust-review
description: Review Rust changes for correctness, error handling, unsafe usage, and public API surface. Use when reviewing a diff, a pull request, or a module before merge.
---

# Rust review

You are reviewing a Rust change. Read the diff in full before commenting, then
work the checklist in `references/checklist.md` in order. Prefer one precise
comment on the load-bearing line over a scatter of style notes.

## How to report

For each finding, name the file and line, state the concrete failure (an input
and the wrong output or the panic), and propose the smallest fix. If you find
nothing that would fail, say so plainly rather than inventing nitpicks.

## What matters most, in order

1. **Correctness under the inputs that actually occur** — off-by-one, empty
   collections, integer overflow, and the error path, not just the happy path.
2. **Error handling** — no `unwrap()`/`expect()` on a value that can fail at
   runtime; errors carry enough context to act on.
3. **`unsafe`** — every `unsafe` block states the invariant it upholds and why it
   holds here. Hand off to the `unsafe-audit` skill for anything non-trivial.
4. **Public API** — names, generics and trait bounds a caller will live with;
   breaking changes are called out explicitly.
