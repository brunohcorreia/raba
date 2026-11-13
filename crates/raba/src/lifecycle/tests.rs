//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

use super::*;

/// Every phase, for the properties that have to hold in all of them.
const ALL: [Phase; 6] =
    [Phase::Waiting, Phase::Boot, Phase::Outro, Phase::Deactivated, Phase::Revealed, Phase::Done];

const EVENTS: [Event; 7] = [
    Event::ShowDelayElapsed,
    Event::TimeoutElapsed,
    Event::OutroEnded,
    Event::Quit,
    Event::Deactivate,
    Event::Reactivate,
    Event::Terminate,
];

fn at(phase: Phase, outro: bool) -> Lifecycle {
    Lifecycle { phase, outro }
}

#[test]
fn an_ordinary_boot_runs_delay_then_loop_then_outro_then_out() {
    let mut life = Lifecycle::new(true);
    assert_eq!(life.phase(), Phase::Waiting);
    assert_eq!(life.on(Event::ShowDelayElapsed), Some(Phase::Boot));
    assert_eq!(life.on(Event::Quit), Some(Phase::Outro));
    assert_eq!(life.on(Event::OutroEnded), Some(Phase::Done));
}

#[test]
fn without_an_outro_quit_goes_straight_out() {
    let mut life = at(Phase::Boot, false);
    assert_eq!(life.on(Event::Quit), Some(Phase::Done));
}

/// The documented handoff: deactivate, let the display manager take the
/// terminal, then quit. The quit must not draw anything.
#[test]
fn quit_after_deactivate_does_not_play_an_outro_over_the_display_manager() {
    let mut life = Lifecycle::new(true);
    life.on(Event::ShowDelayElapsed);
    assert_eq!(life.on(Event::Deactivate), Some(Phase::Deactivated));
    assert_eq!(life.on(Event::Quit), Some(Phase::Done));
}

/// Same argument from the other released state: the user is reading kernel
/// messages on that console, and an outro would paint over them.
#[test]
fn quit_after_a_timeout_reveal_does_not_take_the_console_back() {
    let mut life = at(Phase::Revealed, true);
    assert_eq!(life.on(Event::Quit), Some(Phase::Done));
}

/// `Outro.MaxDuration` bounds the outro, not `Boot.Timeout`. A boot timeout
/// landing in that second-long window would turn a successful handoff into a
/// "the boot has stalled" reveal.
#[test]
fn the_boot_timeout_cannot_fire_during_the_outro() {
    let mut life = at(Phase::Outro, true);
    assert_eq!(life.on(Event::TimeoutElapsed), None);
    assert_eq!(life.phase(), Phase::Outro);
}

#[test]
fn a_timeout_reveals_the_console_rather_than_exiting() {
    let mut life = at(Phase::Boot, true);
    assert_eq!(life.on(Event::TimeoutElapsed), Some(Phase::Revealed));
    assert!(!life.phase().is_done());
    assert!(!life.phase().drawing());
}

#[test]
fn a_revealed_console_can_be_taken_back_on_request() {
    for phase in [Phase::Deactivated, Phase::Revealed] {
        let mut life = at(phase, true);
        assert_eq!(life.on(Event::Reactivate), Some(Phase::Boot));
    }
}

/// `ShowDelay` longer than `Timeout` is a misconfiguration, and the machine
/// still has to do something sensible: reveal, and stay revealed rather than
/// starting to draw after the boot was already declared stalled.
#[test]
fn a_timeout_before_the_show_delay_does_not_start_drawing_afterwards() {
    let mut life = Lifecycle::new(true);
    assert_eq!(life.on(Event::TimeoutElapsed), Some(Phase::Revealed));
    assert_eq!(life.on(Event::ShowDelayElapsed), None);
    assert_eq!(life.phase(), Phase::Revealed);
}

/// Nothing was ever on screen, so there is nothing to animate away from.
#[test]
fn quitting_during_the_show_delay_never_draws_anything() {
    let mut life = Lifecycle::new(true);
    assert_eq!(life.on(Event::Quit), Some(Phase::Done));
}

#[test]
fn deactivating_mid_outro_releases_rather_than_finishing_the_animation() {
    let mut life = at(Phase::Outro, true);
    assert_eq!(life.on(Event::Deactivate), Some(Phase::Done));
}

#[test]
fn a_signal_ends_it_from_any_phase() {
    for phase in ALL {
        let mut life = at(phase, true);
        life.on(Event::Terminate);
        assert!(life.phase().is_done(), "{phase:?} survived a Terminate");
    }
}

#[test]
fn done_absorbs_everything() {
    for event in EVENTS {
        let mut life = at(Phase::Done, true);
        assert_eq!(life.on(event), None, "{event:?} moved us out of Done");
        assert_eq!(life.phase(), Phase::Done);
    }
}

/// A repeated command from a client is redundant, not wrong, and must never
/// be a second transition.
#[test]
fn repeating_a_command_changes_nothing_the_second_time() {
    for event in EVENTS {
        for phase in ALL {
            let mut life = at(phase, true);
            if life.on(event).is_some() {
                assert_eq!(life.on(event), None, "{event:?} from {phase:?} fired twice");
            }
        }
    }
}

/// `Waiting` is a start state, not a destination. A splash that went back to
/// it would be a show delay starting over in the middle of a boot, which is
/// the screen going blank for a reason the user has no way to interpret.
#[test]
fn nothing_ever_returns_to_waiting() {
    for phase in ALL.into_iter().filter(|phase| *phase != Phase::Waiting) {
        for event in EVENTS {
            let mut life = at(phase, true);
            life.on(event);
            assert_ne!(
                life.phase(),
                Phase::Waiting,
                "{event:?} from {phase:?} went back to Waiting"
            );
        }
    }
}

#[test]
fn only_the_drawing_phases_hold_a_terminal() {
    for phase in ALL {
        assert_eq!(phase.drawing(), phase.holds_terminal());
        assert_eq!(phase.drawing(), matches!(phase, Phase::Boot | Phase::Outro));
    }
}

/// The names are the wire protocol. Changing one is a protocol change.
#[test]
fn every_phase_has_a_distinct_reportable_name() {
    let mut names: Vec<&str> = ALL.iter().map(|phase| phase.name()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count);
    assert!(names.iter().all(|name| !name.is_empty()));
}
