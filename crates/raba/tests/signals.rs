//! Signals, checked by sending them.
//!
//! These run in-process and change the process signal mask, so they must not run
//! beside each other or beside anything else that cares. `cargo test` gives each
//! integration test file its own binary but runs the tests inside it in
//! parallel, so the file keeps one lock and takes it in every test.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
// A test that cannot send a signal cannot check anything about signals. The
// workspace lint is about the boot binary; this is not it.
#![allow(unsafe_code)]

use raba::signals::{Signal, Signals};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// One at a time: the signal mask is per process.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|poison| poison.into_inner())
}

fn raise(signal: i32) {
    // SAFETY: `raise` takes a signal number and nothing else. The signal is
    // blocked by `Signals::arm`, so it becomes pending rather than delivered.
    let sent = unsafe { libc::raise(signal) };
    assert_eq!(sent, 0, "could not raise {signal}");
}

#[test]
fn nothing_pending_is_the_usual_answer() {
    let _alone = alone();
    let mut signals = Signals::arm().expect("arming");
    for _ in 0..100 {
        assert_eq!(signals.poll(), None);
    }
}

#[test]
fn a_termination_signal_is_seen_at_the_next_poll() {
    let _alone = alone();
    for (number, expected) in
        [(libc::SIGTERM, Signal::Terminate), (libc::SIGINT, Signal::Terminate)]
    {
        let mut signals = Signals::arm().expect("arming");
        raise(number);
        assert_eq!(signals.poll(), Some(expected), "signal {number}");
    }
}

/// `man 8 raba`: SIGUSR1 begins the outro, as though `raba-ctl quit` had been
/// called. It is a different event from a termination and must not be folded in.
#[test]
fn sigusr1_asks_for_the_outro_rather_than_the_exit() {
    let _alone = alone();
    let mut signals = Signals::arm().expect("arming");
    raise(libc::SIGUSR1);
    assert_eq!(signals.poll(), Some(Signal::Quit));
}

/// The process is not killed by the signal it is supposed to handle. If the
/// block were not working, this test would not report a failure — it would take
/// the test binary down with it, which is its own kind of evidence.
#[test]
fn a_blocked_signal_does_not_kill_us() {
    let _alone = alone();
    let mut signals = Signals::arm().expect("arming");
    raise(libc::SIGTERM);
    assert!(signals.poll().is_some());
    // Still here.
}

/// The escape hatch. After the first signal the block is lifted, so a second
/// one is delivered normally — which is what makes a stuck RABA killable by
/// somebody holding down Ctrl-C rather than only by SIGKILL.
#[test]
fn the_block_is_lifted_after_the_first_signal() {
    let _alone = alone();
    let mut signals = Signals::arm().expect("arming");
    assert!(signals.armed());
    raise(libc::SIGTERM);
    assert_eq!(signals.poll(), Some(Signal::Terminate));
    assert!(!signals.armed(), "still blocking after the first signal");
    // And it stays disarmed, rather than quietly re-arming on the next poll.
    assert_eq!(signals.poll(), None);
    assert!(!signals.armed());
}

/// Dropping it must not leave the process with a mask it did not start with:
/// anything that runs afterwards — in this file, the next test — would silently
/// stop receiving signals.
#[test]
fn dropping_restores_the_mask() {
    let _alone = alone();
    {
        let _signals = Signals::arm().expect("arming");
    }
    // SIGUSR1's default disposition is to terminate. If the mask had been left
    // blocked, this would sit pending instead and the assertion below would
    // pass for the wrong reason — so instead, arm again and check that nothing
    // from before is waiting.
    let mut signals = Signals::arm().expect("arming");
    assert_eq!(signals.poll(), None, "a signal survived from a previous test");
}

/// Only the three RABA has an opinion about are taken over. Blocking more would
/// be changing the behaviour of signals it has no plan for.
#[test]
fn other_signals_keep_their_ordinary_behaviour() {
    let _alone = alone();
    let mut signals = Signals::arm().expect("arming");
    // SIGCHLD is ignored by default, so raising it is harmless, and it must not
    // be reported as something RABA asked for.
    raise(libc::SIGCHLD);
    assert_eq!(signals.poll(), None);
    assert!(signals.armed(), "an unwatched signal disarmed the watcher");
}
