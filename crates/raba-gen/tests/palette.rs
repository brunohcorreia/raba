//! Fitting colours to a console.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::Palette;
use raba_gen::build::{build, Recipe, SpinnerSpec};
use raba_gen::layout::Position;
use raba_gen::palette::{clamp_background, downsample, nearest_console_colour, rgb_of_xterm};

#[test]
fn the_system_colours_are_themselves() {
    for index in 0u8..16 {
        assert_eq!(downsample(index), index);
    }
}

#[test]
fn the_colour_cube_steps_are_not_evenly_spaced() {
    // 16 is the cube's black corner, 231 its white one. The levels jump 0, 95, 135,
    // 175, 215, 255 -- a conversion assuming `level * 51` lands wrong everywhere but
    // the ends, and looks plausible enough to ship.
    assert_eq!(rgb_of_xterm(16), (0, 0, 0));
    assert_eq!(rgb_of_xterm(231), (255, 255, 255));
    assert_eq!(rgb_of_xterm(196), (255, 0, 0), "pure red in the cube");
    assert_eq!(rgb_of_xterm(46), (0, 255, 0), "pure green");
    assert_eq!(rgb_of_xterm(21), (0, 0, 255), "pure blue");
    // The first step off zero is 95, not 51.
    assert_eq!(rgb_of_xterm(16 + 36), (95, 0, 0));
}

#[test]
fn the_grey_ramp_runs_from_8_to_238() {
    assert_eq!(rgb_of_xterm(232), (8, 8, 8));
    assert_eq!(rgb_of_xterm(255), (238, 238, 238));
}

#[test]
fn a_pure_primary_keeps_its_hue_rather_than_its_brightness() {
    // The console's light red is 255,85,85 -- pinker than pure red, not just
    // brighter. Dropping to the dim variant at 170,0,0 is the closer match, which
    // is not what "bright colour goes to bright slot" would predict.
    assert_eq!(downsample(196), 1, "255,0,0");
    assert_eq!(downsample(46), 2, "0,255,0");
    assert_eq!(downsample(21), 4, "0,0,255");
}

#[test]
fn the_secondaries_go_bright_because_vga_put_brown_where_yellow_should_be() {
    // The asymmetry with the primaries above, and it comes from the palette rather
    // than from the metric: console colour 3 is 170,85,0, a brown, so there is no
    // dim yellow to fall back to. Same story for cyan and magenta, whose dim forms
    // sit further from the pure colour than the bright ones do.
    assert_eq!(downsample(226), 11, "255,255,0");
    assert_eq!(downsample(51), 14, "0,255,255");
    assert_eq!(downsample(201), 13, "255,0,255");
}

#[test]
fn orange_has_nowhere_better_to_go_than_light_red() {
    // A 16-colour console has no orange. Worth a test because it is the first thing
    // a theme author will try and be surprised by.
    assert_eq!(downsample(208), 9);
}

#[test]
fn greys_spread_across_the_four_the_console_has() {
    // black, dark grey, light grey, white -- and nothing lands between them.
    let greys: Vec<u8> = (232u8..=255).map(downsample).collect();
    assert!(greys.iter().all(|index| matches!(index, 0 | 7 | 8 | 15)), "{greys:?}");
    assert_eq!(downsample(232), 0, "nearly black");
    assert_eq!(downsample(255), 15, "nearly white");
}

#[test]
fn the_console_palette_is_the_target_not_xterms() {
    // xterm renders blue as 0,0,238 and the console as 0,0,170. Fitting against
    // xterm's table would shift every hue on the machine the theme is for.
    assert_eq!(nearest_console_colour((0, 0, 170)), 4);
    assert_eq!(nearest_console_colour((170, 85, 0)), 3, "console brown, not yellow");
}

#[test]
fn a_mid_blue_does_not_become_grey() {
    // What an unweighted distance gets wrong, and the reason for the redmean
    // weighting -- which only works because it is scaled: `mean / 256` in integer
    // arithmetic is always zero, and the formula collapses to a fixed weighting
    // that still looks plausible.
    assert_eq!(nearest_console_colour((0, 0, 200)), 4);
    assert_eq!(nearest_console_colour((40, 40, 190)), 4);
    assert_eq!(downsample(62), 12, "95,95,215 is light blue, not dark grey");
}

#[test]
fn backgrounds_keep_their_hue_when_they_lose_their_brightness() {
    // 9 is light red; the console would blink it. Dropping to 1 keeps the colour the
    // author chose and loses only the part the hardware will not give.
    assert_eq!(clamp_background(9), 1);
    assert_eq!(clamp_background(12), 4);
    assert_eq!(clamp_background(15), 7);
    // Above the sixteen there is no blink bit to avoid: the cube's 240 is a grey,
    // not a bright anything. Folding it in gave `(240 - 8) & 7` = 0, which turned
    // every 256-colour background black.
    for index in [16u8, 17, 128, 232, 240, 255] {
        assert_eq!(clamp_background(index), index, "the 256-colour cube is untouched");
    }
    for index in 0u8..8 {
        assert_eq!(clamp_background(index), index, "the low eight are untouched");
    }
}

#[test]
fn building_with_downsample_reports_what_it_changed() {
    // A 16-colour theme whose indices are xterm-256 values: raba-theme(5) says they
    // are fitted rather than refused, so the palette stays Ansi16 here.
    let recipe = Recipe {
        colours: vec![196, 46],
        default_fg: 255,
        bg: 232,
        spinner: Some(SpinnerSpec {
            frames: vec!["|".to_owned()],
            fg: 226,
            position: Position::Below,
            gap: 1,
            period: None,
        }),
        ..Recipe::default()
    };
    let built = build("$1a$2b", &recipe).unwrap();

    assert_eq!(built.movie.palette, Palette::Ansi16, "the output is what a console has");
    assert!(built.movie.ops.iter().all(|op| op.fg <= 15 && op.bg <= 15));

    let changes: Vec<String> = built.adjustments.iter().map(ToString::to_string).collect();
    assert!(changes.iter().any(|c| c.contains("slot $1") && c.contains("196")), "{changes:?}");
    // 232 is near-black, which becomes 0 -- and 0 needs no clamping afterwards, so
    // the background is reported once rather than twice.
    assert_eq!(changes.iter().filter(|c| c.starts_with("background")).count(), 1, "{changes:?}");
    assert!(changes.iter().any(|c| c.contains("spinner colour")), "{changes:?}");
}

#[test]
fn a_blinking_background_is_fixed_whether_or_not_you_asked() {
    // Not optional and not behind a flag: there is no case where an author wanted
    // the background to flash, and the recipe cannot say so anyway.
    let recipe = Recipe { bg: 12, ..Recipe::default() };
    let built = build("$1a", &recipe).unwrap();
    assert!(built.movie.ops.iter().all(|op| op.bg == 4));
    assert_eq!(built.adjustments.len(), 1);
    assert!(built.adjustments[0].reason.contains("blink"));
}

#[test]
fn a_theme_that_needed_nothing_reports_nothing() {
    let built = build("$1a", &Recipe { colours: vec![2], ..Recipe::default() }).unwrap();
    assert!(built.adjustments.is_empty());
}

#[test]
fn a_256_colour_theme_keeps_its_colours_when_not_downsampling() {
    // Backgrounds are only clamped for a 16-colour output: in 256 colours there is
    // no high bit doing double duty.
    let recipe =
        Recipe { palette: Palette::Xterm256, colours: vec![208], bg: 236, ..Recipe::default() };
    let built = build("$1a", &recipe).unwrap();
    assert!(built.adjustments.is_empty());
    assert!(built.movie.ops.iter().any(|op| op.fg == 208));
}
