#!/usr/bin/env python3
"""What was actually on the screen?

QEMU's `screendump` writes a binary PPM of whatever the display had on it.
scripts/distro-test.sh takes one every second or so and hands the directory to
this, which answers the only question worth asking of a splash: was there ever
a frame with a *centred block of console text* on it?

Centred is what separates the three things a boot puts on a screen:

    the firmware logo      dozens of colours, a bitmap
    the boot log           one or two colours, hard against the left margin
    a splash               one or two colours, a block in the middle

Classifying rather than thresholding is the difference between a check that
passes on the TianoCore logo and one that means something. It was written the
other way first, and the TianoCore logo is exactly what it passed on.

Prints a verdict line, then `best=<file>` for the harness to keep as evidence.
Exits 0 either way: this is evidence, not a verdict.
"""

import sys
from pathlib import Path

# Every nth pixel. A 1280x800 frame is a million pixels; a seventh of a percent
# of them still lands dozens of samples inside an 8x16 character cell.
STRIDE = 7
LIT = 60  # sum of the three channels above which a sample counts as drawn


def read_ppm(path):
    """P6 PPM -> (width, height, pixels). None if it is not one."""
    data = path.read_bytes()
    if not data.startswith(b"P6"):
        return None
    fields, offset = [], 2
    while len(fields) < 3 and offset < len(data):
        while offset < len(data) and data[offset : offset + 1].isspace():
            offset += 1
        if data[offset : offset + 1] == b"#":
            while offset < len(data) and data[offset] != 0x0A:
                offset += 1
            continue
        start = offset
        while offset < len(data) and not data[offset : offset + 1].isspace():
            offset += 1
        fields.append(int(data[start:offset]))
    if len(fields) != 3:
        return None
    return fields[0], fields[1], data[offset + 1 :]


def look(path):
    """One frame, described: kind, how much was lit, how many colours."""
    frame = read_ppm(path)
    if frame is None:
        return None
    width, height, pixels = frame
    colours = set()
    lit = 0
    left = width
    right = 0
    top = height
    bottom = 0
    for index in range(0, len(pixels) - 3, 3 * STRIDE):
        pixel = pixels[index : index + 3]
        if sum(pixel) <= LIT:
            continue
        lit += 1
        colours.add(bytes(pixel))
        position = index // 3
        x, y = position % width, position // width
        left = min(left, x)
        right = max(right, x)
        top = min(top, y)
        bottom = max(bottom, y)

    if lit < 20:
        return "blank", lit, len(colours), width, height
    if len(colours) > 16:
        return "graphical", lit, len(colours), width, height
    # Console text. Where it sits, and how much room it takes, is what says
    # which console text it is. The height test is not optional: QEMU's own
    # "Guest has not initialized the display (yet)" is a single line, dead
    # centre, and passes every other test here.
    indented = left > width // 8
    lowered = top > height // 8
    tall = (bottom - top) > height // 10
    wide = (right - left) > width // 10
    kind = "centred" if indented and lowered and tall and wide else "left-aligned"
    return kind, lit, len(colours), width, height


RANK = {"centred": 3, "left-aligned": 2, "graphical": 1, "blank": 0}

VERDICT = {
    "centred": "a centred block of console text — a splash",
    "left-aligned": "console text against the left margin — a boot log or a prompt",
    "graphical": "a bitmap with many colours — firmware or a graphical mode",
    "blank": "nothing but black",
}


def main():
    directory = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    best = None
    tally = {kind: 0 for kind in RANK}
    for shot in sorted(directory.glob("*.ppm")):
        seen = look(shot)
        if seen is None:
            continue
        tally[seen[0]] += 1
        # Rank first, then how much was drawn. The tiebreak is not cosmetic: the
        # first left-aligned frame of a boot is QEMU's own "Guest has not
        # initialized the display (yet)", a hundred and fifty lit samples, and
        # keeping *that* as the evidence for a whole boot wasted a debugging
        # session. The frame worth looking at is the fullest one.
        if best is None or (RANK[seen[0]], seen[1]) > (RANK[best[1][0]], best[1][1]):
            best = (shot, seen)
    if best is None:
        print("no frames were captured")
        print("best=")
        return
    path, (kind, lit, colours, width, height) = best
    # Every frame counted, not just the winner: "40 frames, all blank" and "40
    # frames, 38 of them a boot log" are different failures.
    counted = ", ".join(f"{count} {name}" for name, count in tally.items() if count)
    print(f"{VERDICT[kind]} ({lit} lit samples, {colours} colours, {width}x{height})"
          f" — of {sum(tally.values())} frames: {counted}")
    print(f"best={path}")


if __name__ == "__main__":
    main()
