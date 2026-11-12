# Which characters a console can draw

ASCII art is made of characters, and the Linux console can draw fewer of them than the
terminal you authored the art in. A character outside the loaded font is replaced and
**the kernel discards the original codepoint**, so there is no error, no fallback and no
way to recover at runtime. The art is simply wrong on the user's screen and right on
yours.

`raba-gen --check` is the only place this can be caught.

## The question is not "what is in the console font"

That is the usual framing and it is the wrong one for RABA, because **RABA draws before
anyone knows what the console font is.**

The splash starts in the initramfs. `systemd-vconsole-setup`, or the distribution's
equivalent, runs `setfont` later — possibly while the animation is already playing, which
can change the glyphs under it mid-frame. So the only defensible rule is an intersection:

> **Safe** means present in the kernel's built-in font *and* in the fonts a distribution
> is likely to load over it.

The built-in font is CP437. The common replacements — `lat2-16`, `lat9w-16`, `ter-116n` —
all keep ASCII and the drawing characters, and all differ about everything else.

## Three levels

| level | what it means | examples |
|---|---|---|
| **safe** | draws on any console, before or after `setfont` | ASCII `0x20`–`0x7E`, `█ ▀ ▄ ▌ ▐ ░ ▒ ▓ ■`, box drawing `─ │ ┌ ┤ ╬ ═ ║` |
| **unreliable** | in the built-in font, missing from some that replace it | `é à ü ñ ç`, `α Γ π Σ Ω`, `∞ ≡ ± ≥ ÷ √`, `£ ¥ ½ ¬` |
| **unsafe** | not in a console font at all | braille `⠋ ⠙ ⠹`, quadrants `▘ ▝ ▖ ▗`, sextants, arrows `→ ←`, emoji, CJK, control characters |

**Unreliable is the level that earns its place.** CP437's accented letters, Greek and
mathematical symbols work on most machines and stop working the moment `setfont` loads a
font that drops them. A boolean would have to call that either safe, which is a lie, or
unsafe, which would reject half the logos that render perfectly well. The author decides;
the validator reports.

### The ones that surprise people

**Braille.** The obvious choice for a smooth spinner, and the reason that idea died here:
at 256 codepoints a braille set would consume half the console's entire glyph budget,
which is exactly why distributions ship it as a separate console font package rather
than in the default.

**Quadrant and sextant blocks.** `▘▝▖▗` and the Symbols for Legacy Computing block are
what terminal art tools reach for to get sub-cell resolution. The full blocks `█▀▄▌▐` are
CP437 and safe; the quadrants sitting beside them in Unicode are not. Sextants were added
in 2019 and need a terminal like kitty or Ghostty — see [3d-logos.md](3d-logos.md), where
this decides which shading modes could ever work at boot.

This is not hypothetical. **Several fastfetch logos are drawn this way and cannot be used
at boot at all.** NixOS is the example: its current logo is quadrants, `nixos_small` is
sextants, and on a console both arrive as a logo full of holes. The fix is to find the
distribution's ASCII variant — `NixOS2`, in that case — which is what
[themes/README.md](../themes/README.md) tells a contributor to look for first.

## Why an unsafe glyph is a warning and a bad colour is an error

The decoder *rejects* a colour index outside its palette. It does *not* reject an unsafe
glyph. The difference is whether a correct fallback exists.

For a colour there is none. A renderer handed index 200 in a 16-colour palette can mask
the high bits or clamp to the default, and both are arbitrary inventions that show the
wrong pixel without saying so.

For a glyph there is one: draw a space. That is already what the renderer does with
control characters, which it filters at the last moment before writing — C1 `0x9B` is a
single-byte CSI on a terminal in 8-bit mode, so filtering `ESC` alone would not be
enough. A boot splash with one wrong character beats no boot splash.

So the glyph rule lives at authoring time, where it can be reported to someone who can
act on it, and the runtime degrades instead of failing. See [format.md](format.md) on the
validation the decoder does do.

## Checking

```sh
raba-gen --check theme.raba
```

Reports each problem character once, with how many cells use it and where it first
appears — worst first, so the top line is the one worth fixing. A logo drawn entirely in
braille is one finding with a large count, not four hundred findings.

`--allow-unsafe-glyphs` builds anyway. It exists for themes aimed at a console you
control, and it is the first thing to suspect when art looks wrong on someone else's
machine.
