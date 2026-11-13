//! The generated animations.
//!
//! These exist to answer "can RABA do what my display manager does", so the
//! names are Ly's. What has to hold for each is the same: it draws on a console,
//! it is deterministic, and — except for fire, which says so — it loops.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use raba_core::glyph::{safety, Safety};
use raba_core::{apply, encode, Grid, Movie, Palette};
use raba_gen::generate::{generate, Kind, Rng, Spec};

fn grids(movie: &Movie) -> Vec<Grid> {
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).expect("grid");
    movie
        .frames
        .iter()
        .map(|frame| {
            apply(&mut grid, movie, frame);
            grid.clone()
        })
        .collect()
}

fn differences(a: &Grid, b: &Grid) -> usize {
    a.cells().iter().zip(b.cells().iter()).filter(|(x, y)| x != y).count()
}

#[test]
fn every_animation_generates_and_draws_on_a_console() {
    // The art is useless if the console cannot render it. CMatrix's katakana and
    // a shader's quadrant blocks are both out, which is why these are built from
    // ASCII and the CP437 block elements.
    for kind in Kind::ALL {
        let movie = generate(kind, &Spec::default());
        assert!(!movie.frames.is_empty(), "{}: no frames", kind.name());
        let bad: Vec<char> = movie
            .ops
            .iter()
            .map(|op| op.ch)
            .filter(|ch| safety(*ch) != Safety::Safe)
            .collect();
        assert!(bad.is_empty(), "{}: unsafe glyphs {bad:?}", kind.name());
    }
}

#[test]
fn every_animation_stays_inside_the_console_palette() {
    for kind in Kind::ALL {
        let movie = generate(kind, &Spec::default());
        assert_eq!(movie.palette, Palette::Ansi16, "{}", kind.name());
        for op in &movie.ops {
            assert!(op.fg <= 15, "{}: fg {}", kind.name(), op.fg);
            // A console's high background bit is blink, not brightness.
            assert!(op.bg <= 7, "{}: bg {}", kind.name(), op.bg);
        }
    }
}

#[test]
fn the_same_seed_gives_the_same_file() {
    // Reproducible builds, and the precondition for any golden test of these.
    for kind in Kind::ALL {
        let spec = Spec { seed: 7, ..Spec::default() };
        assert_eq!(
            encode(&generate(kind, &spec)),
            encode(&generate(kind, &spec)),
            "{} is not deterministic",
            kind.name()
        );
    }
}

#[test]
fn a_different_seed_gives_a_different_picture() {
    // Otherwise --seed is decoration. Life is excluded: its seed moves the
    // oscillators around but the garden is laid out on a fixed grid, so a small
    // board can come out the same.
    for kind in [Kind::Doom, Kind::Matrix] {
        let one = generate(kind, &Spec { seed: 1, ..Spec::default() });
        let two = generate(kind, &Spec { seed: 2, ..Spec::default() });
        assert_ne!(encode(&one), encode(&two), "{} ignores its seed", kind.name());
    }
}

#[test]
fn the_seamless_ones_really_are_seamless() {
    // The test that matters for a boot splash, because the animation repeats
    // until the system is ready. A loop that does not close shows a jump in the
    // same place every time.
    //
    // Checked by behaviour rather than by trusting the arithmetic: the number of
    // cells that change from the last frame back to the first must be in the same
    // range as any other step. A seam would be a visible outlier.
    for kind in Kind::ALL {
        if !kind.loops_seamlessly() {
            continue;
        }
        let movie = generate(kind, &Spec::default());
        let grids = grids(&movie);
        assert!(grids.len() > 2, "{}: too few frames to judge", kind.name());

        let mut steps: Vec<usize> =
            grids.windows(2).map(|pair| differences(&pair[0], &pair[1])).collect();
        let wrap = differences(&grids[grids.len() - 1], &grids[0]);

        steps.sort_unstable();
        let median = steps[steps.len() / 2];
        let largest = *steps.last().unwrap();
        assert!(
            wrap <= largest.max(median * 2),
            "{}: wrapping costs {wrap} changes against a median step of {median} \
             and a largest of {largest}; the loop has a seam",
            kind.name()
        );
    }
}

#[test]
fn fire_is_honest_about_not_looping() {
    // Not a defect to fix but a property to report: fire is chaotic, so there is
    // no frame count at which the state returns. If this ever starts passing as
    // seamless, something has quietly become periodic and is worth looking at.
    assert!(!Kind::Doom.loops_seamlessly());
    assert!(Kind::Matrix.loops_seamlessly());
    assert!(Kind::Colormix.loops_seamlessly());
    assert!(Kind::GameOfLife.loops_seamlessly());
}

#[test]
fn life_finds_its_own_cycle_rather_than_running_for_the_frames_asked_for() {
    // The board is a garden of oscillators with coprime periods — 15, 3 and 2 —
    // so the whole board repeats every 30 generations and the search finds
    // exactly that, whatever budget it is given.
    for budget in [60u16, 120, 200] {
        let movie = generate(Kind::GameOfLife, &Spec { frames: budget, ..Spec::default() });
        assert_eq!(
            movie.frames.len(),
            30,
            "a budget of {budget} should still find the 30-generation cycle"
        );
    }
}

#[test]
fn a_random_soup_would_have_been_the_wrong_choice() {
    // Recording why the garden exists. Life on a board this size burns down to
    // still lifes and blinkers, so a soup's exact cycle is two frames of things
    // twitching -- correct, and not worth watching. This checks the claim rather
    // than leaving it as a comment.
    let columns = 80usize;
    let lines = 25usize;
    let mut rng = Rng::new(9);
    let mut cells: Vec<bool> = (0..columns * lines).map(|_| rng.chance(35)).collect();

    let alive = |cells: &[bool], c: isize, l: isize| -> bool {
        let c = (c.rem_euclid(columns as isize)) as usize;
        let l = (l.rem_euclid(lines as isize)) as usize;
        cells[l * columns + c]
    };
    for _ in 0..1500 {
        let mut next = vec![false; cells.len()];
        for l in 0..lines {
            for c in 0..columns {
                let mut n = 0u8;
                for dl in -1_isize..=1 {
                    for dc in -1_isize..=1 {
                        if (dc, dl) != (0, 0) && alive(&cells, c as isize + dc, l as isize + dl) {
                            n += 1;
                        }
                    }
                }
                next[l * columns + c] =
                    matches!((alive(&cells, c as isize, l as isize), n), (true, 2 | 3) | (false, 3));
            }
        }
        cells = next;
    }
    // Settled: one more generation and then another returns to where it was.
    let settled = cells.clone();
    for _ in 0..2 {
        let mut next = vec![false; cells.len()];
        for l in 0..lines {
            for c in 0..columns {
                let mut n = 0u8;
                for dl in -1_isize..=1 {
                    for dc in -1_isize..=1 {
                        if (dc, dl) != (0, 0) && alive(&cells, c as isize + dc, l as isize + dl) {
                            n += 1;
                        }
                    }
                }
                next[l * columns + c] =
                    matches!((alive(&cells, c as isize, l as isize), n), (true, 2 | 3) | (false, 3));
            }
        }
        cells = next;
    }
    assert_eq!(cells, settled, "a soup settles to period 2 (or 1), which is why the garden exists");
}

#[test]
fn a_degenerate_canvas_does_not_panic() {
    for kind in Kind::ALL {
        for (columns, lines) in [(1u16, 1u16), (1, 40), (40, 1), (3, 3)] {
            let spec = Spec { columns, lines, frames: 4, ..Spec::default() };
            let movie = generate(kind, &spec);
            assert_eq!((movie.columns, movie.lines), (columns, lines), "{}", kind.name());
        }
    }
}

#[test]
fn delta_encoding_still_pays_on_a_full_screen_animation() {
    // The worry that .raba stops being worth it once the whole screen moves. It
    // does get expensive -- fire changes about half the canvas every frame -- but
    // never worse than storing every cell, which is the floor a dense format
    // would sit at.
    for kind in Kind::ALL {
        let movie = generate(kind, &Spec::default());
        let dense = usize::from(movie.columns)
            * usize::from(movie.lines)
            * movie.frames.len();
        assert!(
            movie.ops.len() < dense,
            "{}: {} ops against {dense} for a dense format",
            kind.name(),
            movie.ops.len()
        );
    }
}

#[test]
fn names_match_lys_configuration_values() {
    // These exist so somebody can move a Ly setup across. The names have to be
    // the ones in /etc/ly/config.ini.
    for name in ["doom", "matrix", "colormix", "gameoflife"] {
        assert!(Kind::named(name).is_some(), "{name} is a Ly animation value");
    }
    assert!(Kind::named("none").is_none(), "Ly's `none` is not an animation");
    assert!(Kind::named("nonsense").is_none());
    assert_eq!(Kind::named("DOOM"), Kind::named("doom"), "names are case-insensitive");
}
