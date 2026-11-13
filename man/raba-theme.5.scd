raba-theme(5)

# NAME

raba-theme - theme recipe format for raba-gen(1)

# SYNOPSIS

_themes/\*.toml_

# DESCRIPTION

A TOML file describing how to compose a *.raba* animation. *raba-gen*(1) reads it and
produces the animation; *raba*(8) only ever sees the result.

A recipe, not the generated file, is what belongs in version control and what you send
in a pull request.

# [logo] SECTION

*source=*_string_
	*fastfetch:*_name_ uses a bundled logo. *file:*_path_ reads a text file.
	*inline* takes the art from the *art* key.

*art=*_string_
	Multi-line art, used when *source* is *inline*. Colour is marked with *$1*
	through *$9*, following the fastfetch convention: everything after a marker takes
	that colour until the next marker or the end of the line.

*colors=*[_n_, ...]
	Palette indices the *$1*..*$9* markers resolve to. 0-15 are the ANSI colours,
	16-255 are xterm-256color. In a 16-colour theme anything above 15 is fitted to
	the nearest console colour at build time and reported; it is not an error.
	*colours* is accepted too.

*background=*_n_
	Background colour index. Default: *0*

	In a 16-colour theme this is clamped to 0-7. The high background bit on a
	Linux console drives blink, not brightness, so a background of 9 would be
	flashing red rather than bright red.

*default_color=*_n_
	Colour for art outside any *$n* marker, and for the padding that squares off
	a logo's shorter lines. Rarely needed. Default: *7*

# [spinner] SECTION

Omit the section for a static theme.

*style=*_string_
	One of:

	- *ascii* — *|* */* *-* *\\* (4 frames, safe everywhere)
	- *dots* — *.* *..* *...* (3 frames, safe everywhere)
	- *blocks* — *░▒▓█▓▒* (6 frames, CP437 shades, safe on a console)
	- *bar* — *▁▂▃▄▅▆▇█* (8 frames, *terminal only*)
	- *arc* — *◜◝◞◟* (4 frames, *terminal only*)
	- *custom* — from *frames*

	*bar* and *arc* are not console-safe. Only *▄* and *█* of the bar set are in
	CP437; the eighth blocks *▁▂▃▅▆▇* and the arc characters are not in any
	console font, so a console draws blanks. *raba-gen*(1) refuses to build them
	without *--allow-unsafe-glyphs*. Use *blocks* for a non-ASCII spinner that
	works at boot.

*frames=*[_string_, ...]
	Frames for *style = "custom"*. Frames shorter than the widest are padded with
	spaces, so *["."* , *".."* , *"..."]* is a three-step spinner three cells wide.

*position=*_above_|_below_|_left_|_right_|_overlay_
	Placement relative to the logo. Default: *below*

*gap=*_n_
	Blank cells between logo and spinner. Default: *1*

*color=*_n_
	Default: *7*

*period=*_n_
	Frames per full cycle. Defaults to the frame count of the chosen style. Must
	be a multiple of it, so every step is held for the same length; a remainder
	would read as a stutter and look like a fault in the player.

# [text] SECTION

Optional caption. Subject to the same glyph validation as everything else.

*content=*_string_, *position=*_..._, *gap=*_n_, *color=*_n_

# [canvas] SECTION

*framerate=*_fps_
	Playback rate, written into the animation as a delay on every frame. The
	format has no single rate field, so a later version can hold one frame longer
	than the rest. Default: *10*

*palette=*_16_|_256_
	Which palette the colour indices mean. 16 is what a console has; 256 is for
	themes aimed at a terminal emulator. Default: *16*

*repeat=*_bool_
	Whether the animation loops. An outro sets this to *false*. Default: *true*

*padding=*_n_
	Blank cells around the bounding box. Keep it small. Default: *0*

*align=*_string_
	How *raba*(8) places the canvas on screen: *center*, *top*, *bottom*,
	*top-left* and so on. Default: *center*

*x_offset=*_n_, *y_offset=*_n_
	Cells added after alignment. Negative values permitted. Default: *0*

	*align*, *x_offset* and *y_offset* are *not honoured yet*. Where a theme sits
	depends on the console's size, which is only known at run time, and the
	format has no field to carry the intent. They are accepted so recipes written
	against this page still load, and *raba-gen*(1) says so when they are set.
	Use *padding* to add space inside the theme itself.

# EXAMPLES

A distribution logo with a spinner:

```
[logo]
source = "fastfetch:arch"
colors = [6, 4]

[spinner]
style = "ascii"
position = "below"
gap = 1
color = 7

[canvas]
framerate = 10
align = "center"
```

An outro that plays once:

```
[logo]
source = "fastfetch:arch"
colors = [6, 4]

[canvas]
framerate = 24
repeat = false
```

For the outro to be invisible, its final frame must match the first thing drawn after
*raba*(8) exits. With *Handoff.Mode=retain* the frame simply stays on screen, which
covers most cases without any coordination.

# SEE ALSO

*raba-gen*(1), *raba.conf*(5), *raba*(8)
