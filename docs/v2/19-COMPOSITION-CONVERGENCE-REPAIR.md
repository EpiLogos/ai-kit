# V2 Composition Convergence Repair Receipt

Status: integration repair receipt for #42 / #44.

## What regressed

PR #78 established the accepted shared Profile/SkillSet application seam. The later SessionSpace branch contained commit `c5440c74190d6bba074c7dca2b74c8574ba36624` solely to remove borrowed #78 diffs and keep that branch independent while #78 was still converging. When the independent SessionSpace tranche was subsequently merged after #78, that branch-local cleanup also removed the already-accepted #78 application modules, tests and parity documentation from live `main`.

This was an integration-order regression, not a new semantic decision to remove the composition application contract.

## Repair law

The repair is the exact inverse of that branch-local cleanup against the current production tree, with one intentional coexistence resolution in `aikit-tui::ApplicationService`:

- retain all newer SessionSpace and Knowledge application operations;
- restore the accepted #78 `application_context`, `composition_workspace`, store-owned composition apply seam and conformance tests;
- restore shared `changed_ground` disclosure;
- bind accepted composition previews to the before and projected-after resolution basis;
- re-preview immediately before apply and reject stale accepted state with `composition.preview_stale`;
- do not create a new ContextSource selector writer or take over the still-open #42 ownership frontier.

The repair also keeps the current production Knowledge address/history code intact. A small Rust-stable clippy cleanup in that already-merged store code is mechanical and does not change Knowledge semantics.

## Verification

Before publication of the helper-free repair head, the self-cleaning repair runner proved on current code:

- `aikit-core` Profile composition conformance;
- `aikit-store` composition application preview/apply/stale-law tests;
- final TUI stale-preview and domain-parity tests;
- the complete current `aikit-cli` all-target suite, including Knowledge and SessionSpace application tests;
- workspace-wide strict clippy on current stable Rust.

The normal repository CI on the human-authored receipt head remains the merge authority.

## Ownership after repair

This repair restores accepted groundwork only. #42 remains the owner of its explicitly unfinished Project→SkillSet durable-selection and ContextSource authored-selector semantics. #44 remains a read-only Explain/History consumer and must not become a mutation authority merely because the restored evidence is now available again.
