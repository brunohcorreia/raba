//! Tier 2: the real binary, writing to a real pseudo-terminal.
//!
//! This is the layer between the unit tests and the virtual machine. It cannot
//! reach `VT_OPENQRY` or `KD_SETMODE` — those need a real kernel console — but it
//! covers the part people assume needs a VM and does not: that `raba` asks the
//! kernel how big its terminal is and gets a real answer, that every byte it emits
//! survives the tty layer unmangled, and that what arrives turns into the picture
//! that was supposed to be drawn.
//!
//! The oracle is `raba_core::testing::Terminal`, the same strict model the
//! renderer's own tests use: anything that is not CUP, SGR or a printable
//! character is a panic rather than a shrug. So this asserts on the binary's whole
//! output alphabet, not only on the parts a test thought to look at.
//!
//! Linux only. A pty is not portable and neither is the thing being built.

#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use raba_core::render::{best_fit, resamples, to_ansi};
use raba_core::surface::Size;
use raba_core::testing::Terminal;
use raba_core::{apply, Grid, Movie};
use rustix::termios::Winsize;
use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::process::{Command, Stdio};

/// A pseudo-terminal of a stated size.
struct Pty {
    controller: OwnedFd,
    user: OwnedFd,
}

impl Pty {
    fn open(size: Size) -> Pty {
        let controller = rustix::pty::openpt(rustix::pty::OpenptFlags::NOCTTY).expect("openpt");
        rustix::pty::grantpt(&controller).expect("grantpt");
        rustix::pty::unlockpt(&controller).expect("unlockpt");
        let name = rustix::pty::ptsname(&controller, Vec::new()).expect("ptsname");
        let user = rustix::fs::open(
            name.to_str().expect("a path"),
            rustix::fs::OFlags::RDWR | rustix::fs::OFlags::NOCTTY,
            rustix::fs::Mode::empty(),
        )
        .expect("open the pty");

        // The size is the whole point: `raba` has to read this back through
        // TIOCGWINSZ rather than falling back to 80x25.
        let winsize =
            Winsize { ws_row: size.lines, ws_col: size.columns, ws_xpixel: 0, ws_ypixel: 0 };
        rustix::termios::tcsetwinsize(&user, winsize).expect("set the window size");
        Pty { controller, user }
    }

    /// Run `raba` with this pty as its stdout and collect everything it wrote.
    fn run(self, args: &[&str]) -> Vec<u8> {
        let Pty { controller, user } = self;
        let mut child = Command::new(env!("CARGO_BIN_EXE_raba"))
            .args(args)
            .stdout(Stdio::from(user))
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .expect("run raba");

        let status = child.wait().expect("wait");
        assert!(status.success(), "raba exited with {status}");

        // The child's stdout was the only other handle on the user side, so now
        // that it has exited the controller reads EIO rather than blocking. EIO is
        // how a pty reports the far end closing: the normal end of this read.
        let mut file = std::fs::File::from(controller);
        let mut out = Vec::new();
        let _ = file.read_to_end(&mut out);
        out
    }
}

/// Write a `.raba` file to a temporary directory and give back its path.
fn fixture(name: &str, movie: &Movie) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("raba-pty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join(format!("{name}.raba"));
    std::fs::write(&path, raba_core::encode(movie)).expect("write the fixture");
    path
}

/// A canvas filled with a recognisable, colourful pattern.
fn pattern(columns: u16, lines: u16) -> Movie {
    let mut grid = Grid::new(columns, lines, raba_core::Palette::Ansi16).unwrap();
    for line in 0..lines {
        for column in 0..columns {
            let n = usize::from(line).wrapping_mul(7).wrapping_add(usize::from(column));
            let ch = ['#', '.', '+', 'o', 'X', '=', '-', '*'][n % 8];
            let cell = raba_core::Cell {
                ch,
                fg: u8::try_from(n % 16).unwrap(),
                bg: u8::try_from(n % 4).unwrap(),
            };
            grid.set(column, line, cell);
        }
    }
    movie_from(&[grid])
}

fn movie_from(grids: &[Grid]) -> Movie {
    let first = &grids[0];
    let mut ops = Vec::new();
    let mut frames = Vec::new();
    let mut previous: Option<&Grid> = None;
    for grid in grids {
        let start = ops.len();
        for (index, cell) in grid.cells().iter().enumerate() {
            if previous.is_some_and(|p| p.cells()[index] == *cell) {
                continue;
            }
            ops.push(raba_core::Op {
                cell: u32::try_from(index).unwrap(),
                ch: cell.ch,
                fg: cell.fg,
                bg: cell.bg,
            });
        }
        frames.push(raba_core::Frame {
            delay_ms: 10,
            op_start: u32::try_from(start).unwrap(),
            op_count: u16::try_from(ops.len() - start).unwrap(),
        });
        previous = Some(grid);
    }
    Movie {
        columns: first.columns(),
        lines: first.lines(),
        palette: first.palette(),
        scaling: raba_core::Scaling::Crop,
        looping: false,
        frames,
        ops,
    }
}

/// What the movie should look like once it has been played onto a screen.
fn expected(movie: &Movie, screen: Size) -> Grid {
    let mut canvas = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    for frame in &movie.frames {
        apply(&mut canvas, movie, frame);
    }
    best_fit(&canvas, screen, resamples(movie))
}

/// What `StdoutSurface::restore` writes: reset the attributes, then one newline so
/// whatever prints next does not land on the last line of the art.
const RESTORE_WRITTEN: &[u8] = b"\x1b[0m\n";

/// What a terminal on the other side of a tty actually receives.
///
/// Not the same bytes. The line discipline's `ONLCR` turns an outbound `\n` into
/// `\r\n`, which is the difference between a pipe and a terminal and the reason
/// the renderer emits no line feeds at all: one mid-frame would silently carry a
/// carriage return with it and move the cursor to column 0.
const RESTORE_RECEIVED: &[u8] = b"\x1b[0m\r\n";

/// Everything the program wrote while it was animating.
///
/// The restore is split off rather than fed to the oracle. It is the only newline
/// in the output and the only part allowed to scroll, so letting it through would
/// hide exactly what the no-scroll test is looking for.
fn playback(out: &[u8]) -> &[u8] {
    let (body, tail) = out.split_at(out.len().saturating_sub(RESTORE_RECEIVED.len()));
    assert_eq!(tail, RESTORE_RECEIVED, "the terminal was not put back the way it should be");
    body
}

/// Replay bytes onto a blank screen and give back what a terminal would show.
fn replay(bytes: &[u8], screen: Size, palette: raba_core::Palette) -> Grid {
    let mut terminal = Terminal::blank(screen.columns, screen.lines, palette);
    terminal.apply(bytes);
    terminal.into_screen()
}

#[test]
fn the_binary_reads_the_real_terminal_size_and_fills_it() {
    // 100x30 is neither the 80x25 fallback nor the canvas size, so output of that
    // shape can only have come from asking the kernel.
    let screen = Size { columns: 100, lines: 30 };
    let movie = pattern(40, 12);
    let path = fixture("size", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);
    let drawn = replay(playback(&out), screen, movie.palette);

    assert_eq!((drawn.columns(), drawn.lines()), (100, 30));
    assert_eq!(drawn, expected(&movie, screen), "the picture that arrived is not the one drawn");
}

#[test]
fn a_canvas_larger_than_the_terminal_does_not_scroll_it() {
    // The one thing a splash must never do is leave the console scrolled. On a
    // real tty that is not a theory: write past the last line and it happens.
    let screen = Size { columns: 20, lines: 8 };
    let movie = pattern(60, 20);
    let path = fixture("crop", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);
    let drawn = replay(playback(&out), screen, movie.palette);

    assert_eq!((drawn.columns(), drawn.lines()), (20, 8));
    assert_eq!(drawn, expected(&movie, screen));
    // A line feed on a full screen scrolls it, so there must not be one anywhere
    // in the animation. The single one in the restore is deliberate and is checked
    // by `playback` instead.
    assert!(!playback(&out).contains(&b'\n'), "a line feed during playback");
}

#[test]
fn the_tty_rewrites_line_feeds_which_is_why_the_renderer_emits_none() {
    // This is the whole reason tier 2 exists rather than stopping at the model.
    // A pipe delivers what you wrote; a tty delivers what the line discipline made
    // of it. The one newline the program writes comes back with a carriage return
    // attached, and a renderer that used newlines to move down a line would get
    // the same treatment and end up in column 0 every time.
    let screen = Size { columns: 16, lines: 4 };
    let movie = pattern(16, 2);
    let path = fixture("onlcr", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);

    assert!(out.ends_with(RESTORE_RECEIVED), "ONLCR should have added a carriage return");
    assert!(
        !out.ends_with(RESTORE_WRITTEN),
        "if this ever passes, the tty stopped translating and these tests prove less"
    );
    // And the only line feed in the whole stream is that one.
    assert_eq!(out.iter().filter(|byte| **byte == b'\n').count(), 1);
}

#[test]
fn every_byte_of_a_frame_survives_the_tty_layer() {
    // A pty is not a pipe. The line discipline rewrites output — ONLCR turns a
    // line feed into CR LF — so "what the encoder produced" and "what a terminal
    // received" are the same thing only if the encoder never relies on either.
    let screen = Size { columns: 30, lines: 10 };
    let movie = pattern(30, 10);
    let path = fixture("bytes", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);

    let mut canvas = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    apply(&mut canvas, &movie, &movie.frames[0]);
    let direct = to_ansi(&canvas, None);

    // Not byte-for-byte equal overall — playback clears the screen first and parks
    // the cursor after — but the frame itself has to be in there intact and in order.
    assert!(
        out.windows(direct.len()).any(|window| window == direct.as_slice()),
        "the frame did not arrive intact through the tty"
    );
}

#[test]
fn several_frames_arrive_as_several_frames() {
    let screen = Size { columns: 24, lines: 6 };
    let mut grids = Vec::new();
    for step in 0..4u16 {
        let mut grid = Grid::new(24, 6, raba_core::Palette::Ansi16).unwrap();
        grid.set(step, 2, raba_core::Cell { ch: '*', fg: 11, bg: 0 });
        grids.push(grid);
    }
    let movie = movie_from(&grids);
    let path = fixture("frames", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);
    let drawn = replay(playback(&out), screen, movie.palette);

    // The last frame is what is left on screen, and the marker has moved to it.
    assert_eq!(drawn.get(3, 2).unwrap().ch, '*');
    assert_eq!(drawn.get(3, 2).unwrap().fg, 11);
    assert_eq!(drawn.get(0, 2).unwrap().ch, ' ', "the first position was cleared");
}

#[test]
fn the_cursor_is_left_somewhere_a_prompt_can_follow() {
    // Whatever runs next writes where the cursor is. Leaving it inside the art
    // means the next thing printed lands on top of the picture.
    let screen = Size { columns: 20, lines: 6 };
    let movie = pattern(20, 3);
    let path = fixture("cursor", &movie);

    let out = Pty::open(screen).run(&["--preview", path.to_str().unwrap(), "--loops", "1"]);
    let mut terminal = Terminal::blank(screen.columns, screen.lines, movie.palette);
    terminal.apply(playback(&out));
    let (column, line) = terminal.cursor();

    assert_eq!(column, 0, "the cursor should be at the left margin");
    assert_eq!(line, screen.lines - 1, "the cursor should be on the last line");
    // Which is to say: on the screen. An earlier version parked one line past the
    // bottom on every frame and nothing noticed, because terminals clamp.
    assert!(line < screen.lines);
}
