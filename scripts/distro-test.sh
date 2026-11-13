#!/usr/bin/env bash
# Tier 3b: install RABA on a real distribution, reboot it, and look at the screen.
#
# Everything before this tier runs against a kernel plus an initramfs we built.
# This one takes the distribution's own cloud image, installs RABA the way a
# person on that distribution would, rebuilds the real initramfs with the real
# generator, and reboots into it. The checks then run inside the booted system,
# and the host watches the screen through QEMU's monitor while it happens.
#
#   arch     the packages the PKGBUILD produces, with pacman -U, plus mkinitcpio
#   debian   from source with the distribution's own cargo, plus initramfs-tools
#   fedora   from source with the distribution's own cargo, plus dracut
#
# The two source builds are a real question and not a formality: they say
# whether somebody on that distribution can build RABA with what is in its
# repositories today.
#
# No root on the host: the image is a copy-on-write overlay, the cloud-init seed
# and the payload are FAT filesystems made with mkfs.vfat, and QEMU runs as you.
#
# Usage:
#   scripts/distro-test.sh [--distro arch|debian|fedora] [--keep] [--console]
#   scripts/distro-test.sh --all          each of them in turn
#   scripts/distro-test.sh --refresh      download the cloud image again
#
# The download is cached in target/distro/<distro>/base.qcow2 and is the only
# slow part on a second run. See docs/development.md.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
distros=(arch)
keep=0
show_console=0
refresh=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --distro)
            shift
            distros=("${1:-arch}")
            ;;
        --all) distros=(arch debian fedora) ;;
        --keep) keep=1 ;;
        --console) show_console=1 ;;
        --refresh) refresh=1 ;;
        -h | --help)
            sed -n '2,28p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "distro-test: unknown option $1" >&2
            exit 2
            ;;
    esac
    shift
done

die() {
    echo "distro-test: $*" >&2
    exit 1
}

skip() {
    echo "# skipped: $*"
    exit 0
}

for tool in qemu-system-x86_64 qemu-img mkfs.vfat mcopy curl; do
    command -v "$tool" > /dev/null || skip "$tool is not installed"
done
firmware=/usr/share/edk2/x64/OVMF.4m.fd
[ -r "$firmware" ] || firmware=/usr/share/OVMF/OVMF_CODE.fd
[ -r "$firmware" ] || skip "no OVMF firmware found (package: edk2-ovmf)"

# Indent a file so it can be the body of a cloud-config `content: |` block.
indented() {
    sed 's/^/      /' "$1"
}

one_distro() {
    local distro="$1"
    local work="${root}/target/distro/${distro}"
    local base="${work}/base.qcow2"
    local url timeout_seconds
    local payload_kind=source

    case "$distro" in
        arch)
            url="https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2"
            payload_kind=packages
            timeout_seconds=600
            ;;
        debian)
            url="https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2"
            # Installs a toolchain from the network and builds the workspace.
            timeout_seconds=2400
            ;;
        fedora)
            url="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2"
            timeout_seconds=2400
            ;;
        *) die "unknown distribution: ${distro}" ;;
    esac
    timeout_seconds="${RABA_DISTRO_TIMEOUT:-$timeout_seconds}"

    local installer="${root}/integration/tests/distro/install-${distro}.sh"
    [ -r "$installer" ] || die "no installer for ${distro}"

    mkdir -p "${work}/shots"
    rm -f "${work}/shots"/*.ppm "${work}/serial.log"

    # --- the payload ---------------------------------------------------------

    local payload="${work}/payload.img"
    local staging="${work}/payload"
    rm -rf "$staging"
    mkdir -p "$staging"

    if [ "$payload_kind" = packages ]; then
        local packages="${root}/target/package-test"
        if ! ls "${packages}"/*.pkg.tar.* > /dev/null 2>&1; then
            echo "distro-test: building the packages"
            "${root}/packaging/tests/package.sh" > /dev/null \
                || die "the packages did not build"
        fi
        cp "${packages}"/*.pkg.tar.* "$staging/"
    else
        # The working tree, the same tarball the packaging test uses: this is
        # run while changing the thing it checks.
        ( cd "$root" && bsdtar -czf "${staging}/raba-source.tar.gz" \
            -s "|^\./|raba/|" \
            --exclude ./.git --exclude ./target --exclude ./notas . )
    fi

    # --- the cloud image -----------------------------------------------------

    if [ "$refresh" = 1 ] || [ ! -s "$base" ]; then
        echo "distro-test: downloading the ${distro} cloud image (cached after this)"
        curl -fSL --progress-bar -o "${base}.part" "$url" || die "the download failed"
        mv "${base}.part" "$base"
    fi

    local run="${work}/run.qcow2"
    rm -f "$run"
    qemu-img create -q -f qcow2 -F qcow2 -b "$base" "$run" > /dev/null \
        || die "could not make a copy-on-write overlay"

    # --- what the guest is told to do ----------------------------------------

    local seed="${work}/seed.img"
    rm -f "$seed"
    mkdir -p "${work}/seed"

    cat > "${work}/seed/meta-data" << META
instance-id: raba-${distro}
local-hostname: raba-${distro}
META

    # First boot installs and reboots; second boot runs the checks and powers
    # off. The condition on the unit is what keeps the first boot from running
    # the checks before anything is installed.
    {
        cat << 'HEAD'
#cloud-config
write_files:
  - path: /usr/local/bin/raba-test-console
    permissions: '0755'
    content: |
HEAD
        indented "${root}/integration/tests/distro/console.sh"
        cat << 'INSTALL'
  - path: /usr/local/bin/raba-install
    permissions: '0755'
    content: |
INSTALL
        indented "$installer"
        cat << 'MID'
  - path: /usr/local/bin/raba-check
    permissions: '0755'
    content: |
MID
        indented "${root}/integration/tests/distro/check.sh"
        cat << 'TAIL'
  - path: /etc/systemd/system/raba-check.service
    content: |
      [Unit]
      Description=RABA post-boot checks
      After=multi-user.target
      ConditionPathExists=/var/lib/raba-installed
      [Service]
      Type=oneshot
      ExecStart=/usr/local/bin/raba-check
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=multi-user.target
runcmd:
  - /usr/local/bin/raba-install
  - systemctl enable raba-check.service
  - touch /var/lib/raba-installed
  - systemctl reboot
TAIL
    } > "${work}/seed/user-data"

    mkfs.vfat -n CIDATA -C "$seed" 1440 > /dev/null 2>&1 || die "could not make the seed"
    mcopy -i "$seed" "${work}/seed/meta-data" "${work}/seed/user-data" :: \
        || die "could not write the seed"

    local size_kib
    size_kib=$(du -sk "$staging" | cut -f1)
    mkfs.vfat -n RABAPKGS -C "$payload" $((size_kib + 8192)) > /dev/null 2>&1 \
        || die "could not make the payload disk"
    mcopy -i "$payload" "$staging"/* :: || die "could not copy the payload"

    # --- boot it -------------------------------------------------------------

    echo "distro-test: booting ${distro} (up to ${timeout_seconds}s)"
    local monitor="${work}/monitor.in"
    rm -f "$monitor"
    mkfifo "$monitor"

    local display=(-display none)
    [ "$show_console" = 1 ] && display=()
    local accel=(-cpu max)
    [ -w /dev/kvm ] && accel=(-enable-kvm -cpu host)

    qemu-system-x86_64 \
        -drive "file=${run},if=virtio,format=qcow2" \
        -drive "file=${seed},if=virtio,format=raw" \
        -drive "file=${payload},if=virtio,format=raw" \
        -bios "$firmware" \
        -m 2048 \
        -smp 2 \
        "${accel[@]}" \
        "${display[@]}" \
        -vga std \
        -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
        -serial "file:${work}/serial.log" \
        -monitor stdio \
        < "$monitor" > "${work}/monitor.log" 2>&1 &
    local qemu=$!

    # Held open for the life of the loop: qemu quits the monitor at EOF.
    exec 3> "$monitor"

    local shots=0
    local deadline=$((SECONDS + timeout_seconds))
    while kill -0 "$qemu" 2> /dev/null && [ "$SECONDS" -lt "$deadline" ]; do
        shots=$((shots + 1))
        printf 'screendump %s/shots/%04d.ppm\n' "$work" "$shots" >&3 2> /dev/null || true
        # Every second: the splash is up for a handful of them, and a sampling
        # rate close to the thing being sampled is how a real pass turns flaky.
        sleep 1
    done

    local timed_out=0
    if kill -0 "$qemu" 2> /dev/null; then
        timed_out=1
        kill "$qemu" 2> /dev/null || true
    fi
    exec 3>&-
    wait "$qemu" 2> /dev/null || true

    # --- what came back ------------------------------------------------------

    local serial="${work}/serial.log"
    local tap="${work}/tap.log"
    local plan failures drew
    [ -s "$serial" ] || die "${distro}: the guest said nothing at all"

    # The guest's TAP arrives through the journal, so each line comes wrapped in
    # a kernel timestamp and a unit prefix — `[   40.414569] raba-check[442]: ok
    # 1 - ...` — and the console is full of colour. Both are stripped before
    # anything is read, because a harness that cannot see a passing test is
    # worse than one that fails.
    sed -E 's/\x1b\[[0-9;?]*[a-zA-Z]//g; s/\r$//; s/^\[[ 0-9.]+\] //; s/^[A-Za-z0-9_.@-]+\[[0-9]+\]: //' \
        "$serial" > "$tap"

    grep -aE '^(ok|not ok|# |1\.\.)' "$tap" || true

    plan="$(grep -aoE '^1\.\.[0-9]+' "$tap" | tail -1 || true)"
    failures="$(grep -ac '^not ok' "$tap" || true)"

    # Did anything reach the screen, and was it ours? The classifier tells a
    # centred block of console text from a boot log and from the firmware's own
    # logo; see scripts/screen-colours.py.
    local verdict="no screen analysis" best= drew=0
    if command -v python3 > /dev/null; then
        local analysis
        analysis="$(python3 -I "${root}/scripts/screen-colours.py" "${work}/shots" || true)"
        verdict="$(head -1 <<< "$analysis")"
        best="$(sed -n 's/^best=//p' <<< "$analysis")"
        case "$verdict" in *splash*) drew=1 ;; esac
    fi
    echo "# screen: ${verdict}"

    # One frame kept as evidence, the rest thrown away: a run leaves a hundred
    # three-megabyte screenshots behind otherwise.
    if [ -n "$best" ] && [ -f "$best" ]; then
        cp "$best" "${work}/screen.ppm"
        if command -v magick > /dev/null; then
            magick "${work}/screen.ppm" "${work}/screen.png" 2> /dev/null || true
        fi
        echo "# kept: ${work}/screen.$( [ -f "${work}/screen.png" ] && echo png || echo ppm )"
    fi
    [ "$keep" = 1 ] || rm -f "${work}/shots"/*.ppm

    if [ "$keep" != 1 ]; then
        rm -f "$run" "$seed" "$payload" "$monitor"
        rm -rf "$staging"
    fi

    [ "$timed_out" = 1 ] && echo "distro-test: ${distro} timed out" >&2
    if [ -z "$plan" ]; then
        echo "distro-test: ${distro} never ran the checks. The last of what it said:" >&2
        tail -30 "$tap" | sed 's/^/#   /' >&2
        return 1
    fi
    if [ "$failures" != "0" ]; then
        echo "distro-test: ${distro}: ${failures} failed" >&2
        return 1
    fi
    if [ "$drew" != 1 ]; then
        echo "distro-test: ${distro}: the guest passed, but no captured frame showed a" >&2
        echo "             splash. Either it never drew, or the window between the" >&2
        echo "             first frame and the handoff was shorter than the sampling." >&2
        return 1
    fi
    echo "distro-test: ${distro} passed"
    return 0
}

status=0
for distro in "${distros[@]}"; do
    one_distro "$distro" || status=1
done
exit "$status"
