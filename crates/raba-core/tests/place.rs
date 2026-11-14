//! Fitting a canvas to a screen that is not its size.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use raba_core::render::{
    all_shading, best_fit, crops, fit, intensity, resample, scaled_size, Cell, Grid, Placement,
    RAMP,
};
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

/// Build a grid from rows of characters, every cell the same colour.
fn grid_of(rows: &[&str]) -> Grid {
    let columns = u16::try_from(rows.iter().map(|r| r.chars().count()).max().unwrap_or(0)).unwrap();
    let lines = u16::try_from(rows.len()).unwrap();
    let mut grid = Grid::new(columns, lines, Palette::Ansi16).unwrap();
    for (line, row) in rows.iter().enumerate() {
        for (column, ch) in row.chars().enumerate() {
            let (column, line) = (u16::try_from(column).unwrap(), u16::try_from(line).unwrap());
            grid.set(column, line, Cell { ch, fg: 7, bg: 0 });
        }
    }
    grid
}

fn render(grid: &Grid) -> Vec<String> {
    (0..grid.lines())
        .map(|line| (0..grid.columns()).map(|c| grid.get(c, line).unwrap().ch).collect::<String>())
        .collect()
}

#[test]
fn shading_art_is_recognised_and_line_art_is_not() {
    assert!(all_shading("  ░▒▓█..\u{b7}".chars()));
    // A box-drawing logo: halving a line does not give you half a line.
    assert!(!all_shading("┌──┐ ".chars()));
    // One stray glyph is enough to disqualify a whole movie.
    assert!(!all_shading("░▒▓█X".chars()));
    // Nothing at all is not shading art: there is nothing to resample.
    assert!(!all_shading("".chars()));
}

#[test]
fn intensity_is_the_position_on_the_ramp() {
    assert_eq!(intensity(' '), Some(0));
    assert_eq!(intensity('█'), Some(6));
    assert_eq!(intensity('X'), None);
    for (index, ch) in RAMP.iter().enumerate() {
        assert_eq!(intensity(*ch), Some(u8::try_from(index).unwrap()));
    }
}

#[test]
fn resampling_averages_coverage_rather_than_sampling_it() {
    // Four 2x2 blocks, each a different uniform intensity. Halving must give one
    // cell per block at exactly that intensity.
    let source = grid_of(&["██░░", "██░░", "    ", "    "]);
    let out = resample(&source, Size { columns: 2, lines: 2 });
    assert_eq!(render(&out), vec!["█░".to_string(), "  ".to_string()]);

    // A block that is half full and half empty averages to the middle of the ramp
    // rather than to whichever corner happened to be sampled.
    let half = grid_of(&["█ ", " █"]);
    let out = resample(&half, Size { columns: 1, lines: 1 });
    assert_eq!(intensity(out.get(0, 0).unwrap().ch), Some(3));
}

#[test]
fn resampling_takes_the_colour_of_the_brightest_contributor() {
    // Averaging colour would turn a bright core on a dark sky into uniform mud.
    let mut source = Grid::new(2, 1, Palette::Ansi16).unwrap();
    source.set(0, 0, Cell { ch: '█', fg: 15, bg: 0 });
    source.set(1, 0, Cell { ch: ' ', fg: 1, bg: 0 });
    let out = resample(&source, Size { columns: 1, lines: 1 });
    assert_eq!(out.get(0, 0).unwrap().fg, 15);
}

#[test]
fn resampling_always_returns_exactly_the_size_asked_for() {
    let source = grid_of(&["█▓▒░ ░▒▓█", "▓▒░  ░▒▓ ", "▒░    ░▒ "]);
    for (columns, lines) in [(1, 1), (3, 2), (9, 3), (20, 10), (80, 25)] {
        let out = resample(&source, Size { columns, lines });
        assert_eq!((out.columns(), out.lines()), (columns, lines));
    }
}

#[test]
fn scaling_keeps_the_proportions_and_stays_inside_the_screen() {
    // A wide canvas on a tall screen is limited by width, not stretched into it.
    let out = scaled_size(Size { columns: 240, lines: 67 }, Size { columns: 80, lines: 60 });
    assert_eq!(out.columns, 80);
    assert_eq!(out.lines, 22); // 67 * 80 / 240
                               // And the other way round.
    let out = scaled_size(Size { columns: 40, lines: 40 }, Size { columns: 200, lines: 20 });
    assert_eq!((out.columns, out.lines), (20, 20));
    // Never larger than the screen, whatever the shape.
    for (sc, sl) in [(1, 1000), (1000, 1), (7, 13), (240, 67)] {
        let out = scaled_size(Size { columns: sc, lines: sl }, Size { columns: 80, lines: 25 });
        assert!(out.columns <= 80 && out.lines <= 25 && out.columns >= 1 && out.lines >= 1);
    }
}

#[test]
fn best_fit_resamples_shading_art_but_only_when_it_does_not_fit() {
    let source = grid_of(&["████", "████"]);
    let screen = Size { columns: 2, lines: 1 };

    // Shading art that overflows is scaled down: all four cells survive as coverage.
    let scaled = best_fit(&source, screen, true);
    assert_eq!((scaled.columns(), scaled.lines()), (2, 1));
    assert_eq!(render(&scaled), vec!["██".to_string()]);

    // The same art declared not-shading is cropped instead, which is what a logo wants.
    let cropped = best_fit(&source, screen, false);
    assert_eq!((cropped.columns(), cropped.lines()), (2, 1));

    // Art that already fits is centred, never blown up: a scaled-up logo looks broken
    // where a centred one looks deliberate.
    let small = grid_of(&["█"]);
    let big = best_fit(&small, Size { columns: 5, lines: 3 }, true);
    assert_eq!((big.columns(), big.lines()), (5, 3));
    assert_eq!(render(&big), vec!["     ".to_string(), "  █  ".to_string(), "     ".to_string()]);
}

#[test]
fn art_already_the_size_of_the_screen_is_handed_back_untouched() {
    // The common case at boot on a console the theme was drawn for, and the
    // one place `fit` must not allocate a second grid and copy cell by cell:
    // it runs once per frame on a machine with nothing else to do yet.
    let art = filled(8, 3, '#');
    let same = fit(&art, Size { columns: 8, lines: 3 });
    assert_eq!(same.cells(), art.cells());
    assert_eq!((same.columns(), same.lines()), (8, 3));
}

#[test]
fn resampling_to_the_same_size_keeps_the_picture_recognisable() {
    let art = filled(8, 4, '\u{2588}');
    let same = resample(&art, Size { columns: 8, lines: 4 });
    assert_eq!((same.columns(), same.lines()), (8, 4));
    assert_eq!(row(&same, 0), row(&art, 0));
}

#[test]
fn resampling_art_with_no_cells_in_it_gives_back_a_blank_screen_rather_than_nothing() {
    // `Grid::new(0, n, _)` is a legal grid, and art can reach here with no
    // columns — an ANSI file of nothing but newlines, for one. The output
    // still has to be the size that was asked for, because the caller hands
    // it straight to the encoder.
    let nothing = Grid::new(0, 4, Palette::Ansi16).unwrap();
    let out = resample(&nothing, Size { columns: 6, lines: 2 });
    assert_eq!((out.columns(), out.lines()), (6, 2));
    assert!(out.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn a_screen_wider_than_a_canvas_may_be_is_resampled_onto_in_full() {
    // `resample` and `fit` both produce a *screen*, not a canvas, so they are
    // bounded by cells and not by either axis — otherwise a console wider than
    // 1024 columns would be handed back art of the wrong size, which the
    // encoder would then write with cursor moves for a screen that is not
    // there.
    let source = filled(10, 4, '#');
    let wide = resample(&source, Size { columns: 2000, lines: 50 });
    assert_eq!((wide.columns(), wide.lines()), (2000, 50));
    let placed = fit(&source, Size { columns: 2000, lines: 50 });
    assert_eq!((placed.columns(), placed.lines()), (2000, 50));
    assert_eq!(
        placed.get(995, 23).map(|cell| cell.ch),
        Some('#'),
        "the art was centred on 1024 columns rather than on 2000"
    );

    // Past the cell bound both clamp, and still come back as a usable screen.
    let huge = fit(&source, Size { columns: u16::MAX, lines: u16::MAX });
    assert_eq!((huge.columns(), huge.lines()), (1024, 1024));

    // Zero is the other end of the same story: the smallest grid there is.
    let nothing = resample(&source, Size { columns: 0, lines: 0 });
    assert_eq!((nothing.columns(), nothing.lines()), (1, 1));
}
