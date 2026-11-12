# Dependencies

Every direct dependency of `raba` needs an entry here before it is added. This is the
budget from [docs/maintenance.md](docs/maintenance.md) made concrete.

The two binaries have opposite constraints and separate budgets.

## `raba` — the boot binary

**Budget: 1 direct dependency.** It runs as root, early in boot, inside an initramfs. Every
crate here is code that can break a boot on someone else's release schedule.

| crate | what for | why not `std` | removable? |
|---|---|---|---|
| `raba-core` | format, layout, rendering | ours | no — it is the program |

Not here, deliberately:

- **no JSON parser** — `.raba` is a fixed binary layout. This is the main reason the
  format is ours.
- **no decompression** — the gzip-bomb failure mode cannot be expressed.
- **no async runtime** — a frame loop is a loop. Banned in `deny.toml`.
- **`rustix`** will be added at milestone 3 for ioctls and termios, in preference to
  `libc` because it avoids a C dependency and suits a static musl build.

## `raba-core` — the pure core

**Budget: 0.** Pure data transformation, no I/O, no syscalls. If something here needs a
dependency, it is probably not pure and belongs in `raba`.

## `raba-gen` — the build tool

**No budget.** It never enters an initramfs and is never on the boot path. A crate that
breaks here is a build failure someone fixes at leisure, not an unbootable machine.

Expected: `toml`, `serde`, `flate2` and a JSON parser for `.dur` import, `anyhow`.

## Dev-dependencies

**Free.** `proptest`, `cargo-fuzz`, pty helpers — none of it ships.

## Adding one

1. Add a row above with a real answer in every column.
2. State what breaks if it is removed.
3. For `raba`, say why the standard library is not enough.
4. `cargo deny check` must pass.
5. Check the binary size gate.
