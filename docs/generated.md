# Generated animations

Not every animation has a source file. Ly's built-ins — DOOM fire, CMatrix rain, a
colour-mixing shader, Conway's Life — are procedural code, so there is nothing to
import with [`raba-gen convert`](dur-support.md). They are generated instead:

```sh
raba-gen generate matrix --preview
raba-gen generate doom --columns 100 --lines 30 -o fire.raba
```

The names are Ly's, from `/etc/ly/config.ini`, so a setup can be carried across.

Generating rather than running them is what the project's rule asks for:

> **Authoring time may depend on anything. Run time depends on nothing.**

The arithmetic happens once, in a build tool, and the boot path only ever plays
frames. Nothing here runs as root or in an initramfs.

## Everything has to loop

`raba`(8) repeats the boot animation until the system is ready. A generator that
merely stops has a seam, and a seam shows up in the same place every few seconds.
Each of these closes its loop a different way:

| | period | how it closes |
|---|---|---|
| `matrix` | the frame count | every column's fall cycle is a **divisor** of it |
| `gameoflife` | 30 | the cycle is **found**, not imposed |
| `colormix` | the frame count | sine waves over a whole number of turns |
| `doom` | none | it does not — see below |

**matrix** gives each column one drop that moves a row per frame and wraps after a
cycle longer than the screen. Choosing every cycle from the divisors of the frame
count means every column is back where it started on the last frame. That is also
why the frame count matters more than it looks: 60 frames on a 25-line screen has
only two usable divisors, so only two speeds, where 120 has four.

**gameoflife** is deterministic on a finite board, so the state must eventually
repeat; remembering every state seen gives the exact period. The frame count is a
budget, not a length.

**doom** is chaotic and never returns. The seam is left in rather than hidden:
forcing the state back would be a visible jump in the same place every time, and a
crossfade needs blending a console cannot do. Fire has no persistent structure for
an eye to track, so in practice it reads as one more flicker. `raba-gen` says so
when it generates one.

## Why Life is a garden rather than a soup

It was a random soup first, and that was measured rather than guessed: on an 80x25
torus **every seed tried settled to period 2** within a few hundred generations.
That is simply what Life does — soups burn down to still lifes and blinkers. The
cycle detection was returning a real and exact answer, and the answer was a
two-frame file of things twitching.

The board is now seeded with known oscillators whose periods are coprime: the
pentadecathlon at 15, the pulsar at 3, toads and blinkers at 2. Spaced far enough
apart not to interfere, the board's period is their lowest common multiple — 30 —
and the search finds exactly that. The seed still moves the patterns around; it
just cannot change the period.

## Glyphs and colours

Composed from ASCII and the CP437 block elements only, checked by the same
validator a hand-written theme gets. CMatrix draws half-width katakana and a shader
would like the quadrant blocks; a console has neither before `setfont` runs.

Colour is dithered rather than flat. Sixteen colours quantise a smooth field into
visible bands, so each colour is blended against the one below it with `░▒▓█`,
which gives roughly four times the apparent depth. Backgrounds stay in 0–7: on a
console the high background bit is blink, not brightness. That last rule is now
checked at compile time, because the first version of the shader got it wrong and
would have flashed its brightest band.

## What they cost

At 80x25, 60 frames:

| | raw | compressed | changed per frame |
|---|---|---|---|
| `gameoflife` | 102 KB | 2 KB | 0.9% |
| `matrix` | 185 KB | 33 KB | 11.7% |
| `doom` | 726 KB | 113 KB | 50.8% |
| `colormix` | 980 KB | 129 KB | 69.1% |

For comparison, the 240x67 black hole that drove the `.dur` importer is 689 KB raw
and 133 KB compressed, changing 6.8% per frame.

The compressed figure is the one that matters for an initramfs, since the image is
compressed; the raw figure is what is parsed into memory at boot. Both are
comfortable. The wider point is that **delta encoding keeps paying even when most
of the screen moves** — fire changes half the canvas every frame and still costs
less than storing every cell would.
