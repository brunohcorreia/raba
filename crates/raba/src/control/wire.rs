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
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_simple_commands_parse() {
        assert_eq!(Command::parse("ping"), Ok(Command::Ping));
        assert_eq!(Command::parse("status"), Ok(Command::Status));
        assert_eq!(Command::parse("deactivate"), Ok(Command::Deactivate));
        assert_eq!(Command::parse("reactivate"), Ok(Command::Reactivate));
        assert_eq!(Command::parse("quit"), Ok(Command::Quit { wait: false }));
        assert_eq!(Command::parse("quit --wait"), Ok(Command::Quit { wait: true }));
    }

    #[test]
    fn an_unknown_command_is_refused_rather_than_guessed_at() {
        for line in ["", "qui", "quitt", "QUIT", "exec", "message"] {
            assert!(Command::parse(line).is_err(), "{line:?} parsed");
        }
    }

    /// The whole reason `message` takes the rest of the line rather than an
    /// argument list: no quoting, so no quoting to get wrong.
    #[test]
    fn a_message_keeps_its_spaces_and_quotes_verbatim() {
        let text = "  Checking \"filesystems\"   on /dev/sda1  ";
        assert_eq!(
            Command::parse(&format!("message {text}")),
            Ok(Command::Message(text.to_owned()))
        );
    }

    #[test]
    fn a_message_with_no_text_is_a_missing_argument() {
        assert_eq!(Command::parse("message"), Err(Rejection::MissingArgument));
        assert_eq!(Command::parse("message "), Err(Rejection::MissingArgument));
    }

    #[test]
    fn progress_takes_a_fraction_either_way_round() {
        assert_eq!(
            Command::parse("progress 0.4"),
            Ok(Command::Progress { fraction: 0.4, advance: false })
        );
        assert_eq!(
            Command::parse("progress --advance 0.1"),
            Ok(Command::Progress { fraction: 0.1, advance: true })
        );
    }

    /// `"nan".parse::<f32>()` succeeds, and NaN compares false against every
    /// bound — so a range check alone would let it through to the renderer.
    #[test]
    fn progress_rejects_the_values_a_range_check_alone_would_admit() {
        for line in ["progress nan", "progress inf", "progress -inf", "progress NaN"] {
            assert_eq!(Command::parse(line), Err(Rejection::BadArgument), "{line:?}");
        }
    }

    #[test]
    fn progress_rejects_what_is_not_a_fraction() {
        for line in ["progress 1.5", "progress -0.1", "progress 40%", "progress"] {
            assert!(Command::parse(line).is_err(), "{line:?} parsed");
        }
    }

    #[test]
    fn a_stray_carriage_return_is_tolerated_and_nothing_else_is() {
        assert_eq!(Command::parse("ping\r"), Ok(Command::Ping));
        assert!(Command::parse("ping\u{0}").is_err());
        assert!(Command::parse("\u{1b}[2J").is_err());
    }

    /// A control character inside a message is not rejected here, because this
    /// is not where that is handled: `raba_core::message::sanitise` is, at the
    /// point of drawing. Parsing that tried to also be filtering would give two
    /// places to believe the job was done.
    #[test]
    fn parsing_does_not_pretend_to_sanitise() {
        let hostile = "\u{1b}[2Jlogin:";
        assert_eq!(
            Command::parse(&format!("message {hostile}")),
            Ok(Command::Message(hostile.to_owned()))
        );
        assert_eq!(raba_core::sanitise(hostile, 80), "[2Jlogin:");
    }

    #[test]
    fn replies_render_as_the_document_says() {
        assert_eq!(Reply::Ok.render(), "+OK\n");
        assert_eq!(Reply::OkWith("raba 0.1.0".to_owned()).render(), "+OK raba 0.1.0\n");
        assert_eq!(Reply::Failed(Rejection::TooLong).render(), "-ERR too long\n");
        assert_eq!(
            Reply::Report(vec![("protocol", "1".to_owned()), ("phase", "boot".to_owned())])
                .render(),
            "+OK\nprotocol: 1\nphase: boot\n"
        );
    }

    /// Every reply is one or more complete lines. A client reads lines; half a
    /// line is a client that blocks until the connection closes under it.
    #[test]
    fn every_reply_is_whole_lines() {
        let replies = [
            Reply::Ok,
            Reply::OkWith("x".to_owned()),
            Reply::Failed(Rejection::Unknown),
            Reply::Report(vec![("a", "b".to_owned())]),
        ];
        for reply in replies {
            let rendered = reply.render();
            assert!(rendered.ends_with('\n'), "{rendered:?}");
            assert!(!rendered.is_empty());
        }
    }

    /// The reasons are the wire format, and two reasons that render the same
    /// would make a client unable to tell two failures apart.
    #[test]
    fn every_rejection_has_its_own_wording() {
        let all = [
            Rejection::Unknown,
            Rejection::TooLong,
            Rejection::NotUtf8,
            Rejection::MissingArgument,
            Rejection::BadArgument,
            Rejection::NotDrawing,
            Rejection::NotConfigured,
        ];
        let mut reasons: Vec<&str> = all.iter().map(|r| r.reason()).collect();
        let count = reasons.len();
        reasons.sort_unstable();
        reasons.dedup();
        assert_eq!(reasons.len(), count);
        assert!(reasons.iter().all(|reason| !reason.contains('\n')));
    }

    /// A value containing a newline would forge reply lines, and for `status`
    /// that is a client reading fields nobody sent.
    #[test]
    fn a_value_cannot_forge_an_extra_reply_line() {
        assert_eq!(Reply::OkWith("a\nprotocol: 99".to_owned()).render(), "+OK a protocol: 99\n");
        assert_eq!(
            Reply::Report(vec![("theme", "x\r\nphase: boot".to_owned())]).render(),
            "+OK\ntheme: x  phase: boot\n"
        );
        for reply in
            [Reply::OkWith("a\nb".to_owned()), Reply::Report(vec![("k", "a\nb".to_owned())])]
        {
            assert_eq!(reply.render().matches('\n').count(), reply.render().lines().count());
        }
    }
}
