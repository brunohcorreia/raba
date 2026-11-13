//! Theme recipes, as `raba-theme`(5) specifies them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::Palette;
use raba_gen::build::{build, Recipe as BuildRecipe};
use raba_gen::layout::Position;
use raba_gen::recipe::{parse, ArtSource, RecipeError};
use std::path::{Path, PathBuf};

const SHIPPED: &str = include_str!("../../../themes/cachyos.toml");
const ART: &str = include_str!("../../../themes/cachyos.txt");

#[test]
fn a_recipe_needs_only_a_logo() {
    let theme = parse("[logo]\nart = \"##\"").unwrap();
    assert_eq!(theme.logo.source, "inline");
    assert_eq!(theme.canvas.framerate, 10);
    assert_eq!(theme.canvas.palette, 16);
    assert!(theme.canvas.repeat);
    assert!(theme.spinner.is_none(), "no section means a static theme");

    let recipe = BuildRecipe::from(&theme);
    assert!(recipe.spinner.is_none());
    assert_eq!(recipe.delay_ms(), 100, "10 fps");
}

#[test]
fn the_documented_example_parses() {
    // Straight out of raba-theme(5), minus the fastfetch source, which needs art
    // that is not vendored yet.
    let theme = parse(
        r#"
[logo]
source = "file:arch.txt"
colors = [6, 4]

[spinner]
style = "ascii"
position = "below"
gap = 1
color = 7

[canvas]
framerate = 10
align = "center"
"#,
    )
    .unwrap();

    let recipe = BuildRecipe::from(&theme);
    assert_eq!(recipe.colours, vec![6, 4]);
    let spinner = recipe.spinner.unwrap();
    assert_eq!(spinner.frames.len(), 4);
    assert_eq!(spinner.position, Position::Below);
    assert_eq!(spinner.gap, 1);
}

#[test]
fn framerate_becomes_a_per_frame_delay() {
    // The header has no frame-rate field: a movie carries a delay per frame so a
    // theme can hold one longer than the rest.
    let at = |rate: u16| {
        let source = format!("[logo]\nart = \"#\"\n[canvas]\nframerate = {rate}");
        BuildRecipe::from(&parse(&source).unwrap()).delay_ms()
    };
    assert_eq!(at(10), 100);
    assert_eq!(at(24), 41);
    assert_eq!(at(1), 1000);
    assert_eq!(at(0), 1000, "a frame rate of zero is one frame per second, not a panic");
}

#[test]
fn an_outro_plays_once() {
    let theme = parse("[logo]\nart = \"#\"\n[canvas]\nrepeat = false").unwrap();
    assert!(!BuildRecipe::from(&theme).looping);
}

#[test]
fn a_misspelled_key_is_refused_rather_than_ignored() {
    // The worst available outcome is a theme built in the wrong colours with no
    // complaint, which is what accepting unknown keys would produce.
    let error = parse("[logo]\nart = \"#\"\ncolrs = [1]").unwrap_err();
    assert!(matches!(error, RecipeError::Toml(_)), "{error}");
    assert!(error.to_string().contains("colrs"), "{error}");
}

#[test]
fn british_spellings_are_accepted() {
    // raba-theme(5) writes "colors"; the project writes "colours", and so will half
    // the people sending recipes.
    let theme = parse("[logo]\nart = \"#\"\ncolours = [3]\n[spinner]\ncolour = 5").unwrap();
    assert_eq!(theme.logo.colors, vec![3]);
    assert_eq!(theme.spinner.unwrap().color, 5);
}

// --- the logo source -------------------------------------------------------

#[test]
fn a_file_source_resolves_next_to_the_recipe() {
    let theme = parse("[logo]\nsource = \"file:art.txt\"").unwrap();
    assert_eq!(
        theme.art_source(Path::new("themes/x.toml")).unwrap(),
        ArtSource::File(PathBuf::from("themes/art.txt"))
    );
}

#[test]
fn an_absolute_art_path_is_left_alone() {
    let theme = parse("[logo]\nsource = \"file:/usr/share/x.txt\"").unwrap();
    assert_eq!(
        theme.art_source(Path::new("themes/x.toml")).unwrap(),
        ArtSource::File(PathBuf::from("/usr/share/x.txt"))
    );
}

#[test]
fn an_inline_source_carries_its_own_art() {
    let theme = parse("[logo]\nart = \"$1##\"").unwrap();
    assert_eq!(
        theme.art_source(Path::new("x.toml")).unwrap(),
        ArtSource::Inline("$1##".to_owned())
    );
}

#[test]
fn a_fastfetch_source_names_the_logo() {
    let theme = parse("[logo]\nsource = \"fastfetch:arch\"").unwrap();
    assert_eq!(
        theme.art_source(Path::new("x.toml")).unwrap(),
        ArtSource::Fastfetch("arch".to_owned())
    );
}

#[test]
fn an_unknown_source_form_says_what_the_forms_are() {
    let error = parse("[logo]\nsource = \"http://example.com/art.txt\"").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("file:") && message.contains("inline"), "{message}");
    assert!(message.contains("fastfetch:"), "{message}");
}

#[test]
fn source_and_art_must_agree() {
    // Each of these is a recipe saying two things, where acting on one means
    // silently ignoring the other.
    let error = parse("[logo]\nsource = \"inline\"").unwrap_err();
    assert_eq!(error, RecipeError::ArtMismatch { source: "inline".to_owned() });

    let error = parse("[logo]\nsource = \"file:a.txt\"\nart = \"##\"").unwrap_err();
    assert_eq!(error, RecipeError::ArtMismatch { source: "file:a.txt".to_owned() });
}

// --- spinner ---------------------------------------------------------------

#[test]
fn an_unknown_spinner_lists_the_ones_that_exist() {
    let error = parse("[logo]\nart = \"#\"\n[spinner]\nstyle = \"braille\"").unwrap_err();
    let message = error.to_string();
    for style in ["ascii", "dots", "bar", "arc", "blocks", "custom"] {
        assert!(message.contains(style), "{style} missing from {message}");
    }
}

#[test]
fn style_and_frames_must_agree() {
    let error = parse("[logo]\nart = \"#\"\n[spinner]\nstyle = \"custom\"").unwrap_err();
    assert_eq!(error, RecipeError::FramesMismatch { style: "custom".to_owned() });

    let error =
        parse("[logo]\nart = \"#\"\n[spinner]\nstyle = \"ascii\"\nframes = [\"x\"]").unwrap_err();
    assert_eq!(error, RecipeError::FramesMismatch { style: "ascii".to_owned() });
}

#[test]
fn custom_frames_may_be_more_than_one_cell_wide() {
    let theme =
        parse("[logo]\nart = \"#\"\n[spinner]\nstyle = \"custom\"\nframes = [\"<  >\", \" << \"]")
            .unwrap();
    let spinner = BuildRecipe::from(&theme).spinner.unwrap();
    assert_eq!(spinner.frames, vec!["<  >".to_owned(), " << ".to_owned()]);
}

#[test]
fn a_position_that_is_not_one_of_the_five_is_refused() {
    let error = parse("[logo]\nart = \"#\"\n[spinner]\nposition = \"beneath\"").unwrap_err();
    assert_eq!(
        error,
        RecipeError::BadPosition { field: "spinner.position", got: "beneath".to_owned() }
    );

    let error =
        parse("[logo]\nart = \"#\"\n[text]\ncontent = \"x\"\nposition = \"middle\"").unwrap_err();
    assert_eq!(
        error,
        RecipeError::BadPosition { field: "text.position", got: "middle".to_owned() }
    );
}

// --- canvas ----------------------------------------------------------------

#[test]
fn only_16_and_256_are_palettes() {
    let error = parse("[logo]\nart = \"#\"\n[canvas]\npalette = 88").unwrap_err();
    assert_eq!(error, RecipeError::BadPalette { got: 88 });
}

#[test]
fn a_256_colour_index_in_a_16_colour_theme_is_fitted_not_refused() {
    // raba-theme(5): anything above 15 is downsampled at build time. The recipe is
    // valid; raba-gen reports what it changed.
    let theme = parse("[logo]\nart = \"$1#\"\ncolors = [196]").unwrap();
    let built = build("$1#", &BuildRecipe::from(&theme)).unwrap();
    assert_eq!(built.movie.palette, Palette::Ansi16);
    assert!(!built.adjustments.is_empty(), "and says so");
    assert!(built.movie.ops.iter().all(|op| op.fg <= 15));
}

#[test]
fn placement_keys_are_accepted_and_reported_as_not_yet_honoured() {
    // raba-theme(5) documents them, the format has no field for them, and silently
    // ignoring a documented key is the thing this schema refuses to do elsewhere.
    let theme = parse("[logo]\nart = \"#\"\n[canvas]\nalign = \"top\"\nx_offset = 3").unwrap();
    let notes = theme.unhonoured();
    assert_eq!(notes.len(), 2);
    assert!(notes[0].contains("align"), "{notes:?}");
    assert!(notes[1].contains("x_offset"), "{notes:?}");

    // The default alignment is not a change, so it is not reported.
    assert!(parse("[logo]\nart = \"#\"").unwrap().unhonoured().is_empty());
}

// --- the shipped theme -----------------------------------------------------

#[test]
fn the_shipped_recipe_builds_the_shipped_theme() {
    // The recipe in themes/ is the one that is tested, so a change to it that breaks
    // the theme cannot pass CI.
    let theme = parse(SHIPPED).unwrap();
    assert_eq!(
        theme.art_source(Path::new("themes/cachyos.toml")).unwrap(),
        ArtSource::File(PathBuf::from("themes/cachyos.txt"))
    );
    assert!(theme.unhonoured().is_empty(), "it uses nothing we cannot deliver");

    let built = build(ART, &BuildRecipe::from(&theme)).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (54, 24));
    assert_eq!(built.movie.frames.len(), 4);
    assert!(built.adjustments.is_empty(), "its colours need no fitting");
}

// --- the corners of the schema -----------------------------------------------

#[test]
fn a_source_scheme_this_tool_does_not_know_is_refused_when_the_art_is_asked_for() {
    // `parse` already catches this — `an_unknown_source_form_says_what_the_forms_are`
    // above is the first line of defence. This is the second: `art_source` is
    // public and a caller that built a `Theme` by hand gets the same refusal
    // rather than a path called `carrier-pigeon:arch`.
    let mut theme = parse("[logo]\nart = \"##\"\n").expect("the recipe parses");
    theme.logo.source = "carrier-pigeon:arch".to_owned();
    let why = theme.art_source(Path::new("thing.toml")).expect_err("an unknown scheme is refused");
    assert!(matches!(why, RecipeError::BadSource { .. }), "{why:?}");
    assert!(why.to_string().contains("carrier-pigeon"), "{why}");
    assert!(why.to_string().contains("fastfetch:"), "{why}: say what it accepts");
}

#[test]
fn a_recipe_with_no_logo_section_is_refused_and_says_which_section_is_missing() {
    // Not defaulted. A recipe with no art is a theme with nothing to draw, and
    // "missing field `logo`" names the fix; an empty canvas five steps later
    // does not.
    let why = parse("[canvas]\nframerate = 12\n").expect_err("a recipe needs a logo");
    assert!(why.to_string().contains("logo"), "{why}");
}

#[test]
fn a_256_colour_canvas_asks_for_the_256_colour_palette_and_nothing_else_does() {
    let wide = parse("[logo]\nart = \"##\"\n[canvas]\npalette = 256\n").expect("parses");
    assert_eq!(BuildRecipe::from(&wide).palette, Palette::Xterm256);

    let narrow = parse("[logo]\nart = \"##\"\n[canvas]\npalette = 16\n").expect("parses");
    assert_eq!(BuildRecipe::from(&narrow).palette, Palette::Ansi16);
}

#[test]
fn a_text_section_becomes_a_text_overlay_with_everything_it_was_given() {
    let theme = parse(
        "[logo]\nart = \"##\"\n\
         [text]\ncontent = \"booting\"\ncolor = 4\nposition = \"above\"\ngap = 2\n",
    )
    .expect("parses");
    let spec = BuildRecipe::from(&theme).text.expect("a text overlay");
    assert_eq!(spec.content, "booting");
    assert_eq!(spec.fg, 4);
    assert_eq!(spec.position, Position::Above);
    assert_eq!(spec.gap, 2);
}

#[test]
fn a_position_that_is_not_one_falls_back_rather_than_losing_the_overlay() {
    // The parser has already refused an unknown position by the time this
    // runs — `[text] position = "sideways"` is a `BadPosition`. This is the
    // belt to that brace: if one ever got through, the text still appears.
    let theme = parse("[logo]\nart = \"##\"\n[text]\ncontent = \"x\"\n").expect("parses");
    let spec = BuildRecipe::from(&theme).text.expect("a text overlay");
    assert_eq!(spec.position, Position::Below, "the documented default");
}
