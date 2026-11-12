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
                elif 90 <= value <= 97:
                    base = value - 90 + 8
        if base is None:
            return None
        return base + 8 if self.bold and base < 8 else base


def extract(name: str):
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
    return "\n".join(lines) + "\n", colours


def recipe(name: str, colours, source: str) -> str:
    listed = ", ".join(str(c) for c in colours)
    note = "" if source == name else f"# fastfetch calls this logo {source!r}.\n"
    return f"""\
# {name}. Art and colours extracted from fastfetch; see scripts/fastfetch-logo.py.
# Distribution logos are trademarks of their respective projects.
{note}
[logo]
source = "file:{name}.txt"
colors = [{listed}]

[spinner]
style = "ascii"
position = "below"
gap = 1
color = {colours[0] if colours else 7}

[canvas]
framerate = 8
"""


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3, 4):
        raise SystemExit(__doc__)
    source = sys.argv[1]
    logo, colours = extract(source)
    if len(sys.argv) == 2:
        sys.stdout.write(logo)
    else:
        name = sys.argv[3] if len(sys.argv) == 4 else source
        directory = Path(sys.argv[2])
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"{name}.txt").write_text(logo)
        (directory / f"{name}.toml").write_text(recipe(name, colours, source))
        width = max((len(line) for line in logo.split("\n")), default=0)
        print(f"{name}: colors = {colours}, about {width}x{logo.count(chr(10))}")
