#!/bin/bash
# First boot, Fedora: same as Debian, with dnf and dracut. The dracut module
# needs no configuration — its own check() decides whether to be included — so
# the only step after `make install` is regenerating the image.
set -x

# A screen the host can photograph; see raba-test-console.
/usr/local/bin/raba-test-console
dnf install -y cargo scdoc make
cargo --version

mkdir -p /mnt/raba /usr/src/raba
mount -L RABAPKGS /mnt/raba
tar -C /usr/src/raba --strip-components=1 -xf /mnt/raba/raba-source.tar.gz
umount /mnt/raba

cd /usr/src/raba
make all
make install

printf '[Boot]\nTheme=fedora\nTimeout=25\n' > /etc/raba/raba.conf
raba --check

dracut --force --regenerate-all
systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
