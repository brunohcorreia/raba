//! The protocol, with no sockets in it.
//!
//! Parsing a request and rendering a reply are the two places a protocol goes
//! wrong, and neither of them needs a file descriptor. Keeping them here means
//! every malformed request anybody can think of is one `cargo test` away, rather
//! than needing a running daemon and a client to reproduce.
//!
//! `docs/protocol.md` is the specification. This file is supposed to be boring
//! next to it; where they disagree, the document is right and this is a bug.

use std::fmt;

/// The protocol number reported by `status`.
pub const VERSION: u32 = 1;

/// The most a request may be, newline included.
///
/// A bound on memory rather than a courtesy: the server reads at most this much
/// and then stops, so a client that never sends a newline cannot make it grow.
pub const MAX_REQUEST: usize = 1024;

/// What a client asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Ping,
    Status,
    Quit {
        /// Hold the connection open until the process is gone, so the client can
        /// wait on EOF instead of polling for a pid that may be reused.
        wait: bool,
    },
    Deactivate,
    Reactivate,
    /// Text to show. Still raw at this point: it is sanitised where it is drawn,
    /// not where it is parsed, so there is no way to hold a `Command` containing
    /// text that has been checked and a `Command` containing text that has not.
    Message(String),
    Progress {
        fraction: f32,
        /// `--advance`: add to the current value rather than replacing it.
        advance: bool,
    },
}

/// Why a request was refused. The wording is the wire format — see the error
/// table in `docs/protocol.md` — so these strings are API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Unknown,
    TooLong,
    NotUtf8,
    MissingArgument,
    BadArgument,
    /// `message` or `progress` while not drawing.
    NotDrawing,
    /// `progress` without `Progress.Source=ctl`.
    NotConfigured,
}

impl Rejection {
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Rejection::Unknown => "unknown command",
            Rejection::TooLong => "too long",
            Rejection::NotUtf8 => "not utf-8",
            Rejection::MissingArgument => "missing argument",
            Rejection::BadArgument => "bad argument",
            Rejection::NotDrawing => "not drawing",
            Rejection::NotConfigured => "not configured",
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl Command {
    /// Parse one request line. The terminating newline is not included.
    ///
    /// # Errors
    ///
    /// Every malformed request is a [`Rejection`] and never a panic: this is the
    /// first thing in the process to touch bytes from outside it.
    pub fn parse(line: &str) -> Result<Command, Rejection> {
        // A stray carriage return costs somebody half an hour with `socat` and
        // buys no safety, so it is tolerated. Nothing else is.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "ping" => Ok(Command::Ping),
            "status" => Ok(Command::Status),
            "deactivate" => Ok(Command::Deactivate),
            "reactivate" => Ok(Command::Reactivate),
            "quit" => match rest.trim() {
                "" => Ok(Command::Quit { wait: false }),
                "--wait" => Ok(Command::Quit { wait: true }),
                _ => Err(Rejection::BadArgument),
            },
            // The rest of the line, verbatim. No quoting rules means no quoting
            // rules to get wrong, and a newline cannot appear because a newline
            // is what ended the request.
            "message" => {
                if rest.is_empty() {
                    Err(Rejection::MissingArgument)
                } else {
                    Ok(Command::Message(rest.to_owned()))
                }
            }
            "progress" => parse_progress(rest),
            "" => Err(Rejection::Unknown),
            _ => Err(Rejection::Unknown),
        }
    }

    /// The command word, for logs and for `status`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Command::Ping => "ping",
            Command::Status => "status",
            Command::Quit { .. } => "quit",
            Command::Deactivate => "deactivate",
            Command::Reactivate => "reactivate",
            Command::Message(_) => "message",
            Command::Progress { .. } => "progress",
        }
    }
}

fn parse_progress(rest: &str) -> Result<Command, Rejection> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(Rejection::MissingArgument);
    }
    let (advance, value) = match rest.strip_prefix("--advance") {
        Some(tail) => (true, tail.trim()),
        None => (false, rest),
    };
    if value.is_empty() {
        return Err(Rejection::MissingArgument);
    }
    let fraction: f32 = value.parse().map_err(|_| Rejection::BadArgument)?;
    // `is_finite` rather than a range check alone: `"nan".parse()` succeeds, and
    // NaN compares false against every bound, so a range check on its own lets
    // it through.
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(Rejection::BadArgument);
    }
    Ok(Command::Progress { fraction, advance })
}

/// What the server sends back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Ok,
    OkWith(String),
    /// `status`: `+OK` and then `key: value` lines, in the order given.
    Report(Vec<(&'static str, String)>),
    Failed(Rejection),
}

impl Reply {
    /// The bytes to send, newlines included.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Reply::Ok => "+OK\n".to_owned(),
            Reply::OkWith(text) => format!("+OK {}\n", one_line(text)),
            Reply::Failed(why) => format!("-ERR {}\n", why.reason()),
            Reply::Report(fields) => {
                let mut out = String::from("+OK\n");
                for (key, value) in fields {
                    out.push_str(key);
                    out.push_str(": ");
                    out.push_str(&one_line(value));
                    out.push('\n');
                }
                out
            }
        }
    }
}

/// Flatten anything that would end a reply line early.
///
/// Every value in a reply today is something this program produced — a phase
/// name, a path, a number — so this changes nothing in practice. It is here
/// because the day one of them is a theme name taken from a file, a newline in
/// that name would let the file forge reply lines, and `status` is parsed by a
/// client that would believe them. Cheaper to make it impossible than to
/// remember which values are safe.
fn one_line(text: &str) -> String {
    text.chars().map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch }).collect()
}

#[cfg(test)]
mod tests;
