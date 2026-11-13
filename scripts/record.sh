#!/bin/bash
# Record a real boot as a GIF, for the top of the README.
#
# Not a mock-up and not a terminal recording: this boots the same installed
# virtual machine the tests boot, photographs the screen through QEMU's monitor
# ten times a second, and assembles the frames. What ends up in the README is a
# machine starting, with RABA on it, and nothing staged.
#
# It needs an installed image. scripts/distro-test.sh --keep-installed leaves
# one; this builds one if it is missing, which takes about ten minutes.
#
# Usage:
#   scripts/record.sh                       # arch, 14 seconds, docs/images/boot.gif
#   scripts/record.sh --distro fedora
#   scripts/record.sh --seconds 20 --out /tmp/boot.gif
#   scripts/record.sh --crop 1280x800       # the whole screen, no cropping
#   scripts/record.sh --keep                # leave the frames behind
#
# SPDX-License-Identifier: GPL-3.0-or-later
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
distro=arch
seconds=14
fps=10
# A crop, not a scale, and this is not a detail.
#
# The guest's console is 1280x800 of 8x16 glyphs. Resampling that to a
# README-friendly width turns every glyph into a smear and the logo into
# horizontal stripes — the first version of this image looked like a damaged
# CRT. Cutting a window out of the middle at one-to-one instead keeps every
# pixel the kernel drew, and costs nothing in file size because what is cut
# away is black.
crop=1024x640
out="${root}/docs/images/boot.gif"
keep=0

die() { echo "record: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --distro)  distro="${2:-}"; shift 2 ;;
        --seconds) seconds="${2:-}"; shift 2 ;;
        --fps)     fps="${2:-}"; shift 2 ;;
        --crop)    crop="${2:-}"; shift 2 ;;
        --out)     out="${2:-}"; shift 2 ;;
        --keep)    keep=1; shift ;;
        -h|--help) sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

command -v qemu-system-x86_64 > /dev/null || die "qemu is not installed"
command -v ffmpeg > /dev/null || die "ffmpeg is not installed (package: ffmpeg)"

work="${root}/target/distro/${distro}"
installed="${work}/installed.qcow2"
seed="${work}/seed.img"
firmware=/usr/share/edk2/x64/OVMF.4m.fd
[ -r "$firmware" ] || firmware=/usr/share/OVMF/OVMF_CODE.fd
[ -r "$firmware" ] || die "no OVMF firmware found (package: edk2-ovmf)"

if [ ! -s "$installed" ]; then
    echo "record: no installed image yet; building one"
    "${root}/scripts/distro-test.sh" --distro "$distro" --keep-installed \
        || die "the install run failed"
fi

frames="${work}/record"
rm -rf "$frames"
mkdir -p "$frames"

# A copy-on-write overlay, so recording never writes to the image the tests use.
disk="${work}/record.qcow2"
rm -f "$disk"
qemu-img create -q -f qcow2 -F qcow2 -b "$installed" "$disk" > /dev/null \
    || die "could not make an overlay"

monitor="${work}/record.in"
rm -f "$monitor"
mkfifo "$monitor"

accel=(-cpu max)
[ -w /dev/kvm ] && accel=(-enable-kvm -cpu host)

disks=(-drive "file=${disk},if=virtio,format=qcow2")
# The cloud-init seed goes back in, or the guest spends a minute of the
# recording looking for a datasource that is no longer there.
[ -s "$seed" ] && disks+=(-drive "file=${seed},if=virtio,format=raw")

echo "record: booting ${distro} and photographing ${seconds}s at ${fps} frames a second"
qemu-system-x86_64 \
    "${disks[@]}" \
    -bios "$firmware" \
    -m 2048 -smp 2 "${accel[@]}" \
    -display none -vga std \
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
    -serial "file:${work}/record-serial.log" \
    -monitor stdio \
    < "$monitor" > "${work}/record-monitor.log" 2>&1 &
qemu=$!

exec 3> "$monitor"

interval=$(awk "BEGIN{print 1/$fps}")
total=$(awk "BEGIN{print int($seconds * $fps)}")
shot=0
while [ "$shot" -lt "$total" ] && kill -0 "$qemu" 2> /dev/null; do
    shot=$((shot + 1))
    printf 'screendump %s/%04d.ppm\n' "$frames" "$shot" >&3 2> /dev/null || true
    sleep "$interval"
done

exec 3>&-
kill "$qemu" 2> /dev/null
wait "$qemu" 2> /dev/null
rm -f "$monitor" "$disk"

count=$(find "$frames" -name '*.ppm' -size +0 | wc -l)
[ "$count" -gt 0 ] || die "no frames were captured"
echo "record: ${count} frames"

# Find the splash and cut to it.
#
# A recording of a whole boot is mostly not the thing anybody came to see: four
# seconds of the firmware's own logo, then a kernel log, then about a second and
# a half of RABA, then a login prompt. Opening a README on somebody else's logo
# would be worse than opening on nothing.
#
# scripts/screen-colours.py already knows the difference between a bitmap, a
# boot log and a centred block of console text, because the distribution tests
# need to. The same classifier picks the range here: the splash, plus a second
# of the quiet console on either side so that the thing it replaces and the
# thing that replaces it are both visible.
range=$(python3 -I - "$frames" "$fps" <<'PYTHON'
import importlib.util, pathlib, sys

here = pathlib.Path(__file__).resolve().parent if "__file__" in dir() else pathlib.Path(".")
spec = importlib.util.spec_from_file_location("sc", "scripts/screen-colours.py")
classifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(classifier)

frames = sorted(pathlib.Path(sys.argv[1]).glob("*.ppm"))
fps = int(sys.argv[2])
kinds = [classifier.look(f)[0] for f in frames]

# The longest *run* of centred frames, not every centred frame there is. A login
# prompt with one short line on it classifies as centred too, so taking the span
# from the first to the last would quietly include the whole rest of the boot.
best = run = None
for i, kind in enumerate(kinds + [None]):
    if kind == "centred":
        run = i if run is None else run
        continue
    if run is not None:
        if best is None or i - run > best[1] - best[0]:
            best = (run, i)
        run = None

if best is None:
    print(f"{frames[0].stem} {frames[-1].stem} none")
else:
    start, stop = best
    # Enough of the console on either side to show what the splash replaced and
    # what replaced it, and no more. A full second of padding is most of the
    # image when the splash itself lasts under one: a hero that is two thirds
    # black screen is a hero of a black screen.
    pad = max(2, min(fps // 3, (stop - start) // 2))
    first = max(0, start - pad)
    last = min(len(frames) - 1, stop - 1 + pad)
    print(f"{frames[first].stem} {frames[last].stem} {stop - start}")
PYTHON
) || die "could not classify the frames"
read -r first last drawn <<< "$range"
if [ "$drawn" = none ]; then
    echo "record: no splash in this recording; using all of it" >&2
else
    echo "record: the splash is ${drawn} frames; cutting ${first}..${last}"
fi

mkdir -p "$(dirname "$out")"
palette="${frames}/palette.png"

# Centred on the screen, which is where a splash is by definition.
window="crop=${crop%x*}:${crop#*x}"
# `dither=none` because console text is flat colour: dithering invents noise in
# the one place the image has none, and a dithered glyph is a damaged glyph.

# Two passes, because a GIF has 256 colours and letting ffmpeg pick them per
# frame produces a different palette every frame and a file several times the
# size. The console uses sixteen; `max_colors` says so rather than paying for
# 256 slots that are never filled.
# How long the cut runs, as a duration, because `-t` before an input is the one
# way ffmpeg will limit *both* passes the same: as an output option `-frames:v`
# would be read as applying to the palette image in the second pass and fail.
frame_count=$((10#$last - 10#$first + 1))
length=$(awk "BEGIN{printf \"%.3f\", $frame_count / $fps}")

ffmpeg -loglevel error -y \
    -start_number "$((10#$first))" -framerate "$fps" -t "$length" -i "${frames}/%04d.ppm" \
    -vf "${window},palettegen=max_colors=32:stats_mode=full" \
    "$palette" || die "palettegen failed"

ffmpeg -loglevel error -y \
    -start_number "$((10#$first))" -framerate "$fps" -t "$length" -i "${frames}/%04d.ppm" \
    -i "$palette" \
    -lavfi "${window}[v];[v][1:v]paletteuse=dither=none" \
    -loop 0 "$out" || die "the gif was not written"

size=$(du -h "$out" | cut -f1)
echo "record: ${out} (${size})"

[ "$keep" = 1 ] || rm -rf "$frames"
