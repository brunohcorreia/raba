#!/usr/bin/env bash
# Build the AUR package out of the working tree and check what lands inside it.
#
# makepkg is given the real PKGBUILD, unedited. The only trick is that the
# tarball it expects is put next to it first — makepkg does not download a
# source it already has — so what is packaged is this tree rather than a
# published tag that may not exist yet.
#
# Which is also why `--skipchecksums` is here. The recipe pins the sha256 of
# the published tarball, as an AUR recipe must; the tarball this builds is the
# working tree under the same name, and it will never have that digest. The
# check being skipped is the one this substitution deliberately breaks, and
# nothing else: `makepkg -f` in packaging/aur/raba, which is what the release
# procedure in docs/packaging.md actually runs, still verifies it.
#
# Usage:
#   packaging/tests/package.sh           build and check
#   packaging/tests/package.sh --check   run the suite inside the build too
#   packaging/tests/package.sh --keep    leave the packages behind

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
recipe="${root}/packaging/aur/raba"
work="${root}/target/package-test"

run_check=0
keep=0
for arg in "$@"; do
    case "$arg" in
        --check) run_check=1 ;;
        --keep) keep=1 ;;
        -h | --help)
            sed -n '2,12p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "package-test: unknown option $arg" >&2
            exit 2
            ;;
    esac
done

skip() {
    echo "# skipped: $*"
    exit 0
}

command -v makepkg > /dev/null || skip "makepkg is not installed (package: pacman)"
command -v scdoc > /dev/null || skip "scdoc is not installed (package: scdoc)"

pkgver="$(sed -n 's/^pkgver=//p' "${recipe}/PKGBUILD")"
[ -n "$pkgver" ] || { echo "package-test: no pkgver in the PKGBUILD" >&2; exit 1; }

rm -rf "$work"
mkdir -p "$work"
cp "${recipe}/PKGBUILD" "${recipe}/raba.install" "$work/"

# The working tree, not HEAD: this is run while changing the thing it checks,
# and a packaging mistake should be caught before it is committed. git's own
# ignore list decides what is source, so target/ and generated themes stay out.
( cd "$root" && bsdtar -czf "${work}/raba-${pkgver}.tar.gz" \
    -s "|^\./|raba-${pkgver}/|" \
    --exclude ./.git --exclude ./target --exclude ./notas . )

echo "package-test: makepkg"
makepkg_args=(--noconfirm --nodeps --force --skipchecksums)
[ "$run_check" = 1 ] || makepkg_args+=(--nocheck)
if ! (cd "$work" && makepkg "${makepkg_args[@]}" > makepkg.log 2>&1); then
    sed 's/^/#   /' "${work}/makepkg.log" | tail -40 >&2
    echo "package-test: makepkg failed" >&2
    exit 1
fi

# --- what is in them ---------------------------------------------------------

count=0
failed=0
tap() {
    count=$((count + 1))
    if [ "$1" = 0 ]; then
        echo "ok ${count} - $2"
    else
        failed=$((failed + 1))
        echo "not ok ${count} - $2"
        [ -n "${3:-}" ] && echo "#   $3"
    fi
}

contents() {
    local package
    package="$(find "$work" -maxdepth 1 -name "${1}-${pkgver}-*.pkg.tar*" | head -1)"
    [ -n "$package" ] || return 1
    bsdtar -tf "$package" | sed 's|^|/|;s|/$||'
}

# `.gz` as well as the plain name: makepkg compresses man pages on the way in,
# so the path the PKGBUILD writes is not the path that ends up in the archive.
has() {
    local list="$1" path="$2" what="$3"
    if printf '%s\n' "$list" | grep -qxE -- "${path}(\.gz)?"; then
        tap 0 "$what"
    else
        tap 1 "$what" "$path is not in the package"
    fi
}

raba="$(contents raba)" || { echo "package-test: no raba package was built" >&2; exit 1; }
themes="$(contents raba-themes)" || { echo "package-test: no raba-themes package" >&2; exit 1; }
gen="$(contents raba-gen)" || { echo "package-test: no raba-gen package" >&2; exit 1; }

has "$raba" /usr/bin/raba                              'the splash is in the package'
has "$raba" /usr/bin/raba-ctl                          'so is the client'
has "$raba" /usr/bin/raba-setup                        'and the setup tool'
has "$raba" /etc/raba/raba.conf                        'the configuration pacman is told to back up'
has "$raba" /usr/share/raba/raba.defaults              'and the defaults that belong to the package'
has "$raba" /usr/share/raba/themes/generic.raba        'the fallback theme ships with the program'
has "$raba" /usr/lib/initcpio/install/raba             'the mkinitcpio install hook'
has "$raba" /usr/lib/initcpio/hooks/raba               'the mkinitcpio runtime hook'
has "$raba" /usr/lib/dracut/modules.d/50raba/module-setup.sh 'the dracut module'
has "$raba" /usr/lib/systemd/system/raba-start.service 'the systemd units'
has "$raba" /usr/lib/systemd/system/raba-initrd.service 'the initramfs unit'
has "$raba" /usr/share/libalpm/hooks/90-raba.hook      'the pacman hook that notices a stale image'
has "$raba" /usr/share/man/man8/raba.8                 'the man page'
has "$raba" /usr/share/licenses/raba/LICENSE           'the licence'

has "$themes" /usr/share/raba/themes/arch.raba         'a distribution logo is in raba-themes'
has "$gen"    /usr/bin/raba-gen                        'the authoring tool is its own package'
has "$gen"    /usr/share/man/man5/raba-theme.5         'with the format it writes documented'

# A theme in both packages is a file conflict, and pacman refuses the install.
overlap="$(comm -12 \
    <(printf '%s\n' "$raba" | grep '\.raba$' | sort) \
    <(printf '%s\n' "$themes" | grep '\.raba$' | sort))"
if [ -z "$overlap" ]; then
    tap 0 'no theme is in two packages at once'
else
    tap 1 'no theme is in two packages at once' "$(echo "$overlap" | tr '\n' ' ')"
fi

# The themes package is art. A binary in it would mean the split went wrong.
if printf '%s\n' "$themes" | grep -q '^/usr/bin/'; then
    tap 1 'raba-themes ships no binaries' "$(printf '%s\n' "$themes" | grep '^/usr/bin/' | tr '\n' ' ')"
else
    tap 0 'raba-themes ships no binaries'
fi

stray="$(printf '%s\n' "$raba" "$themes" "$gen" | grep -E '^/(usr/local|opt)/' || true)"
if [ -z "$stray" ]; then
    tap 0 'nothing is installed outside the FHS paths a package may use'
else
    tap 1 'nothing is installed outside the FHS paths a package may use' "$stray"
fi

echo "1..${count}"
[ "$keep" = 1 ] || rm -rf "${work}/src" "${work}/pkg"

if [ "$failed" != 0 ]; then
    echo "package-test: ${failed} failed" >&2
    exit 1
fi
echo "package-test: passed"
