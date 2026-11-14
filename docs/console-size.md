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

Three kinds of art, three answers. The question "does this survive a console that
is not the size it was drawn for" has a different answer for each, and sorting them
out is most of the work.

| kind | example | at the wrong size |
|---|---|---|
| procedural | `generate matrix` | perfect — it is generated at the console's size; a *packaged* one says which of the two below it wants |
| shading | the black hole, anything from `video-to-dur` | **resampled**, proportions kept |
| line art and logos | every theme in `themes/` | centred, and cropped if it overflows |

**Procedural animations have no native size.** `raba-gen generate` defaults its
canvas to the terminal it is run in, and nothing stops it being run again for
another one. There is no information to lose.

**But the alphabet is only a guess, and the file gets the last word.** Three of the
four generated animations are drawn entirely in the ramp, and resampling is right
for exactly one of them — a Game of Life board uses `█` for a live cell and `░`
for its trail, which are symbols, not intensities, and averaging four of them gives
a smear rather than three quarters of a glider. The `.raba` header therefore stores
`scaling`: `auto` guesses from the alphabet, and anything RABA generates itself
states the answer outright. [generated.md](generated.md) has the four decisions and
[format.md](format.md) the byte.

**Shading art is not really text.** The black hole is drawn in ` .·░▒▓█` and
nothing else — seven characters that are a greyscale ramp, which makes the picture
an image written in a coarse alphabet. An image can be resampled: average the
coverage of the block of source cells each output cell stands for, and pick the
character nearest that average. `raba_core::render::resample` does exactly that,
and `best_fit` chooses it over cropping when every character in the movie is on the
ramp — a property computed once per movie at load time, so no format change was
needed and nothing has to be declared in a recipe.

Two details decide whether it looks right:

- **Colour comes from the brightest contributing cell, not the average.** A bright
  core on dark sky averages to uniform mud and the structure dissolves. Taking the
  brightest contributor keeps edges where the art put them. 60 distinct colours
  survive 240x67 → 80x25 on the black hole.
- **Proportions are kept and the remainder centred.** 240x67 into an 80x60 console
  becomes 80x22 centred, not a squashed 80x60.

The result at 100x30 is still legibly a black hole — event horizon, the disk
curving, the jet — from a source nearly nine times its area.

**Line art does not survive this and must not be put through it.** Halving a line
does not give you half a line; a box-drawing logo resampled is mush. These stay
centred and cropped, which is also what you want: a centred logo looks deliberate,
a blown-up one looks broken. For the same reason nothing is ever scaled *up* —
small art on a big console is centred.

So the rule from the earlier version of this file — *the bigger the art, the more
it assumes* — is still true, but it is not about size. It is about whether the
art's alphabet carries intensity.

## Canvas limits and console limits

A canvas and a console are bounded by different rules, because only one of them is
written to a file.

| | bound | enforced by |
|---|---|---|
| canvas — the picture in a `.raba` | 1024 columns and 1024 lines | `decode`, before any allocation; `Grid::new` |
| console — what the kernel reports | 1 048 576 cells, in any shape | `Grid::screen` |

`limits::MAX_SCREEN_CELLS` is 1024 × 1024, so the largest screen grid allocates
exactly what the largest canvas does. Within that total the axes are unconstrained:
2000x60 and 240x4000 are both accepted and drawn on in full.

| console size | result |
|---|---|
| within the cell bound | the grid is the console; art is centred on the real width |
| over it | each axis is clamped to 1024 and the art is drawn in that region |
| zero in either axis | the axis is taken as 1 |

The second row is outside what hardware produces — an 8K panel with the smallest
font in the table above gives 960x540, or 518 400 cells — and is defined rather
than refused so that no console size can leave the draw path without a grid.

## Does the font have to change?

**No.** The font decides the grid, and nothing scales it — the framebuffer console
draws glyph bitmaps one pixel to one pixel, and its only sysfs knobs are
`cursor_blink`, `power`, `rotate` and `rotate_all`. There is no zoom to turn. So if
the console had to be made to fit the art, the only lever would be loading a
different font, with everything that costs below.

The art is the easier end to move. Changing the console changes it for everything
that draws afterwards and has to be undone on every exit path including a crash;
resampling a picture changes nothing outside RABA's own output buffer and is a pure
function with no syscalls, which is where `raba-core` is allowed to work. That is
why the answer above is resampling and not `setfont`.

The rest of this section stays because the font question is still open for other
reasons — the glyph restriction, and the sprite-sheet idea at the end — not because
screen size needs it.

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
- [dur-support.md](dur-support.md) — importing art authored at somebody else's size
- [generated.md](generated.md) — animations generated at the terminal's size
