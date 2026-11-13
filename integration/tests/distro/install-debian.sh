#!/bin/bash
# First boot, Debian: there is no RABA package, so this is what a packager or a
# person building from source does — the distribution's own toolchain, `make`,
# `make install`, and the distribution's own initramfs generator.
#
# The cargo that comes from apt is the one that matters: if it is older than the
# MSRV, that is the finding, and it shows up as this script failing loudly.
# `-e`, not just `-x`. cloud-init's runcmd does not stop at a failed entry: a
# build that died halfway was followed by `touch /var/lib/raba-installed` and a
# reboot, and the guest came back presenting itself as installed. Stopping here
# puts the failing command last in the serial log instead of burying it under
# the cascade, and the marker gives the host something to grep for.
set -ex
trap 'echo "raba-test-install-failed: line ${LINENO}: ${BASH_COMMAND}" > /dev/console' ERR

# A screen the host can photograph; see raba-test-console.
/usr/local/bin/raba-test-console
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends cargo scdoc make initramfs-tools
cargo --version

mkdir -p /mnt/raba /usr/src/raba
mount -L RABAPKGS /mnt/raba
tar -C /usr/src/raba --strip-components=1 -xf /mnt/raba/raba-source.tar.gz
umount /mnt/raba

cd /usr/src/raba
# --offline would need a vendored tree; this is a network test as much as a
# build test, and a packager has the network too.
make all
make install

printf '[Boot]\nTheme=debian\nTimeout=25\n' > /etc/raba/raba.conf
raba --check

echo 'FRAMEBUFFER=y' > /etc/initramfs-tools/conf.d/raba
update-initramfs -u -k all
systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
