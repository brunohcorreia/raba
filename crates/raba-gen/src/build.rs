//! Art source plus a recipe, to a playable movie.
//!
//! The whole authoring path in one function, so `raba-gen` the binary is argument
//! parsing and nothing else — and so the path is testable without running a process.

use crate::art::{self, Art, ArtError};
use crate::compose::{compose, spinners, ComposeError, Overlay};
use raba_core::format::Palette;
use raba_core::{Cell, Grid, Movie};
use std::fmt;

/// A spinner to put under the logo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpinnerSpec {
    pub glyphs: Vec<char>,
    pub fg: u8,
    /// Where to put it. `None` centres it two lines under the art, which is what a
    /// theme wants almost always and what the canvas is sized for.
    pub at: Option<(u16, u16)>,
}

impl SpinnerSpec {
    /// Look up a built-in by name.
    #[must_use]
    pub fn named(name: &str, fg: u8) -> Option<Self> {
        spinners::ALL.iter().find(|(known, _)| *known == name).map(|(_, glyphs)| SpinnerSpec {
            glyphs: glyphs.to_vec(),
            fg,
            at: None,
        })
    }
}

/// Everything needed to turn art into a theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub palette: Palette,
    /// Foreground for slot 1, slot 2, and so on. A slot without an entry falls back
    /// to `default_fg`.
    pub colours: Vec<u8>,
    pub default_fg: u8,
    pub bg: u8,
    pub spinner: Option<SpinnerSpec>,
    pub delay_ms: u16,
    pub looping: bool,
}

impl Default for Recipe {
    fn default() -> Self {
        Recipe {
            palette: Palette::Ansi16,
            colours: Vec::new(),
            default_fg: 7,
            bg: 0,
            spinner: SpinnerSpec::named("ascii", 7),
            delay_ms: 100,
            looping: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    Art(ArtError),
    Compose(ComposeError),
    /// The canvas the recipe implies is beyond what a `.raba` can describe.
    CanvasTooLarge {
        columns: u32,
        lines: u32,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Art(e) => write!(f, "{e}"),
            Self::Compose(e) => write!(f, "{e}"),
            Self::CanvasTooLarge { columns, lines } => {
                write!(f, "the canvas would be {columns}x{lines}, which is too large")
            }
        }
    }
}

impl std::error::Error for BuildError {}

/// What the recipe produced, with the detail a tool wants to report.
#[derive(Debug, Clone)]
pub struct Built {
    pub movie: Movie,
    /// Colour slots the art uses. A slot past the end of `Recipe::colours` was drawn
    /// in the default, which is worth telling the author about.
    pub slots_used: Vec<u8>,
}

/// Parse art, lay it out, attach the spinner, and compose.
pub fn build(source: &str, recipe: &Recipe) -> Result<Built, BuildError> {
    let art = art::parse(source).map_err(BuildError::Art)?;
    let logo = art.to_grid(recipe.palette, &recipe.colours, recipe.default_fg, recipe.bg).ok_or(
        BuildError::CanvasTooLarge {
            columns: u32::from(art.columns()),
            lines: u32::from(art.lines()),
        },
    )?;

    let (canvas, overlays) = lay_out(&art, &logo, recipe)?;
    let movie = compose(&canvas, &overlays, recipe.delay_ms, recipe.looping)
        .map_err(BuildError::Compose)?;

    Ok(Built { movie, slots_used: art.slots_used() })
}

/// Size the canvas and place the art and the spinner on it.
///
/// With an auto-placed spinner the canvas grows by two lines: one blank, so the
/// spinner is not crowded against the logo, and one for the spinner itself.
fn lay_out(art: &Art, logo: &Grid, recipe: &Recipe) -> Result<(Grid, Vec<Overlay>), BuildError> {
    let Some(spinner) = &recipe.spinner else {
        return Ok((logo.clone(), Vec::new()));
    };
    // `Overlay::spinner` draws one cell per step, whatever the glyph list is: the
    // list is how many steps there are, not how wide the spinner is.
    let width: u16 = 1;

    let (columns, lines, at) = match spinner.at {
        // An explicit position is taken literally: the author asked for it, and the
        // canvas is whatever the art already is.
        Some((column, line)) => (
            art.columns().max(column.saturating_add(width)),
            art.lines().max(line.saturating_add(1)),
            (column, line),
        ),
        None => {
            let columns = art.columns().max(width);
            let lines = art.lines().saturating_add(2);
            let column = columns.saturating_sub(width) / 2;
            (columns, lines, (column, lines.saturating_sub(1)))
        }
    };

    let mut canvas =
        Grid::new(columns, lines, recipe.palette).ok_or(BuildError::CanvasTooLarge {
            columns: u32::from(columns),
            lines: u32::from(lines),
        })?;
    canvas.fill(Cell { ch: ' ', fg: recipe.default_fg, bg: recipe.bg });
    canvas.blit(logo, 0, 0);

    let overlay = Overlay::spinner(at.0, at.1, &spinner.glyphs, spinner.fg, recipe.bg);
    Ok((canvas, vec![overlay]))
}
