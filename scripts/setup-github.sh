#!/usr/bin/env bash
# Create the GitHub milestones and issues for work that is still open.
#
#   ./scripts/setup-github.sh
#
# Milestones are idempotent. Issues are matched by title, so re-running will not
# create duplicates — which matters, because this is the kind of script somebody
# runs twice to see what it does.
#
# Completed work is not filed as a closed issue. docs/milestones.md tracks what is
# done and the git history says how; filing eleven issues in order to close them is
# noise in both places.
set -euo pipefail

repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo "repository: $repo"

milestone() {
    if gh api "repos/$repo/milestones" --method POST \
        -f title="$1" -f description="$2" >/dev/null 2>&1; then
        echo "  + $1"
    else
        echo "  = $1 (exists)"
    fi
}

label() {
    gh label create "$1" --color "$2" --description "$3" >/dev/null 2>&1 \
        && echo "  + $1" || echo "  = $1 (exists)"
}

issue() {
    local title=$1 body=$2 milestone=$3 labels=$4
    if gh issue list --state all --search "\"$title\" in:title" \
        --json title -q '.[].title' | grep -qxF "$title"; then
        echo "  = $title (exists)"
        return
    fi
    gh issue create --title "$title" --body "$body" \
        --milestone "$milestone" --label "$labels" >/dev/null
    echo "  + $title"
}

echo "milestones:"
milestone "M0 — See a logo animate"  "Validates the premise: does ASCII art look good at console cell size? Complete."
milestone "M1 — Theme authoring"     "Themes contributable without writing Rust."
milestone "M2 — Import"              ".dur, ANSI and text import."
milestone "M3 — The dangerous part"  "VT layer. VM harness first."
milestone "M4 — Lifecycle"           "Socket, state, resume, handoff."
milestone "M5 — Early boot"          "initramfs hooks, units, raba-setup."
milestone "M6 — Breadth"             "Other inits, LUKS, packaging."

echo "labels:"
label "themes"      "0e8a16" "Theme art, recipes and the generator"
label "core"        "1d76db" "The format, renderer and anything on the boot path"
label "testing"     "fbca04" "Harnesses, fixtures and coverage"
label "good first issue" "7057ff" "Contained, with a clear finish line"

echo "M1 issues:"

issue "Vendor fastfetch logo art at a pinned commit" \
"\`logo.source = \"fastfetch:NAME\"\` is in \`raba-theme(5)\` and currently errors, because
there is no bundled art to resolve it against.

fastfetch compiles its logos into the binary rather than shipping files, so the art has
to come from its repository. Vendor at a pinned commit, record the commit, and treat
updates as deliberate — see \`docs/risks.md\`.

Credit matters here: distribution logos are trademarks of their projects, and fastfetch
is MIT. \`docs/packaging.md\` already says what to credit.

\`scripts/fastfetch-logo.py\` extracts one logo from an installed fastfetch and is the
stopgap, not the answer.

Blocks \`--list-logos\`." \
"M1 — Theme authoring" "themes"

issue "raba-gen --list-logos and --preview" \
"Both are documented in \`raba-gen(1)\` and both error as unimplemented.

\`--list-logos\` needs the vendored art, so it waits on that issue.

\`--preview\` renders a built theme in the current terminal instead of writing a file.
\`raba --preview\` already does this; the open question is whether \`StdoutSurface\` moves
somewhere both binaries can use, or whether raba-gen keeps its own fifteen lines. Putting
terminal I/O into \`raba-core\` would break the rule that the core is pure, so duplication
may be the cheaper of the two." \
"M1 — Theme authoring" "themes"

issue "Golden render tests for themes" \
"Every shipped theme, rendered to bytes and compared against a recorded expectation, so
a change to the renderer or the composer that alters what a theme looks like has to be
acknowledged rather than discovered.

The renderer is deterministic and there is already a terminal model in
\`crates/raba-core/tests/terminal/\` that turns its output back into a grid. A golden file
of the grid is more readable in a diff than a golden file of escape sequences, and it
does not churn when an unrelated optimisation changes the byte stream." \
"M1 — Theme authoring" "testing"

issue "Themes for the first ten distributions" \
"One recipe and one art file each, under \`themes/\`. Arch, Debian, Fedora, Ubuntu, Void,
Gentoo, Alpine, openSUSE, NixOS and Linux Mint is a reasonable first ten.

Each needs: art that fits 80x25, colours that survive the 16-colour fit, and a clean
\`raba-gen --check\`. The CachyOS theme in \`themes/\` is the worked example.

Good first issue: adding one distribution is self-contained and needs no Rust." \
"M1 — Theme authoring" "themes,good first issue"

echo
echo "done. docs/milestones.md is the plan; this only files what is open."
