//! Every reason `raba-gen` refuses something, as the sentence a person reads.
//!
//! Coverage found the same hole in six different files: the error variants were
//! produced and matched on, and their `Display` was never run. That is the
//! wrong half to leave untested. A theme contributor never sees a
//! `ComposeError::RaggedOverlay`; they see one line of text, and whether that
//! line names the overlay, the step and both widths is the whole difference
//! between a tool that helps and a tool that says no.
//!
//! What is checked, for each variant:
//!
//! - it says something, and that something is a sentence rather than a number;
//! - every value the variant carries appears in it, because a message that
//!   drops the offending number sends the reader to look at the wrong line;
//! - where there is a limit, the limit is named too: "too wide" without the
//!   width that would have fitted is a refusal with no way forward.
//!
//! Constructed directly rather than provoked through the parsers. The parsers
//! have their own tests next to this one; what is under test here is the
//! wording, and reaching `FrameTooBig` through a real Durdraw file would be a
//! test of gzip.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba_gen::ansi::AnsiError;
use raba_gen::art::ArtError;
use raba_gen::build::BuildError;
use raba_gen::compose::ComposeError;
use raba_gen::dur::DurError;
use raba_gen::recipe::RecipeError;
use raba_gen::text::TextError;

/// Assert that `error` reads like something a person can act on.
///
/// `must_contain` is every value the variant carries, as it should appear.
fn says(error: &dyn std::fmt::Display, must_contain: &[&str]) {
    let said = error.to_string();
    // Low on purpose: the shortest honest message in the tree is "no frames",
    // which is eight letters and says everything there is to say. The bar is
    // against a message that is only a number or only punctuation.
    assert!(
        said.chars().filter(char::is_ascii_alphabetic).count() >= 8,
        "{said:?} is not a sentence"
    );
    assert!(!said.ends_with(' ') && !said.starts_with(' '), "{said:?} has whitespace at an end");
    for fragment in must_contain {
        assert!(said.contains(fragment), "{said:?} does not contain {fragment:?}");
    }
}

#[test]
fn art_refusals_name_the_line_the_column_and_what_to_write_instead() {
    says(&ArtError::Empty, &[]);
    // Line and column, because art is edited in a text editor and "there is a
    // dollar somewhere" is not a bug report anybody can act on.
    says(&ArtError::DanglingMarker { line: 3, column: 12 }, &["3", "12", "$$"]);
    says(&ArtError::UnknownMarker { line: 2, column: 5, got: 'z' }, &["2", "5", "z", "$1"]);
    says(&ArtError::Tab { line: 9, column: 1 }, &["9", "1", "spaces"]);
    says(&ArtError::ControlCharacter { line: 4, column: 7, value: 0x1b }, &["4", "7", "0x001b"]);
    says(&ArtError::TooLarge { columns: 2000, lines: 3000 }, &["2000", "3000", "1024"]);
}

#[test]
fn composition_refusals_name_the_overlay_and_the_step_that_does_not_fit() {
    says(&ComposeError::OverlayOffCanvas { column: 70, line: 2, width: 20 }, &["70", "2", "20"]);
    says(&ComposeError::RaggedOverlay { expected: 8, got: 5, step: 3 }, &["8", "5", "3"]);
    says(&ComposeError::EmptyOverlay, &[]);
    says(&ComposeError::PeriodsDoNotFit { periods: vec![3, 5], frames: 8 }, &["3", "5", "8"]);
    says(&ComposeError::ColourOutOfPalette { value: 200, max: 15 }, &["200", "15"]);
}

#[test]
fn recipe_refusals_say_which_key_was_wrong_and_what_it_accepts() {
    says(&RecipeError::Toml("expected a value at line 4".to_owned()), &["line 4"]);
    says(&RecipeError::BadSource { got: "carrier-pigeon:arch".to_owned() }, &["carrier-pigeon"]);
    // Both halves: "inline with no art" and "art with a source that is not
    // inline" are opposite mistakes and the same variant, and the message has
    // to say which one was made — one of them means the art is being ignored.
    says(&RecipeError::ArtMismatch { source: "inline".to_owned() }, &["inline", "empty"]);
    says(
        &RecipeError::ArtMismatch { source: "fastfetch:arch".to_owned() },
        &["fastfetch:arch", "ignored"],
    );
    says(&RecipeError::BadPalette { got: 64 }, &["64"]);
    // The known list, because a typo in a spinner name is otherwise a guessing
    // game against a set the recipe format never shows anywhere.
    says(
        &RecipeError::UnknownSpinner {
            got: "whirl".to_owned(),
            known: vec!["dots".to_owned(), "line".to_owned()],
        },
        &["whirl", "dots", "line"],
    );
    // Likewise: frames with the wrong style, and the custom style with no
    // frames. The fix is different each way round.
    says(&RecipeError::FramesMismatch { style: "dots".to_owned() }, &["dots", "custom"]);
    says(&RecipeError::FramesMismatch { style: "custom".to_owned() }, &["custom", "empty"]);
    says(
        &RecipeError::BadPosition { field: "logo.x", got: "middle".to_owned() },
        &["logo.x", "middle"],
    );
}

#[test]
fn build_refusals_carry_the_reason_from_underneath_rather_than_replacing_it() {
    // A `BuildError::Art` that says only "the art is wrong" has thrown away the
    // line number the layer below worked out.
    says(&BuildError::Art(ArtError::Tab { line: 6, column: 2 }), &["6", "2"]);
    says(&BuildError::Text(ArtError::Empty), &[]);
    says(&BuildError::Compose(ComposeError::EmptyOverlay), &[]);
    says(&BuildError::CanvasTooLarge { columns: 5000, lines: 4000 }, &["5000", "4000"]);
    says(&BuildError::EmptySpinner, &[]);
    says(&BuildError::PeriodNotAMultiple { period: 7, frames: 20 }, &["7", "20"]);
}

#[test]
fn text_refusals_name_the_size_and_the_limit() {
    says(&TextError::Empty, &[]);
    says(&TextError::TooWide { columns: 2000 }, &["2000", "1024"]);
    says(&TextError::TooTall { lines: 2000 }, &["2000", "1024"]);
}

#[test]
fn ansi_refusals_name_the_size_and_the_limit() {
    says(&AnsiError::Empty, &[]);
    says(&AnsiError::TooWide { columns: 2000 }, &["2000", "1024"]);
    says(&AnsiError::TooTall { lines: 2000 }, &["2000", "1024"]);
}

#[test]
fn durdraw_refusals_say_which_part_of_the_file_this_converter_does_not_take() {
    says(&DurError::Gzip("invalid header".to_owned()), &["invalid header"]);
    says(&DurError::Json("expected `,`".to_owned()), &["expected `,`"]);
    // The versions it *does* know, because "formatVersion 9" alone leaves the
    // reader unable to tell whether their file is too old or too new.
    says(&DurError::UnsupportedVersion(9), &["9", "7", "8"]);
    says(&DurError::UnsupportedColorFormat("xterm".to_owned()), &["xterm", "16", "256"]);
    says(&DurError::UnsupportedEncoding("cp437".to_owned()), &["cp437", "utf-8"]);
    says(&DurError::NoFrames, &[]);
    says(&DurError::EmptyCanvas, &[]);
    says(&DurError::CanvasTooLarge { columns: 2000, lines: 3000 }, &["2000", "3000", "1024"]);
    says(&DurError::TooManyFrames(99_999), &["99999"]);
    says(&DurError::FrameTooBig { frame: 4, ops: 900_000 }, &["4", "900000"]);
    says(
        &DurError::ColourMapMismatch { frame: 2, got: 30, columns: 10, lines: 4 },
        &["2", "30", "10", "4"],
    );
}

/// Every one of these is what a `?` in `main.rs` turns into, so they all have
/// to be real errors and not just printable.
#[test]
fn every_refusal_is_an_error_in_the_sense_the_language_means() {
    fn check(error: &dyn std::error::Error) {
        assert!(!error.to_string().is_empty());
    }
    check(&ArtError::Empty);
    check(&ComposeError::EmptyOverlay);
    check(&RecipeError::BadPalette { got: 64 });
    check(&BuildError::EmptySpinner);
    check(&TextError::Empty);
    check(&AnsiError::Empty);
    check(&DurError::NoFrames);
}
