# How big is the console

A console has no inherent size. The grid is the screen divided by one character
cell, so the number of characters available is set by the **font**:

```
columns = screen_width_px  ÷ font_width_px
lines   = screen_height_px ÷ font_height_px
```

On a 1920x1080 framebuffer, with fonts that are actually installed on an ordinary
Arch system:

| font | cell | grid | cells |
|---|---|---|---|
| `drdos8x6` | 8x6 | 240x180 | 43,200 |
| `Cyr_a8x8` | 8x8 | 240x135 | 32,400 |
| `Goha-12` | 8x12 | 240x90 | 21,600 |
| **`lat2-16`, `ter-116n` — the usual default** | **8x16** | **240x67** | **16,080** |
| `LatGrkCyr-12x22` | 12x22 | 160x49 | 7,840 |
| `latarcyrheb-sun32` | 16x32 | 120x33 | 3,960 |
| `solar24x32` | 24x32 | 80x33 | 2,640 |

**Sixteen times the resolution between the ends of that table**, on the same
screen. This is why `/etc/ly/blackhole.dur` is 240x67 and not some other number:
it was authored for a 1080p framebuffer with an 8x16 font, which is the default.

## What this means for art

A full-screen animation is **tied to the font it was drawn for**. The black hole
is the size of a 1080p console at 8x16; on the same screen with
`latarcyrheb-sun32` loaded, the console is 120x33 and most of the art is off the
edges. `raba_core::render::place` centres and crops rather than scrambling, and
`raba-gen convert` warns when a canvas is larger than the 80x25 fallback, but
neither can invent the missing resolution.

A logo does not have this problem, because it is small and gets centred. The rule
is the obvious one: **the bigger the art, the more it assumes.**

## Could RABA set the font itself?

Yes. `setfont`(8) does it with the `KDFONTOP` ioctl and `KD_FONT_OP_SET`, and the
framebuffer console recalculates its grid from the new cell size. Nothing stops a
program from doing the same.

It would solve a real problem. [glyphs.md](glyphs.md) exists entirely because RABA
cannot know what font is loaded, so it restricts itself to the intersection of the
kernel's built-in CP437 and the fonts distributions load over it — a few hundred
characters. A font RABA loaded itself would be a font RABA knew, and the whole
safe/unreliable/unsafe distinction would collapse into "whatever we shipped".

It would also make the canvas a choice rather than a discovery: art could state the
grid it wants and get it.

### What it costs

**Half the foreground colours, if the font has more than 256 glyphs.** This is the
part that is easy to get backwards. The attribute byte has one intensity bit, and
PC video hardware lets it mean *either* bright foreground *or* "use the second
256 glyphs" — not both. From `setfont`(8):

> if a font has more than 256 glyphs, only 8 out of 16 colors can be used
> simultaneously … the console will be reduced to 8 (instead of 16) colors.

So 512 glyphs costs indices 8–15. Every theme here uses them — bright green for
the CachyOS spinner, bright red and bright yellow in the fire ramp, white heads in
the matrix rain. A 256-glyph font keeps all sixteen colours, and 256 glyphs is
already more than the safe set RABA restricts itself to today.

**The restore problem.** A font loaded and not put back leaves the console wrong
for whoever logs in next. That has to survive every exit path including a crash,
and the project's first rule is that nothing it does may stop a machine booting.
Lowering the console log level already has this shape and is already handled, so
the pattern exists — but a font is more visible when it goes wrong.

**A visible reflow.** Changing cell size resizes the grid, which the user sees.
Acceptable once at the start of a splash; not acceptable mid-animation.

### The interesting version

A console font is 256 or 512 arbitrary bitmaps of the cell size. Nothing requires
them to be letters. A 256-glyph font at 8x16 is **a sprite sheet of 256 tiles**,
each 8x16 pixels, that the renderer can place anywhere on the grid — which is a
different medium from ASCII art, much closer to a tile-based game, while still
being a text console with no GPU driver involved.

That is a long way past what this project has promised and is recorded here as a
possibility rather than a plan. It belongs after M3, because it needs the same VT
ioctl machinery and the same restore-on-every-path discipline, and that is built
and tested in a virtual machine before anything else.

## See also

- [glyphs.md](glyphs.md) — the problem a self-loaded font would remove
- [dur-support.md](dur-support.md) — why canvases should stay small
- [generated.md](generated.md) — animations generated at the terminal's size
