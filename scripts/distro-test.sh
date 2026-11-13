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
keep_installed=0
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
        # Leave an installed disk behind for scripts/resilience.sh, which boots
        # one over and over — power cut at a chosen second, a sabotage case on a
        # disk — and has no business spending ten minutes installing each time.
        --keep-installed) keep_installed=1 ;;
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
        # Rebuilt whenever the tree has moved on. Caching them on existence
        # alone meant an Arch run could install a package built before the fix
        # it was meant to be testing — which happened, and cost a run and the
        # half hour of looking at the wrong thing afterwards.
        local newest_package
        # `|| true` is load-bearing. On a tree that has never built a package
        # the glob matches nothing, `ls` exits 2, `pipefail` carries that out of
        # the command substitution and `set -e` ends the run — exit 2, no
        # message, before a single line of output. Which is what a new
        # contributor got for running this first.
        newest_package=$(ls -t "${packages}"/*.pkg.tar.* 2> /dev/null | head -1 || true)
        if [ -z "$newest_package" ] || [ -n "$(find "$root" -type f \
                -not -path "${root}/target/*" -not -path "${root}/.git/*" \
                -newer "$newest_package" -print -quit 2> /dev/null)" ]; then
            echo "distro-test: building the packages (the tree is newer than the last build)"
            rm -f "${packages}"/*.pkg.tar.*
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
        cat << 'CASE'
  - path: /usr/local/bin/raba-case
    permissions: '0755'
    content: |
CASE
        indented "${root}/integration/tests/distro/case.sh"
        cat << 'SURVIVED'
  - path: /usr/local/bin/raba-survived
    permissions: '0755'
    content: |
SURVIVED
        indented "${root}/integration/tests/distro/survived.sh"
        cat << 'UNITS'
  - path: /etc/systemd/system/raba-case.service
    content: |
      [Unit]
      Description=RABA test: break the splash on purpose
      # Before the splash and after the root filesystem is writable, which is
      # the whole window in which an installed RABA can be sabotaged.
      DefaultDependencies=no
      After=systemd-remount-fs.service
      Before=raba-start.service
      [Service]
      Type=oneshot
      RemainAfterExit=yes
      ExecStart=/usr/local/bin/raba-case
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=sysinit.target
  - path: /etc/systemd/system/raba-survived.service
    content: |
      [Unit]
      Description=RABA post-boot checks after a sabotage
      After=multi-user.target
      ConditionPathExists=/run/raba-case-applied
      [Service]
      Type=oneshot
      ExecStart=/usr/local/bin/raba-survived
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=multi-user.target
  - path: /usr/local/bin/raba-watch
    permissions: '0755'
    content: |
UNITS
        indented "${root}/integration/tests/distro/watch.sh"
        cat << 'WATCHUNIT'
  - path: /etc/systemd/system/raba-watch.service
    content: |
      [Unit]
      Description=RABA test: which terminal is on screen
      # No default dependencies, so this starts beside the splash rather than
      # after the filesystems — and therefore no condition on a file under
      # /var either, which on Fedora is not mounted yet at that point and
      # skipped the whole unit. Running on the install boot too costs twelve
      # seconds of sampling a console nobody is drawing on.
      DefaultDependencies=no
      After=raba-start.service
      [Service]
      Type=simple
      ExecStart=/usr/local/bin/raba-watch
      [Install]
      WantedBy=sysinit.target
  - path: /etc/systemd/system/raba-check.service
    content: |
      [Unit]
      Description=RABA post-boot checks
      After=multi-user.target
      ConditionPathExists=/var/lib/raba-installed
      # A sabotaged boot gets raba-survived.service instead: asking whether the
      # initramfs still has a theme in it, on a machine whose themes were
      # truncated on purpose, is asking the wrong question.
      ConditionPathExists=!/run/raba-case-applied
      [Service]
      Type=oneshot
      ExecStart=/usr/local/bin/raba-check
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=multi-user.target
runcmd:
  - /usr/local/bin/raba-install
  - systemctl enable raba-check.service raba-watch.service raba-case.service raba-survived.service
  - touch /var/lib/raba-installed
  - [sh, -c, "echo raba-test-rebooting > /dev/console"]
  - systemctl reboot
WATCHUNIT
    } > "${work}/seed/user-data"

    # Look at it before a VM has to. cloud-init does not fail a boot over a
    # broken cloud-config: it logs a warning nobody is reading, installs
    # nothing, and leaves a login prompt that looks exactly like a machine still
    # working. One mismatched heredoc delimiter in the block above cost a
    # forty-minute run that way, and the script it was supposed to embed ended
    # up in the file as prose.
    #
    # Not a YAML parser — PyYAML is not a dependency this harness is allowed to
    # grow. The check is narrower and catches the thing that actually happens:
    # everything the heredocs emit is either a top-level key we know about or
    # indented under one, so an unindented line that is not in this list is text
    # that escaped a heredoc.
    local leaked
    leaked=$(grep -vn '^[[:space:]]' "${work}/seed/user-data" \
        | grep -Ev '^[0-9]+:(#cloud-config|write_files:|runcmd:|users:)?$' || true)
    [ -z "$leaked" ] || die "the cloud-config has lines that escaped a heredoc: ${leaked}"

    mkfs.vfat -n CIDATA -C "$seed" 1440 > /dev/null 2>&1 || die "could not make the seed"
    mcopy -i "$seed" "${work}/seed/meta-data" "${work}/seed/user-data" :: \
        || die "could not write the seed"

    local size_kib
    size_kib=$(du -sk "$staging" | cut -f1)
    # `-C` refuses to create a file that is already there, so a run killed
    # before its cleanup would otherwise poison every run after it with
    # "could not make the payload disk" and no further explanation.
    rm -f "$payload"
    mkfs.vfat -n RABAPKGS -C "$payload" $((size_kib + 8192)) > /dev/null 2>&1 \
        || die "could not make the payload disk (mkfs.vfat on ${payload})"
    mcopy -i "$payload" "$staging"/* :: || die "could not copy the payload"

    # --- boot it -------------------------------------------------------------

    echo "distro-test: booting ${distro} (up to ${timeout_seconds}s)"
    local monitor="${work}/monitor.in"
    rm -f "$monitor"
    mkfifo "$monitor"

    # Ctrl-C has to take the guest with it. Each distribution runs in its own
    # subshell, so this trap belongs to that subshell and is undone when the
    # next one starts. Without it an interrupted run leaves a headless qemu
    # photographing a screen nobody is watching into target/ — a gigabyte and a
    # half of frames, found days later.
    # Not `local`: an EXIT trap runs after the function has returned, when its
    # locals are already gone, and reading one there is an unbound variable
    # under `set -u` rather than a cleanup.
    qemu_pid=
    monitor_path="$monitor"
    cleanup_guest() {
        [ -n "$qemu_pid" ] && kill "$qemu_pid" 2> /dev/null
        rm -f "$monitor_path"
    }
    trap cleanup_guest EXIT INT TERM

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
    qemu_pid=$qemu

    # Held open for the life of the loop: qemu quits the monitor at EOF.
    exec 3> "$monitor"

    # The splash is on the screen for about a second and a half, somewhere
    # inside a run that lasts ten minutes. Sampling the whole run at that rate
    # would be fourteen gigabytes of screenshots; sampling it at one a second
    # misses the splash more often than it finds it — a Fedora run caught it in
    # exactly one frame out of 378, and a Debian run that did everything right
    # caught it in none.
    #
    # So: follow the guest. The installer says `raba-test: rebooting` on the
    # console when it is done, and the checks print a TAP plan once the splash
    # is long over. Between those two lines is the second boot, which is the
    # only part of this worth photographing, and it is sampled four times a
    # second. Outside them a frame every three seconds is plenty to notice that
    # a machine is stuck on a firmware screen.
    # A frame is three megabytes, so the fast window is capped as well as
    # bounded: a guest that reboots and then hangs forever must not fill the
    # disk while nobody is watching.
    local shots=0 fast=0
    local deadline=$((SECONDS + timeout_seconds))
    local interval
    while kill -0 "$qemu" 2> /dev/null && [ "$SECONDS" -lt "$deadline" ]; do
        interval=3
        if [ "$fast" -lt 400 ] \
            && grep -qa 'raba-test-rebooting' "${work}/serial.log" 2> /dev/null \
            && ! grep -qa 'in-guest:' "${work}/serial.log" 2> /dev/null; then
            interval=0.25
            fast=$((fast + 1))
        fi
        shots=$((shots + 1))
        printf 'screendump %s/shots/%04d.ppm\n' "$work" "$shots" >&3 2> /dev/null || true
        sleep "$interval"
    done

    local timed_out=0
    if kill -0 "$qemu" 2> /dev/null; then
        timed_out=1
        kill "$qemu" 2> /dev/null || true
    fi
    exec 3>&-
    wait "$qemu" 2> /dev/null || true
    qemu_pid=

    # --- what came back ------------------------------------------------------

    local serial="${work}/serial.log"
    local tap="${work}/tap.log"
    local plan failures
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

    if [ "$keep_installed" = 1 ]; then
        # The overlay as the guest left it: RABA installed, the units enabled,
        # cloud-init already done with this instance. The seed stays with it
        # because a boot without the datasource it was installed from spends a
        # minute looking for it.
        mv -f "$run" "${work}/installed.qcow2"
        echo "distro-test: kept ${work}/installed.qcow2"
    fi
    if [ "$keep" != 1 ]; then
        rm -f "$run" "$payload" "$monitor"
        [ "$keep_installed" = 1 ] || rm -f "$seed"
        rm -rf "$staging"
    fi

    [ "$timed_out" = 1 ] && echo "distro-test: ${distro} timed out" >&2
    if [ -z "$plan" ]; then
        echo "distro-test: ${distro} never ran the checks. The last of what it said:" >&2
        tail -30 "$tap" | sed 's/^/#   /' >&2
        # Which half failed, named out loud. A run where the guest never even
        # reached the reboot is a *harness* failure — most likely this file's
        # own cloud-config, which cloud-init will have complained about and then
        # carried on without, leaving a machine sitting at a login prompt doing
        # nothing for the whole timeout. That happened, cost forty minutes, and
        # the cause was a colon inside an unquoted YAML scalar.
        if grep -qa 'raba-test-install-failed' "$tap"; then
            echo "distro-test: the install script failed. Where:" >&2
            grep -a 'raba-test-install-failed' "$tap" | tail -3 | sed 's/^/#   /' >&2
        elif ! grep -qa 'raba-test-rebooting' "$tap"; then
            echo "distro-test: it never got as far as installing. The seed:" >&2
            grep -aiE 'cloud-init.*(error|traceback|failed|invalid|yaml)' "$tap" \
                | tail -5 | sed 's/^/#   /' >&2
            echo "#   (user-data is at ${work}/seed/user-data)" >&2
        fi
        return 1
    fi
    if [ "$failures" != "0" ]; then
        echo "distro-test: ${distro}: ${failures} failed" >&2
        return 1
    fi
    # A photograph is evidence, not the verdict. QEMU renders the VGA device in
    # whatever mode it last saw programmed, and a guest whose initramfs carries
    # no KMS driver writes into a firmware framebuffer the emulator is not
    # looking at — so a correctly drawn console photographs black on Debian and
    # in colour on Fedora, for reasons that have nothing to do with RABA. The
    # guest reads its own console out of /dev/vcs and that check is in the plan
    # above; this only says what the camera saw.
    if [ "$drew" != 1 ]; then
        echo "distro-test: ${distro}: nothing the camera caught looked like a splash;" >&2
        echo "             the console check in the plan above is what settles it." >&2
    fi
    echo "distro-test: ${distro} passed"
    return 0
}

status=0
for distro in "${distros[@]}"; do
    # In a subshell, so that `die` inside it ends that distribution and not the
    # run. It exits, because a failed download or a missing payload has to stop
    # the function right there — and without the subshell that exit took the
    # other two distributions with it, in a loop written precisely to keep
    # going.
    ( one_distro "$distro" ) || status=1
done
exit "$status"
