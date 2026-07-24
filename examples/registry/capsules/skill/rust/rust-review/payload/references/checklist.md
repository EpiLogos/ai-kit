# Rust review checklist

Progressive disclosure: the model reads `SKILL.md` first and opens this file only
when a review is actually underway. Keep each item to something a reviewer can
answer yes/no about a specific line.

## Correctness
- [ ] Slice and index accesses are proven in-bounds or use `get`.
- [ ] Integer arithmetic that can overflow uses `checked_`/`saturating_`/`wrapping_`
      deliberately, not by accident.
- [ ] Empty and single-element collections are handled.
- [ ] `?` propagates the right error type; no silent `let _ =` on a fallible call
      that matters.

## Error handling
- [ ] No `unwrap()`/`expect()` on runtime-fallible values in library paths.
- [ ] Errors are typed and carry context; messages name what failed, not just
      "error".

## Concurrency and unsafe
- [ ] Shared state is behind the right synchronisation; no data races.
- [ ] Every `unsafe` block documents the invariant it relies on.

## API and clarity
- [ ] Public items have doc comments explaining *why*, not restating the code.
- [ ] Names read at the call site; generics and bounds are not gratuitous.
- [ ] The change has a test that would fail without it.
