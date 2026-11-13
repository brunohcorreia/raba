#!/usr/bin/env python3
"""Did anything reach the screen?

QEMU's `screendump` writes a binary PPM of whatever the display had on it. A
boot that drew a splash has at least one frame with real colour in it; a boot
that drew nothing has frames that are black, or black with a little white text.

Prints one line for scripts/distro-test.sh to put in its output, and exits 0
either way: this is evidence, not a verdict.
"""

import sys
from pathlib import Path


def read_ppm(path):
    """P6 PPM -> (width, height, pixel bytes). None if it is not one."""
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
    return (fields[0], fields[1], data[offset + 1 :]) if len(fields) == 3 else None


def describe(path):
    frame = read_ppm(path)
    if frame is None:
        return None
    width, height, pixels = frame
    colours = set()
    lit = 0
    # Every 97th pixel: enough to characterise a 640x480 console, cheap enough
    # to run over a few hundred frames.
    for index in range(0, len(pixels) - 3, 3 * 97):
        pixel = pixels[index : index + 3]
        colours.add(bytes(pixel))
        if sum(pixel) > 48:
            lit += 1
    return width, height, len(colours), lit


def main():
    directory = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    best = None
    for shot in sorted(directory.glob("*.ppm")):
        seen = describe(shot)
        if seen and (best is None or seen[2] > best[1][2]):
            best = (shot.name, seen)
    if best is None:
        print("no frames were captured")
        return
    name, (width, height, colours, lit) = best
    verdict = "something was drawn" if colours > 2 and lit > 0 else "nothing but a blank console"
    print(f"{verdict} ({colours} colours, {lit} lit samples, {width}x{height}, {name})")


if __name__ == "__main__":
    main()
