//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::super::wire::Rejection;
use super::*;

/// A socket of our own, in a directory of our own, deleted afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-test-{}-{name}", std::process::id()));
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

fn send(path: &Path, request: &str) -> String {
    let mut client = UnixStream::connect(path).expect("connecting");
    client.write_all(request.as_bytes()).expect("writing");
    client.flush().expect("flushing");
    let mut reply = String::new();
    client.read_to_string(&mut reply).expect("reading");
    reply
}

/// Serve exactly one request, the way the daemon's frame loop would.
fn serve_one(listener: &Listener) -> Option<Command> {
    for _ in 0..2000u32 {
        if let Some(mut connection) = listener.accept() {
            let outcome = connection.request().expect("reading the request");
            let reply = match &outcome {
                Ok(Command::Ping) => Reply::Ok,
                Ok(command) => Reply::OkWith(command.name().to_owned()),
                Err(why) => Reply::Failed(*why),
            };
            connection.reply(&reply).expect("replying");
            return outcome.ok();
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("nothing connected");
}

/// The mode has to be right from the instant the socket exists, which is
/// why it is set with `umask` and not with a `chmod` afterwards.
#[test]
fn the_socket_is_created_already_private() {
    let scratch = Scratch::new("mode");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    let mode = std::fs::metadata(listener.path()).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, MODE, "mode was {:o}", mode & 0o777);
}

#[test]
fn a_request_gets_its_reply_and_then_end_of_file() {
    let scratch = Scratch::new("roundtrip");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    let path = listener.path().to_path_buf();
    let client = std::thread::spawn(move || send(&path, "ping\n"));
    assert_eq!(serve_one(&listener), Some(Command::Ping));
    // `read_to_string` returned, which it only does at EOF.
    assert_eq!(client.join().expect("client thread"), "+OK\n");
}

/// A socket file left by a dead instance gives every client ECONNREFUSED
/// forever. Binding over it is the only way back.
#[test]
fn a_socket_left_by_a_dead_instance_is_replaced() {
    let scratch = Scratch::new("stale");
    let first = Listener::bind(scratch.socket()).expect("first bind");
    std::mem::forget(first); // as if killed: no Drop, the file stays
    assert!(scratch.socket().exists());
    let second = Listener::bind(scratch.socket());
    assert!(second.is_ok(), "could not replace a stale socket");
}

#[test]
fn the_socket_is_gone_after_an_ordinary_exit() {
    let scratch = Scratch::new("cleanup");
    let path = {
        let listener = Listener::bind(scratch.socket()).expect("binding");
        listener.path().to_path_buf()
    };
    assert!(!path.exists());
}

/// The bound on memory. A client that sends a megabyte with no newline must
/// not make the server hold a megabyte.
#[test]
fn an_oversized_request_is_refused_rather_than_buffered() {
    let scratch = Scratch::new("toolong");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    let path = listener.path().to_path_buf();
    let client = std::thread::spawn(move || {
        let flood = "message ".to_owned() + &"A".repeat(MAX_REQUEST * 8);
        send(&path, &flood)
    });
    let mut connection = loop {
        if let Some(connection) = listener.accept() {
            break connection;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let outcome = connection.request().expect("reading");
    assert_eq!(outcome, Err(Rejection::TooLong));
    connection.reply(&Reply::Failed(Rejection::TooLong)).expect("replying");
    drop(connection);
    assert_eq!(client.join().expect("client"), "-ERR too long\n");
}

#[test]
fn a_request_that_is_not_utf8_is_refused_and_not_decoded_anyway() {
    let scratch = Scratch::new("utf8");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    let path = listener.path().to_path_buf();
    let client = std::thread::spawn(move || {
        let mut client = UnixStream::connect(&path).expect("connecting");
        client.write_all(b"message \xff\xfe\n").expect("writing");
        let mut reply = String::new();
        client.read_to_string(&mut reply).expect("reading");
        reply
    });
    assert_eq!(serve_one(&listener), None);
    assert_eq!(client.join().expect("client"), "-ERR not utf-8\n");
}

/// The rule itself, with identities a test can make up. This is the whole
/// access decision; everything around it is plumbing.
#[test]
fn only_the_user_running_raba_may_command_it() {
    assert!(permitted(0, 0), "root talking to a root instance");
    assert!(permitted(1000, 1000), "a user talking to their own instance");
    assert!(!permitted(1000, 0), "a user talking to the boot splash");
    assert!(!permitted(0, 1000), "root talking to a user's instance");
    assert!(!permitted(1001, 1000), "the next user along");
}

/// And that the rule is actually reached, from the only side a unit test can
/// see it from: our own connection, which is the permitted case whatever
/// user happens to be running the suite.
#[test]
fn our_own_connection_passes_the_peer_check() {
    let scratch = Scratch::new("peer");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    assert_eq!(listener.owner(), rustix::process::getuid().as_raw());
    let path = listener.path().to_path_buf();
    let client = std::thread::spawn(move || send(&path, "ping\n"));
    assert_eq!(serve_one(&listener), Some(Command::Ping));
    assert_eq!(client.join().expect("client"), "+OK\n");
}

#[test]
fn a_bind_that_cannot_be_moved_into_place_leaves_nothing_of_ours_behind() {
    // The socket is bound under a name nobody is told about and renamed into
    // place, so that it is never reachable at `path` in the wrong mode. If
    // that rename fails — here because something that is not a socket is
    // already sitting at the path — the staging file must go with it.
    // `/run` is shared, and a .raba.sock.new nobody owns outlives the boot.
    let scratch = Scratch::new("bind-blocked");
    let blocked = scratch.socket();
    std::fs::create_dir_all(&blocked).expect("something in the way");
    std::fs::write(blocked.join("occupied"), b"x").expect("making it non-empty");

    let why = Listener::bind(blocked.clone()).expect_err("a rename onto a directory fails");
    assert!(why.kind() != std::io::ErrorKind::NotFound, "{why}");

    let left: Vec<String> = std::fs::read_dir(&scratch.0)
        .expect("reading the directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".new"))
        .collect();
    assert!(left.is_empty(), "a staging socket was left behind: {left:?}");
}

/// The same flood, newline-terminated — which is what a script writing to the
/// socket with `echo` actually sends. The draining loop stops at that newline
/// rather than reading on to its byte limit, and the point of draining at all
/// is this assertion: the client gets the `-ERR too long` the protocol
/// promises, not the `ECONNRESET` that closing a socket with unread bytes in
/// it would produce.
#[test]
fn an_oversized_request_that_ends_in_a_newline_still_gets_its_answer() {
    let scratch = Scratch::new("toolong-newline");
    let listener = Listener::bind(scratch.socket()).expect("binding");
    let path = listener.path().to_path_buf();
    let client = std::thread::spawn(move || {
        let flood = "message ".to_owned() + &"A".repeat(MAX_REQUEST * 2) + "\n";
        send(&path, &flood)
    });
    let mut connection = loop {
        if let Some(connection) = listener.accept() {
            break connection;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(connection.request().expect("reading"), Err(Rejection::TooLong));
    connection.reply(&Reply::Failed(Rejection::TooLong)).expect("replying");
    drop(connection);
    assert_eq!(client.join().expect("client"), "-ERR too long\n");
}
