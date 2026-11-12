#!/usr/bin/env bash
# Vendor fastfetch's ASCII logo art at a pinned commit.
#
# fastfetch compiles its logos into the binary rather than shipping them as data,
# so the art has to come from the repository. Updating is deliberate: change the
# two constants below, run this, and read the diff. See docs/risks.md.
#
# usage: scripts/vendor-fastfetch-logos.sh
set -euo pipefail

FASTFETCH_TAG="2.71.0"
FASTFETCH_COMMIT="8c9d9c0449dd515e0088c4a60a38d2dc40c5ccd0"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="$root/vendor/fastfetch-logos"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "fetching fastfetch $FASTFETCH_TAG ($FASTFETCH_COMMIT)"
curl -fsSL "https://github.com/fastfetch-cli/fastfetch/archive/$FASTFETCH_COMMIT.tar.gz" \
    -o "$work/fastfetch.tar.gz"

# Extract only the art. Nothing else from the archive is read or run.
tar -xzf "$work/fastfetch.tar.gz" -C "$work" \
    "fastfetch-$FASTFETCH_COMMIT/src/logo/ascii" \
    "fastfetch-$FASTFETCH_COMMIT/src/common/color.h" \
    "fastfetch-$FASTFETCH_COMMIT/LICENSE"

src="$work/fastfetch-$FASTFETCH_COMMIT"
rm -rf "$dest/ascii"
mkdir -p "$dest"
cp -r "$src/src/logo/ascii" "$dest/ascii"
cp "$src/LICENSE" "$dest/LICENSE"
cp "$src/src/common/color.h" "$dest/color.h"

count=$(find "$dest/ascii" -name '*.txt' | wc -l)
bytes=$(find "$dest/ascii" -name '*.txt' -exec cat {} + | wc -c)

cat > "$dest/PINNED" <<EOF
tag $FASTFETCH_TAG
commit $FASTFETCH_COMMIT
logos $count
bytes $bytes
EOF

echo "vendored $count logos, $bytes bytes, into vendor/fastfetch-logos/ascii"
