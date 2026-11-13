#!/bin/bash
# First boot, Arch: install the packages the PKGBUILD produced, the way the
# .install message tells a person to, then reboot into the result.
# `-e`, not just `-x`. cloud-init's runcmd does not stop at a failed entry: a
# build that died halfway was followed by `touch /var/lib/raba-installed` and a
# reboot, and the guest came back presenting itself as installed. Stopping here
# puts the failing command last in the serial log instead of burying it under
# the cascade, and the marker gives the host something to grep for.
set -ex
trap 'echo "raba-test-install-failed: line ${LINENO}: ${BASH_COMMAND}" > /dev/console' ERR

# A screen the host can photograph; see raba-test-console.
/usr/local/bin/raba-test-console
mkdir -p /mnt/raba
mount -L RABAPKGS /mnt/raba
pacman -U --noconfirm /mnt/raba/*.pkg.tar.*

# The file RABA is not allowed to edit for you. Here the machine is the user.
sed -i 's/^HOOKS=(\(.*\))$/HOOKS=(\1 raba)/' /etc/mkinitcpio.conf
grep '^HOOKS=' /etc/mkinitcpio.conf

# A named theme, not `auto`: this is a test of the plumbing, and a cloud image
# whose ID= happens not to match would make it a test of the theme set.
printf '[Boot]\nTheme=arch\nTimeout=25\n' > /etc/raba/raba.conf
raba --check

mkinitcpio -P
systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
umount /mnt/raba
