//! Theme recipes: the TOML file `raba-theme`(5) describes.
//!
//! A recipe, not the generated `.raba`, is what belongs in version control and what
//! you send in a pull request. It lives next to the art and is reviewed in the same
//! diff, so why a logo is the colour it is has an answer in the repository rather
//! than in somebody's shell history.
//!
//! Unknown keys are refused. A recipe that quietly ignored `colour =` because the
//! field is spelled `color` would produce a theme in the wrong colours and no
//! complaint, which is the worst outcome available.

use crate::build::{Recipe as BuildRecipe, SpinnerSpec, TextSpec};
use crate::layout::Position;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// A theme recipe, as written in TOML.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub logo: Logo,
    /// Omit the section for a static theme.
    pub spinner: Option<Spinner>,
    /// Optional caption.
    pub text: Option<Text>,
    #[serde(default)]
    pub canvas: Canvas,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    /// `file:`_path_, `inline`, or `fastfetch:`_name_.
    #[serde(default = "default_source")]
    pub source: String,
    /// The art itself, for `source = "inline"`.
    #[serde(default)]
    pub art: String,
    /// Palette indices the `$1`..`$9` markers resolve to.
    #[serde(default, alias = "colours")]
    pub colors: Vec<u8>,
    #[serde(default)]
    pub background: u8,
    /// Colour for art outside any marker, and for a logo's padding. Rarely needed:
    /// padding is blank, and most art starts with `$1`.
    #[serde(default = "default_foreground", alias = "default_colour")]
    pub default_color: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spinner {
    /// `ascii`, `dots`, `bar`, `arc`, `blocks` or `custom`.
    #[serde(default = "default_spinner_style")]
    pub style: String,
    /// Frames for `style = "custom"`. Shorter ones are padded to the widest.
    #[serde(default)]
    pub frames: Vec<String>,
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "default_gap")]
    pub gap: u16,
    #[serde(default = "default_foreground", alias = "colour")]
    pub color: u8,
    /// Frames per full cycle. Defaults to the frame count of the chosen style.
    pub period: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub content: String,
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "default_gap")]
    pub gap: u16,
    #[serde(default = "default_foreground", alias = "colour")]
    pub color: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    #[serde(default = "default_framerate")]
    pub framerate: u16,
    /// 16 or 256. Colours above a 16-colour palette's range are fitted to it at
    /// build time rather than refused; `raba-gen` reports what it changed.
    #[serde(default = "default_palette")]
    pub palette: u16,
    /// Blank cells around the bounding box.
    #[serde(default)]
    pub padding: u16,
    /// Whether the animation repeats. An outro sets this to `false`.
    #[serde(default = "default_repeat")]
    pub repeat: bool,
    /// How `raba`(8) places the canvas on screen. **Not yet honoured** — the format
    /// has no field for it, since the console's size is a run-time fact. Accepted
    /// and reported so recipes written against `raba-theme`(5) still load.
    #[serde(default = "default_align")]
    pub align: String,
    #[serde(default)]
    pub x_offset: i32,
    #[serde(default)]
    pub y_offset: i32,
}

fn default_source() -> String {
    "inline".to_owned()
}
fn default_foreground() -> u8 {
    7
}
fn default_spinner_style() -> String {
    "ascii".to_owned()
}
fn default_position() -> String {
    "below".to_owned()
}
fn default_gap() -> u16 {
    1
}
fn default_framerate() -> u16 {
    10
}
fn default_palette() -> u16 {
    16
}
fn default_repeat() -> bool {
    true
}
fn default_align() -> String {
    "center".to_owned()
}

impl Default for Logo {
    fn default() -> Self {
        Logo {
            source: default_source(),
            art: String::new(),
            colors: Vec::new(),
            background: 0,
            default_color: default_foreground(),
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas {
            framerate: default_framerate(),
            palette: default_palette(),
            padding: 0,
            repeat: default_repeat(),
            align: default_align(),
            x_offset: 0,
            y_offset: 0,
        }
    }
}

/// Where a recipe's art comes from. Resolved by the caller, so this module does no
/// I/O and stays testable without a filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtSource {
    /// A path, already resolved relative to the recipe.
    File(PathBuf),
    /// The `art` key.
    Inline(String),
    /// A bundled fastfetch logo, by name.
    Fastfetch(String),
}

/// What can be wrong with a recipe, beyond TOML not parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeError {
    Toml(String),
    /// `logo.source` is not one of the three forms.
    BadSource {
        got: String,
    },
    /// `source = "inline"` with no `art`, or `art` set alongside a `file:` source.
    ArtMismatch {
        source: String,
    },
    /// `canvas.palette` is neither 16 nor 256.
    BadPalette {
        got: u16,
    },
    /// A spinner style that does not exist.
    UnknownSpinner {
        got: String,
        known: Vec<String>,
    },
    /// `style = "custom"` with no `frames`, or `frames` set for a built-in style —
    /// either way the recipe says two things that contradict each other.
    FramesMismatch {
        style: String,
    },
    /// A `position` that is not one of the five.
    BadPosition {
        field: &'static str,
        got: String,
    },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(message) => write!(f, "{message}"),
            Self::BadSource { got } => {
                write!(f, "logo.source is {got:?}; expected file:PATH, inline, or fastfetch:NAME")
            }
            Self::ArtMismatch { source } => {
                if source == "inline" {
                    write!(f, "logo.source is inline but logo.art is empty")
                } else {
                    write!(
                        f,
                        "logo.art is set but logo.source is {source:?}; one of them is ignored"
                    )
                }
            }
            Self::BadPalette { got } => {
                write!(f, "canvas.palette is {got}; it must be 16 or 256")
            }
            Self::UnknownSpinner { got, known } => {
                write!(f, "spinner.style is {got:?}; try one of {}, or custom", known.join(", "))
            }
            Self::FramesMismatch { style } => {
                if style == "custom" {
                    write!(f, "spinner.style is \"custom\" but spinner.frames is empty")
                } else {
                    write!(
                        f,
                        "spinner.frames is set but spinner.style is {style:?}; \
                         use style = \"custom\" to choose your own"
                    )
                }
            }
            Self::BadPosition { field, got } => {
                write!(f, "{field} is {got:?}; expected one of {}", Position::NAMES.join(", "))
            }
        }
    }
}

impl std::error::Error for RecipeError {}

/// Parse a recipe and check what TOML cannot.
pub fn parse(source: &str) -> Result<Theme, RecipeError> {
    let theme: Theme = toml::from_str(source).map_err(|e| RecipeError::Toml(e.to_string()))?;
    theme.validate()?;
    Ok(theme)
}

impl Theme {
    /// Where the art comes from, given where the recipe is.
    pub fn art_source(&self, recipe_path: &Path) -> Result<ArtSource, RecipeError> {
        let source = self.logo.source.as_str();
        if source == "inline" {
            return Ok(ArtSource::Inline(self.logo.art.clone()));
        }
        if let Some(path) = source.strip_prefix("file:") {
            let path = Path::new(path);
            let resolved = match recipe_path.parent() {
                Some(directory) if path.is_relative() => directory.join(path),
                _ => path.to_path_buf(),
            };
            return Ok(ArtSource::File(resolved));
        }
        if let Some(name) = source.strip_prefix("fastfetch:") {
            return Ok(ArtSource::Fastfetch(name.to_owned()));
        }
        Err(RecipeError::BadSource { got: source.to_owned() })
    }

    /// Keys this build accepts and does not act on, so a tool can say so out loud
    /// rather than leaving an author to wonder why nothing moved.
    #[must_use]
    pub fn unhonoured(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.canvas.align != default_align() {
            notes.push(format!(
                "canvas.align = {:?} is not honoured yet: where a theme sits depends on \
                 the console's size, which is a run-time fact the format has no field for",
                self.canvas.align
            ));
        }
        if self.canvas.x_offset != 0 || self.canvas.y_offset != 0 {
            notes.push(
                "canvas.x_offset and canvas.y_offset are not honoured yet, for the same \
                 reason as canvas.align; use canvas.padding to add space inside the theme"
                    .to_owned(),
            );
        }
        notes
    }

    fn validate(&self) -> Result<(), RecipeError> {
        let source = self.logo.source.as_str();
        let is_inline = source == "inline";
        if is_inline && self.logo.art.trim().is_empty() {
            return Err(RecipeError::ArtMismatch { source: source.to_owned() });
        }
        if !is_inline && !self.logo.art.is_empty() {
            return Err(RecipeError::ArtMismatch { source: source.to_owned() });
        }
        if !is_inline && !source.starts_with("file:") && !source.starts_with("fastfetch:") {
            return Err(RecipeError::BadSource { got: source.to_owned() });
        }

        if self.canvas.palette != 16 && self.canvas.palette != 256 {
            return Err(RecipeError::BadPalette { got: self.canvas.palette });
        }

        if let Some(spinner) = &self.spinner {
            let known: Vec<String> =
                crate::build::spinners::ALL.iter().map(|(n, _)| (*n).to_owned()).collect();
            let style = spinner.style.as_str();
            let is_builtin = known.iter().any(|name| name == style);
            match style {
                "custom" if spinner.frames.is_empty() => {
                    return Err(RecipeError::FramesMismatch { style: style.to_owned() })
                }
                "custom" => {}
                _ if is_builtin && !spinner.frames.is_empty() => {
                    return Err(RecipeError::FramesMismatch { style: style.to_owned() })
                }
                _ if is_builtin => {}
                other => return Err(RecipeError::UnknownSpinner { got: other.to_owned(), known }),
            }
            position(&spinner.position, "spinner.position")?;
        }

        if let Some(text) = &self.text {
            position(&text.position, "text.position")?;
        }
        Ok(())
    }
}

fn position(name: &str, field: &'static str) -> Result<Position, RecipeError> {
    Position::parse(name).ok_or_else(|| RecipeError::BadPosition { field, got: name.to_owned() })
}

impl From<&Theme> for BuildRecipe {
    fn from(theme: &Theme) -> Self {
        let spinner = theme.spinner.as_ref().map(|spinner| {
            let frames = if spinner.style == "custom" {
                spinner.frames.clone()
            } else {
                crate::build::spinners::named(&spinner.style)
                    .unwrap_or(crate::build::spinners::ASCII)
                    .iter()
                    .map(|&frame| frame.to_owned())
                    .collect()
            };
            SpinnerSpec {
                frames,
                fg: spinner.color,
                position: Position::parse(&spinner.position).unwrap_or_default(),
                gap: spinner.gap,
                period: spinner.period,
            }
        });

        let text = theme.text.as_ref().map(|text| TextSpec {
            content: text.content.clone(),
            fg: text.color,
            position: Position::parse(&text.position).unwrap_or_default(),
            gap: text.gap,
        });

        BuildRecipe {
            palette: if theme.canvas.palette == 256 {
                raba_core::format::Palette::Xterm256
            } else {
                raba_core::format::Palette::Ansi16
            },
            colours: theme.logo.colors.clone(),
            default_fg: theme.logo.default_color,
            bg: theme.logo.background,
            spinner,
            text,
            framerate: theme.canvas.framerate,
            padding: theme.canvas.padding,
            looping: theme.canvas.repeat,
        }
    }
}
