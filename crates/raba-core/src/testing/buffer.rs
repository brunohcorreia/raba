//! A surface that keeps frames instead of showing them.
//!
//! Behind the `testing` feature so it cannot reach the boot binary. The binary size
//! gate would notice, but not depending on the gate to notice is cheaper than
//! explaining why a boot program contains a test double.

use crate::surface::{Size, Surface};
use std::io;

/// Records what was presented, so a test can assert on it.
///
/// The frame count matters as much as the content: because each [`present`] is one
/// frame, `frames().len()` is the number of writes, and a renderer that started
/// emitting incrementally would show up here as a frame count that no longer matches
/// the animation.
///
/// [`present`]: Surface::present
#[derive(Debug, Clone)]
pub struct BufferSurface {
    size: Size,
    frames: Vec<Vec<u8>>,
    restores: usize,
    fail_next: bool,
}

impl BufferSurface {
    #[must_use]
    pub fn new(columns: u16, lines: u16) -> Self {
        BufferSurface {
            size: Size { columns, lines },
            frames: Vec::new(),
            restores: 0,
            fail_next: false,
        }
    }

    /// Every frame presented, in order. One entry per [`Surface::present`] call.
    #[must_use]
    pub fn frames(&self) -> &[Vec<u8>] {
        &self.frames
    }

    /// Everything presented so far, concatenated — what a terminal would have seen.
    #[must_use]
    pub fn transcript(&self) -> Vec<u8> {
        self.frames.concat()
    }

    /// How many times [`Surface::restore`] was called. Restoring twice is allowed and
    /// expected: the normal exit path and the signal handler both do it.
    #[must_use]
    pub fn restores(&self) -> usize {
        self.restores
    }

    /// Make the next `present` fail, for testing that playback survives a write
    /// error rather than aborting a boot over one dropped frame.
    pub fn fail_next_present(&mut self) {
        self.fail_next = true;
    }

    /// Change the reported size, standing in for a console resize between frames.
    pub fn resize(&mut self, columns: u16, lines: u16) {
        self.size = Size { columns, lines };
    }
}

impl Surface for BufferSurface {
    fn size(&self) -> Size {
        self.size
    }

    fn present(&mut self, frame: &[u8]) -> io::Result<()> {
        if self.fail_next {
            self.fail_next = false;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected failure"));
        }
        self.frames.push(frame.to_vec());
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        self.restores = self.restores.saturating_add(1);
        Ok(())
    }
}
