#!/usr/bin/env bash
# What the tests actually execute, line by line and branch by branch.
#
# Not a score to maximise. A coverage report answers one question — "is there
# code here that no test has ever run?" — and in a program whose promise is
# that it never breaks a boot, an unexercised branch on the boot path is a
# branch that will first run on somebody's computer.
#
# Uses rustc's own instrumentation and the LLVM tools already on the machine;
# no cargo-llvm-cov, no tarpaulin, nothing to install. The instrumented build
# lives in its own target directory so it never evicts the ordinary one.
#
# Usage:
#   scripts/coverage.sh                  summary per file
#   scripts/coverage.sh --html           also write target/coverage/html
#   scripts/coverage.sh --show <path>    annotated source for one file
#   scripts/coverage.sh --uncovered      every region no test reaches
#
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${root}/target/coverage"
html=0
show=
uncovered=0

die() {
    echo "coverage: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --html) html=1; shift ;;
        --show) show="${2:-}"; shift 2 ;;
        --uncovered) uncovered=1; shift ;;
        -h | --help) sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

command -v llvm-profdata > /dev/null || die "llvm-profdata is not installed (package: llvm)"
command -v llvm-cov > /dev/null || die "llvm-cov is not installed (package: llvm)"

# The LLVM that wrote the counters and the LLVM that reads them have to agree
# about the format. A major-version mismatch fails with "unsupported instrumentation
# profile format version", which is worth saying before a four-minute build.
system_llvm=$(llvm-cov --version | sed -n 's/.*LLVM version \([0-9]*\).*/\1/p')
rustc_llvm=$(rustc --version --verbose | sed -n 's/^LLVM version: \([0-9]*\).*/\1/p')
[ "$system_llvm" = "$rustc_llvm" ] \
    || die "llvm-cov is LLVM ${system_llvm} and rustc is LLVM ${rustc_llvm}; the profile format will not match"

rm -rf "${out}/raw"
mkdir -p "${out}/raw"

export CARGO_TARGET_DIR="${out}/target"
export RUSTFLAGS="${RUSTFLAGS:-} -C instrument-coverage"
export LLVM_PROFILE_FILE="${out}/raw/%p-%m.profraw"

echo "coverage: building and running the suite with instrumentation"
cargo test --workspace --all-features --no-fail-fast > "${out}/test.log" 2>&1 || {
    tail -30 "${out}/test.log" >&2
    die "the suite did not pass; coverage of a failing suite is not worth reading"
}
grep -hE '^test result:' "${out}/test.log" | awk '
    {passed += $4; failed += $6}
    END {printf "coverage: %d tests passed, %d failed\n", passed, failed}'

# The binaries whose counters are in the profile. `--no-run` prints them as
# JSON; anything else in that stream is not an executable we instrumented.
mapfile -t binaries < <(
    cargo test --workspace --all-features --no-run --message-format=json 2> /dev/null \
        | grep -o '"executable":"[^"]*"' \
        | cut -d'"' -f4
)
[ "${#binaries[@]}" -gt 0 ] || die "cargo listed no test binaries"

objects=()
for binary in "${binaries[@]}"; do
    objects+=(--object "$binary")
done

# What the virtual machine ran, if anybody has asked it. `crates/raba/src/vt`
# is the half of this program that needs a real console and root, so on a host
# alone it reads 0% and the total is a sentence with a hole in it. The profile
# comes out of the guest over the serial line — see `scripts/vm-test.sh
# --coverage` — and the binary that wrote it is a different object from any of
# the test binaries above, built for musl, so llvm-cov needs both.
profiles=("${out}/raw"/*.profraw)
if [ -s "${out}/vm/objects.txt" ]; then
    guest_count=$(find "${out}/vm" -name '*.profraw' | wc -l)
    # Stale is worse than absent, and silently so. A guest binary built from
    # older sources carries an older coverage map: llvm-cov is then holding two
    # descriptions of the same files, reports the union of both, and the extra
    # regions are ones no current line corresponds to. The symptom is a file
    # growing by a hundred uncovered regions after an edit that deleted code.
    # Only the crates the guest binary is built from, and only their `src`:
    # cargo does not relink an unchanged binary, so its mtime stays where it
    # was while a test file that has nothing to do with it moves ahead.
    #
    # The sibling `tests.rs` modules are excluded for the same reason one step
    # further in: they are behind `#[cfg(test)]` and the guest binary is never
    # built with it, so editing one cannot change a single byte of the guest's
    # coverage map. Counting them was sending every run back to the virtual
    # machine for nothing.
    guest_sources=("${root}/crates/raba/src" "${root}/crates/raba-core/src" "${root}/crates/raba-term/src")
    newest_source=$(find "${guest_sources[@]}" -name '*.rs' ! -name 'tests.rs' \
        -newer "$(head -1 "${out}/vm/objects.txt")" -print -quit 2> /dev/null || true)
    if [ -n "$newest_source" ]; then
        echo "coverage: the virtual-machine profile is older than ${newest_source#"${root}/"}," >&2
        echo "coverage: so it is being left out. Run scripts/vm-test.sh --coverage again" >&2
        echo "coverage: to measure crates/raba/src/vt; without it that module reads 0%." >&2
        guest_count=0
    fi
    if [ "$guest_count" -gt 0 ]; then
        while read -r guest; do
            [ -x "$guest" ] || die "the guest binary ${guest} is gone; run scripts/vm-test.sh --coverage again"
            objects+=(--object "$guest")
        done < "${out}/vm/objects.txt"
        while IFS= read -r profile; do
            profiles+=("$profile")
        done < <(find "${out}/vm" -name '*.profraw')
        echo "coverage: including ${guest_count} profile(s) from the virtual machine"
    fi
else
    echo "coverage: no virtual-machine profile; crates/raba/src/vt will read 0%."
    echo "coverage: run scripts/vm-test.sh --coverage first to include it."
fi

llvm-profdata merge -sparse "${profiles[@]}" -o "${out}/all.profdata" \
    || die "the profiles would not merge"

# Only our own product code. Three things are left out, and each omission is a
# claim about what the number means:
#
#   registry and rustc sources   not ours; without this the report is nine
#                                tenths dependencies and "the standard library
#                                is 3% covered" is not a finding.
#   anything under tests/ and    the tests themselves. A suite is 100% covered
#   any file named tests.rs      by definition — it is what ran — so counting it
#                                inflates the one number anybody reads. This is
#                                why `#[cfg(test)]` blocks live in a sibling
#                                `tests.rs` in this tree rather than at the foot
#                                of the module they test.
#   crates/raba-vmtest           the harness that runs the QEMU tier. It is the
#                                instrument, not the thing being measured, and
#                                no `cargo test` executes it.
ignore=(--ignore-filename-regex='(/\.cargo/registry/|/rustc/|/tests?/|/tests\.rs$|/target/|/crates/raba-vmtest/)')

common=("${objects[@]}" --instr-profile="${out}/all.profdata" "${ignore[@]}")

if [ -n "$show" ]; then
    # `--sources`, not a bare path: with every binary passed as `--object`, a
    # trailing positional is read as one more object and llvm-cov answers "not
    # recognized as a valid object file" about a `.rs` file.
    llvm-cov show "${common[@]}" --show-line-counts-or-regions --use-color --sources "$show"
    exit 0
fi

if [ "$uncovered" = 1 ]; then
    # Region-level, because a line can be "covered" while the branch on it
    # never went the other way — which is exactly what this is here to find.
    llvm-cov show "${common[@]}" --region-coverage-lt=1 --show-line-counts-or-regions
    exit 0
fi

llvm-cov report "${common[@]}" --show-region-summary

if [ "$html" = 1 ]; then
    rm -rf "${out}/html"
    llvm-cov show "${common[@]}" --format=html --output-dir="${out}/html" --show-line-counts-or-regions
    echo "coverage: ${out}/html/index.html"
fi
