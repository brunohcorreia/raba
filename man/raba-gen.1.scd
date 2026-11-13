raba-gen(1)

# NAME

raba-gen - build and convert .raba animations

# SYNOPSIS

*raba-gen* build _recipe.toml_ [*-o* _out.raba_]

*raba-gen* *generate* _animation_
	Produce an animation that has no source file. _animation_ is one of *doom*,
	*matrix*, *colormix* or *gameoflife* — the names Ly uses in its own
	configuration, so a setup can be carried across.

	*--columns* _N_, *--lines* _N_
		Canvas size. Defaults to the terminal's, or 80x25 when that cannot be
		read. A generated animation is whatever size it is asked for, so
		generating at 80x25 and previewing on a larger terminal would draw a
		box in the middle of it.
	*--frames* _N_
		How long the loop runs. For *gameoflife* this is a budget rather than a
		length: the animation repeats every 30 generations and stops there.
	*--delay* _MS_
		Milliseconds per frame. Default 80.
	*--seed* _N_
		The same seed gives the same file, so a build is reproducible.
	*--palette* _16_|_256_
		Colour depth. Default 16, which is what a console has.

	All of these loop except *doom*, which is chaotic and says so when it runs.
	See *docs/generated.md*.

*--check* _file.raba_

*raba-gen* convert _input.dur_ [*-o* _out.raba_] [*--preview*]

*raba-gen* generate _animation_ [*-o* _out.raba_] [*--preview*]

*raba-gen* *--list-logos* [_pattern_]

# DESCRIPTION

*raba-gen* produces *.raba* animations, which is the only format *raba*(8) plays.

*build* composes a logo, a spinner and optional text from a recipe. *convert* imports an
existing animation — *.dur* from Durdraw, *.ans* ANSI art, or plain text.

It exists because hand-drawing a spinner across twenty frames for fifty distributions
is not maintainable. A recipe is about ten lines of TOML and is what you contribute
when you add a theme; the *.raba* file is a build artifact.

*raba-gen* is a build-time tool. It is not installed into the initramfs, is never on the
boot path, and is not needed to display a theme. This is what allows it to depend on
whatever it likes while *raba*(8) depends on nothing.

# OPTIONS

*-o* _path_
	Output file. Defaults to the recipe's name with a *.raba* extension.

*--list-logos* [_pattern_]
	List the bundled logo names, optionally filtered. _pattern_ is matched as a
	substring, case-insensitively; *|* separates alternatives.

	Each line says whether a console can draw that logo. fastfetch renders in a
	terminal with the user's font, so some of its logos are braille, sextants or
	quadrant blocks and cannot be drawn before *setfont* runs; those are marked
	*terminal*. A *console\** mark means the glyphs are in the kernel's built-in
	font but not in every font that replaces it. See *docs/glyphs.md*.

	The art is vendored from fastfetch at a pinned commit, reported in the
	footer of the listing.

*generate* _animation_
	Produce an animation that has no source file. _animation_ is one of *doom*,
	*matrix*, *colormix* or *gameoflife* — the names Ly uses in its own
	configuration, so a setup can be carried across.

	*--columns* _N_, *--lines* _N_
		Canvas size. Defaults to the terminal's, or 80x25 when that cannot be
		read. A generated animation is whatever size it is asked for, so
		generating at 80x25 and previewing on a larger terminal would draw a
		box in the middle of it.
	*--frames* _N_
		How long the loop runs. For *gameoflife* this is a budget rather than a
		length: the animation repeats every 30 generations and stops there.
	*--delay* _MS_
		Milliseconds per frame. Default 80.
	*--seed* _N_
		The same seed gives the same file, so a build is reproducible.
	*--palette* _16_|_256_
		Colour depth. Default 16, which is what a console has.

	All of these loop except *doom*, which is chaotic and says so when it runs.
	See *docs/generated.md*.

*--check* _file.raba_
	Validate an existing *.raba* without generating anything. Reports format version,
	canvas size, frame count, and any glyph the console cannot render.

*--preview*
	Play the result in the current terminal instead of writing a file, three
	times through. Nothing is left on disk. *raba --preview* _file.raba_ plays a
	theme that has already been built.

*--also-dur* _path_
	Additionally write a *.dur* alongside the *.raba*. Useful when something else — a
	display manager that plays its own animation, say — should show matching art.
	Not implemented yet; it arrives with *convert*.

*--dur-version* _7_|_8_
	Format version for *--also-dur*. Default: *7*, which more readers accept.

*--allow-unsafe-glyphs*
	Downgrade the glyph check from an error to a warning. The resulting theme may
	display incorrectly on a real console. Intended for themes targeting a terminal
	emulator rather than boot.

# GLYPH VALIDATION

By default *raba-gen* refuses to build a theme containing characters the Linux console
cannot display.

The console holds 256 or 512 glyphs. Anything outside the loaded font is replaced and
the original codepoint is lost by the kernel. The practical safe set is ASCII, CP437
block elements (*█ ▀ ▄ ▌ ▐ ░ ▒ ▓*) and box-drawing characters. Braille patterns, the
quadrant blocks (*▘▝▖▗*), arrows and emoji are not — Braille alone would consume 256
of the available slots, which is why distributions ship it as a separate console font.

Accented letters and CP437's Greek and mathematics are a third case: they are in the
kernel's built-in font and missing from some fonts that replace it, so they are
reported as a warning rather than an error. See _docs/glyphs.md_ for the rule and why
it is an intersection of fonts rather than a single font.

A failure lists each offending character with its codepoint and the frame and position
where it appears. This turns an invisible boot-time rendering bug into a build error.

# FRAME COUNT

When elements animate at different periods, *raba-gen* emits the least common multiple
of those periods so the loop closes without a visible jump. A logo pulsing over 4
frames beside a 10-frame spinner produces 20 frames.

# CANVAS SIZE

The canvas is the bounding box of the composed elements, not the terminal size.
*raba*(8) centres it at display time. Padding to the terminal size buys nothing and
enlarges the first frame, which is the only one stored in full.

# EXAMPLES

```
raba-gen build themes/arch.toml -o arch.raba
raba-gen build themes/arch.toml --preview
raba-gen convert art.dur -o art.raba
raba-gen --list-logos 'void|artix'
raba-gen --check /usr/share/raba/themes/cachyos.raba
```

# EXIT STATUS

*0* success. *1* validation failure. *2* usage error.

# SEE ALSO

*raba-theme*(5), *raba*(8), *durdraw*(1)

The *.raba* format is described in _docs/format.md_.
