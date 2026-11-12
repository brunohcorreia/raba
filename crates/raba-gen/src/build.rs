//! A recipe and some art, to a playable movie.
//!
//! The whole authoring path in one function, so `raba-gen` the binary is argument
//! parsing and nothing else, and so the path is testable without running a process.
//! The schema this serves is `raba-theme`(5).

use crate::art::{self, ArtError};
use crate::compose::{compose, ComposeError, Overlay};
use crate::layout::{Layout, Position};
use crate::palette;
use raba_core::format::Palette;
use raba_core::{Cell, Grid, Movie};
use std::fmt;

/// Built-in spinner cycles, as `raba-theme`(5) documents them.
///
/// `bar` and `arc` are **not** console-safe: the eighth blocks `▁▂▃▅▆▇` and the arc
/// characters are outside CP437, so a console draws blanks. They are here because
/// they are documented and they look right in a terminal emulator; the glyph
/// validator warns, which is the correct division of labour. `blocks` is the
/// non-ASCII cycle that does work on a console.
pub mod spinners {
    pub const ASCII: &[&str] = &["|", "/", "-", "\\"];
    pub const DOTS: &[&str] = &[".", "..", "..."];
    pub const BAR: &[&str] = &["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    pub const ARC: &[&str] = &["◜", "◝", "◞", "◟"];
    pub const BLOCKS: &[&str] = &["░", "▒", "▓", "█", "▓", "▒"];

    /// Every built-in, by the name a recipe uses.
    pub const ALL: &[(&str, &[&str])] =
        &[("ascii", ASCII), ("dots", DOTS), ("bar", BAR), ("arc", ARC), ("blocks", BLOCKS)];

    /// Look one up.
    #[must_use]
    pub fn named(name: &str) -> Option<&'static [&'static str]> {
        ALL.iter().find(|(known, _)| *known == name).map(|(_, frames)| *frames)
    }
}

/// A spinner: a cycle of frames placed relative to the art.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpinnerSpec {
    /// One entry per step. Shorter frames are padded to the widest, so `.`, `..`,
    /// `...` is a three-step spinner three cells wide rather than an error.
    pub frames: Vec<String>,
    pub fg: u8,
    pub position: Position,
    pub gap: u16,
    /// Stretch the cycle to this many movie frames, holding each step for several.
    /// `None` means one movie frame per step.
    pub period: Option<usize>,
}

impl SpinnerSpec {
    /// A built-in by name, with everything else defaulted.
    #[must_use]
    pub fn named(name: &str, fg: u8) -> Option<Self> {
        spinners::named(name).map(|frames| SpinnerSpec {
            frames: frames.iter().map(|&frame| frame.to_owned()).collect(),
            fg,
            position: Position::Below,
            gap: 1,
            period: None,
        })
    }
}

/// A static caption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpec {
    pub content: String,
    pub fg: u8,
    pub position: Position,
    pub gap: u16,
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
    pub text: Option<TextSpec>,
    /// Frames per second, written into the animation as a per-frame delay.
    pub framerate: u16,
    pub padding: u16,
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
            text: None,
            framerate: 10,
            padding: 0,
            looping: true,
        }
    }
}

impl Recipe {
    /// How long one frame is held, from the frame rate.
    #[must_use]
    pub fn delay_ms(&self) -> u16 {
        let rate = u32::from(self.framerate.max(1));
        let delay = 1000u32.checked_div(rate).unwrap_or(100);
        u16::try_from(delay.max(1)).unwrap_or(u16::MAX)
    }
}

/// A colour the recipe asked for and the console cannot give, with what was used
/// instead. Reported rather than applied silently: a theme coming out in different
/// colours than its recipe says is worth a line of output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjustment {
    pub what: String,
    pub from: u8,
    pub to: u8,
    pub reason: &'static str,
}

impl fmt::Display for Adjustment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} became {} ({})", self.what, self.from, self.to, self.reason)
    }
}

impl Recipe {
    /// Fit the recipe's colours to what the target can show, reporting every change.
    ///
    /// Two transforms, in order.
    ///
    /// **256 down to 16**, automatically rather than behind a flag, because that is
    /// what `raba-theme`(5) promises: an index above 15 in a 16-colour theme is an
    /// xterm-256 colour the author drew in, and the console's nearest equivalent is
    /// a better answer than an error. It is reported, so a typo still surfaces.
    ///
    /// **Backgrounds down to eight**, which is not optional on a console: the high
    /// bit there is blink rather than brightness, so a background of 9 is flashing
    /// red and not bright red.
    pub fn normalise(&mut self) -> Vec<Adjustment> {
        let mut changes = Vec::new();
        let mut note = |what: String, from: u8, to: u8, reason: &'static str| {
            if from != to {
                changes.push(Adjustment { what, from, to, reason });
            }
        };

        if self.palette == Palette::Ansi16 {
            const REASON: &str = "fitted to the console's 16 colours";
            for (index, slot) in self.colours.iter_mut().enumerate() {
                let fitted = palette::downsample(*slot);
                note(format!("slot ${}", index.saturating_add(1)), *slot, fitted, REASON);
                *slot = fitted;
            }
            let fitted = palette::downsample(self.default_fg);
            note("default foreground".to_owned(), self.default_fg, fitted, REASON);
            self.default_fg = fitted;

            let fitted = palette::downsample(self.bg);
            note("background".to_owned(), self.bg, fitted, REASON);
            self.bg = fitted;

            if let Some(spinner) = &mut self.spinner {
                let fitted = palette::downsample(spinner.fg);
                note("spinner colour".to_owned(), spinner.fg, fitted, REASON);
                spinner.fg = fitted;
            }
            if let Some(text) = &mut self.text {
                let fitted = palette::downsample(text.fg);
                note("text colour".to_owned(), text.fg, fitted, REASON);
                text.fg = fitted;
            }

            let clamped = palette::clamp_background(self.bg);
            note(
                "background".to_owned(),
                self.bg,
                clamped,
                "a console reads the high background bit as blink, not brightness",
            );
            self.bg = clamped;
        }

        changes
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
    /// A spinner with no frames animates nothing.
    EmptySpinner,
    /// `period` is not a multiple of the number of frames, so the steps cannot be
    /// held for equal lengths. Refused rather than rounded, because an uneven
    /// spinner reads as a stutter and looks like a bug in the player.
    PeriodNotAMultiple {
        period: usize,
        frames: usize,
    },
    /// A caption with a control character or something else art may not contain.
    Text(ArtError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Art(e) => write!(f, "{e}"),
            Self::Compose(e) => write!(f, "{e}"),
            Self::CanvasTooLarge { columns, lines } => {
                write!(f, "the canvas would be {columns}x{lines}, which is too large")
            }
            Self::EmptySpinner => write!(f, "the spinner has no frames"),
            Self::PeriodNotAMultiple { period, frames } => write!(
                f,
                "spinner.period is {period} and the style has {frames} frames; \
                 period must be a multiple so every step is held the same length"
            ),
            Self::Text(e) => write!(f, "text.content: {e}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// What the recipe produced, with the detail a tool wants to report.
#[derive(Debug, Clone)]
pub struct Built {
    pub movie: Movie,
    /// Colours the console could not give, and what was used instead.
    pub adjustments: Vec<Adjustment>,
    /// Colour slots the art uses. A slot past the end of `Recipe::colours` was drawn
    /// in the default, which is worth telling the author about.
    pub slots_used: Vec<u8>,
}

/// Parse art, lay it out, attach the spinner and caption, and compose.
pub fn build(source: &str, recipe: &Recipe) -> Result<Built, BuildError> {
    // Normalising inside `build` rather than asking callers to remember it: a path
    // that skipped it would produce a theme with a blinking background and no sign
    // of why.
    let mut recipe = recipe.clone();
    let adjustments = recipe.normalise();
    let recipe = &recipe;

    let art = art::parse(source).map_err(BuildError::Art)?;
    let logo = art.to_grid(recipe.palette, &recipe.colours, recipe.default_fg, recipe.bg).ok_or(
        BuildError::CanvasTooLarge {
            columns: u32::from(art.columns()),
            lines: u32::from(art.lines()),
        },
    )?;

    // The caption goes through the art parser: it is art, and it has the same reasons
    // to reject a tab or an escape sequence.
    let caption = match &recipe.text {
        Some(text) => Some(art::parse(&text.content).map_err(BuildError::Text)?),
        None => None,
    };

    let frames = match &recipe.spinner {
        Some(spinner) => Some(spinner_frames(spinner)?),
        None => None,
    };

    let mut layout = Layout::of(logo.columns(), logo.lines());
    let mut index = 0usize;
    let spinner_index = frames.as_ref().map(|frames| {
        index = index.saturating_add(1);
        let width = u16::try_from(frames.width).unwrap_or(u16::MAX);
        if let Some(spec) = &recipe.spinner {
            layout.attach(width, 1, spec.position, spec.gap);
        }
        index
    });
    let caption_index = caption.as_ref().map(|caption| {
        index = index.saturating_add(1);
        if let Some(spec) = &recipe.text {
            layout.attach(caption.columns(), caption.lines(), spec.position, spec.gap);
        }
        index
    });
    layout.pad(recipe.padding);

    let mut canvas = Grid::new(layout.columns, layout.lines, recipe.palette).ok_or(
        BuildError::CanvasTooLarge {
            columns: u32::from(layout.columns),
            lines: u32::from(layout.lines),
        },
    )?;
    canvas.fill(Cell { ch: ' ', fg: recipe.default_fg, bg: recipe.bg });

    let (logo_column, logo_line) = layout.placement(0).unwrap_or((0, 0));
    canvas.blit(&logo, logo_column, logo_line);

    // The caption is static, so it is painted into the canvas rather than made an
    // overlay: a cell that never changes costs nothing in frame 0 and nothing after.
    if let (Some(caption), Some(spec), Some(slot)) = (&caption, &recipe.text, caption_index) {
        if let Some(grid) = caption.to_grid(recipe.palette, &[], spec.fg, recipe.bg) {
            let (column, line) = layout.placement(slot).unwrap_or((0, 0));
            canvas.blit(&grid, column, line);
        }
    }

    let mut overlays = Vec::new();
    if let (Some(frames), Some(spec), Some(slot)) = (&frames, &recipe.spinner, spinner_index) {
        let (column, line) = layout.placement(slot).unwrap_or((0, 0));
        overlays.push(Overlay {
            column,
            line,
            steps: frames
                .steps
                .iter()
                .map(|step| {
                    step.iter().map(|&ch| Cell { ch, fg: spec.fg, bg: recipe.bg }).collect()
                })
                .collect(),
        });
    }

    let movie = compose(&canvas, &overlays, recipe.delay_ms(), recipe.looping)
        .map_err(BuildError::Compose)?;

    Ok(Built { movie, adjustments, slots_used: art.slots_used() })
}

/// A spinner's steps, padded to a common width and stretched to its period.
struct SpinnerFrames {
    steps: Vec<Vec<char>>,
    width: usize,
}

fn spinner_frames(spec: &SpinnerSpec) -> Result<SpinnerFrames, BuildError> {
    if spec.frames.is_empty() {
        return Err(BuildError::EmptySpinner);
    }

    // Padding rather than refusing: `.`, `..`, `...` is what an author means by a
    // three-step spinner, and making them write the trailing spaces would be making
    // them do the obvious thing by hand.
    let width = spec.frames.iter().map(|frame| frame.chars().count()).max().unwrap_or(0);
    let mut steps: Vec<Vec<char>> = spec
        .frames
        .iter()
        .map(|frame| {
            let mut cells: Vec<char> = frame.chars().collect();
            cells.resize(width, ' ');
            cells
        })
        .collect();

    if let Some(period) = spec.period {
        let count = steps.len();
        if period == 0 || period.checked_rem(count).unwrap_or(1) != 0 {
            return Err(BuildError::PeriodNotAMultiple { period, frames: count });
        }
        let hold = period.checked_div(count).unwrap_or(1);
        if hold > 1 {
            steps = steps.into_iter().flat_map(|step| std::iter::repeat_n(step, hold)).collect();
        }
    }

    Ok(SpinnerFrames { steps, width })
}
