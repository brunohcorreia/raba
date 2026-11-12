#!/usr/bin/env bash
# Build every theme in themes/ from its art source.
#
# Themes are generated, so themes/*.raba is gitignored and this is how you get one.
# M1 replaces the flags here with themes/*.toml recipes; until then the recipe is
# this script. See docs/milestones.md.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release -p raba-gen
GEN=target/release/raba-gen

# CachyOS: the logo's three colour slots are green, bright green and cyan.
"$GEN" build themes/cachyos.txt -o themes/cachyos.raba \
  --colours 2,10,6 --spinner ascii --spinner-fg 10 --delay 120

for theme in themes/*.raba; do
  "$GEN" check "$theme"
done
