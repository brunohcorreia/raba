#!/bin/bash
# Run RABA's dracut module against a stub of dracut.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The companion to mkinitcpio-build.sh, for the other half of the world: Fedora
# and openSUSE with systemd, and Void, Gentoo and Chimera without it.
#
# dracut aborts an image build when an `inst_*` function is handed something
# that is not there — `dfatal` and exit — so the stubs below do the same, and
# every case asserts that it never happened. An initramfs that fails to build
# is a kernel update that fails, on a distribution whose users will reasonably
# blame the last thing they installed.
#
# Usage: dracut-build.sh [path-to-raba-setup]

set -uo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
moddir_source="$here/../dracut/50raba"
units="$here/../systemd"
real_setup="${1:-}"

passed=0
failed=0

# --------------------------------------------------------------- the stub ---

declare -a calls warnings
declare sandbox initdir moddir calls_file warnings_file fatal_file
declare systemdsystemunitdir module_list

record() { printf '%s\n' "$*" >> "$calls_file"; }

# dracut's dfatal aborts the build. Nothing in this module may reach it.
dfatal() {
    printf '%s\n' "$*" >> "$fatal_file"
    record "FATAL: $*"
}

dwarning() { printf '%s\n' "$*" >> "$warnings_file"; }
dinfo() { :; }

require_binaries() {
    local binary
    for binary in "$@"; do
        if ! type -P "$binary" > /dev/null; then
            return 1
        fi
    done
    return 0
}

inst_multiple() {
    local optional=0 item
    for item in "$@"; do
        if [[ "$item" == '-o' ]]; then
            optional=1
            continue
        fi
        if type -P "$item" > /dev/null || [[ -f "$item" ]]; then
            record "inst $item"
        elif (( optional )); then
            record "skipped $item"
        else
            dfatal "Failed to install $item"
            return 1
        fi
    done
}

inst_simple() {
    if [[ ! -f "$1" ]]; then
        dfatal "Failed to install $1"
        return 1
    fi
    record "inst $1"
}

inst_hook() {
    if [[ ! -f "$3" ]]; then
        dfatal "Failed to install hook $3"
        return 1
    fi
    record "inst_hook $1 $2 ${3##*/}"
}

dracut_module_included() {
    [[ " $module_list " == *" $1 "* ]]
}

collect() {
    mapfile -t calls < "$calls_file"
    mapfile -t warnings < "$warnings_file"
}

# ------------------------------------------------------------- the sandbox --

make_sandbox() {
    sandbox="$(mktemp -d)"
    initdir="$sandbox/initdir"
    moddir="$sandbox/50raba"
    mkdir -p "$sandbox/bin" "$sandbox/themes" "$sandbox/etc" "$initdir" "$moddir"
    mkdir -p "$sandbox/units"
    calls_file="$sandbox/calls"
    warnings_file="$sandbox/warnings"
    fatal_file="$sandbox/fatal"
    : > "$calls_file"
    : > "$warnings_file"
    : > "$fatal_file"
    calls=()
    warnings=()
    module_list=''
    systemdsystemunitdir="$sandbox/units"

    printf 'not really a theme\n' > "$sandbox/themes/generic.raba"
    printf 'ID=generic\n' > "$sandbox/etc/os-release"
    printf '[Boot]\nTheme=generic\n' > "$sandbox/etc/raba.conf"
    cp "$moddir_source"/*.sh "$moddir/"
    cp "$units"/*.service "$sandbox/units/"

    install_binary raba
    install_binary raba-ctl
    install_setup
    PATH="$sandbox/bin:$PATH"
}

install_binary() {
    printf '#!/bin/sh\nexit 0\n' > "$sandbox/bin/$1"
    chmod +x "$sandbox/bin/$1"
}

install_setup() {
    {
        printf '#!/bin/sh\n'
        if [[ -n "$real_setup" ]]; then
            printf 'exec %q "$@" --config %q --defaults /nonexistent \\\n' \
                "$real_setup" "$sandbox/etc/raba.conf"
            printf '  --themes %q --cmdline /nonexistent --os-release %q\n' \
                "$sandbox/themes" "$sandbox/etc/os-release"
        else
            printf 'theme=%q\n' "$sandbox/themes/generic.raba"
            printf '[ -f "$theme" ] || exit 1\n'
            printf 'case "$1" in\n'
            printf '  --freeze) printf "[Boot]\\nTheme=%%s\\n" "$theme" ;;\n'
            printf '  --print-files) printf "%%s\\n" "$theme" ;;\n'
            printf '  *) exit 2 ;;\n'
            printf 'esac\n'
        fi
    } > "$sandbox/bin/raba-setup"
    chmod +x "$sandbox/bin/raba-setup"
}

clean_sandbox() {
    PATH="${PATH#"$sandbox/bin:"}"
    rm -rf -- "$sandbox"
}

# ------------------------------------------------------------- the checks ---

run_module() {
    unset -f check depends install
    # shellcheck disable=SC1091  # the path is a variable by design
    . "$moddir/module-setup.sh"
    check > /dev/null 2>&1
    check_status=$?
    install 2>> "$sandbox/stderr"
    install_status=$?
    collect
}

# Named `expect` and not `check`, because the module under test defines a
# function called `check` and sourcing it would quietly replace this one.
expect() {
    local name="$1"
    shift
    local problem
    if problem="$("$@")"; then
        printf 'ok %d - %s\n' "$(( ++passed + failed ))" "$name"
    else
        (( ++failed ))
        printf 'not ok %d - %s\n' "$(( passed + failed ))" "$name"
        printf '#   %s\n' "$problem"
        printf '#   calls: %s\n' "${calls[*]:-none}"
        printf '#   warnings: %s\n' "${warnings[*]:-none}"
    fi
}

assert_no_fatal() {
    if [[ -s "$fatal_file" ]]; then
        printf 'dracut would have aborted: %s' "$(cat "$fatal_file")"
        return 1
    fi
    if (( install_status != 0 )); then
        printf 'install() returned %d' "$install_status"
        return 1
    fi
    return 0
}

called() {
    local wanted="$1" call
    for call in "${calls[@]:-}"; do
        # shellcheck disable=SC2053  # the glob on the right is the point
        [[ "$call" == $wanted ]] && return 0
    done
    printf 'no call matching %s' "$wanted"
    return 1
}

not_called() {
    local wanted="$1" call
    for call in "${calls[@]:-}"; do
        # shellcheck disable=SC2053  # the glob on the right is the point
        if [[ "$call" == $wanted ]]; then
            printf 'unexpected call %s' "$call"
            return 1
        fi
    done
    return 0
}

warned() {
    if (( ${#warnings[@]} == 0 )); then
        printf 'the module said nothing about it'
        return 1
    fi
    return 0
}

frozen_in_image() {
    local file="$initdir/etc/raba/raba.conf"
    if [[ ! -f "$file" ]]; then
        printf 'no frozen configuration in the image'
        return 1
    fi
    if ! grep -q '\[Boot\]' "$file"; then
        printf 'the frozen configuration is not one: %s' "$(cat "$file")"
        return 1
    fi
    return 0
}

no_frozen_in_image() {
    if [[ -f "$initdir/etc/raba/raba.conf" ]]; then
        printf 'a configuration was written into the image anyway'
        return 1
    fi
    return 0
}

# ---------------------------------------------------------------- the cases -

printf '# dracut module\n'

# 1. Void, Gentoo, Chimera: dracut without systemd.
make_sandbox
run_module
expect 'a complete install puts everything in' assert_no_fatal
expect 'check() says yes' test "$check_status" -eq 0
expect 'the binary goes in' called 'inst raba'
expect 'so does raba-ctl' called 'inst raba-ctl'
expect 'the theme goes in' called "inst $sandbox/themes/generic.raba"
expect 'the configuration is frozen into the image' frozen_in_image
expect 'and a start script runs before udev triggers' called 'inst_hook pre-trigger 10 raba-start.sh'
clean_sandbox

# 2. Fedora, openSUSE: dracut with systemd.
make_sandbox
module_list='systemd udev-rules'
run_module
expect 'a systemd image is the same module' assert_no_fatal
expect 'the unit goes in' called "inst $sandbox/units/raba-initrd.service"
expect 'and nothing sources a shell hook' not_called 'inst_hook*'
expect 'the unit is wanted from inside the image' \
    test -L "$initdir/$systemdsystemunitdir/sysinit.target.wants/raba-initrd.service"
# Its own name, so that the real root still starts raba-start.service after
# switch_root. See integration/systemd/raba-initrd.service.
expect 'and not the real root'"'"'s unit name' \
    test ! -e "$initdir/$systemdsystemunitdir/raba-start.service"
clean_sandbox

# 3. The program is not installed at all.
make_sandbox
rm -f "$sandbox/bin/raba"
run_module
expect 'check() declines when raba is not installed' test "$check_status" -ne 0
clean_sandbox

# 4. No theme resolves.
make_sandbox
rm -f "$sandbox/themes"/*.raba
run_module
expect 'no theme is a warning, not an aborted image' assert_no_fatal
expect 'and nothing is installed' not_called 'inst *'
expect 'and no configuration is written' no_frozen_in_image
expect 'and it says so' warned
clean_sandbox

# 5. The art disappears between being named and being copied.
make_sandbox
cat > "$sandbox/bin/raba-setup" <<'STUB'
#!/bin/sh
case "$1" in
  --freeze) printf '[Boot]\nTheme=/vanished/theme.raba\n' ;;
  --print-files) printf '/vanished/theme.raba\n' ;;
  *) exit 2 ;;
esac
STUB
chmod +x "$sandbox/bin/raba-setup"
run_module
expect 'art that vanished mid-build is a warning, not an aborted image' assert_no_fatal
expect 'and inst_simple is never handed the missing path' not_called 'inst /vanished/*'
expect 'and it says so' warned
clean_sandbox

# 6. A systemd image with the units not installed.
make_sandbox
module_list='systemd'
rm -f "$sandbox/units"/*.service
run_module
expect 'a missing unit is a warning, not an aborted image' assert_no_fatal
expect 'and nothing is installed from the unit directory' not_called "inst $sandbox/units/*"
expect 'and it says so' warned
clean_sandbox

# 7. raba-ctl alone is missing.
make_sandbox
rm -f "$sandbox/bin/raba-ctl"
run_module
expect 'a missing raba-ctl does not abort the image' assert_no_fatal
expect 'the splash still goes in' called 'inst raba'
expect 'and raba-ctl is skipped rather than demanded' called 'skipped raba-ctl'
clean_sandbox

printf '1..%d\n' "$(( passed + failed ))"
if (( failed )); then
    printf 'dracut-build: %d of %d failed\n' "$failed" "$(( passed + failed ))"
    exit 1
fi
printf 'dracut-build: passed\n'
