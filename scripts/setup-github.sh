#!/usr/bin/env bash
# Create GitHub milestones and the M0 issues.
#
# Run this once, after the remote exists:
#   gh repo create raba --public --source=. --remote=origin
#   ./scripts/setup-github.sh
#
# Idempotent for milestones; issues are created unconditionally, so run it once.
set -euo pipefail

repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "repository: $repo"

milestone() {
    gh api "repos/$repo/milestones" --method POST \
        -f title="$1" -f description="$2" >/dev/null 2>&1 \
        && echo "  + $1" || echo "  = $1 (exists)"
}

echo "milestones:"
milestone "M0 — See a logo animate"  "Validates the premise: does ASCII art look good at console cell size?"
milestone "M1 — Theme authoring"     "Themes contributable without writing Rust."
milestone "M2 — Import"              ".dur, ANSI and text import."
milestone "M3 — The dangerous part"  "VT layer. VM harness first."
milestone "M4 — Lifecycle"           "Socket, state, resume, handoff."
milestone "M5 — Early boot"          "initramfs hooks, units, raba-setup."
milestone "M6 — Breadth"             "Other inits, LUKS, packaging."

issue() {
    gh issue create --title "$1" --body "$2" --milestone "M0 — See a logo animate" \
        --label "${3:-}" >/dev/null && echo "  + $1"
}

echo "M0 issues:"
issue "raba-core: .raba format types" \
"Define \`Movie\`, \`Frame\`, \`Op\`, \`Palette\` per \`docs/format.md\`.

Freezing the byte layout is part of this issue — the sketch in the doc is not final, and
a format gets specified by being implemented. Update \`docs/format.md\` to match whatever
is built.

No I/O. \`raba-core\` has a dependency budget of zero." "core"

issue "raba-core: encoder" \
"\`Movie\` → bytes. Frame 0 full, later frames as deltas, periodic keyframes.

Include a round-trip test: encode, decode, assert equality." "core"

issue "raba-core: decoder with bounded validation" \
"bytes → \`Movie\`.

The rule from \`docs/security.md\`: **total size comes from the header and is validated
before a single allocation.** Hard ceilings on canvas dimensions and frame count. Every
offset bounds-checked once at load so the renderer can index without further checks.

This is the code that makes the gzip-bomb class of failure structurally impossible. It is
also the highest-value fuzz target in the project." "core,security"

issue "The failure table" \
"The project's central promise expressed as code. See \`docs/development.md\`.

Every malformed input asserts **exit 0 and no panic**: truncated header, \`total_bytes\`
disagreeing with file size, canvas 10^9 x 10^9, frame count over ceiling, op index out of
range, zero frames, CRC mismatch, absent file.

Write this early and review it more carefully than anything else." "core,test"

issue "raba-core: Surface trait" \
"\`trait Surface { fn size(); fn write(); fn restore(); }\` with \`BufferSurface\` for tests.

The abstraction that keeps the impure layer small enough to review line by line, and that
makes \`--preview\` the same code path as a real terminal rather than a separate one." "core"

issue "raba-core: grid to ANSI encoder" \
"Compose a frame into a cell grid, emit escape sequences, minimise redraw by diffing
against the previous grid.

Property test: \`to_ansi(grid, prev)\` applied to \`prev\` yields \`grid\`. Easy to get
subtly wrong, hard to catch by example." "core"

issue "raba-core: glyph validator" \
"Accept ASCII, CP437 block elements and box-drawing. Reject Braille, arrows, mathematical
symbols and emoji.

The Linux console holds 256-512 glyphs; anything outside the loaded font renders as a
replacement character and the kernel discards the codepoint. Validation turns an invisible
boot-time rendering bug into a build error.

Report the offending character, its codepoint, and where it appears." "core"

issue "raba-gen: load fastfetch-style text art" \
"Plain text with \`\$1\`..\`\$9\` colour markers: everything after a marker takes that
colour until the next marker or end of line.

Source art is fastfetch's, MIT, 534 logos." "gen"

issue "raba-gen: compose logo and spinner" \
"Place a spinner relative to a logo. Canvas is the bounding box, not the terminal size.

Frame count is the least common multiple of the element periods, so the loop closes
without a visible jump — a 4-frame pulse beside a 10-frame spinner needs 20 frames." "gen"

issue "raba: --preview" \
"Render a \`.raba\` in the current terminal and exit. No VT, no root, no system state.

Not a debug feature: it is the same core driving a different \`Surface\`, and it is what
makes art iteration possible without a VM. Needed before anything in M3." "cli"

issue "First theme, and the verdict on the premise" \
"Build a theme for CachyOS, run \`raba --preview\`, and **look at it**.

The question this milestone exists to answer: does a ~40x20 cell logo on an 8x16 console
font look good enough to justify the project?

If the answer is no, that is a legitimate outcome and the design docs become a record of
a decision not to proceed. Write the verdict in \`notas/arquitetura.md\` either way." "theme"

echo
echo "done. remaining milestones are in docs/milestones.md; create their issues as you reach them."
