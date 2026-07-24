---
name: unsafe-audit
description: Audit every unsafe block in a Rust change. State each block's safety invariant and demonstrate it holds; flag any that cannot be justified. Use before merging code that adds or edits unsafe.
---

# Unsafe audit

Find every `unsafe` block and `unsafe fn` the change touches. For each one, do
three things and nothing else until they are done:

1. **Name the invariant.** What does this block promise the compiler it has
   checked? (Pointer is valid and aligned; the slice length is correct; the
   lifetime outlives the borrow; the value is initialised; no aliasing of `&mut`.)
2. **Show it holds here.** Point at the concrete lines that establish the
   invariant before the `unsafe` runs — a bounds check, a `NonNull` construction,
   a length computed from the same source.
3. **State the blast radius if it does not.** Use-after-free, out-of-bounds read,
   uninitialised memory, a broken `Send`/`Sync` promise.

If a block cannot be justified in those terms, that is the finding: recommend a
safe alternative (`get`, `split_at`, `MaybeUninit` done correctly) or a comment
that records the invariant so the next reader is not left guessing.

Prefer removing `unsafe` to documenting it. The best audit result is a smaller
`unsafe` surface, not a better-commented one.
