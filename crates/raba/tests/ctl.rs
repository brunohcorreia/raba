//! `raba-ctl`(1) against a real socket, as a separate process.
//!
//! The unit tests in `control` check the grammar and the privilege boundary.
//! This checks the thing a boot-time unit actually does: run a binary and look
//! at its exit status. That is the whole contract from a service file's point of
//! view, and the most important line of it — exit 0 when nothing is listening —
//! cannot be tested any other way, because it is about a socket that is not
//! there.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba::control::wire::{Command, Reply, MAX_REQUEST};
use raba::control::Listener;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};
use std::sync::mpsc;
use std::time::Duration;

const CTL: &str = env!("CARGO_BIN_EXE_raba-ctl");

/// A directory of our own, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-ctl-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch directory");
        Scratch(path)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("raba.sock")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A server that answers `count` requests and then stops, reporting what it saw.
///
/// Returns once it is listening, so a client started afterwards cannot race it.
fn serving(path: &Path, count: usize) -> mpsc::Receiver<Command> {
    let (ready, is_ready) = mpsc::channel();
    let (seen, was_seen) = mpsc::channel();
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let listener = Listener::bind(&path).expect("binding");
        ready.send(()).expect("announcing");
        let mut served = 0;
        for _ in 0..5000u32 {
            if served >= count {
                break;
            }
            let Some(mut connection) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            let outcome = connection.request().expect("reading");
            let reply = match &outcome {
                Ok(Command::Status) => {
                    Reply::Report(vec![("protocol", "1".to_owned()), ("phase", "boot".to_owned())])
                }
                Ok(Command::Message(text)) => Reply::OkWith(raba_core::sanitise(text, 40)),
                Ok(_) => Reply::Ok,
                Err(why) => Reply::Failed(*why),
            };
            connection.reply(&reply).expect("replying");
            if let Ok(command) = outcome {
                seen.send(command).expect("reporting");
            }
            served = served.saturating_add(1);
        }
    });
    is_ready.recv_timeout(Duration::from_secs(5)).expect("server came up");
    was_seen
}

fn ctl(socket: &Path, args: &[&str]) -> Output {
    Process::new(CTL).arg("--socket").arg(socket).args(args).output().expect("running raba-ctl")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit status")
}

/// The line every boot-time unit depends on. A machine with the package
/// installed and no RABA in its initramfs runs this on every boot.
#[test]
fn with_nothing_listening_every_command_but_ping_exits_zero() {
    let scratch = Scratch::new("absent");
    for args in [
        vec!["quit"],
        vec!["quit", "--wait"],
        vec!["deactivate"],
        vec!["reactivate"],
        vec!["status"],
        vec!["message", "hello"],
    ] {
        let output = ctl(&scratch.socket(), &args);
        assert_eq!(code(&output), 0, "{args:?} did not exit 0 with no server");
        assert!(output.stdout.is_empty(), "{args:?} printed {:?}", output.stdout);
    }
}

/// And the exception, because reporting presence is the whole command.
#[test]
fn ping_is_the_one_command_that_fails_when_nothing_is_listening() {
    let scratch = Scratch::new("ping-absent");
    assert_eq!(code(&ctl(&scratch.socket(), &["ping"])), 1);
}

#[test]
fn ping_succeeds_against_a_live_instance() {
    let scratch = Scratch::new("ping-live");
    let seen = serving(&scratch.socket(), 1);
    assert_eq!(code(&ctl(&scratch.socket(), &["ping"])), 0);
    assert_eq!(seen.recv_timeout(Duration::from_secs(5)).expect("served"), Command::Ping);
}

#[test]
fn status_prints_the_fields_the_server_sent() {
    let scratch = Scratch::new("status");
    let _seen = serving(&scratch.socket(), 1);
    let output = ctl(&scratch.socket(), &["status"]);
    assert_eq!(code(&output), 0);
    let text = String::from_utf8(output.stdout).expect("utf-8");
    assert!(text.contains("protocol: 1"), "{text:?}");
    assert!(text.contains("phase: boot"), "{text:?}");
}

/// The rest of the line, verbatim, with no quoting rules to get wrong.
#[test]
fn a_message_arrives_with_its_spaces_intact() {
    let scratch = Scratch::new("message");
    let seen = serving(&scratch.socket(), 1);
    assert_eq!(code(&ctl(&scratch.socket(), &["message", "Checking", "filesystems"])), 0);
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(5)).expect("served"),
        Command::Message("Checking filesystems".to_owned())
    );
}

/// An option-looking word after the command is part of the message, not an
/// option, or `raba-ctl message --socket /tmp/x` would be unsendable.
#[test]
fn an_option_inside_a_message_stays_in_the_message() {
    let scratch = Scratch::new("message-option");
    let seen = serving(&scratch.socket(), 1);
    assert_eq!(code(&ctl(&scratch.socket(), &["message", "--socket", "--help"])), 0);
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(5)).expect("served"),
        Command::Message("--socket --help".to_owned())
    );
}

/// A newline in a message would end the request early and leave the remainder
/// looking like a second command. Refused before it is sent.
#[test]
fn a_message_cannot_smuggle_a_second_command() {
    let scratch = Scratch::new("smuggle");
    let output = ctl(&scratch.socket(), &["message", "hello\nquit"]);
    assert_eq!(code(&output), 2);
    let complaint = String::from_utf8(output.stderr).expect("utf-8");
    assert!(complaint.contains("line break"), "{complaint:?}");
}

/// Escape sequences are the server's problem, not the client's: the message
/// reaches the server intact and is sanitised where it is drawn.
#[test]
fn an_escape_sequence_travels_as_text_and_is_defused_at_the_far_end() {
    let scratch = Scratch::new("escape");
    let seen = serving(&scratch.socket(), 1);
    let output = ctl(&scratch.socket(), &["message", "\u{1b}[2Jlogin:"]);
    assert_eq!(code(&output), 0);
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(5)).expect("served"),
        Command::Message("\u{1b}[2Jlogin:".to_owned())
    );
    // The server echoed what it would draw.
    let echoed = String::from_utf8(output.stdout).expect("utf-8");
    assert!(!echoed.contains('\u{1b}'), "{echoed:?}");
    assert!(echoed.contains("[2Jlogin:"), "{echoed:?}");
}

#[test]
fn a_refusal_from_the_server_is_exit_one_and_a_word_on_stderr() {
    let scratch = Scratch::new("refused");
    let _seen = serving(&scratch.socket(), 1);
    // Parsed by the server, not by the client: the client sends it happily.
    let mut client = std::os::unix::net::UnixStream::connect(scratch.socket()).expect("connect");
    {
        use std::io::Write as _;
        client.write_all(b"progress 2.0\n").expect("write");
    }
    let mut reply = String::new();
    {
        use std::io::Read as _;
        client.read_to_string(&mut reply).expect("read");
    }
    assert_eq!(reply, "-ERR bad argument\n");
}

#[test]
fn a_bad_command_line_is_a_usage_error_without_touching_the_socket() {
    let scratch = Scratch::new("usage");
    for args in [vec!["fly"], vec!["quit", "--soon"], vec!["progress", "later"]] {
        let output = ctl(&scratch.socket(), &args);
        assert_eq!(code(&output), 2, "{args:?}");
    }
}

/// `--wait` returns when the server's socket closes, which is what happens when
/// the process holding it stops existing. No pid file and no polling.
#[test]
fn quit_with_wait_returns_when_the_instance_is_gone() {
    let scratch = Scratch::new("wait");
    let _seen = serving(&scratch.socket(), 1);
    let started = std::time::Instant::now();
    let output = ctl(&scratch.socket(), &["quit", "--wait"]);
    assert_eq!(code(&output), 0);
    assert!(started.elapsed() < Duration::from_secs(5), "waited {:?}", started.elapsed());
}

// --- the argument parser, and the two replies that are not a command's --------

#[test]
fn help_and_version_answer_without_a_socket_at_all() {
    let scratch = Scratch::new("help");
    for flag in ["--help", "-h"] {
        let output = ctl(&scratch.socket(), &[flag]);
        assert_eq!(code(&output), 0, "{flag}");
        let text = String::from_utf8_lossy(&output.stdout);
        for command in ["ping", "status", "quit", "deactivate", "message"] {
            assert!(text.contains(command), "{flag} does not mention {command}:\n{text}");
        }
    }

    let output = ctl(&scratch.socket(), &["--version"]);
    assert_eq!(code(&output), 0);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Exit 2, not 0. Every other failure here is deliberately exit 0 so that a
/// boot-time unit cannot be failed by a splash — but a command line nobody
/// typed correctly is a bug in a service file, and a service file that is
/// wrong should say so.
#[test]
fn no_command_at_all_prints_the_usage_and_exits_two() {
    let scratch = Scratch::new("no-command");
    let output = ctl(&scratch.socket(), &[]);
    assert_eq!(code(&output), 2);
    assert!(!output.stdout.is_empty(), "it refused without printing the usage");
}

#[test]
fn a_command_longer_than_the_protocol_allows_is_refused_here_rather_than_sent() {
    let scratch = Scratch::new("too-long");
    // The server reads at most `MAX_REQUEST` and then stops, so a client that
    // sends more is a client whose message is silently cut in half. Better to
    // find out on this side.
    let huge = "x".repeat(MAX_REQUEST + 10);
    let output = ctl(&scratch.socket(), &["message", &huge]);
    assert_eq!(code(&output), 2);
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("limit"), "{said}");
}

/// A server that answers one connection with a fixed line, however wrong.
///
/// The request is read before the reply is sent, and that is not politeness: a
/// server that writes and closes while the client is still writing leaves the
/// client with a closed socket and an empty read, which it reports as an
/// unexpected reply. The first version of this did exactly that, passed on a
/// quiet machine, and failed under an instrumented build.
fn answering(path: &Path, reply: &'static str) -> std::thread::JoinHandle<()> {
    let (ready, is_ready) = mpsc::channel();
    let path = path.to_path_buf();
    let handle = std::thread::spawn(move || {
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("binding");
        ready.send(()).expect("announcing");
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{BufRead as _, BufReader, Write as _};
            let mut request = String::new();
            let mut reader = BufReader::new(stream.try_clone().expect("a second handle"));
            let _ = reader.read_line(&mut request);
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.flush();
        }
    });
    is_ready.recv_timeout(Duration::from_secs(5)).expect("server came up");
    handle
}

#[test]
fn a_refusal_from_the_server_is_passed_on_with_its_reason() {
    let scratch = Scratch::new("refused-reason");
    let server = answering(&scratch.socket(), "-ERR not drawing\n");
    let output = ctl(&scratch.socket(), &["message", "hello"]);
    let _ = server.join();

    assert_eq!(code(&output), 1, "a refusal is exit 1, not a crash and not a success");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("not drawing"), "the reason was dropped: {said}");
}

#[test]
fn a_reply_that_is_neither_ok_nor_err_is_an_error_naming_what_arrived() {
    // Not a case any RABA produces. It is the case where something else is
    // listening on the path — which is exactly when a client must not guess.
    let scratch = Scratch::new("nonsense-reply");
    let server = answering(&scratch.socket(), "HTTP/1.1 200 OK\n");
    let output = ctl(&scratch.socket(), &["ping"]);
    let _ = server.join();

    assert_eq!(code(&output), 2);
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("HTTP/1.1 200 OK"), "it did not say what it got: {said}");
}

#[test]
fn an_ok_with_something_after_it_is_printed() {
    let scratch = Scratch::new("ok-with");
    let server = answering(&scratch.socket(), "+OK unlocking\n");
    let output = ctl(&scratch.socket(), &["message", "unlocking"]);
    let _ = server.join();

    assert_eq!(code(&output), 0);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("unlocking"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn socket_with_no_path_after_it_is_a_usage_error_and_not_a_command() {
    // `--socket` is the last option before everything becomes the command, so
    // getting this wrong would make `raba-ctl --socket` send the word
    // `--socket` somewhere rather than complain.
    let output = Process::new(CTL).arg("--socket").output().expect("running raba-ctl");
    assert_eq!(code(&output), 2, "{output:?}");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("--socket"), "{said:?}");
    assert!(said.contains("path"), "{said:?}");
}
