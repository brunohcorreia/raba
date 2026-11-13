#!/usr/bin/env bash
# Tier 3b: install RABA on a real distribution, reboot it, and look at the screen.
#
# Everything before this tier runs against a kernel plus an initramfs we built.
# This one takes the distribution's own cloud image, installs the packages the
# PKGBUILD produces the way a user would, adds the hook to the real
# /etc/mkinitcpio.conf, rebuilds the real initramfs, and reboots into it. The
# checks then run inside the booted system, and the host watches the screen
# through QEMU's monitor while it happens.
#
# No root on the host: the image is a copy-on-write overlay, the cloud-init seed
# is a FAT filesystem made with mkfs.vfat, and QEMU runs as you.
#
# Usage:
#   scripts/distro-test.sh              Arch, build packages if needed, run
#   scripts/distro-test.sh --keep       leave the disk images behind
#   scripts/distro-test.sh --console    show the guest's screen while it runs
#   scripts/distro-test.sh --refresh    download the cloud image again
#
# The download is cached in target/distro/<distro>/base.qcow2 and is the only
# slow part. See docs/development.md.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
distro=arch
keep=0
show_console=0
refresh=0
timeout_seconds="${RABA_DISTRO_TIMEOUT:-600}"

for arg in "$@"; do
    case "$arg" in
        --keep) keep=1 ;;
        --console) show_console=1 ;;
        --refresh) refresh=1 ;;
        -h | --help)
            sed -n '2,22p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "distro-test: unknown option $arg" >&2
            exit 2
            ;;
    esac
done

work="${root}/target/distro/${distro}"
base="${work}/base.qcow2"
url="https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2"

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

# --- the packages, exactly as a user would get them ---------------------------

packages="${root}/target/package-test"
if ! ls "${packages}"/*.pkg.tar.* > /dev/null 2>&1; then
    echo "distro-test: building the packages"
    "${root}/packaging/tests/package.sh" > /dev/null || die "the packages did not build"
fi
ls "${packages}"/*.pkg.tar.* > /dev/null 2>&1 || skip "no packages to install"

# --- the cloud image ----------------------------------------------------------

mkdir -p "$work"
if [ "$refresh" = 1 ] || [ ! -s "$base" ]; then
    echo "distro-test: downloading the ${distro} cloud image (about 550 MiB, cached)"
    curl -fSL --progress-bar -o "${base}.part" "$url" || die "the download failed"
    mv "${base}.part" "$base"
fi

run="${work}/run.qcow2"
rm -f "$run"
qemu-img create -q -f qcow2 -F qcow2 -b "$base" "$run" > /dev/null \
    || die "could not make a copy-on-write overlay"

# --- what the guest is told to do --------------------------------------------

seed="${work}/seed.img"
payload="${work}/payload.img"
rm -f "$seed" "$payload" "${work}/serial.log"
mkdir -p "${work}/seed" "${work}/payload" "${work}/shots"
rm -f "${work}/shots"/*.ppm

cat > "${work}/seed/meta-data" << 'META'
instance-id: raba-distro-test
local-hostname: raba-test
META

# First boot: install, configure, rebuild the initramfs, reboot. Second boot:
# the unit written below runs the checks and powers off. Everything a person
# would do by hand after reading raba.install, and nothing else.
cat > "${work}/seed/user-data" << 'USER'
#cloud-config
write_files:
  - path: /usr/local/bin/raba-install
    permissions: '0755'
    content: |
      #!/bin/bash
      # First boot. Anything here failing is a failed test, so it is noisy.
      set -x
      mkdir -p /mnt/raba
      mount -L RABAPKGS /mnt/raba
      pacman -U --noconfirm /mnt/raba/*.pkg.tar.*
      # The one file RABA is not allowed to edit for you. Here, the machine is
      # the user, so the test does what the .install message tells a person.
      sed -i 's/^HOOKS=(\(.*\))$/HOOKS=(\1 raba)/' /etc/mkinitcpio.conf
      grep '^HOOKS=' /etc/mkinitcpio.conf
      # A theme that does not depend on this image's ID= matching ours.
      printf '[Boot]\nTheme=arch\nTimeout=25\n' > /etc/raba/raba.conf
      raba --check
      mkinitcpio -P
      systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
      systemctl enable raba-check.service
      umount /mnt/raba
      touch /var/lib/raba-installed
      systemctl reboot
  - path: /usr/local/bin/raba-check
    permissions: '0755'
    content: |
      #!/bin/bash
      # Second boot. TAP on the console, which is the serial line.
      count=0
      failed=0
      tap() {
        count=$((count+1))
        if [ "$1" = 0 ]; then echo "ok ${count} - $2"
        else failed=$((failed+1)); echo "not ok ${count} - $2"; [ -n "$3" ] && echo "#   $3"; fi
      }
      image=/boot/initramfs-linux.img
      contents=$(lsinitcpio "$image" 2>/dev/null)
      grep -qx 'usr/bin/raba' <<< "$contents" \
        && tap 0 'the binary is in the real initramfs' \
        || tap 1 'the binary is in the real initramfs' 'no usr/bin/raba'
      grep -qx 'etc/raba/raba.conf' <<< "$contents" \
        && tap 0 'so is the frozen configuration' \
        || tap 1 'so is the frozen configuration' 'no etc/raba/raba.conf'
      grep -qE '^usr/share/raba/themes/.*\.raba$' <<< "$contents" \
        && tap 0 'and the theme it resolved to' \
        || tap 1 'and the theme it resolved to' 'no theme in the image'
      if raba-setup --check > /tmp/check.out 2>&1; then
        tap 0 'raba-setup says the image is current'
      else
        tap 1 'raba-setup says the image is current' "$(tr '\n' ' ' < /tmp/check.out)"
      fi
      journal=$(journalctl -b --no-pager 2>/dev/null)
      grep -q 'raba: not drawing' <<< "$journal" \
        && tap 1 'raba did not refuse to draw' "$(grep -m1 'not drawing' <<< "$journal")" \
        || tap 0 'raba did not refuse to draw'
      systemctl is-enabled raba-quit.service > /dev/null \
        && tap 0 'the handoff unit is enabled' \
        || tap 1 'the handoff unit is enabled'
      if systemctl is-failed --quiet raba-start.service; then
        tap 1 'no raba unit failed' "$(systemctl status raba-start.service | tail -5 | tr '\n' ' ')"
      else
        tap 0 'no raba unit failed'
      fi
      [ ! -e /run/raba.sock ] \
        && tap 0 'nothing of raba is left running' \
        || tap 1 'nothing of raba is left running' 'the control socket is still there'
      # getty got the screen back, which is what the splash was holding.
      systemctl is-active getty@tty1.service > /dev/null \
        && tap 0 'the login prompt is up' \
        || tap 1 'the login prompt is up'
      echo "1..${count}"
      [ "$failed" = 0 ] && echo "# in-guest: all passed" || echo "# in-guest: ${failed} failed"
      systemctl poweroff
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
USER

mkfs.vfat -n CIDATA -C "$seed" 1440 > /dev/null 2>&1 || die "could not make the seed"
mcopy -i "$seed" "${work}/seed/meta-data" "${work}/seed/user-data" :: \
    || die "could not write the seed"

# The packages, on a disk of their own rather than inlined in the seed: a
# cloud-init payload is not a place to put half a megabyte of binary.
size_kib=$(du -sk "$packages" | cut -f1)
mkfs.vfat -n RABAPKGS -C "$payload" $((size_kib + 4096)) > /dev/null 2>&1 \
    || die "could not make the package disk"
mcopy -i "$payload" "${packages}"/*.pkg.tar.* :: || die "could not copy the packages"

# --- boot it ------------------------------------------------------------------

echo "distro-test: booting ${distro}"
monitor="${work}/monitor.in"
rm -f "$monitor"
mkfifo "$monitor"

display=(-display none)
[ "$show_console" = 1 ] && display=()

accel=(-cpu max)
[ -w /dev/kvm ] && accel=(-enable-kvm -cpu host)

# The monitor's stdin is a fifo this script writes `screendump` into, so the
# host can see what the screen actually had on it. Held open by a writer that
# outlives the loop, or qemu reads EOF and quits the monitor.
qemu-system-x86_64 \
    -drive "file=${run},if=virtio,format=qcow2" \
    -drive "file=${seed},if=virtio,format=raw" \
    -drive "file=${payload},if=virtio,format=raw" \
    -bios "$firmware" \
    -m 2048 \
    "${accel[@]}" \
    "${display[@]}" \
    -vga std \
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
    -serial "file:${work}/serial.log" \
    -monitor stdio \
    < "$monitor" > "${work}/monitor.log" 2>&1 &
qemu=$!

exec 3> "$monitor"

shots=0
deadline=$((SECONDS + timeout_seconds))
while kill -0 "$qemu" 2> /dev/null && [ "$SECONDS" -lt "$deadline" ]; do
    shots=$((shots + 1))
    printf 'screendump %s/shots/%04d.ppm\n' "$work" "$shots" >&3 2> /dev/null || true
    sleep 2
done

if kill -0 "$qemu" 2> /dev/null; then
    echo "distro-test: timed out after ${timeout_seconds}s" >&2
    kill "$qemu" 2> /dev/null || true
fi
exec 3>&-
wait "$qemu" 2> /dev/null || true

# --- what came back -----------------------------------------------------------

serial="${work}/serial.log"
[ -s "$serial" ] || die "the guest said nothing at all"

grep -aE '^(ok|not ok|# |1\.\.)' "$serial" | sed 's/\r$//' || true

plan="$(grep -aoE '^1\.\.[0-9]+' "$serial" | tail -1 || true)"
failures="$(grep -ac '^not ok' "$serial" || true)"

# Did anything ever reach the screen? A boot that drew a splash has a frame with
# more than a console's worth of black in it; a boot that drew nothing does not.
drew=unknown
if command -v python3 > /dev/null; then
    drew="$(python3 -I "${root}/scripts/screen-colours.py" "${work}/shots" || echo unknown)"
fi
echo "# screen: ${drew}"

[ "$keep" = 1 ] || rm -f "$run" "$seed" "$payload" "$monitor"

if [ -z "$plan" ]; then
    echo "distro-test: the guest never ran the checks. The last of what it said:" >&2
    tail -30 "$serial" | sed 's/^/#   /' >&2
    exit 1
fi
if [ "$failures" != "0" ]; then
    echo "distro-test: ${failures} failed" >&2
    exit 1
fi
echo "distro-test: passed"
