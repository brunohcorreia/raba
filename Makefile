# One description of where RABA's files go, so that the PKGBUILD, a .deb, an
# .rpm and somebody doing it by hand cannot disagree about it.
#
#   make                 build the binaries, the themes and the man pages
#   sudo make install    all three parts into /usr
#   make install-raba DESTDIR=/tmp/stage   just the program, staged
#
# The three install targets match the three packages: the program, the art and
# the authoring tool. See docs/packaging.md for why they are separate.
#
# `manpages` and not `man`, because a target named after a directory that exists
# is a target make will quietly call up to date.

PREFIX     ?= /usr
DESTDIR    ?=
BINDIR     ?= $(PREFIX)/bin
DATADIR    ?= $(PREFIX)/share
LIBDIR     ?= $(PREFIX)/lib
SYSCONFDIR ?= /etc
MANDIR     ?= $(DATADIR)/man

CARGO       ?= cargo
CARGO_FLAGS ?= --release
TARGETDIR   ?= target/release
SCDOC       ?= scdoc

INSTALL         = install
INSTALL_PROGRAM = $(INSTALL) -Dm755
INSTALL_DATA    = $(INSTALL) -Dm644

MANPAGES = man/raba.8 man/raba-setup.8 man/raba-ctl.1 man/raba.conf.5 \
           man/raba-gen.1 man/raba-theme.5

.PHONY: all binaries themes manpages install install-raba install-themes \
        install-gen install-pacman-hook check clean

all: binaries themes manpages

binaries:
	$(CARGO) build $(CARGO_FLAGS)

# Generated from their recipes, never committed: a stale .raba is a theme that
# does not match the format the binary next to it reads.
themes: binaries
	@for recipe in themes/*.toml; do $(TARGETDIR)/raba-gen build "$$recipe" >/dev/null; done

manpages: $(MANPAGES)

man/%.8: man/%.8.scd
	$(SCDOC) < $< > $@
man/%.5: man/%.5.scd
	$(SCDOC) < $< > $@
man/%.1: man/%.1.scd
	$(SCDOC) < $< > $@

check:
	$(CARGO) test --workspace $(CARGO_FLAGS)

install: install-raba install-themes install-gen

# The program, its configuration, every initramfs and init-system integration,
# and the one theme that is not a logo.
install-raba:
	$(INSTALL_PROGRAM) $(TARGETDIR)/raba       $(DESTDIR)$(BINDIR)/raba
	$(INSTALL_PROGRAM) $(TARGETDIR)/raba-ctl   $(DESTDIR)$(BINDIR)/raba-ctl
	$(INSTALL_PROGRAM) $(TARGETDIR)/raba-setup $(DESTDIR)$(BINDIR)/raba-setup
	$(INSTALL_DATA) packaging/raba.defaults $(DESTDIR)$(DATADIR)/raba/raba.defaults
	$(INSTALL_DATA) packaging/raba.conf     $(DESTDIR)$(SYSCONFDIR)/raba/raba.conf
	$(INSTALL_DATA) themes/generic.raba     $(DESTDIR)$(DATADIR)/raba/themes/generic.raba
	$(INSTALL_DATA) integration/mkinitcpio/install/raba $(DESTDIR)$(LIBDIR)/initcpio/install/raba
	$(INSTALL_DATA) integration/mkinitcpio/hooks/raba   $(DESTDIR)$(LIBDIR)/initcpio/hooks/raba
	$(INSTALL_PROGRAM) integration/dracut/50raba/module-setup.sh \
	    $(DESTDIR)$(LIBDIR)/dracut/modules.d/50raba/module-setup.sh
	$(INSTALL_PROGRAM) integration/dracut/50raba/raba-start.sh \
	    $(DESTDIR)$(LIBDIR)/dracut/modules.d/50raba/raba-start.sh
	$(INSTALL_PROGRAM) integration/initramfs-tools/hooks/raba \
	    $(DESTDIR)$(DATADIR)/initramfs-tools/hooks/raba
	$(INSTALL_PROGRAM) integration/initramfs-tools/scripts/init-premount/raba \
	    $(DESTDIR)$(DATADIR)/initramfs-tools/scripts/init-premount/raba
	@for unit in integration/systemd/*.service; do \
	    $(INSTALL_DATA) "$$unit" "$(DESTDIR)$(LIBDIR)/systemd/system/$$(basename $$unit)"; \
	done
	$(INSTALL_PROGRAM) integration/init/openrc-raba       $(DESTDIR)$(DATADIR)/raba/init/openrc-raba
	$(INSTALL_PROGRAM) integration/init/runit/raba/run    $(DESTDIR)$(DATADIR)/raba/init/runit/run
	$(INSTALL_PROGRAM) integration/init/runit/raba/finish $(DESTDIR)$(DATADIR)/raba/init/runit/finish
	$(INSTALL_PROGRAM) integration/init/s6/raba/run       $(DESTDIR)$(DATADIR)/raba/init/s6/run
	$(INSTALL_DATA)    integration/init/dinit-raba        $(DESTDIR)$(DATADIR)/raba/init/dinit-raba
	$(INSTALL_DATA) man/raba.8       $(DESTDIR)$(MANDIR)/man8/raba.8
	$(INSTALL_DATA) man/raba-setup.8 $(DESTDIR)$(MANDIR)/man8/raba-setup.8
	$(INSTALL_DATA) man/raba-ctl.1   $(DESTDIR)$(MANDIR)/man1/raba-ctl.1
	$(INSTALL_DATA) man/raba.conf.5  $(DESTDIR)$(MANDIR)/man5/raba.conf.5
	$(INSTALL_DATA) LICENSE   $(DESTDIR)$(DATADIR)/licenses/raba/LICENSE
	$(INSTALL_DATA) README.md $(DESTDIR)$(DATADIR)/doc/raba/README.md
	@for doc in docs/integration.md docs/luks.md docs/secure-boot.md docs/security.md \
	            docs/troubleshooting.md docs/compatibility.md; do \
	    $(INSTALL_DATA) "$$doc" "$(DESTDIR)$(DATADIR)/doc/raba/$$(basename $$doc)"; \
	done

# Arch only: the hook that reports an initramfs gone stale against its art.
# Separate because a file under /usr/share/libalpm on a Debian machine is litter.
install-pacman-hook:
	$(INSTALL_DATA) packaging/90-raba.hook $(DESTDIR)$(DATADIR)/libalpm/hooks/90-raba.hook

# The logos, which are the part a trademark complaint would be about.
# generic.raba is not here: it belongs to the program, which has to draw
# something even with this uninstalled.
install-themes:
	@for theme in themes/*.raba; do \
	    case "$$(basename $$theme)" in generic.raba) continue ;; esac; \
	    $(INSTALL_DATA) "$$theme" "$(DESTDIR)$(DATADIR)/raba/themes/$$(basename $$theme)"; \
	done
	$(INSTALL_DATA) themes/README.md $(DESTDIR)$(DATADIR)/doc/raba-themes/README.md

install-gen:
	$(INSTALL_PROGRAM) $(TARGETDIR)/raba-gen $(DESTDIR)$(BINDIR)/raba-gen
	$(INSTALL_DATA) man/raba-gen.1   $(DESTDIR)$(MANDIR)/man1/raba-gen.1
	$(INSTALL_DATA) man/raba-theme.5 $(DESTDIR)$(MANDIR)/man5/raba-theme.5

clean:
	$(CARGO) clean
	rm -f $(MANPAGES)
	rm -f themes/*.raba
