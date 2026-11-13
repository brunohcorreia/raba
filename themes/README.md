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

## The one that is not a logo

`themes/generic.toml` is the theme `Theme=auto` falls back to when a machine's `ID=`
and `ID_LIKE=` match nothing we ship. It is a framed prompt and a spinner: no logo, no
trademark, nothing claiming to be a distribution it is not. Its art is inline in the
recipe, so the theme is one file, and it ships in the `raba` package rather than in
`raba-themes` — a splash whose fallback can be uninstalled is a splash that draws
nothing on an unknown machine.

## What ships, and why these

Fifty. There is no authoritative count of users per distribution, so the set is drawn
from four sources that measure different populations and disagree with each other in
useful ways:

| source | what it measures | leaders |
|---|---|---|
| [Steam hardware survey](https://store.steampowered.com/hwsurvey?platform=linux), Oct 2026 | Linux gamers who opted in | CachyOS 16.4%, SteamOS Holo 13.9%, Arch 8.9%, Mint 8.4%, Bazzite 6.7% |
| Stack Overflow developer survey 2025 | developers | Ubuntu 27.8%, Debian 11.4%, then Fedora and Arch |
| W3Techs | web servers | Ubuntu, Debian, CentOS, RHEL and its rebuilds |
| [distro.watch/rankings](https://distro.watch/rankings), 7 Nov 2026 | search and encyclopedia attention, 51 distributions | Ubuntu 43%, Debian 12%, Mint 10%, Arch 7.2%, Kali 7.2%, CachyOS 4.9% |

The last one is the widest and the only one that covers the long tail, because it is
the only one that does not need a distribution to be big enough to show up in a survey
sample. It is built from Google Trends and Wikipedia pageviews, which makes it a
measure of attention rather than installs — but attention is what decides whether
somebody goes looking for a theme, and a distribution nobody searches for is a
distribution nobody will miss here.

Two sources were tried and are not used, for reasons worth recording so nobody repeats
the work:

**Reddit subscriber counts** cannot be had. Reddit blocks unauthenticated access to
`/r/<name>/about.json`, and the third-party aggregators that republish the numbers
disagree by more than fifty per cent on the same subreddit — r/archlinux shows up as
226K, 285K and 344K across three of them, with only one carrying a date. A number that
imprecise cannot order anything.

**DistroWatch page hits** are a leading indicator rather than a user count, and the
earlier version of this file dismissed them too flatly. They are the measure that put
CachyOS at the top roughly a year before the Steam survey did, so they do carry real
signal about where attention is heading. They are still not install counts, and
DistroWatch says so itself. Treat them as an early warning about what to add next, not
as a ranking.

The enterprise four — RHEL, CentOS, Rocky, AlmaLinux — are here for the server side,
where a splash matters less but the install base is large. Raspberry Pi OS is here for
a hardware population nothing else counts. The Ubuntu flavours — Kubuntu, Lubuntu,
Xubuntu — are separate entries because they have separate logos and separate
identities, whatever the package archive says.

### What the widest source caught

Checking the set against all 51 ranked distributions found a hole that the survey-based
sources could not: **Omarchy** sits ninth worldwide by search share, ahead of Zorin,
NixOS, Manjaro and Pop!_OS, all four of which already shipped. It is young enough that
no survey has sampled it yet. **Tails**, **Qubes OS**, **antiX** and **Puppy Linux**
were the same kind of miss further down.

### Two that are still missing

**Bazzite** is 6.7% of Linux Steam users and eighth worldwide by search share, and
fastfetch has no logo for it. **Nobara** has one, drawn in braille, which no console
font has. Both need art drawn by hand rather than extracted. That is a contained job
that needs no Rust.

## Adding a distribution

This needs no Rust, and the tests in `crates/raba-gen/tests/themes.rs` read this
directory at run time, so a new theme is covered the moment you add it.

The quickest route is no art file at all. 545 fastfetch logos are vendored, so a
recipe can name one and inherit its colours:

```toml
[logo]
source = "fastfetch:Omarchy"
```

```sh
raba-gen --list-logos omarchy          # check the name, and that a console can draw it
raba-gen build themes/mine.toml --preview
```

Use the extractor below instead when you want the art in a file you can edit — which is
what every theme here does, because several needed hand edits that a `fastfetch:` source
cannot carry.

```sh
scripts/fastfetch-logo.py arch themes/
./scripts/build-themes.sh
raba --preview themes/arch.raba
```

The script renders the logo twice — once with every colour slot forced to a known
value, which recovers the `$1`..`$9` markers, and once untouched, which recovers the
colours the distribution actually uses — then pairs them by position. The recipe it
writes is faithful to the branding rather than to a guess.

It also refuses a name fastfetch does not have. That check is not pedantry: asking for
a missing logo does not fail, it falls back to the logo of the distribution *you* are
running. Without the check, `fastfetch-logo.py bazzite` on a CachyOS machine writes a
CachyOS logo into `bazzite.txt` and says nothing.

If the art is tall enough that a spinner under it would overflow 25 lines, the recipe
is written with `position = "right"` instead. Parrot is the example.

Then look at it, because nothing above can:

```sh
raba-gen build themes/mine.toml --preview
```

Finally record it, so that a later change to the renderer cannot alter your theme
without somebody noticing:

```sh
RABA_BLESS=1 cargo test -p raba-gen --test golden
```

`themes/golden/` holds one file per theme: the characters, the foreground plane and the
background plane, as a modelled terminal sees them after the real encoder has drawn
them. Read the diff before committing a re-record.

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
- fit 80x25, the size `raba`(8) assumes when it cannot read the console. Parrot's art
  is 24 lines, so its spinner sits beside the logo rather than under it
- keep backgrounds in 0–7, because the high bit there is blink and not brightness
- animate, and loop
- cost one canvas in frame 0 and a handful of cells per frame after it

## Credit

Art comes from [fastfetch](https://github.com/fastfetch-cli/fastfetch), MIT licensed.
Distribution logos are trademarks of their respective projects.

## Colours fastfetch writes in 256 colours

Twenty-nine of fastfetch's logos name their colour as `38;5;N` rather than as one of the
sixteen a Linux console has. `scripts/fastfetch-logo.py` converts those: it works out
what the value means in RGB and picks the nearest console colour, measuring with redmean
and, for a saturated source, refusing to answer "grey" — a logo has a colour on purpose,
and the worst of the fifteen coloured choices is closer to the intent than the best grey.

**It is an approximation and sometimes it is wrong.** Azure sits between cyan and blue
and the arithmetic has to pick one. If a logo comes out in a colour its project would not
recognise, change the number in the recipe and say why in a comment above it; nothing
regenerates a recipe that already exists unless somebody asks it to.

The first one this mattered for was Omarchy, whose `38;5;149` was being dropped entirely
— the reader understood `31` and `1;36` and nothing else — so the recipe said
`colors = []` and the logo drew in the default grey.
