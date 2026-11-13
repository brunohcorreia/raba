//! `/run/raba.state`: the round trip, and every way it is refused.
//!
//! Resume is the mechanism that makes being killed a non-event, so the
//! interesting tests are not "does it round trip" — it does — but the ones where
//! the file is wrong. A state file that is believed when it should not be is a
//! root process drawing on a terminal somebody else chose; a state file that is
//! refused when it should not be is a visible flash of black on every boot that
//! crosses `switch_root`. Both are here.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba::lifecycle::Phase;
use raba::state::{self, Invalid, State, FRESH};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

const FRAMES: u32 = 600;
const NOW: u64 = 8_000;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        // Two tests with the same name would share a directory, and the first
        // thing this does is empty it — so they would delete each other's
        // files, in parallel, and the symptom would be a *security* test
        // failing for no reason anybody could reproduce. Cheaper to refuse.
        static TAKEN: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        assert!(
            TAKEN.lock().expect("the name registry").insert(name),
            "two tests use the scratch name {name:?}; they would erase each other"
        );

        let mut path = std::env::temp_dir();
        path.push(format!("raba-state-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("raba.state")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample() -> State {
    State {
        terminal: 2,
        previous: 1,
        frame: 431,
        phase: Phase::Boot,
        theme: PathBuf::from("/usr/share/raba/themes/arch.raba"),
        written_ms: 7_900,
    }
}

#[test]
fn a_state_file_survives_the_round_trip() {
    let scratch = Scratch::new("roundtrip");
    state::save(scratch.file(), &sample()).expect("saving");
    let loaded = state::load(scratch.file(), FRAMES, NOW).expect("loading");
    assert_eq!(loaded, Some(sample()));
}

#[test]
fn every_phase_survives_the_round_trip() {
    let scratch = Scratch::new("phases");
    for phase in [
        Phase::Waiting,
        Phase::Boot,
        Phase::Outro,
        Phase::Deactivated,
        Phase::Revealed,
        Phase::Done,
    ] {
        let state = State { phase, ..sample() };
        state::save(scratch.file(), &state).expect("saving");
        assert_eq!(
            state::load(scratch.file(), FRAMES, NOW).expect("loading"),
            Some(state),
            "{phase:?}"
        );
    }
}

/// The ordinary first boot. Not an error, and not something to log.
#[test]
fn no_state_file_is_not_a_problem() {
    let scratch = Scratch::new("absent");
    assert_eq!(state::load(scratch.file(), FRAMES, NOW), Ok(None));
}

/// Written every frame, so a rename that was not atomic would eventually be
/// read halfway through by the next instance.
#[test]
fn saving_repeatedly_leaves_exactly_one_file_and_no_staging() {
    let scratch = Scratch::new("atomic");
    for frame in 0..50u32 {
        state::save(scratch.file(), &State { frame, ..sample() }).expect("saving");
    }
    let entries: Vec<_> = std::fs::read_dir(&scratch.0)
        .expect("listing")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(entries.len(), 1, "left {entries:?} behind");
    assert_eq!(state::load(scratch.file(), FRAMES, NOW).expect("loading").unwrap().frame, 49);
}

/// Everything below here is a file that must not be believed. Each one is also
/// a file that must not be left on disk afterwards: a rejection that stays is a
/// rejection repeated on every start.
fn refuse(name: &str, text: &str) -> Invalid {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.file(), text).expect("planting");
    let why = state::load(scratch.file(), FRAMES, NOW).expect_err("should have been refused");
    assert!(!scratch.file().exists(), "{name}: the bad file was left behind");
    why
}

#[test]
fn a_file_that_is_not_ours_is_refused() {
    assert_eq!(refuse("alien", "hello\nterminal=2\n"), Invalid::NotOurs);
    assert_eq!(refuse("empty", ""), Invalid::NotOurs);
    assert_eq!(refuse("binary", "\u{0}\u{1}\u{2}"), Invalid::NotOurs);
}

#[test]
fn a_missing_field_is_refused_rather_than_defaulted() {
    let full = sample().render();
    for key in ["terminal", "previous", "frame", "phase", "written_ms", "theme"] {
        let stripped: String = full
            .lines()
            .filter(|line| !line.starts_with(&format!("{key}=")))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(refuse(&format!("no-{key}"), &stripped), Invalid::Missing(key));
    }
}

#[test]
fn a_field_that_is_not_a_number_is_refused() {
    let text = sample().render().replace("frame=431", "frame=NaN");
    assert_eq!(refuse("nan", &text), Invalid::Unreadable("frame"));
}

/// `security.md` finding 3: whoever writes this file chooses the terminal a root
/// process draws on. The kernel has 63 of them and the file does not get a vote.
#[test]
fn a_terminal_outside_the_kernels_range_is_refused() {
    for (name, bad) in [("zero", "0"), ("high", "64"), ("huge", "65000")] {
        let text = sample().render().replace("terminal=2", &format!("terminal={bad}"));
        assert_eq!(refuse(&format!("tty-{name}"), &text), Invalid::Terminal(bad.parse().unwrap()));
    }
}

/// Checked against the theme that was actually loaded, not against anything the
/// file says about it.
#[test]
fn a_frame_past_the_end_of_the_theme_is_refused() {
    let text = sample().render().replace("frame=431", "frame=600");
    assert_eq!(refuse("overrun", &text), Invalid::Frame { frame: 600, frames: FRAMES });
}

#[test]
fn a_stale_file_is_refused_and_a_fresh_one_is_not() {
    let scratch = Scratch::new("freshness");
    state::save(scratch.file(), &State { written_ms: 1_000, ..sample() }).expect("saving");
    let why = state::load(scratch.file(), FRAMES, 1_000 + FRESH + 1).expect_err("stale");
    assert_eq!(why, Invalid::Stale { age_ms: FRESH + 1 });

    state::save(scratch.file(), &State { written_ms: 1_000, ..sample() }).expect("saving");
    assert!(state::load(scratch.file(), FRAMES, 1_000 + FRESH).expect("fresh").is_some());
}

/// A file claiming to be from the future is as wrong as one too old, and is what
/// a previous boot's file would look like if `/run` turned out not to be a
/// tmpfs after all.
#[test]
fn a_file_from_the_future_is_refused() {
    let scratch = Scratch::new("future");
    state::save(scratch.file(), &State { written_ms: 90_000, ..sample() }).expect("saving");
    assert!(state::load(scratch.file(), FRAMES, 1_000).is_err());
}

#[test]
fn a_relative_theme_path_is_refused() {
    let text = sample()
        .render()
        .replace("theme=/usr/share/raba/themes/arch.raba", "theme=../../../etc/shadow");
    assert_eq!(refuse("relative", &text), Invalid::Theme);
}

/// Forward compatibility, so a later version writing a field this one has never
/// heard of does not make every resume fail.
#[test]
fn an_unknown_field_is_ignored_rather_than_fatal() {
    let scratch = Scratch::new("future-key");
    let text = format!("{}progress=0.4\nsomething entirely different\n", sample().render());
    std::fs::write(scratch.file(), &text).expect("planting");
    assert_eq!(state::load(scratch.file(), FRAMES, NOW).expect("loading"), Some(sample()));
}

/// `security.md` finding 4. A symlink planted at this path turns a root process
/// into a root process writing wherever somebody else points it.
#[test]
fn a_symlink_is_neither_followed_nor_written_through() {
    let scratch = Scratch::new("symlink");
    let target = scratch.0.join("elsewhere");
    std::fs::write(&target, "untouched").expect("target");
    std::os::unix::fs::symlink(&target, scratch.file()).expect("symlink");

    assert_eq!(state::load(scratch.file(), FRAMES, NOW), Err(Invalid::NotOurs));
    assert!(state::save(scratch.file(), &sample()).is_ok());

    // The save replaced the symlink rather than writing through it.
    assert_eq!(std::fs::read_to_string(&target).expect("reading"), "untouched");
    assert!(!std::fs::symlink_metadata(scratch.file()).expect("stat").file_type().is_symlink());
}

/// And the mode is the third half of it.
///
/// The writer preserves the destination's permissions so that replacing a file
/// cannot silently widen it. Read with `metadata`, that courtesy becomes the
/// attack: `metadata` follows a symlink, so a link pointing at a setuid file
/// hands its `st_mode` to a root-owned file that is then renamed into place. A
/// symlink has no mode worth preserving, and a destination that is one gets
/// what any new file gets.
#[test]
fn the_mode_is_never_taken_from_the_far_end_of_a_symlink() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("symlink-mode");
    let target = scratch.0.join("elsewhere");
    std::fs::write(&target, "untouched").expect("target");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o4777)).expect("chmod");
    std::os::unix::fs::symlink(&target, scratch.file()).expect("symlink");

    state::save(scratch.file(), &sample()).expect("saving");

    let mode = std::fs::symlink_metadata(scratch.file()).expect("stat").permissions().mode();
    assert_eq!(mode & 0o7000, 0, "setuid came across the link: {mode:o}");
    assert_eq!(mode & 0o022, 0, "world- or group-writable: {mode:o}");
}

/// The staging path is the other half of the same attack.
#[test]
fn a_symlink_at_the_staging_path_is_not_written_through() {
    let scratch = Scratch::new("symlink-staging");
    let target = scratch.0.join("elsewhere");
    std::fs::write(&target, "untouched").expect("target");
    let staging = scratch.0.join("raba.state.new");
    std::os::unix::fs::symlink(&target, &staging).expect("symlink");

    let _ = state::save(scratch.file(), &sample());
    assert_eq!(std::fs::read_to_string(&target).expect("reading"), "untouched");
}

/// The only unbounded read in the program would be this one.
#[test]
fn an_enormous_file_is_not_read_into_memory() {
    let scratch = Scratch::new("huge");
    let mut text = sample().render();
    text.push_str(&"padding=x\n".repeat(200_000));
    std::fs::write(scratch.file(), &text).expect("planting");
    // Refused outright rather than parsed from its first 4 KiB: a prefix of
    // something enormous that happens to start with a valid state is exactly
    // the file somebody would plant.
    assert_eq!(state::load(scratch.file(), FRAMES, NOW), Err(Invalid::NotOurs));
    assert!(!scratch.file().exists());
}

#[test]
fn the_file_is_six_short_lines_and_legible_by_eye() {
    let rendered = sample().render();
    assert_eq!(rendered.lines().count(), 7);
    assert!(rendered.starts_with("raba-state 1\n"));
    assert!(rendered.contains("phase=boot\n"));
    assert!(rendered.len() < 200, "{} bytes", rendered.len());
}

/// The clock is the one the resume logic compares against, so it has to be a
/// clock that counts from power-on and cannot be dragged backwards.
#[test]
fn the_clock_counts_from_boot_and_moves_forwards() {
    let first = state::now_ms();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let second = state::now_ms();
    assert!(second >= first, "{second} < {first}");
    assert!(second.saturating_sub(first) < 5_000, "a 20ms sleep took {}ms", second - first);
    // A machine that has been up for less than a millisecond cannot be running
    // this test, so a zero here would mean the wrong clock.
    assert!(first > 0);
}

/// The sabotage case `state-in-the-way`, as a host test.
///
/// A directory at the resume file's path makes every save fail, and for a long
/// time it failed silently — `let _ = state::save(..)` on every frame, sixty
/// discarded errors a second, and a boot that could not survive `switch_root`
/// with nothing anywhere to say why. The boot loop now reports the first one.
/// What is asserted here is the part that makes that possible: the failure
/// reaches the caller as an error rather than being swallowed inside `save`.
#[test]
fn a_directory_at_the_path_is_an_error_and_not_a_silent_no_op() {
    let scratch = Scratch::new("directory");
    let path = scratch.file();
    std::fs::create_dir_all(&path).expect("a directory where the file goes");

    let error = state::save(&path, &sample()).expect_err("saving onto a directory");
    assert!(!error.to_string().is_empty(), "the error has to say something: {error:?}");
    // And nothing of ours is left behind. The staging file is created before
    // the rename that fails, so a save that cannot finish has to take it away
    // again or every frame of a sabotaged boot leaves litter in /run.
    let staging = path.with_file_name("raba.state.new");
    assert!(!staging.exists(), "a failed save left {} behind", staging.display());
}
