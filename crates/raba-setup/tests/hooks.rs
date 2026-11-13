//! The initramfs glue, tested where it lives: in a shell.
//!
//! `integration/tests/mkinitcpio-build.sh` runs the real build hook against a
//! stub of mkinitcpio that fails and counts errors exactly where the real one
//! does. This file exists so that `cargo test` runs it — the project's rule is
//! that a contributor gets real confidence from one command, and a hook that
//! can fail somebody's kernel update is not a thing to leave to a CI job
//! nobody runs locally.
//!
//! The real `raba-setup` is handed to the script, so the passing cases are end
//! to end: configuration files in a sandbox, through the real parser, through
//! the real freeze, into the hook.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integration/tests").join(name)
}

/// Run one of the shell harnesses, with the real `raba-setup` for it to drive.
fn harness(name: &str) {
    let output = Command::new("bash")
        .arg(script(name))
        .arg(env!("CARGO_BIN_EXE_raba-setup"))
        .output()
        .unwrap_or_else(|why| panic!("running {name}: {why}"));

    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{report}");
}

#[test]
fn the_mkinitcpio_hook_cannot_fail_an_image_build() {
    harness("mkinitcpio-build.sh");
}

/// Fedora and openSUSE with systemd; Void, Gentoo and Chimera without it.
#[test]
fn the_dracut_module_cannot_abort_an_image_build() {
    harness("dracut-build.sh");
}

/// Debian and Ubuntu, where a hook that exits non-zero aborts
/// `update-initramfs` and takes the kernel update with it.
#[test]
fn the_initramfs_tools_hook_cannot_abort_an_image_build() {
    harness("initramfs-tools-build.sh");
}

fn integration(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integration").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|why| panic!("{}: {why}", path.display()))
}

/// `key=value` out of a unit file, every occurrence.
fn directives<'a>(unit: &'a str, key: &str) -> Vec<&'a str> {
    unit.lines()
        .filter_map(|line| line.trim().split_once('='))
        .filter(|(name, _)| *name == key)
        .map(|(_, value)| value)
        .collect()
}

/// The units are a contract with every init-system integration in
/// `docs/integration.md`, and nothing in a `cargo test` would otherwise read
/// them. These assert the handful of directives that are decisions rather than
/// boilerplate.
#[test]
fn the_systemd_units_say_what_the_documentation_says_they_say() {
    let start = integration("systemd/raba-start.service");
    assert_eq!(directives(&start, "ExecStart"), ["/usr/bin/raba"]);
    assert_eq!(directives(&start, "WantedBy"), ["sysinit.target"]);
    assert_eq!(directives(&start, "Type"), ["exec"]);
    // The kill switch, honoured before the process is even started.
    assert!(start.contains("ConditionKernelCommandLine=!raba.enable=0"), "{start}");
    // The runtime check is in the binary; this one is so that a skipped unit
    // reads usefully in `systemctl status`. See docs/coexistence.md.
    assert!(start.contains("ConditionPathExists=!/run/plymouth.pid"), "{start}");

    // The initramfs unit: the same job under a name of its own, because systemd
    // carries unit state across switch_root and would not start a second unit
    // of the same name in the real root. Everything above has to hold for it
    // too, plus the marker that keeps it inside an image.
    let initrd = integration("systemd/raba-initrd.service");
    assert_eq!(directives(&initrd, "ExecStart"), ["/usr/bin/raba"]);
    assert_eq!(directives(&initrd, "WantedBy"), ["sysinit.target"]);
    assert!(initrd.contains("ConditionPathExists=/etc/initrd-release"), "{initrd}");
    assert!(initrd.contains("ConditionKernelCommandLine=!raba.enable=0"), "{initrd}");

    for name in ["systemd/raba-quit.service", "systemd/raba-quit-wait.service"] {
        let unit = integration(name);
        let exec = directives(&unit, "ExecStart");
        // `-`: a display manager that failed to start because the splash was
        // absent would be the splash breaking a boot.
        assert!(exec.iter().all(|line| line.starts_with('-')), "{name}: {exec:?}");
        assert_eq!(directives(&unit, "WantedBy"), ["multi-user.target"], "{name}");
        assert_eq!(directives(&unit, "Before"), ["display-manager.service"], "{name}");
        // Not Plymouth's TimeoutSec=0. A splash that hangs must not own the
        // boot it was supposed to decorate.
        let timeout = directives(&unit, "TimeoutSec");
        assert_eq!(timeout.len(), 1, "{name}");
        let seconds: u32 = timeout.first().unwrap().parse().expect("a number of seconds");
        assert!((1..=60).contains(&seconds), "{name}: TimeoutSec={seconds}");
    }
}

/// The hook names units in a list; the package ships them in a directory. A
/// rename on one side and not the other is an image that builds fine and has
/// no splash, which is the failure mode this project is least able to notice.
#[test]
fn every_unit_the_mkinitcpio_hook_asks_for_is_one_we_ship() {
    let hook = integration("mkinitcpio/install/raba");
    let line = hook
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("units=("))
        .expect("the hook lists its units");

    let named: Vec<&str> = line
        .trim_start_matches("units=(")
        .trim_end_matches(')')
        .split_whitespace()
        .map(|word| word.trim_matches('\''))
        .collect();
    assert!(!named.is_empty(), "{line}");
    for unit in named {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../integration/systemd").join(unit);
        assert!(path.is_file(), "the hook asks for {unit} and nothing ships it");
    }
}

/// The init-system service files, which no harness can run.
///
/// OpenRC, runit, s6 and dinit need a machine running them to prove anything,
/// and that is the full-distribution matrix in M6. What *can* be checked here
/// is the thing most likely to be wrong in a file nobody on this machine can
/// execute: that it refers to the program by the name the package installs,
/// and that the one decision each init system gets wrong by default is
/// addressed in writing.
#[test]
fn every_init_system_gets_a_service_file_that_says_the_right_thing() {
    for name in ["init/openrc-raba", "init/runit/raba/run", "init/s6/raba/run", "init/dinit-raba"] {
        let text = integration(name);
        assert!(text.contains("raba"), "{name}");
        assert!(text.contains("SPDX-License-Identifier") || text.contains("type ="), "{name}");
    }

    // runit and s6 restart a service that exits, and a splash that came back
    // after the handoff would draw over whatever took the screen.
    assert!(integration("init/runit/raba/finish").contains("sv down"));
    assert!(integration("init/s6/raba/run").contains("down-when-finished"));
    // dinit says it in the service file itself.
    assert!(integration("init/dinit-raba").contains("restart = false"));
    // OpenRC has the only one that can hand off politely by itself.
    assert!(integration("init/openrc-raba").contains("raba-ctl quit --wait"));
}

/// Every runtime script starts the splash the same way, and every build hook
/// resolves it the same way.
///
/// Three dialects of shell, three initramfs generators, one answer to "which
/// raba" — `command -v`, which is what `add_binary` and `inst_multiple` do
/// anyway. A script that hardcoded `/usr/bin/raba` would work everywhere the
/// package put it there and nowhere else.
#[test]
fn nothing_in_the_glue_hardcodes_a_path_to_the_binary() {
    for name in [
        "mkinitcpio/install/raba",
        "mkinitcpio/hooks/raba",
        "dracut/50raba/module-setup.sh",
        "dracut/50raba/raba-start.sh",
        "initramfs-tools/hooks/raba",
        "initramfs-tools/scripts/init-premount/raba",
    ] {
        let text = integration(name);
        for line in text.lines() {
            let code = line.split('#').next().unwrap_or_default();
            assert!(!code.contains("/usr/bin/raba"), "{name}: {line}");
        }
    }
}
