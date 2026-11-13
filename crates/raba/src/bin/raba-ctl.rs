//! `raba-ctl`(1) — tell a running `raba`(8) what to do.
//!
//! The counterpart to `plymouth`(1), and it exists for the same reason: something
//! has to say when the system is ready.
//!
//! # Exiting 0 when nothing is listening
//!
//! The surprising behaviour, and the most important one. Every caller of this is
//! a boot-time unit:
//!
//! ```text
//! ExecStartPre=/usr/bin/raba-ctl deactivate
//! ExecStartPost=/usr/bin/raba-ctl quit --wait
//! ```
//!
//! On a machine where RABA is not in the initramfs — which is most machines that
//! have the package installed and have not rebuilt an image yet — nothing is
//! listening. If that were an error, the display manager would fail to start
//! because the splash was absent, which is the splash breaking a boot: the one
//! thing this project says it will never do.
//!
//! `ping` is the exception, because reporting whether an instance is running is
//! the entire command.
//!
//! # Why there is no retry
//!
//! A command that did not arrive because nothing was listening is a command that
//! did not need to arrive. Retrying would add a delay to every boot that does not
//! use RABA in exchange for nothing.

use raba::control::wire::{Command, MAX_REQUEST};
use raba::control::SOCKET;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

const USAGE: &str = "\
raba-ctl - control a running raba instance

usage:
    raba-ctl <command> [arguments]

commands:
    ping                     exit 0 if an instance is running
    status                   print the current state
    quit [--wait]            play the outro and exit; --wait blocks until it has
    deactivate               stop drawing and release the terminal
    reactivate               draw again
    message <text>           show text under the animation
    progress <fraction>      set the progress indicator, 0.0 to 1.0
    progress --advance <f>   add to it

options:
    --socket <path>          talk to this socket instead of /run/raba.sock
    --version, --help

exits 0 when no instance is running, because the callers are boot-time units
that must not fail when the splash is absent. `ping` is the exception.
";

/// Delivered, or nothing was listening.
const DELIVERED: u8 = 0;
/// An instance is running and refused.
const REFUSED: u8 = 1;
/// The command line was wrong.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("raba-ctl: {message}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}

fn run() -> Result<u8, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut socket = SOCKET.to_owned();
    let mut rest: Vec<String> = Vec::new();
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(DELIVERED);
            }
            "--version" => {
                println!("raba-ctl {}", env!("CARGO_PKG_VERSION"));
                return Ok(DELIVERED);
            }
            "--socket" => {
                let value = args.get(index).ok_or("--socket needs a path")?;
                index = index.saturating_add(1);
                socket = value.clone();
            }
            // Everything from the first non-option word on is the command, so a
            // message containing `--socket` is a message and not an option.
            _ => {
                rest.extend_from_slice(args.get(index.saturating_sub(1)..).unwrap_or_default());
                break;
            }
        }
    }

    if rest.is_empty() {
        print!("{USAGE}");
        return Ok(USAGE_ERROR);
    }
    let request = request_line(&rest)?;
    // Parsed here as well as by the server, so a typo is a usage error with the
    // usage text rather than a round trip that comes back `-ERR unknown
    // command`. The server still parses it: this is a courtesy, not a check.
    let command = Command::parse(&request).map_err(|why| format!("{why}\n\n{USAGE}"))?;

    deliver(&socket, &request, &command)
}

/// Turn the command line into exactly one protocol line.
fn request_line(words: &[String]) -> Result<String, String> {
    let line = words.join(" ");
    // A newline would end the request early and leave the rest of it looking
    // like a second command. The server reads one line and never sees it, so
    // this is belt and braces — but the braces are free and the belt is on the
    // other side of a socket.
    if line.contains('\n') || line.contains('\r') {
        return Err("a command may not contain a line break".to_owned());
    }
    if line.len() >= MAX_REQUEST {
        return Err(format!("command is {} bytes; the limit is {}", line.len(), MAX_REQUEST));
    }
    Ok(line)
}

fn deliver(socket: &str, request: &str, command: &Command) -> Result<u8, String> {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        // Nothing listening. See the module documentation for why this is not
        // an error, and why `ping` is the one command for which it is.
        return Ok(if matches!(command, Command::Ping) { REFUSED } else { DELIVERED });
    };

    writeln!(stream, "{request}").map_err(|e| format!("sending: {e}"))?;
    stream.flush().map_err(|e| format!("sending: {e}"))?;

    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).map_err(|e| format!("reading the reply: {e}"))?;
    let first = first.trim_end_matches('\n');

    if let Some(reason) = first.strip_prefix("-ERR ") {
        eprintln!("raba-ctl: {reason}");
        return Ok(REFUSED);
    }
    let Some(tail) = first.strip_prefix("+OK") else {
        return Err(format!("unexpected reply {first:?}"));
    };
    let tail = tail.trim_start();
    if !tail.is_empty() {
        println!("{tail}");
    }

    match command {
        // `status` is followed by `key: value` lines until the server closes.
        Command::Status => {
            for line in reader.lines() {
                let line = line.map_err(|e| format!("reading the reply: {e}"))?;
                println!("{line}");
            }
        }
        // EOF is the protocol's way of saying "the process is gone": it arrives
        // when the kernel closes the socket the dead process was holding. No pid
        // file, no polling, and no window in which a reused pid looks alive.
        Command::Quit { wait: true } => {
            for line in reader.lines() {
                if line.is_err() {
                    break;
                }
            }
        }
        _ => {}
    }
    Ok(DELIVERED)
}
