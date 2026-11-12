# The `.raba` animation format

> **Implemented and frozen at version 1.** This describes what `raba-core` reads and
> writes, verified by the round-trip property test.

RABA has its own animation format and depends on no external program at runtime.

## Why not just use `.dur`

`.dur` is a good authoring format and a poor runtime format for this specific program.

| | `.dur` | what boot needs |
|---|---|---|
| encoding | gzipped JSON | fixed-size records |
| memory | unknown until decompressed | known from the header |
| hostile input | a gzip bomb is an OOM at boot | structurally impossible |
| static logo + spinner | every frame stores every cell | deltas |
| parser | JSON + inflate (a dependency) | ~150 lines, no dependencies |
| stability | changed between v7 and v8 | ours |

The decisive one is the third. A corrupt or hostile `.dur` can decompress to gigabytes.
Early in boot, as root, with no swap, that is an unbootable machine. With a format whose
total size is stated in a fixed header and validated before a single allocation, that
entire bug class cannot exist.

The second-most decisive is the last. Being pinned between Durdraw's format version and
whatever a display manager happens to accept is a maintenance tax we have no reason to
pay.

## Durdraw is still welcome — at authoring time

Nothing here stops anyone using Durdraw. It is a good ASCII animation editor and that is
exactly what someone making a theme needs.

```sh
raba-gen convert art.dur   -o art.raba
raba-gen convert art.ans   -o art.raba      # ANSI art
raba-gen convert art.txt   -o art.raba      # plain text, static
raba-gen build  theme.toml -o theme.raba    # logo + spinner recipe
```

The distinction that matters:

> **Authoring time** may depend on anything. **Run time** depends on nothing.

`raba-gen` is a build tool. It never enters the initramfs and is never on the boot path.
If Durdraw changes its format, a converter breaks and gets fixed at leisure. Nobody's
machine stops booting.

## Design goals

1. **Bounded by construction.** Total memory is derivable from the header. A malformed
   file is rejected before allocation, not after.
2. **Validated once.** Every offset and length is checked at load. After that the
   renderer indexes without further checks.
3. **Small parser.** No compression, no JSON, no dependencies. Small enough to read in
   one sitting and to fuzz exhaustively.
4. **Delta frames.** A spinner over a static logo should cost a few cells per frame, not
   a full canvas.
5. **No clever tricks.** No self-referential offsets, no variable-length integers, no
   compression. Boring formats have boring bugs.

## Sketch

Little-endian throughout. A 32-byte header, a frame table, then cell operations.

```
header, 32 bytes
  0   magic           "RABA"           4
  4   version         u16              2
  6   flags           u16              2    bit 0: loop
  8   columns         u16              2
 10   lines           u16              2
 12   frame_count     u32              4
 16   total_bytes     u32              4    must equal the file length
 20   palette         u8               1    0 = ANSI 16, 1 = xterm-256
 21   reserved        u8 x3            3    must be zero
 24   op_count        u32              4    total across all frames
 28   crc32           u32              4    over everything after the header

frame table, frame_count entries of 8 bytes
  0   delay_ms        u16              2    0 means use the movie frame rate
  2   op_count        u16              2
  4   op_start        u32              4    index into the op array

op, 12 bytes
  0   cell            u32              4    line * columns + column
  4   codepoint       u32              4
  8   fg              u8               1
  9   bg              u8               1
 10   reserved        u16              2    must be zero
```

Ceilings, enforced before any allocation: 16 MiB per file, 1024 columns, 1024 lines,
65535 frames. The op count is bounded by the file size rather than separately.

### Validation order

The order is the design, not an implementation detail:

1. at least 32 bytes
2. magic, then version
3. `total_bytes` against the real file length, and against the 16 MiB ceiling
4. canvas and frame count against their ceilings; reserved fields zero
5. **the layout arithmetic** — do the declared counts add up to the declared size?
6. CRC over the payload
7. *only now* allocate, then check every frame's op range and every op's cell

Steps 1–6 use 32 bytes and some arithmetic in `u64`. Nothing is allocated on a file that
is going to be rejected, which is the property that makes the gzip-bomb class of failure
inexpressible here.

After decoding, every op's `cell` is inside the canvas and every frame's op range is
inside the op array. The renderer relies on this and does not check again — a guarantee
the fuzz target asserts on every input it accepts.

Frame 0 carries a full canvas. Later frames carry only changed cells. `raba-gen` emits a
periodic keyframe so a renderer can start mid-stream.

Sizes are capped in the header's own validation: `columns * lines` and `frame_count` both
have hard ceilings, checked before anything is allocated.

### Why a full byte per colour

The Linux console gives 16 foreground colours and 8 usable background colours — the high
background bit historically drives blink, not brightness. A byte each is more than the
console needs, and it keeps the op at 12 bytes with clean alignment, which matters more
than the two bits it would save. `raba-gen` downsamples 256-colour art at build time so
the runtime never carries a palette it cannot display; `palette = 1` exists for preview
in a capable terminal.

## What this buys

- The runtime parser has no dependencies, which removes the largest item from the
  dependency budget.
- `serde_json` and `flate2` both disappear, shrinking the binary that goes into an
  initramfs.
- The gzip bomb failure mode is gone, not mitigated.
- The format is ours, so an upstream release cannot break a boot.
- A fuzz target over a fixed-layout binary format reaches meaningful coverage quickly.

## What this costs

- A converter to maintain per source format.
- No existing editor writes `.raba` directly. `raba-gen convert` is the answer, and
  `raba --preview` closes the loop.
- One more thing to document and version.

The converters are the honest cost. They are also the cheapest code in the project: pure
functions, fully testable, and a failure there is a build error rather than a boot
failure.
