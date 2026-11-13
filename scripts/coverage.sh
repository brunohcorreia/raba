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

llvm-profdata merge -sparse "${out}/raw"/*.profraw -o "${out}/all.profdata" \
    || die "the profiles would not merge"

# Only our own code. Without this the report is nine tenths registry sources,
# and "the standard library is 3% covered" is not a finding.
ignore=(--ignore-filename-regex='(/\.cargo/registry/|/rustc/|/tests?/|/target/)')

common=("${objects[@]}" --instr-profile="${out}/all.profdata" "${ignore[@]}")

if [ -n "$show" ]; then
    llvm-cov show "${common[@]}" --show-line-counts-or-regions --use-color "$show"
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
