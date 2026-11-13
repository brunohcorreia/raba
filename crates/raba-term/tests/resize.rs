//! The console changed size under a running player.
//!
//! Not a window being dragged: early in a boot this is the console being
//! *replaced*. Before the graphics driver loads the kernel binds `dummycon`,
//! which answers every ioctl, reports 80x25 and displays nothing; when the
//! driver arrives fbcon takes over at the panel's real size, with a blank
//! screen. Every frame after that is a delta against a picture that no longer
//! exists, so the splash would stay invisible for the rest of the boot.
//!
//! So: after a resize the next frame has to arrive complete. That is the whole
//! of what is asserted here, and it is asserted the way the renderer's own tests
//! assert — by replaying the bytes through the strict terminal model and looking
//! at the picture, rather than by counting bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use raba_core::render::{best_fit, resamples};
use raba_core::surface::{Size, Surface};
use raba_core::testing::Terminal;
use raba_core::{apply, Cell, Frame, Grid, Movie, Op, Palette, Scaling};
use raba_term::play::Player;
use std::io;

/// Collects what was presented, one `Vec` per frame, and lies about its size on
/// demand — which is the part a real console does for real reasons.
struct Recorder {
    size: Size,
    frames: Vec<Vec<u8>>,
}

impl Surface for Recorder {
    fn size(&self) -> Size {
        self.size
    }
    fn present(&mut self, frame: &[u8]) -> io::Result<()> {
        self.frames.push(frame.to_vec());
        Ok(())
    }
    fn restore(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Two frames: a filled canvas, then a single changed cell. The second frame is
/// the delta that a resize has to stop relying on.
fn movie() -> Movie {
    let mut canvas = Grid::new(12, 6, Palette::Ansi16).unwrap();
    let mut ops = Vec::new();
    for line in 0..6u16 {
        for column in 0..12u16 {
            let n = usize::from(line) * 12 + usize::from(column);
            let cell =
                Cell { ch: ['#', '+', 'o', '='][n % 4], fg: u8::try_from(n % 8).unwrap(), bg: 0 };
            canvas.set(column, line, cell);
            ops.push(Op { cell: u32::try_from(n).unwrap(), ch: cell.ch, fg: cell.fg, bg: cell.bg });
        }
    }
    let first = Frame { delay_ms: 10, op_start: 0, op_count: u16::try_from(ops.len()).unwrap() };
    let start = u32::try_from(ops.len()).unwrap();
    ops.push(Op { cell: 0, ch: '@', fg: 7, bg: 0 });
    let second = Frame { delay_ms: 10, op_start: start, op_count: 1 };
    Movie {
        columns: 12,
        lines: 6,
        palette: Palette::Ansi16,
        scaling: Scaling::Crop,
        looping: true,
        frames: vec![first, second],
        ops,
    }
}

/// The picture the movie makes after `played` frames, laid onto a screen.
fn expected(movie: &Movie, played: usize, screen: Size) -> Grid {
    let mut canvas = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    for frame in movie.frames.iter().take(played) {
        apply(&mut canvas, movie, frame);
    }
    best_fit(&canvas, screen, resamples(movie))
}

/// Replay one frame's bytes onto a blank screen: what a console that has just
/// appeared would end up showing.
fn onto_a_blank_screen(bytes: &[u8], screen: Size, palette: Palette) -> Grid {
    let mut terminal = Terminal::blank(screen.columns, screen.lines, palette);
    terminal.apply(bytes);
    terminal.into_screen()
}

#[test]
fn the_frame_after_a_resize_arrives_complete() {
    let movie = movie();
    let small = Size { columns: 80, lines: 25 };
    let large = Size { columns: 100, lines: 30 };

    let mut surface = Recorder { size: small, frames: Vec::new() };
    let mut player = Player::new(&movie, small).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();

    // The console is replaced: bigger, and showing nothing.
    surface.size = large;
    player.resize(&movie, large).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();

    let after = surface.frames.last().unwrap();
    assert_eq!(
        onto_a_blank_screen(after, large, movie.palette),
        expected(&movie, 2, large),
        "the first frame after a resize did not repaint the screen"
    );
}

#[test]
fn without_a_resize_the_frame_is_still_a_delta() {
    // The control. Without this, the test above passes on a player that repaints
    // every frame, which would be a different program with a different cost.
    let movie = movie();
    let screen = Size { columns: 80, lines: 25 };
    let mut surface = Recorder { size: screen, frames: Vec::new() };
    let mut player = Player::new(&movie, screen).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();

    let second = surface.frames.last().unwrap();
    assert_ne!(
        onto_a_blank_screen(second, screen, movie.palette),
        expected(&movie, 2, screen),
        "the second frame repainted the whole screen; the delta is gone"
    );
    assert!(second.len() < surface.frames[0].len() / 4, "a one-cell delta should be tiny");
}

#[test]
fn a_resize_does_not_restart_the_animation() {
    // The viewer should see the splash appear, not begin again: the frame, the
    // round and the canvas all survive.
    let movie = movie();
    let mut surface = Recorder { size: Size { columns: 80, lines: 25 }, frames: Vec::new() };
    let mut player = Player::new(&movie, surface.size).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();
    player.step(&mut surface, &movie, 0).unwrap();
    player.step(&mut surface, &movie, 0).unwrap(); // wraps: round 1, frame 1
    let (frame, round) = (player.frame(), player.round());

    player.resize(&movie, Size { columns: 100, lines: 30 }).unwrap();
    assert_eq!((player.frame(), player.round()), (frame, round));
}
