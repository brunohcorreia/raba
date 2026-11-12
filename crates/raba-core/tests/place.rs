//! Fitting a canvas to a screen that is not its size.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use raba_core::render::{crops, fit, Cell, Grid, Placement};
use raba_core::surface::Size;
use raba_core::Palette;

fn filled(columns: u16, lines: u16, ch: char) -> Grid {
    let mut grid = Grid::new(columns, lines, Palette::Ansi16).unwrap();
    grid.fill(Cell { ch, fg: 7, bg: 0 });
    grid
}

fn row(grid: &Grid, line: u16) -> String {
    (0..grid.columns()).map(|c| grid.get(c, line).unwrap().ch).collect()
}

#[test]
fn a_canvas_the_size_of_the_screen_is_untouched() {
    let grid = filled(4, 2, 'x');
    let fitted = fit(&grid, Size { columns: 4, lines: 2 });
    assert_eq!(fitted, grid);
}

#[test]
fn a_smaller_canvas_is_centred_and_the_rest_is_blank() {
    let grid = filled(2, 1, 'x');
    let fitted = fit(&grid, Size { columns: 6, lines: 3 });
    assert_eq!((fitted.columns(), fitted.lines()), (6, 3));
    assert_eq!(row(&fitted, 0), "      ");
    assert_eq!(row(&fitted, 1), "  xx  ");
    assert_eq!(row(&fitted, 2), "      ");
}

#[test]
fn an_odd_leftover_rounds_the_same_way_every_time() {
    // Which way it rounds matters less than that it never changes: a frame that
    // rounded differently from the frame before it would shimmer by one column.
    let grid = filled(2, 1, 'x');
    let fitted = fit(&grid, Size { columns: 5, lines: 1 });
    assert_eq!(row(&fitted, 0), " xx  ", "leftover goes to the right");
    assert_eq!(
        Placement::centred(Size { columns: 2, lines: 1 }, Size { columns: 5, lines: 1 }),
        Placement { column: 1, line: 0 }
    );
}

#[test]
fn a_larger_canvas_is_cropped_around_its_middle() {
    // The black hole is 240x67 and a console at boot may be 80x25. Showing the
    // middle of the picture is the useful answer; drawing past the last column
    // would leave the console scrolled, which is the one thing a splash must not
    // do.
    let mut grid = Grid::new(5, 1, Palette::Ansi16).unwrap();
    for (column, ch) in "abcde".chars().enumerate() {
        grid.set(u16::try_from(column).unwrap(), 0, Cell { ch, fg: 7, bg: 0 });
    }
    let fitted = fit(&grid, Size { columns: 3, lines: 1 });
    assert_eq!((fitted.columns(), fitted.lines()), (3, 1));
    assert_eq!(row(&fitted, 0), "bcd", "the middle, not the left edge");
}

#[test]
fn cropping_in_one_axis_still_pads_the_other() {
    let grid = filled(6, 1, 'x');
    let fitted = fit(&grid, Size { columns: 2, lines: 3 });
    assert_eq!(row(&fitted, 0), "  ");
    assert_eq!(row(&fitted, 1), "xx");
    assert_eq!(row(&fitted, 2), "  ");
}

#[test]
fn the_result_is_always_exactly_the_requested_size() {
    // What the caller depends on: whatever goes in, what comes out can be handed
    // to the encoder without checking, so its absolute cursor moves are in range.
    for (columns, lines) in [(1u16, 1u16), (3, 7), (80, 25), (240, 67)] {
        let grid = filled(13, 11, 'o');
        let fitted = fit(&grid, Size { columns, lines });
        assert_eq!((fitted.columns(), fitted.lines()), (columns, lines));
    }
}

#[test]
fn crops_says_whether_anything_is_lost() {
    let screen = Size { columns: 80, lines: 25 };
    assert!(!crops(Size { columns: 40, lines: 20 }, screen));
    assert!(!crops(screen, screen));
    assert!(crops(Size { columns: 240, lines: 67 }, screen), "the black hole does");
    assert!(crops(Size { columns: 40, lines: 30 }, screen), "one axis is enough");
}
