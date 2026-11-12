raba-theme(5)

# NAME

raba-theme - theme recipe format for raba-gen(1)

# SYNOPSIS

_themes/*.toml_

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
	16-255 are xterm-256color; anything above 15 is downsampled at build time for
	console targets.

*background=*_n_
	Background colour index. Default: *0*

# [spinner] SECTION

Omit the section for a static theme.

*style=*_string_
	One of:

	- *ascii* — *|* */* *-* *\\* (4 frames, safe everywhere)
	- *dots* — *.* *..* *...* (3 frames)
	- *bar* — *▁▂▃▄▅▆▇█* (8 frames, CP437 blocks)
	- *arc* — *◜◝◞◟* (4 frames, verify against your console font)
	- *custom* — from *frames*

*frames=*[_string_, ...]
	Frames for *style = "custom"*.

*position=*_above_|_below_|_left_|_right_|_overlay_
	Placement relative to the logo. Default: *below*

*gap=*_n_
	Blank cells between logo and spinner. Default: *1*

*color=*_n_
	Default: *7*

*period=*_n_
	Frames per full cycle. Defaults to the frame count of the chosen style.

# [text] SECTION

Optional caption. Subject to the same glyph validation as everything else.

*content=*_string_, *position=*_..._, *gap=*_n_, *color=*_n_

# [canvas] SECTION

*framerate=*_fps_
	Playback rate written into the animation. Default: *10*

*align=*_string_
	How *raba*(8) places the canvas on screen: *center*, *top*, *bottom*,
	*top-left* and so on. Default: *center*

*x_offset=*_n_, *y_offset=*_n_
	Cells added after alignment. Negative values permitted. Default: *0*

*padding=*_n_
	Blank cells around the bounding box. Keep it small. Default: *0*

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
```

For the outro to be invisible, its final frame must match the first thing drawn after
*raba*(8) exits. With *Handoff.Mode=retain* the frame simply stays on screen, which
covers most cases without any coordination.

# SEE ALSO

*raba-gen*(1), *raba.conf*(5), *raba*(8)
