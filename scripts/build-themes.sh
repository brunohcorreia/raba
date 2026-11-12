#!/usr/bin/env bash
# Build every theme in themes/ from its recipe.
#
# Themes are generated, so themes/*.raba is gitignored and this is how you get one.
# Each theme is a themes/<name>.toml recipe next to its art; see man 5 raba-theme.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release -p raba-gen
GEN=target/release/raba-gen

for recipe in themes/*.toml; do
  "$GEN" build "$recipe"
done

for theme in themes/*.raba; do
  "$GEN" --check "$theme"
done
