#!/bin/bash
# Run RABA's mkinitcpio build hook against a stub of mkinitcpio.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The hook's one hard promise is that it cannot fail an image build, and the
# mechanism it has to respect is specific: mkinitcpio installs a RETURN trap
# that counts any `add_*` function returning non-zero, and a non-zero count
# fails the whole image — and with it the kernel update that triggered it.
#
# So the stub below is not a mock that always succeeds. Each `add_*` fails
# exactly where the real one fails — a binary that is not in PATH, a file that
# is not there, a unit that does not exist — and counts the failure the same
# way. Then the hook is run in every state a half-installed system can be in,
# and each case asserts the same thing: nothing was counted.
#
# Usage: mkinitcpio-build.sh [path-to-raba-setup]
#
# With a real raba-setup the good cases are end to end: real configuration
# files, real theme resolution, real freezing, through the real hook.

set -uo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
hook="$here/../mkinitcpio/install/raba"
runtime_hook="$here/../mkinitcpio/hooks/raba"
units="$here/../systemd"
real_setup="${1:-}"

passed=0
failed=0

# --------------------------------------------------------------- the stub ---

# Everything the hook did, kept in files rather than in variables: `add_file`
# is called from inside a pipeline, and a subshell cannot write to its parent's
# arrays. The real mkinitcpio has the same blind spot — a failing `add_*` in a
# pipeline increments a copy of `_builderrors` that is then thrown away — so
# recording to a file makes this stub strictly stricter than the thing it
# imitates, which is the direction a test should err in.
declare -i _builderrors
declare -a calls warnings
declare _d_hooks sandbox stdin_capture
declare calls_file warnings_file errors_file

record() { printf '%s\n' "$*" >> "$calls_file"; }

# mkinitcpio's own accounting, reproduced: a failing add_* is a build error.
add_error() {
    printf '%s\n' "$*" >> "$errors_file"
    record "FAILED: $*"
}

warning() {
    # shellcheck disable=SC2059  # the format string is the caller's, as in mkinitcpio
    printf -- "$@" >> "$warnings_file"
    printf '\n' >> "$warnings_file"
}

error() {
    # The real one prints and the caller usually returns 1; here it is a
    # failure on its own, because this hook is not allowed to call it.
    # shellcheck disable=SC2059
    add_error "ERROR: $(printf -- "$@")"
}

add_binary() {
    if ! type -P "$1" > /dev/null; then
        add_error "add_binary $1"
        return 1
    fi
    record "add_binary $1"
}

add_file() {
    if [[ "$1" == '-' ]]; then
        cat > "$stdin_capture"
        record "add_file - $2 ${3:-}"
        return 0
    fi
    if [[ ! -f "$1" ]]; then
        add_error "add_file $1"
        return 1
    fi
    record "add_file $1"
}

add_symlink() {
    record "add_symlink $1 ${2:-}"
}

add_runscript() {
    local script
    if ! script="$(PATH="$_d_hooks" type -P raba)"; then
        add_error 'add_runscript'
        return 1
    fi
    record "add_runscript $script"
}

# Defined only in the systemd cases, exactly as mkinitcpio defines it only when
# the systemd install hook has been sourced. It looks units up the way the real
# one does, in RABA_UNIT_PATH.
define_add_systemd_unit() {
    add_systemd_unit() {
        if ! PATH="${RABA_UNIT_PATH:-}" type -P "$1" > /dev/null; then
            add_error "add_systemd_unit $1"
            return 1
        fi
        record "add_systemd_unit $1"
    }
}

# Read back what the hook did, including from its pipelines.
collect() {
    mapfile -t calls < "$calls_file"
    mapfile -t warnings < "$warnings_file"
    _builderrors="$(wc -l < "$errors_file")"
}

# ------------------------------------------------------------- the sandbox --

# A machine in whatever state the case wants: which binaries exist, which theme
# files exist, whether the runtime hook was installed.
make_sandbox() {
    sandbox="$(mktemp -d)"
    mkdir -p "$sandbox/bin" "$sandbox/themes" "$sandbox/hooks" "$sandbox/units" "$sandbox/etc"
    stdin_capture="$sandbox/frozen"
    calls_file="$sandbox/calls"
    warnings_file="$sandbox/warnings"
    errors_file="$sandbox/errors"
    : > "$stdin_capture"
    : > "$calls_file"
    : > "$warnings_file"
    : > "$errors_file"
    _builderrors=0
    calls=()
    warnings=()
    _d_hooks="$sandbox/hooks"
    export RABA_UNIT_PATH="$sandbox/units"
    unset -f add_systemd_unit

    printf 'not really a theme\n' > "$sandbox/themes/generic.raba"
    printf 'ID=generic\n' > "$sandbox/etc/os-release"
    printf '[Boot]\nTheme=generic\n' > "$sandbox/etc/raba.conf"

    install_binary raba
    install_binary raba-ctl
    install_setup
    cp "$runtime_hook" "$sandbox/hooks/raba"
    cp "$units"/*.service "$sandbox/units/"
    PATH="$sandbox/bin:$PATH"
}

install_binary() {
    printf '#!/bin/sh\nexit 0\n' > "$sandbox/bin/$1"
    chmod +x "$sandbox/bin/$1"
}

# raba-setup, pointed entirely inside the sandbox so that the host's own
# configuration cannot reach the thing under test.
install_setup() {
    {
        printf '#!/bin/sh\n'
        if [[ -n "$real_setup" ]]; then
            printf 'exec %q "$@" --config %q --defaults /nonexistent \\\n' \
                "$real_setup" "$sandbox/etc/raba.conf"
            printf '  --themes %q --cmdline /nonexistent --os-release %q\n' \
                "$sandbox/themes" "$sandbox/etc/os-release"
        else
            # No real binary: a stub that answers the two questions the hook
            # asks. Keeps this script runnable on its own.
            printf 'case "$1" in\n'
            printf '  --freeze) printf "[Boot]\\nTheme=%s\\n" %q ;;\n' \
                "$sandbox/themes/generic.raba"
            printf '  --print-files) printf "%%s\\n" %q ;;\n' "$sandbox/themes/generic.raba"
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

run_hook() {
    unset -f build help
    # shellcheck disable=SC1090  # the path is a variable by design
    . "$hook"
    # Anything the hook's own children print goes to a file: mkinitcpio would
    # put it on the console, and here it would break the report.
    build 2>> "$sandbox/stderr"
    hook_status=$?
    collect
}

check() {
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

# Every case ends here: no build errors, and build() itself returned 0.
assert_no_build_error() {
    if (( _builderrors != 0 )); then
        printf 'mkinitcpio would have counted %d build error(s)' "$_builderrors"
        return 1
    fi
    if (( hook_status != 0 )); then
        printf 'build() returned %d' "$hook_status"
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
        printf 'the hook said nothing about it'
        return 1
    fi
    return 0
}

frozen_contains() {
    if ! grep -q -- "$1" "$stdin_capture"; then
        printf 'the frozen configuration has no %s: %s' "$1" "$(cat "$stdin_capture")"
        return 1
    fi
    return 0
}

# ---------------------------------------------------------------- the cases -

printf '# mkinitcpio build hook\n'

# 1. A busybox initramfs with everything in place.
make_sandbox
run_hook
check 'a complete install adds the binary, the art and the configuration' assert_no_build_error
check 'the binary goes in' called 'add_binary raba'
check 'so does raba-ctl, for the emergency hook' called 'add_binary raba-ctl'
check 'the theme goes in at its own path' called "add_file $sandbox/themes/generic.raba"
check 'the configuration is written, not copied' called 'add_file - /etc/raba/raba.conf 644'
check 'and it is the frozen one' frozen_contains '\[Boot\]'
check 'a busybox image gets the runtime hook' called "add_runscript $sandbox/hooks/raba"
check 'and nothing was said about it' test -z "${warnings[*]:-}"
clean_sandbox

# 2. A systemd initramfs: same hook name, different half.
make_sandbox
define_add_systemd_unit
run_hook
check 'a systemd initramfs is the same hook' assert_no_build_error
check 'the unit goes in' called 'add_systemd_unit raba-start.service'
check 'wanted from inside the image' \
    called 'add_symlink /usr/lib/systemd/system/sysinit.target.wants/raba-start.service ../raba-start.service'
check 'and no runtime hook, which systemd would never run' not_called 'add_runscript*'
clean_sandbox

# 3. The program is not installed. Somebody has `raba` in HOOKS and no raba.
make_sandbox
rm -f "$sandbox/bin/raba"
run_hook
check 'no raba binary is a warning, not a failed image' assert_no_build_error
check 'and nothing is put in the image' not_called 'add_*'
check 'and it says so' warned
clean_sandbox

# 4. The setup tool is missing: nothing can be frozen.
make_sandbox
rm -f "$sandbox/bin/raba-setup"
run_hook
check 'no raba-setup is a warning, not a failed image' assert_no_build_error
check 'and nothing is put in the image' not_called 'add_*'
check 'and it says so' warned
clean_sandbox

# 5. No theme resolves. The commonest real failure: a themes package that is
#    not installed, or a Theme= naming something that is not there.
make_sandbox
rm -f "$sandbox/themes"/*.raba
run_hook
check 'no theme is a warning, not a failed image' assert_no_build_error
check 'and nothing is put in the image' not_called 'add_*'
check 'and it says so' warned
clean_sandbox

# 6. The art disappears between being named and being copied. A pacman
#    transaction running while mkinitcpio does, which is not hypothetical on a
#    rolling distribution.
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
check 'art that vanished mid-build is a warning, not a failed image' assert_no_build_error
check 'and add_file is never handed the missing path' not_called 'add_file /vanished/*'
check 'and it says so' warned
clean_sandbox

# 7. A systemd initramfs with the units not installed.
make_sandbox
define_add_systemd_unit
rm -f "$sandbox/units"/*.service
run_hook
check 'a missing unit is a warning, not a failed image' assert_no_build_error
check 'and add_systemd_unit is never asked for it' not_called 'add_systemd_unit*'
check 'and it says so' warned
clean_sandbox

# 8. A busybox initramfs with the runtime hook not installed.
make_sandbox
rm -f "$sandbox/hooks/raba"
run_hook
check 'a missing runtime hook is a warning, not a failed image' assert_no_build_error
check 'and add_runscript is never called' not_called 'add_runscript*'
check 'and it says so' warned
clean_sandbox

# 9. raba-ctl alone is missing. The splash still works; only the emergency
#    hook loses its way of asking it to leave.
make_sandbox
rm -f "$sandbox/bin/raba-ctl"
run_hook
check 'a missing raba-ctl does not stop the splash going in' assert_no_build_error
check 'the splash is still added' called 'add_binary raba'
check 'and raba-ctl is not' not_called 'add_binary raba-ctl'
clean_sandbox

printf '1..%d\n' "$(( passed + failed ))"
if (( failed )); then
    printf 'mkinitcpio-build: %d of %d failed\n' "$failed" "$(( passed + failed ))"
    exit 1
fi
printf 'mkinitcpio-build: passed\n'
