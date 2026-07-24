# AIKit standards

This is an open-source product, not a demo. These are the standards it is held
to — by maintainers, by contributors, and by any agent writing code in this
repository. They are written as rules because rules can be checked.

---

## 0. The gradient

Every design decision here bends one way:

> **Inference should concresce.** The work an agent does — figuring out a
> command, a sequence, a piece of guidance, a fix — should leave behind
> something named, reusable, composable and dependable. Where two designs are
> otherwise equal, choose the one that produces a durable artefact.

Today that work evaporates at session end. An agent solves the same problem for
the fourth time because the first three solutions were conversation. AIKit exists
to shorten the path from *an agent worked this out* to *a capability the system
has*, and to make that path traversable without a human writing a manifest.

The gradient has exactly one counterweight, and it is not negotiable:

> **...but nothing becomes trusted because it became durable.** Capture is
> automatic; promotion and activation are deliberate. Frequency is evidence of
> usefulness and no evidence at all of soundness.

Every feature should be readable as either *pushing along the gradient* or
*holding the counterweight*. A feature that does neither needs a reason.

---

## 1. What we refuse to ship

Stated plainly, because "no slop" is only useful if it names things.

- **No stub that returns `Ok(())` and is called done.** If it does not do the
  thing, it does not merge.
- **No test that would pass against a stub.** See §2.
- **No `TODO` in merged code without an issue number.** An unnumbered TODO is a
  decision someone declined to make.
- **No scratch files.** `scratch_dbg.rs`, `test2.rs`, `foo_old.rs`, commented-out
  blocks kept "just in case". Git remembers; the working tree should not.
- **No dead configuration.** A setting that is read nowhere, an env var nothing
  consults, a field that is always default. Delete it or wire it.
- **No silent degradation.** If a feature cannot work here, say so — in the
  return value, in the log, in the UI. Never quietly do less and report success.
- **No error message that only makes sense to the author.** An error names what
  failed, where, and what would fix it.
- **No "it works on my machine" integration test.** A test that silently skips
  when a binary is missing must print that it skipped and why. A green suite
  that tested nothing is worse than a red one.
- **No feature without a docs entry.** If it is not in the specs or the README,
  it does not exist and cannot be relied on.
- **No generated prose in comments.** Comments explaining that `i += 1`
  increments `i` are noise. See §4.

---

## 2. Testing

The one rule everything else follows from:

> **A test that would still pass if the implementation were replaced by a stub
> is not a test.**

### Required

- **Real filesystem.** `tempfile`, real directories, real files. Assert on what
  is *on disk* afterwards, not on what the function returned.
- **Real SQLite.** A real database file in a tempdir. Not an in-memory fake of
  our own.
- **Real subprocesses.** Where we shell out, tests shell out. `tmux` tests run
  against a real tmux server on a private socket, torn down in a guard that runs
  on panic.
- **Real end state.** "Given this registry and this overlay, applying produces
  this generation and the previous one is still intact" — not "materialize
  returned Ok".
- **The failure paths.** The atomicity claims (§6 of `ARCHITECTURE.md`) are only
  true if there is a test that interrupts a build and asserts `current` did not
  move. Write that test.

### Banned

- Asserting a mock was called. That tests the mock.
- `assert!(result.is_ok())` as the whole test.
- Snapshot tests with empty or near-empty snapshots.
- Integration tests that skip silently and report green.
- Tests that assert on log output as a proxy for behaviour.

### Test-driven, not test-adjacent

Write the test. **Run it. Watch it fail for the right reason.** Then implement.
A test written after the code passes immediately and proves nothing — you never
saw it catch anything. If you find you wrote the implementation first, delete it
and start from the test. This is not ceremony; it is the only way to know the
test tests something.

### Naming

Test names are sentences about behaviour, in the domain's words:

```rust
fn an_explicitly_disabled_requirement_fails_rather_than_being_silently_re_enabled()
fn a_failed_projection_leaves_the_previous_generation_active()
fn refusal_survives_an_edit()
```

Not `test_resolve_2`, not `it_works`. The suite should read as a specification,
because that is what it is.

---

## 3. Errors

- **Machine codes are a public interface.** `resolution.required_capability_disabled`
  is stable forever. Message text may be reworded freely; codes may not.
- **Codes are namespaced by domain**: `resolution.*`, `manifest.*`, `trust.*`,
  `generation.*`, `projection.*`, `lock.*`.
- **Details are structured, not interpolated.** `.with("capability", id)`,
  `.with("origin", "…/profile.toml:9")` — so a UI can render it and a test can
  assert on it without regex.
- **An error names the fix where one exists.** "required capability X is disabled
  by the session overlay at ~/.aikit/state/sessions/ses_42/overlay.toml:9" tells
  the user what to edit. "resolution failed" does not.
- **No `unwrap()` or `expect()` on runtime data in library code.** Permitted only
  where the invariant is local and provable, with a comment saying why. `panic!`
  on user input is a bug.

---

## 4. Code as the map

The codebase is read by agents at least as often as by people. Both need the
same thing: to find the right file fast and trust what they find.

1. **`lib.rs` is a map.** Module list, a curated re-export surface, and prose
   saying what the crate is responsible for **and what it refuses to do**.
2. **Every module header states the invariant it owns and why that seam exists.**
   Not what the code does — the code says that. Why this boundary is here, what
   would break without it, what the non-obvious decision was. `resolve/mod.rs`,
   `context.rs` and `trust.rs` are the standard to match.
3. **Comment the *why*, never the *what*.** If a line needs explaining, the
   comment explains the decision behind it, or it does not exist. Density should
   match the surrounding code.
4. **One public surface per crate.** Cross-crate consumers use the crate root's
   re-exports. Internals are `pub(crate)` wherever they can be. A reader should
   learn what a crate offers from one screen.
5. **Names are the domain's words.** `capsule`, `capability`, `generation`,
   `projection`, `effective view` — used exactly as `ARCHITECTURE.md` defines
   them, never loosely. If the code and the spec disagree about a word, one of
   them is a bug.

---

## 5. The user-facing surface

- **`--json` on every substantive command**, in the stable envelope. Alternative
  front-ends, editor integrations and agents depend on it.
- **Anything a human can do, an agent can do headless**, through the same
  service — not a parallel code path. The TUI never shells out to `aikit --json`.
- **Anything doable with a mouse is doable with the keyboard, and vice versa.**
  Tested both ways.
- **No colour, Unicode or Nerd Font is load-bearing.** ASCII fallback carries the
  same information, snapshot-tested.
- **Diff before write, always.** Any command that changes something the user did
  not directly type shows what it will do first.
- **Setup discovers, it does not interrogate.** `aikit init` should index what is
  already on the machine and show it. Asking the user twelve questions before the
  first useful output is a failure of design.

---

## 6. Security posture

These are not guidelines; they are the reason the system can be trusted at all.

- **Trust is never self-declared.** Not by a manifest, not by a profile, not by a
  set, not by a registry's presence on disk.
- **Approval is keyed on content; refusal is keyed on identity.** A block that a
  version bump clears is not a block.
- **Catalogued ≠ reviewed.** A registry sync never changes live behaviour.
- **Aggregation never launders trust.** A set projects only members that pass
  their own gates, and reports what it withheld.
- **Secrets are scanned before storage, not before display.** Quarantined content
  never reaches a preview, a log, or a git write.
- **A system failure and a policy denial are never conflated** — in logs, in
  messages, or in a hook's failure policy.
- **Bypasses are scoped, recorded, and visible.** Never an ambient environment
  variable that outlives its reason.
- **Every write outside `~/.aikit/state/` is a Procedure**: planned, isolated,
  reviewable, reversible.

---

## 7. Dependencies

- **Justify each one in the PR.** What it does, why not the standard library,
  what the removal cost would be.
- **No dependency for something we do once.** A 40-line helper beats a crate we
  have to track.
- **Pin what matters, float what does not.** Anything touching correctness or
  security is pinned and reviewed on bump.
- **AIKit installs its own dependencies only through an explicit
  `script/install/*` capsule**, never implicitly during resolution.

---

## 8. Performance

The budgets in `ARCHITECTURE.md` §13 are experience targets, not correctness
assumptions — the system must be *correct* when it misses them and *say so* when
it does. But they are real targets:

| | |
|---|---|
| cold palette first paint | < 150 ms |
| warm palette first paint | < 60 ms |
| search keystroke | < 16 ms |
| typical context resolution | < 50 ms |
| no-op apply | < 50 ms |
| hook dispatcher startup | < 20 ms before capsule work |

The hook dispatcher budget is the strictest and the most important: it is on the
path of every tool call an agent makes. A regression there is a regression in
everything.

---

## 9. Review

A change is ready when:

- [ ] every new behaviour has a test that was watched to fail first
- [ ] `cargo test --workspace` is green and the output is clean
- [ ] `cargo clippy --workspace --all-targets` is clean, or each allow is
      justified in place
- [ ] no scratch files, no unnumbered TODOs, no dead config
- [ ] error codes are stable and namespaced; details are structured
- [ ] the module header explains any new seam
- [ ] the specs are updated if the behaviour they describe changed
- [ ] anything a human can do, an agent can do with `--json`
- [ ] the security posture in §6 is not weakened anywhere

**Reviewers: be hard to please.** The cost of a rejected change is an hour. The
cost of a merged one that quietly does not work is the credibility of a system
whose entire proposition is that you can trust what it tells you.

---

## 10. When these rules and a deadline conflict

Ship less. Never ship it worse.

A smaller system that is honest about its boundaries is a product. A larger one
that is silently wrong in three places is a liability, and no amount of later
work recovers the trust.
