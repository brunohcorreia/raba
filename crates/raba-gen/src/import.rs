//! What every importer returns.
//!
//! `.dur`, `.ans` and plain text arrive differently and all land here, so the
//! command that drives them does not have to care which it got. `notes` is the
//! part that matters: every importer has to decide something the file does not
//! say outright, and a decision made silently is one nobody can check.

use raba_core::Movie;

/// A converted file, with what the converter had to decide along the way.
#[derive(Debug)]
pub struct Imported {
    pub movie: Movie,
    pub name: String,
    pub artist: String,
    /// Anything a person should know: a guessed orientation, a clamped colour.
    pub notes: Vec<String>,
}

impl Imported {
    /// A bare import with no metadata.
    #[must_use]
    pub fn new(movie: Movie) -> Self {
        Imported { movie, name: String::new(), artist: String::new(), notes: Vec::new() }
    }
}
