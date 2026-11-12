# The `.raba` animation format

> Design draft. Not implemented. Byte layout is not yet frozen.

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
header
  0   magic           "RABA"           4
  4   format_version  u16              2
  6   flags           u16              2    bit0: loop
  8   columns         u16              2
 10   lines           u16              2
 12   frame_count     u32              4
 16   total_bytes     u32              4    must equal file size
 20   palette         u8               1    0 = 16 colour, 1 = 256 colour
 21   reserved        u8 x3            3
 24   ops_offset      u32              4
 28   crc32           u32              4    over everything after the header

frame table, frame_count entries of 8 bytes
  0   delay_ms        u16
  2   op_count        u16
  4   op_index        u32              index into the op array

op, 8 bytes
  0   cell            u32              row * columns + column
  4   codepoint       u24                   (plus 1 byte of attributes)
  7   fg : 4, bg : 4  u8
```

Frame 0 carries a full canvas. Later frames carry only changed cells. `raba-gen` emits a
periodic keyframe so a renderer can start mid-stream.

Sizes are capped in the header's own validation: `columns * lines` and `frame_count` both
have hard ceilings, checked before anything is allocated.

### Why 4-bit colour in the op

The Linux console gives 16 foreground colours and 8 usable background colours — the high
background bit historically drives blink, not brightness. A 256-colour theme is
downsampled by `raba-gen` at build time, so the runtime never carries a palette it cannot
display.

A `palette = 1` variant with wider colour fields exists for terminals that can do more.
The default build target is the console, so the default is the narrow one.

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
