#!/bin/bash
# Run RABA's initramfs-tools hook against a stub of initramfs-tools.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The third dialect, and the one with the most users: Debian and Ubuntu.
# update-initramfs runs hooks under `set -e` and aborts the image on a non-zero
# exit, so every case here asserts an exit code of 0 and a staging directory
# containing either a working splash or nothing at all.
#
# The stub arrives by a side door. The hook sources an absolute path —
# /usr/share/initramfs-tools/hook-functions — which is not there on a machine
# that is not Debian; the `.` fails, the script has no `set -e` of its own, and
# execution carries on with the functions this script exported. That is exactly
# the behaviour being tested, so the hook is run unmodified.
#
# Usage: initramfs-tools-build.sh [path-to-raba-setup]

set -uo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
hook="$here/../initramfs-tools/hooks/raba"
premount="$here/../initramfs-tools/scripts/init-premount/raba"
real_setup="${1:-}"

passed=0
failed=0
declare sandbox DESTDIR calls_file

# What the hook asked initramfs-tools to copy.
copy_exec() {
    if [[ ! -x "$1" ]]; then
        printf 'FATAL: copy_exec %s\n' "$1" >> "$RABA_CALLS"
        return 1
    fi
    printf 'copy_exec %s\n' "$1" >> "$RABA_CALLS"
}
export -f copy_exec

make_sandbox() {
    sandbox="$(mktemp -d)"
    DESTDIR="$sandbox/destdir"
    calls_file="$sandbox/calls"
    mkdir -p "$sandbox/bin" "$sandbox/themes" "$sandbox/etc" "$DESTDIR"
    : > "$calls_file"
    export RABA_CALLS="$calls_file"
    export DESTDIR

    printf 'not really a theme\n' > "$sandbox/themes/generic.raba"
    printf 'ID=generic\n' > "$sandbox/etc/os-release"
    printf '[Boot]\nTheme=generic\n' > "$sandbox/etc/raba.conf"

    printf '#!/bin/sh\nexit 0\n' > "$sandbox/bin/raba"
    cp "$sandbox/bin/raba" "$sandbox/bin/raba-ctl"
    chmod +x "$sandbox/bin/raba" "$sandbox/bin/raba-ctl"
    # The hook copies /usr/bin/raba by absolute path; the sandbox has to have
    # one for copy_exec to find.
    mkdir -p "$sandbox/usr/bin"
    install_setup
    PATH="$sandbox/bin:$PATH"
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

run_hook() {
    bash "$hook" 2>> "$sandbox/stderr"
    hook_status=$?
}

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
        printf '#   calls: %s\n' "$(tr '\n' ' ' < "$calls_file")"
        printf '#   stderr: %s\n' "$(tr '\n' ' ' < "$sandbox/stderr" 2> /dev/null)"
    fi
}

exited_zero() {
    if (( hook_status != 0 )); then
        printf 'the hook exited %d, which aborts update-initramfs' "$hook_status"
        return 1
    fi
    # A copy_exec that failed would have aborted the real thing too.
    if grep -q '^FATAL' "$calls_file"; then
        printf 'initramfs-tools would have aborted: %s' "$(grep '^FATAL' "$calls_file")"
        return 1
    fi
    return 0
}

called() {
    if ! grep -qx -- "$1" "$calls_file"; then
        printf 'no call %s' "$1"
        return 1
    fi
    return 0
}

no_calls() {
    if [[ -s "$calls_file" ]]; then
        printf 'it copied something anyway: %s' "$(tr '\n' ' ' < "$calls_file")"
        return 1
    fi
    return 0
}

frozen_in_image() {
    if ! grep -q '\[Boot\]' "$DESTDIR/etc/raba/raba.conf" 2> /dev/null; then
        printf 'no frozen configuration in the staging directory'
        return 1
    fi
    return 0
}

no_frozen_in_image() {
    if [[ -f "$DESTDIR/etc/raba/raba.conf" ]]; then
        printf 'a configuration was written anyway'
        return 1
    fi
    return 0
}

printf '# initramfs-tools hook\n'

# 1. A complete install. The binaries are looked for at their absolute paths,
#    so the sandbox puts them there.
make_sandbox
cp "$sandbox/bin/raba" "$sandbox/usr/bin/raba"
run_hook
expect 'the hook exits 0' exited_zero
expect 'the configuration is frozen into the staging directory' frozen_in_image
expect 'the theme is copied in' test -f "$DESTDIR/$sandbox/themes/generic.raba"
clean_sandbox

# 2. The program is not installed.
make_sandbox
rm -f "$sandbox/bin/raba"
run_hook
expect 'no raba exits 0 rather than aborting the image' exited_zero
expect 'and copies nothing' no_calls
expect 'and writes no configuration' no_frozen_in_image
clean_sandbox

# 3. No theme resolves.
make_sandbox
rm -f "$sandbox/themes"/*.raba
run_hook
expect 'no theme exits 0 rather than aborting the image' exited_zero
expect 'and copies nothing' no_calls
expect 'and writes no configuration' no_frozen_in_image
clean_sandbox

# 4. The art vanishes mid-build.
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
run_hook
expect 'art that vanished exits 0 rather than aborting the image' exited_zero
expect 'and nothing is copied from the path that vanished' test ! -e "$DESTDIR/vanished"
clean_sandbox

# 5. The runtime script answers `prereqs`, which is how initramfs-tools orders
#    the scripts it is about to run. Getting this wrong hangs or skips a boot.
make_sandbox
output="$(sh "$premount" prereqs)"
status=$?
expect 'the premount script answers prereqs and exits 0' test "$status" -eq 0
expect 'and it names no prerequisite it cannot satisfy' test -z "$output"
clean_sandbox

printf '1..%d\n' "$(( passed + failed ))"
if (( failed )); then
    printf 'initramfs-tools-build: %d of %d failed\n' "$failed" "$(( passed + failed ))"
    exit 1
fi
printf 'initramfs-tools-build: passed\n'
