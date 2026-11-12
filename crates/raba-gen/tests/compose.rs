//! Composing art and overlays into a movie, and the delta encoding that makes it small.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::Palette;
use raba_core::glyph::{safety, Safety};
use raba_core::{apply, decode, encode, Cell, Grid, Movie};
use raba_gen::art::parse;
use raba_gen::compose::{compose, paint, spinners, ComposeError, Overlay};

fn logo() -> Grid {
    parse("$1####\n$1#  #\n$1####").unwrap().to_grid(Palette::Ansi16, &[6], 7, 0).unwrap()
}

/// An overlay with `period` distinct steps. The glyphs repeat past 26; the period is
/// how many steps there are, not how many look different.
fn spinner(period: usize) -> Overlay {
    let glyphs: Vec<char> =
        (0..period).map(|step| char::from(b'a'.wrapping_add((step % 26) as u8))).collect();
    Overlay::spinner(0, 0, &glyphs, 7, 0)
}

/// Play a movie from blank and collect every frame as a grid. This is exactly what
/// `raba` does at boot, which is why the delta encoding is checked through it rather
/// than by inspecting ops.
fn playback(movie: &Movie) -> Vec<Grid> {
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    movie
        .frames
        .iter()
        .map(|frame| {
            apply(&mut grid, movie, frame);
            grid.clone()
        })
        .collect()
}

#[test]
fn a_logo_with_nothing_moving_is_one_frame() {
    // A legitimate theme: a logo that sits there.
    let movie = compose(&logo(), &[], 100, true).unwrap();
    assert_eq!(movie.frames.len(), 1);
    assert_eq!(movie.ops.len(), usize::from(movie.columns) * usize::from(movie.lines));
}

#[test]
fn a_spinner_sets_the_frame_count() {
    let movie = compose(&logo(), &[spinner(4)], 100, true).unwrap();
    assert_eq!(movie.frames.len(), 4);
}

#[test]
fn frames_that_disagree_run_to_their_least_common_multiple() {
    // Four steps and six steps are only back where they started together after
    // twelve. Playing six would cut the spinner mid-cycle; playing four would cut
    // the other.
    let overlays = [spinner(4), Overlay::spinner(3, 0, &['x', 'y', 'z', 'x', 'y', 'z'], 7, 0)];
    assert_eq!(compose(&logo(), &overlays, 100, true).unwrap().frames.len(), 12);
}

#[test]
fn a_period_that_divides_another_adds_no_frames() {
    // The argument for choosing periods that divide each other, stated as a test.
    let overlays = [spinner(2), spinner(4)];
    assert_eq!(compose(&logo(), &overlays, 100, true).unwrap().frames.len(), 4);
}

#[test]
fn awkward_periods_are_expensive_but_allowed() {
    // 11, 13 and 7 need 1001 frames to line up. That is a lot of frames and almost
    // no bytes, because every one of them is a delta -- so there is no honest
    // ground to refuse it, and inventing a tighter ceiling than the format's would
    // be a guess dressed as a rule.
    let overlays = [spinner(11), spinner(13), Overlay::spinner(1, 1, &['a'; 7], 7, 0)];
    let movie = compose(&logo(), &overlays, 100, true).unwrap();
    assert_eq!(movie.frames.len(), 1001);
    assert!(movie.ops.len() < 2500, "deltas, not 1001 canvases: {} ops", movie.ops.len());
}

#[test]
fn periods_past_the_formats_ceiling_are_refused() {
    // 257 and 263 need 67591 frames, past what the format can describe. Say so,
    // with the number, rather than emitting something that will not decode.
    let overlays = [spinner(257), spinner(263)];
    let error = compose(&logo(), &overlays, 100, true).unwrap_err();
    assert!(matches!(error, ComposeError::PeriodsDoNotFit { .. }), "{error}");
    assert!(error.to_string().contains("67591"), "{error}");
}

#[test]
fn a_spinner_over_a_static_logo_costs_one_cell_a_frame() {
    // The entire reason the format carries deltas. Without this, four frames of a
    // 4x3 logo would be 48 ops.
    let movie = compose(&logo(), &[spinner(4)], 100, true).unwrap();
    assert_eq!(movie.frames[0].op_count as usize, 12, "frame 0 is the whole canvas");
    for frame in &movie.frames[1..] {
        assert_eq!(frame.op_count, 1, "later frames carry only the spinner cell");
    }
}

#[test]
fn playing_the_deltas_reproduces_every_frame() {
    // The property from docs/development.md: delta reconstruction equals the
    // full-frame reconstruction, for any frame. An error here would show as an
    // animation that drifts further from the truth the longer it runs.
    let base = logo();
    let overlays = [spinner(4), Overlay::spinner(1, 2, &['-', '='], 3, 0)];
    let movie = compose(&base, &overlays, 100, true).unwrap();

    for (step, played) in playback(&movie).iter().enumerate() {
        assert_eq!(played, &paint(&base, &overlays, step), "frame {step}");
    }
}

#[test]
fn the_last_frame_leads_back_into_the_first() {
    // What makes a loop seamless: after the final frame, applying frame 0's ops
    // again must give frame 0, with no leftover from the end of the cycle. Frame 0
    // carrying the whole canvas is what guarantees it.
    let base = logo();
    let overlays = [spinner(4)];
    let movie = compose(&base, &overlays, 100, true).unwrap();

    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    for _ in 0..2 {
        for frame in &movie.frames {
            apply(&mut grid, &movie, frame);
        }
    }
    apply(&mut grid, &movie, &movie.frames[0]);
    assert_eq!(grid, paint(&base, &overlays, 0));
}

#[test]
fn a_composed_movie_survives_the_format() {
    // compose -> encode -> decode is the whole authoring path, and the decoder is
    // strict enough that a careless composer would be caught here.
    let movie = compose(&logo(), &[spinner(4)], 80, true).unwrap();
    assert_eq!(decode(&encode(&movie)), Ok(movie));
}

#[test]
fn every_built_in_spinner_is_console_safe() {
    // A spinner nobody can see is worse than no spinner. docs/glyphs.md is why
    // braille is not on this list.
    for (name, glyphs) in spinners::ALL {
        for &ch in *glyphs {
            assert_eq!(safety(ch), Safety::Safe, "spinner {name}: {ch:?}");
        }
    }
}

#[test]
fn built_in_spinners_compose() {
    for (name, glyphs) in spinners::ALL {
        let overlay = Overlay::spinner(1, 1, glyphs, 7, 0);
        let movie = compose(&logo(), &[overlay], 100, true).unwrap();
        assert_eq!(movie.frames.len(), glyphs.len(), "{name}");
    }
}

#[test]
fn an_overlay_off_the_edge_is_refused() {
    let wide = Overlay::spinner(3, 0, &['x'], 7, 0);
    assert!(compose(&logo(), &[wide], 100, true).is_ok(), "the last column is on the art");

    let off = Overlay::spinner(4, 0, &['x'], 7, 0);
    assert_eq!(
        compose(&logo(), &[off], 100, true),
        Err(ComposeError::OverlayOffCanvas { column: 4, line: 0, width: 1 })
    );

    let below = Overlay::spinner(0, 3, &['x'], 7, 0);
    assert_eq!(
        compose(&logo(), &[below], 100, true),
        Err(ComposeError::OverlayOffCanvas { column: 0, line: 3, width: 1 })
    );
}

#[test]
fn steps_of_different_widths_are_refused() {
    // Otherwise a narrow step would leave the tail of a wide one on screen, and the
    // bug would look like a rendering fault rather than a theme fault.
    let ragged = Overlay {
        column: 0,
        line: 0,
        steps: vec![
            vec![Cell { ch: 'a', fg: 7, bg: 0 }, Cell { ch: 'b', fg: 7, bg: 0 }],
            vec![Cell { ch: 'c', fg: 7, bg: 0 }],
        ],
    };
    assert_eq!(
        compose(&logo(), &[ragged], 100, true),
        Err(ComposeError::RaggedOverlay { expected: 2, got: 1, step: 1 })
    );
}

#[test]
fn an_overlay_with_no_steps_is_refused() {
    let empty = Overlay { column: 0, line: 0, steps: Vec::new() };
    assert_eq!(compose(&logo(), &[empty], 100, true), Err(ComposeError::EmptyOverlay));
}

#[test]
fn a_colour_the_palette_does_not_have_is_caught_at_build_time() {
    // The decoder rejects this, so catching it here turns a file that will not load
    // into a build error with a name attached.
    let mut base = logo();
    base.set(0, 0, Cell { ch: '#', fg: 200, bg: 0 });
    assert_eq!(
        compose(&base, &[], 100, true),
        Err(ComposeError::ColourOutOfPalette { value: 200, max: 15 })
    );

    let bad_spinner = Overlay::spinner(0, 0, &['|'], 7, 99);
    assert_eq!(
        compose(&logo(), &[bad_spinner], 100, true),
        Err(ComposeError::ColourOutOfPalette { value: 99, max: 15 })
    );
}

#[test]
fn looping_is_recorded() {
    assert!(compose(&logo(), &[], 100, true).unwrap().looping);
    assert!(!compose(&logo(), &[], 100, false).unwrap().looping);
}

#[test]
fn the_whole_authoring_chain_runs() {
    // Art source in, playable movie out: parse, resolve colours, compose, encode,
    // decode, play. Every M0 piece in one line of control flow.
    let art = parse("$1 /\\ \n$1/__\\").unwrap();
    let base = art.to_grid(Palette::Ansi16, &[4], 7, 0).unwrap();
    let overlay = Overlay::spinner(0, 1, spinners::ASCII, 7, 0);
    let movie =
        decode(&encode(&compose(&base, std::slice::from_ref(&overlay), 120, true).unwrap()))
            .unwrap();

    let frames = playback(&movie);
    assert_eq!(frames.len(), 4);
    assert_eq!(frames[0].get(0, 1).unwrap().ch, '|');
    assert_eq!(frames[1].get(0, 1).unwrap().ch, '/');
    // And the art underneath is untouched by the spinner.
    assert_eq!(frames[2].get(1, 0).unwrap().ch, '/');
}
