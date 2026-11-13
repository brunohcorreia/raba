#!/usr/bin/env python3
"""Extract a fastfetch logo as RABA art plus a theme recipe.

fastfetch compiles its logos into the binary rather than shipping them as files, so
there is nothing to read off disk. This renders one twice:

  once with every colour slot forced to a distinct known value, which recovers the
  `$1` to `$9` markers the original source had;

  once untouched, which recovers the colours the distribution actually uses.

Pairing the two by position gives both the art and a recipe faithful to the branding,
rather than art plus a guess about colours.

An M0/M1 convenience. Vendoring the art from the fastfetch repository at a pinned
commit is the honest source and the one that can be credited and diffed; see issue 1.

    scripts/fastfetch-logo.py arch                     # art to stdout
    scripts/fastfetch-logo.py arch themes/             # arch.txt and arch.toml
    scripts/fastfetch-logo.py NixOS2 themes/ nixos     # under a different name

The third form matters more than it looks. Several fastfetch logos are drawn with
quadrant or sextant blocks, which no console font has -- the current NixOS logo is
one, and `nixos_small` is worse. Where a distribution has an ASCII variant, that is
the one a boot splash can use, and it rarely has the obvious name.
"""

import re
import subprocess
import sys
from pathlib import Path

SGR = re.compile(r"\x1b\[([0-9;]*)m")


def known_logos() -> set:
    """Every logo name fastfetch has, lowercased."""
    listing = subprocess.run(["fastfetch", "--list-logos"],
                             capture_output=True, text=True, check=True).stdout
    return {name.lower() for name in re.findall(r'"([^"]+)"', listing)}


def check_known(name: str) -> None:
    """Refuse a name fastfetch does not have.

    Asking for a logo that does not exist does not fail: fastfetch falls back to the
    logo of the distribution it detects. So a typo, or a distribution it has never
    heard of, silently produces *this machine's* logo under the wrong name --
    `bazzite` yields a CachyOS logo on a CachyOS box, and nothing says so.
    """
    known = known_logos()
    if name.lower() in known:
        return
    near = sorted(n for n in known if name.lower()[:4] in n)[:6]
    hint = f"; did you mean {', '.join(near)}?" if near else ""
    raise SystemExit(
        f"fastfetch has no logo called {name!r}{hint}\n"
        f"Without this check it would have fallen back to this machine's own logo."
    )


def render(name: str, overrides: bool) -> str:
    args = ["fastfetch", "-l", name, "--pipe", "false", "-s", "break",
            "--logo-padding-left", "0", "--logo-padding-top", "0"]
    if overrides:
        for slot in range(1, 10):
            args += [f"--logo-color-{slot}", f"38;5;{slot}"]
    result = subprocess.run(args, capture_output=True, text=True, check=True)
    return result.stdout


def runs(line: str, read_value):
    """Walk one rendered line, yielding (column, value) at each colour change.

    `column` counts printable characters only, so the two renderings line up even
    though their escape sequences differ in length.
    """
    column, position, current = 0, 0, None
    for match in SGR.finditer(line):
        column += len(line[position:match.start()])
        position = match.end()
        value = read_value(match.group(1).split(";"))
        if value is not None and value != current:
            current = value
            yield column, value


def read_slot(params):
    # Only `38;5;N` carries a forced slot, and only its third parameter is the slot --
    # reading any 1-9 in the sequence would pick up the `5` that says "256-colour".
    if len(params) == 3 and params[:2] == ["38", "5"] and params[2].isdigit():
        value = int(params[2])
        return value if 1 <= value <= 9 else None
    return None


# The sixteen colours a Linux console actually has, as the VGA palette renders
# them. Everything below is measured against these and nothing else: a logo is
# going to be drawn in one of them whatever fastfetch asked for.
CONSOLE_RGB = [
    (0, 0, 0), (170, 0, 0), (0, 170, 0), (170, 85, 0),
    (0, 0, 170), (170, 0, 170), (0, 170, 170), (170, 170, 170),
    (85, 85, 85), (255, 85, 85), (85, 255, 85), (255, 255, 85),
    (85, 85, 255), (255, 85, 255), (85, 255, 255), (255, 255, 255),
]
CUBE = (0, 95, 135, 175, 215, 255)


def xterm_rgb(index: int):
    """What `38;5;N` actually means, in RGB."""
    if index < 16:
        return CONSOLE_RGB[index]
    if index < 232:
        rest = index - 16
        return (CUBE[rest // 36], CUBE[rest // 6 % 6], CUBE[rest % 6])
    return (8 + 10 * (index - 232),) * 3


def nearest_console(index: int) -> int:
    """The closest of the sixteen to a 256-colour value.

    Twenty-nine of fastfetch's logos are written in the 256-colour palette, and
    until this existed every one of them came out of here with no colour at all:
    the reader below understood `31` and `1;36` and nothing else, so the slot was
    dropped, the recipe said `colors = []`, and the art drew in its default. The
    symptom was a build-time warning nobody read and a logo in the wrong colour
    on somebody's boot.

    Distance is redmean, which is a cheap approximation of how different two
    colours look rather than how far apart their numbers are. Plain Euclidean
    RGB answers "light grey" for a yellow-green often enough to be useless.

    Saturated sources are matched against saturated candidates only. Without
    that rule a muted brand colour lands on grey, which is technically the
    nearest point and visibly the wrong answer: a logo has a colour on purpose,
    and the worst of the fifteen coloured choices is closer to the intent than
    the best grey.
    """
    want = xterm_rgb(index)
    saturated = max(want) - min(want) > 40
    greys = {0, 7, 8, 15}

    def distance(other):
        mean = (want[0] + other[0]) / 2
        dr, dg, db = (want[i] - other[i] for i in range(3))
        return (2 + mean / 256) * dr * dr + 4 * dg * dg + (2 + (255 - mean) / 256) * db * db

    candidates = range(16)
    if saturated:
        candidates = [slot for slot in candidates if slot not in greys]
    return min(candidates, key=lambda slot: distance(CONSOLE_RGB[slot]))


class Colour:
    """Reads a real logo's SGR, tracking bold separately from colour.

    fastfetch sets bold once per line and the colour after it. On a Linux console the
    bold attribute selects the bright half of the palette, so `1;36` is light cyan and
    not cyan -- dropping the bold would ship every theme a shade too dark.
    """

    def __init__(self):
        self.bold = False

    def __call__(self, params):
        base = None
        for param in params:
            if param in ("", "0"):
                self.bold = False
            elif param == "1":
                self.bold = True
            elif param == "22":
                self.bold = False
            elif param.isdigit():
                value = int(param)
                if 30 <= value <= 37:
                    base = value - 30
                elif value == 39:
                    # Default foreground. A logo drawn in it is not uncoloured -- on
                    # a console the default is light grey, and bolded it is white,
                    # which is what MX and elementary actually look like.
                    base = 7
                elif 90 <= value <= 97:
                    base = value - 90 + 8
        # `38;5;N`, read as a whole rather than parameter by parameter: the `5`
        # in it is not a colour and the loop above would be happy to treat it as
        # one.
        if len(params) >= 3 and params[0] == "38" and params[1] == "5" and params[2].isdigit():
            base = nearest_console(int(params[2]))
            # An explicit 256-colour value has already said how bright it is.
            return base
        if base is None:
            return None
        return base + 8 if self.bold and base < 8 else base


def extract(name: str):
    check_known(name)
    marked = render(name, overrides=True).split("\n")
    plain = render(name, overrides=False).split("\n")
    colour = Colour()

    lines, slot_colours, slot = [], {}, 0
    for marked_line, plain_line in zip(marked, plain):
        actual = dict(runs(plain_line, colour))
        text = SGR.sub("", marked_line)
        # Debian's logo is made of dollar signs. A literal one is written `$$`, so
        # escaping has to happen on the way out or the art will not parse.
        escape = lambda chunk: chunk.replace("$", "$$")
        out, position = [], 0
        for column, found in runs(marked_line, read_slot):
            out.append(escape(text[position:column]))
            position = column
            if found != slot:
                slot = found
                out.append(f"${slot}")
            # The colour at this column in the untouched rendering is what this slot
            # means. Later lines agree, so first writer wins.
            if slot not in slot_colours and column in actual:
                slot_colours[slot] = actual[column]
        out.append(escape(text[position:]))
        lines.append("".join(out).rstrip().replace("\t", "    "))

    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    if not lines:
        raise SystemExit(f"fastfetch returned no art for {name!r}")

    highest = max(slot_colours) if slot_colours else 0
    colours = [slot_colours.get(n, 7) for n in range(1, highest + 1)]

    # A logo with no `$n` markers at all is drawn entirely in one colour, and every
    # cell lands in slot 0. Without this the art would come out in the default grey
    # instead of the distribution's colour, which is wrong and not obviously wrong.
    default = 7
    if not colours:
        single = Colour()
        seen = [value for line in plain for _, value in runs(line, single)]
        if seen:
            default = max(set(seen), key=seen.count)
    return "\n".join(lines) + "\n", colours, default


# The size raba(8) assumes when it cannot read the console. A theme that does not fit
# it is clipped on exactly the machines least able to say why.
FALLBACK_LINES = 25


def recipe(name: str, colours, default: int, source: str, lines: int) -> str:
    listed = ", ".join(str(c) for c in colours)
    uncoloured = "" if colours else f"default_color = {default}\n"
    # A tall logo plus a gap plus a spinner line can overflow an 80x25 console. Put
    # the spinner beside the art instead, where it costs a column and no lines.
    beside = lines + 2 > FALLBACK_LINES
    position = "right" if beside else "below"
    why = (
        f"# The art is {lines} lines, so a spinner below it would not fit {FALLBACK_LINES}.\n"
        if beside else ""
    )
    note = "" if source == name else f"# fastfetch calls this logo {source!r}.\n"
    return f"""\
# {name}. Art and colours extracted from fastfetch; see scripts/fastfetch-logo.py.
# Distribution logos are trademarks of their respective projects.
{note}
[logo]
source = "file:{name}.txt"
colors = [{listed}]
{uncoloured}
{why}[spinner]
style = "ascii"
position = "{position}"
gap = 1
color = {colours[0] if colours else default}

[canvas]
framerate = 8
"""


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3, 4):
        raise SystemExit(__doc__)
    source = sys.argv[1]
    logo, colours, default = extract(source)
    if len(sys.argv) == 2:
        sys.stdout.write(logo)
    else:
        name = sys.argv[3] if len(sys.argv) == 4 else source
        directory = Path(sys.argv[2])
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"{name}.txt").write_text(logo)
        (directory / f"{name}.toml").write_text(
            recipe(name, colours, default, source, logo.count(chr(10))))
        width = max((len(line) for line in logo.split("\n")), default=0)
        print(f"{name}: colors = {colours or [default]}, about {width}x{logo.count(chr(10))}")
