//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

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
    assert_eq!(Command::parse(&format!("message {text}")), Ok(Command::Message(text.to_owned())));
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
        Reply::Report(vec![("protocol", "1".to_owned()), ("phase", "boot".to_owned())]).render(),
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
    for reply in [Reply::OkWith("a\nb".to_owned()), Reply::Report(vec![("k", "a\nb".to_owned())])] {
        assert_eq!(reply.render().matches('\n').count(), reply.render().lines().count());
    }
}

/// The word `status` reports for a command, and what a log line calls it.
///
/// Part of the wire protocol — `docs/protocol.md` — so these are API, and a
/// renamed variant that quietly renames the word is a client that stops
/// matching. Nothing exercised them.
#[test]
fn every_command_knows_the_word_it_is_spelled_with() {
    let pairs = [
        (Command::Ping, "ping"),
        (Command::Status, "status"),
        (Command::Quit { wait: false }, "quit"),
        (Command::Quit { wait: true }, "quit"),
        (Command::Deactivate, "deactivate"),
        (Command::Reactivate, "reactivate"),
        (Command::Message("hello".to_owned()), "message"),
        (Command::Progress { fraction: 0.5, advance: false }, "progress"),
    ];
    for (command, word) in pairs {
        assert_eq!(command.name(), word);
    }
    // And for the commands that take nothing, the word parses back to the same
    // one — so the name is the protocol's and not a label beside it. `message`
    // and `progress` are left out because the bare word is a missing argument,
    // which the test below is about.
    for word in ["ping", "status", "quit", "deactivate", "reactivate"] {
        assert_eq!(
            Command::parse(word).map(|parsed| parsed.name()),
            Ok(word),
            "{word} does not parse back"
        );
    }
}

#[test]
fn message_with_nothing_after_it_is_a_missing_argument_and_not_an_empty_message() {
    // An empty message would clear the line, which is a different request. A
    // client that meant that should have to say so.
    assert_eq!(Command::parse("message"), Err(Rejection::MissingArgument));
    assert_eq!(Command::parse("progress"), Err(Rejection::MissingArgument));
    assert_eq!(Command::parse("progress --advance"), Err(Rejection::MissingArgument));
    // Whitespace is not nothing: `message` takes the rest of the line
    // verbatim, deliberately, so that there are no quoting rules to get wrong.
    assert_eq!(Command::parse("message  "), Ok(Command::Message(" ".to_owned())));
}
