## What this changes

<!-- and why -->

## Checklist

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets` is clean
- [ ] If it touches the parser: a fixture was added to `tests/fixtures/`
- [ ] If it touches rendering: golden tests updated, and the diff was inspected
- [ ] If it touches the VT layer: tested in a VM, including the `SIGKILL` path
- [ ] If it adds a dependency to `raba`: `DEPENDENCIES.md` has a row, size gate passes

## The invariant

Every error path in `raba` exits 0 and restores the terminal. If this change adds a
path that can panic, hang, or leave a terminal in `KD_GRAPHICS`, say so here.
