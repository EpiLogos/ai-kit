## Outcome

<!-- What becomes true for the user? -->

## Boundary and reversal

<!-- Which state or authority boundary changes? How does failure/undo behave? -->

## Verification

- [ ] Real behaviour is covered by tests
- [ ] `cargo test --locked --workspace --all-targets --no-fail-fast`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo build --locked --workspace --release`
- [ ] `git diff --check`

## Known limitations

<!-- Name any unavailable platform capability or deliberately deferred work. -->
