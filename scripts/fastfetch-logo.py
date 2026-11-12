#!/usr/bin/env python3
"""Extract a fastfetch logo as RABA art source.

fastfetch compiles its logos into the binary rather than shipping them as files, so
there is nothing to read off disk. This asks fastfetch to render one with each colour
slot forced to a distinct known value, then inverts that mapping to recover the `$1`
to `$9` markers the original source had.

An M0 convenience. M1 vendors the art from the fastfetch repository at a pinned
commit instead, which is the honest source and the one that can be credited and
diffed. See docs/milestones.md.

    scripts/fastfetch-logo.py arch > themes/arch.txt
"""

import re
import subprocess
import sys

# Slot N is forced to xterm colour N, which no logo would pick on its own and which
# is trivial to invert. The bold attribute fastfetch adds is noise here.
SGR = re.compile(r"\x1b\[([0-9;]*)m")


def extract(name: str) -> str:
    overrides = []
    for slot in range(1, 10):
        overrides += [f"--logo-color-{slot}", f"38;5;{slot}"]
    result = subprocess.run(
        ["fastfetch", "-l", name, "--pipe", "false", "-s", "break",
         "--logo-padding-left", "0", "--logo-padding-top", "0", *overrides],
        capture_output=True, text=True, check=True,
    )

    lines = []
    for raw in result.stdout.split("\n"):
        out, slot, position = [], 0, 0
        for match in SGR.finditer(raw):
            out.append(raw[position:match.start()])
            position = match.end()
            params = match.group(1).split(";")
            # Only `38;5;N` carries a slot, and only its third parameter is the slot
            # -- reading any 1-9 in the sequence would pick up the `5` that says
            # "256-colour". Bold and reset leave the slot alone, so a reset between
            # runs does not silently drop the rest of the logo to the default.
            if len(params) == 3 and params[:2] == ["38", "5"] and params[2].isdigit():
                value = int(params[2])
                if 1 <= value <= 9 and value != slot:
                    slot = value
                    out.append(f"${slot}")
        out.append(raw[position:])
        lines.append("".join(out).rstrip().replace("\t", "    "))

    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    if not lines:
        raise SystemExit(f"fastfetch returned no art for {name!r}")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    sys.stdout.write(extract(sys.argv[1]))
