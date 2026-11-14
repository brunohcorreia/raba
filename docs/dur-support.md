# Importing `.dur`

RABA's runtime format is `.raba` — see [format.md](format.md). `.dur` is an **import
format**, converted at build time by `raba-gen`:

```sh
raba-gen convert art.dur -o art.raba
```

Nothing on the boot path parses `.dur`. This page documents what the converter accepts.

[Durdraw](https://github.com/cmang/durdraw) `.dur` files are gzipped JSON holding frames
of text and a colour map. The authoritative specification is `durformat.md` in the
Durdraw repository.

## Supported

| | RABA | Ly |
|---|---|---|
| `formatVersion` 7 | yes | yes |
| `formatVersion` 8 | yes | **no** |
| `colorFormat` `"16"` | yes | yes |
| `colorFormat` `"256"` | yes | yes |
| `colorFormat` `"RGB"`, `"mIRC"` | no | no |
| `encoding` `"utf-8"` | yes | yes |
| `encoding` `"cp437"` | planned | no |
| per-frame `delay` | yes | partial |
| global `framerate` | yes | yes |

Both versions convert correctly. The version only matters if you also want the `.dur`
itself to be readable by something else — Ly, for instance, accepts only version 7.

## The version 7 / 8 trap

Version 8 (Durdraw 0.30.0, August 2026) changed two things:

1. `sizeX` / `sizeY` were renamed to `columns` / `lines`.
2. The colour map orientation was fixed.

The second is the one that bites. In version 7 the colour map is **transposed** relative
to the contents:

```
v7:  contents[line][column]  ->  colorMap[column][line]
v8:  contents[line][column]  ->  colorMap[line][column]
```

A parser that ignores `formatVersion` renders scrambled colours on one of the two.
RABA checks the version and transposes as needed.

## Palette

In `colorFormat` `"256"`, indices 0–15 are the ANSI colours and 16–255 are the standard
xterm-256color palette:

| | | | |
|---|---|---|---|
| 0 black | 4 red | 8 grey | 12 bright red |
| 1 blue | 5 purple | 9 bright blue | 13 bright purple |
| 2 green | 6 yellow/brown | 10 bright green | 14 bright yellow |
| 3 cyan | 7 white | 11 bright cyan | 15 bright white |

In `colorFormat` `"16"` the same colours are indexed from **1** rather than 0.

Whether a 256-colour theme actually displays in 256 colours depends on the console.
Most Linux virtual consoles are limited to 16. Design for 16 and treat anything beyond
as a bonus.

## Glyphs

The format permits any UTF-8. The Linux console does not.

The console holds 256 or 512 glyphs, inherited from VGA hardware. Characters outside
the loaded font are replaced and the original codepoint is discarded by the kernel.

Safe is ASCII, the CP437 block elements and box drawing. Braille, the quadrant blocks
and emoji are not. Accented Latin and CP437's Greek and mathematics are a third case —
they work until `setfont` runs.

**[glyphs.md](glyphs.md) has the rule and the reasoning.** Importing a `.dur` runs the
same validator as building a theme from scratch, because the console does not care where
the art came from.

```sh
raba-gen --check theme.dur
```

## Size

`.dur` stores every cell of every frame, including the colour map, with no inter-frame
compression. An 80x30 canvas over 10 frames is roughly 130 KB of JSON.

Gzip handles this well on disk — frames of a mostly-static logo are nearly identical,
so the file usually lands under 5 KB. Memory at boot is the real cost, since frames are
decompressed and parsed.

Keep the canvas to the bounding box of the art rather than padding it to the terminal
size. RABA centres it at display time, so padding buys nothing and multiplies across
every frame. A 20x22 canvas instead of 80x30 is roughly a quarter of the cost.

### A canvas bigger than the console

`raba-gen convert` warns when the canvas is larger than 80x25, the size `raba`(8)
assumes when it cannot read the console, and says which of two things will happen.

If the art is drawn only in the shading ramp ` .·░▒▓█` — which `video-to-dur` output
is, including `/etc/ly/blackhole.dur` at 240x67 — it is **resampled** to the console,
keeping its proportions. Nothing has to be declared: the property is computed from the
movie's own cells at load time. Anything else is **centred and cropped**, because
halving a box-drawing line does not give you half a line.

So a full-screen recording does not need re-authoring per console size, and a logo is
small enough not to need it either. The case that still wants attention is large art
in a mixed alphabet — one stray letter in a shading picture disqualifies the whole
movie from resampling. [console-size.md](console-size.md) has the details.

## Timing

`framerate` is the global playback rate in frames per second. A frame's `delay`, in
seconds, overrides it for that frame; `0` means use the global rate.

RABA's `FrameRateOverride` ignores the stored rate entirely.

## Writing `.dur`

`raba-gen build theme.toml --also-dur` writes `theme.raba` and `theme.dur` from the
one source. `convert` and `generate` take the same flag.

This is for the handoff. RABA plays an outro and then gets out of the way, and if
what comes next is Ly it plays its own `.dur` background. When the last frame of
the outro *is* the first frame of that background, the transition has no seam: the
picture stays on screen while one program replaces the other. That only works if
both are drawing the same art, and the only way to be sure of that is for both
files to be generated from one source rather than kept in step by hand.

It writes **version 7** by default — `colorMap[column][line]`, `sizeX`/`sizeY` —
and `--dur-version 8` writes the other orientation and the `columns`/`lines`
spelling. Only one spelling goes in either way: the two mean the same thing, and a
reader that accepts both — RABA's does, because real files mix them — would see a
duplicate key and refuse the file.

Seven is the default on evidence rather than on principle. Both `.dur` files on a
machine running Ly 1.4.1 are version 7 — Ly's own `example.dur`, and the one a user
had configured — and the point of writing `.dur` at all is to be read by something
else. What `raba-gen` writes has the same key set as `example.dur` minus `preferredFont`
and
`extra`, which that user's file also lacks and which Ly plays regardless, so both
look optional.

**This has not been run through Ly itself.** Ly is a display manager and testing it
means a virtual machine with a real VT, which belongs with M3. Until then the claim
is "structurally the same as the files Ly reads", not "verified in Ly".

What survives the round trip is checked by test rather than assumed: a movie
written out and read back is identical cell for cell, colours included, which is
the only thing that catches the curses-order trap. A writer and a reader that are
both wrong the same way agree with each other perfectly.

**One thing does not survive: the `scaling` hint.** `.dur` has nowhere to put it,
so a theme that said `crop` comes back saying `auto`. For art like a logo that is
not a behaviour change — `auto` reads the alphabet, and a logo's alphabet is not
the shading ramp — and there is a test saying so. Inventing a key in somebody
else's format to carry it would be worse than losing it.

Note also that `.dur` stores whole screens. A movie that is cheap as `.raba`
because only a spinner moves is not cheap as `.dur`, which is the trade that
format makes and the reason `.raba` exists at all.

## Sharing art with Ly

```lua
-- /etc/ly/config.lua
animation = "dur",
dur_file_path = "/usr/share/raba/themes/arch.dur",
dur_offset_alignment = "center",
```

```ini
# /etc/raba/raba.conf
[Outro]
Theme=arch-outro
```

For an invisible handoff, the last frame of `arch-outro.dur` must equal the first frame
of `arch.dur`. `raba-gen` can emit both from one recipe.
