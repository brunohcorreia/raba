//! The named conflict checks, with a Plymouth made of files.
//!
//! The real check — "is the terminal in `KD_GRAPHICS`" — lives in the VT layer
//! and is exercised in the virtual machine, where there is a terminal to put
//! into graphics mode. These are the ones that exist to produce a better message
//! than "the terminal is busy", and they can be checked anywhere, because what
//! they look at is a pid file and a directory.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba::conflict::{detect, Conflict, Where};
use raba::control::{wire::Reply, Listener};
use std::path::PathBuf;
use std::time::Duration;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-conflict-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    /// Somewhere to look that has nothing in it.
    fn places(&self) -> Where {
        Where {
            plymouth_pid: self.0.join("plymouth.pid"),
            proc: self.0.join("proc"),
            socket: self.0.join("raba.sock"),
        }
    }

    /// A `/proc` with one process in it.
    fn fake_process(&self, pid: u32, name: &str) {
        let dir = self.0.join("proc").join(pid.to_string());
        std::fs::create_dir_all(&dir).expect("fake proc");
        std::fs::write(dir.join("comm"), format!("{name}\n")).expect("comm");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The ordinary case, and the one that has to be fast and quiet.
#[test]
fn an_empty_machine_has_no_conflict() {
    let scratch = Scratch::new("clear");
    assert_eq!(detect(&scratch.places()), None);
}

#[test]
fn a_plymouth_pid_file_is_a_conflict_and_names_the_pid() {
    let scratch = Scratch::new("pidfile");
    std::fs::write(scratch.places().plymouth_pid, "1234\n").expect("pid file");
    assert_eq!(detect(&scratch.places()), Some(Conflict::PlymouthPidFile(Some(1234))));
}

/// A pid file that is empty, or truncated by a crash, is still a pid file.
#[test]
fn an_unreadable_pid_file_is_still_a_conflict() {
    let scratch = Scratch::new("pidfile-junk");
    for contents in ["", "\n", "not a number"] {
        std::fs::write(scratch.places().plymouth_pid, contents).expect("pid file");
        assert_eq!(
            detect(&scratch.places()),
            Some(Conflict::PlymouthPidFile(None)),
            "{contents:?}"
        );
    }
}

/// plymouthd can be started by hand, and a crash can leave no pid file at all.
#[test]
fn a_running_plymouthd_is_found_without_a_pid_file() {
    let scratch = Scratch::new("proc");
    scratch.fake_process(4321, "plymouthd");
    assert_eq!(detect(&scratch.places()), Some(Conflict::Plymouthd(4321)));
}

/// And everything else in `/proc` is left alone, including the entries that are
/// not processes at all.
#[test]
fn other_processes_are_not_mistaken_for_a_splash() {
    let scratch = Scratch::new("proc-busy");
    for (pid, name) in [(1u32, "systemd"), (2, "kthreadd"), (900, "plymouth")] {
        scratch.fake_process(pid, name);
    }
    std::fs::create_dir_all(scratch.places().proc.join("self")).expect("self");
    std::fs::write(scratch.places().proc.join("uptime"), "1 1").expect("uptime");
    // `plymouth` is the *client*, not the daemon, and is not a conflict.
    assert_eq!(detect(&scratch.places()), None);
}

/// A live instance is one that answers, not one that left a socket behind.
#[test]
fn another_raba_answering_on_the_socket_is_a_conflict() {
    let scratch = Scratch::new("live");
    let places = scratch.places();
    let socket = places.socket.clone();
    std::thread::spawn(move || {
        let listener = Listener::bind(&socket).expect("binding");
        for _ in 0..3000u32 {
            if let Some(mut connection) = listener.accept() {
                let _ = connection.request();
                connection.reply(&Reply::Ok).expect("replying");
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    // Give the thread a moment to bind; `detect` is a single attempt by design.
    for _ in 0..500u32 {
        if places.socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(detect(&places), Some(Conflict::Raba));
}

/// The case that must not be a conflict, or no machine would ever resume: a
/// socket file from an instance that is gone.
#[test]
fn a_socket_left_by_a_dead_instance_is_not_a_conflict() {
    let scratch = Scratch::new("dead");
    let places = scratch.places();
    {
        let listener = Listener::bind(&places.socket).expect("binding");
        std::mem::forget(listener); // killed: no Drop, the file stays
    }
    assert!(places.socket.exists());
    assert_eq!(detect(&places), None);
}

/// Plymouth first, because "Plymouth is running" is a more useful thing to read
/// than "another raba is running" when both are true.
#[test]
fn plymouth_is_reported_in_preference_to_anything_else() {
    let scratch = Scratch::new("both");
    std::fs::write(scratch.places().plymouth_pid, "7\n").expect("pid file");
    scratch.fake_process(8, "plymouthd");
    assert_eq!(detect(&scratch.places()), Some(Conflict::PlymouthPidFile(Some(7))));
}

/// Every conflict has to say something a user can act on.
#[test]
fn every_conflict_explains_itself() {
    for conflict in [
        Conflict::PlymouthPidFile(Some(1)),
        Conflict::PlymouthPidFile(None),
        Conflict::Plymouthd(2),
        Conflict::Raba,
    ] {
        let message = conflict.to_string();
        assert!(!message.is_empty());
        assert!(!message.contains('\n'), "{message:?}");
    }
}
