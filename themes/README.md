# Themes

A theme is two files: the art, and a recipe saying what colour each part is.

```
themes/arch.txt     the art, with $1..$9 marking colour changes
themes/arch.toml    the recipe
themes/arch.raba    the build output — generated, gitignored
```

Build them all:

```sh
./scripts/build-themes.sh
raba --preview themes/arch.raba --loops 0
```

## Adding a distribution

This needs no Rust, and the tests in `crates/raba-gen/tests/themes.rs` read this
directory at run time, so a new theme is covered the moment you add it.

```sh
scripts/fastfetch-logo.py arch themes/
./scripts/build-themes.sh
raba --preview themes/arch.raba
```

The script renders the logo twice — once with every colour slot forced to a known
value, which recovers the `$1`..`$9` markers, and once untouched, which recovers the
colours the distribution actually uses — then pairs them by position. The recipe it
writes is faithful to the branding rather than to a guess.

Then look at it, because nothing above can.

## The part that catches people

**Several fastfetch logos cannot be used at boot.** They are drawn with quadrant
blocks (`▘▝▖▗▙▟▛▜`) or sextants, which give sub-cell detail in a terminal emulator and
are in no console font at all. On a console they are blank space, so the logo arrives
as holes.

NixOS is the example. Its current logo is quadrants and `nixos_small` is sextants, so
this repository ships `NixOS2`, which is ASCII:

```sh
scripts/fastfetch-logo.py NixOS2 themes/ nixos
```

When a build fails on glyphs, look for an ASCII variant before anything else — most
distributions have one, and it rarely has the obvious name. `fastfetch --list-logos`
shows them. `raba-gen --check` tells you which characters are the problem.

`--allow-unsafe-glyphs` exists for themes aimed at a terminal rather than a console.
It is not the answer to this.

See [docs/glyphs.md](../docs/glyphs.md) for why the safe set is what it is.

## Hand edits

The art is generated once and then tracked, not regenerated on every build. Editing it
is expected — Linux Mint's art has one, replacing `´` with `'` so the logo does not
lose pieces when `setfont` runs. Record the edit in the recipe's comments, because
re-running the script silently undoes it.

## What the tests enforce

Every theme in this directory must:

- build with no unsafe **and no unreliable** glyphs — stricter than `raba-gen`, which
  only warns about the second kind. A theme that looks right for the first second of
  boot and wrong afterwards is a bug report nobody can diagnose
- fit 80x25, the size `raba`(8) assumes when it cannot read the console
- keep backgrounds in 0–7, because the high bit there is blink and not brightness
- animate, and loop
- cost one canvas in frame 0 and a handful of cells per frame after it

## Credit

Art comes from [fastfetch](https://github.com/fastfetch-cli/fastfetch), MIT licensed.
Distribution logos are trademarks of their respective projects.
