//! The listening end.
//!
//! Small, because the interesting parts are elsewhere: the grammar is in
//! [`wire`](super::wire) and the decisions are in [`lifecycle`](crate::lifecycle).
//! What is left here is the privilege boundary, and that is worth having on its
//! own so it can be read without anything else in the way.
//!
//! # Two checks, not either
//!
//! The socket is mode `0600`. *And* every accepted connection's peer is checked
//! with `SO_PEERCRED` and dropped unless it is the same user that is running
//! RABA.
//!
//! Either alone would be defensible and neither alone is enough.
//! `security.md` finding 1 is the reason: `message` draws caller-chosen text on a
//! console as root, which is a credential harvester if the wrong caller can
//! reach it, and the modes on files in `/run` are set by whichever initramfs
//! generator built the image. A permission check that depends on somebody else's
//! packaging is not a permission check.
//!
//! `security.md` words the rule as "reject any `uid != 0`". The same user is the
//! same rule where it matters — at boot RABA is root, so the only peer it admits
//! is root — and it is the right rule in the case that sentence did not have in
//! mind, which is somebody running the binary as themselves. A hardcoded zero
//! there would make a user's own instance controllable by root alone and not by
//! the user, which is both surprising and useless. It also makes the check
//! reachable from `cargo test` without a second account, and an access check
//! nobody can run is an access check nobody knows works.
//!
//! A refused peer gets **no reply at all**. There is nothing to tell it: that
//! something is listening it learned from `connect`, and a `-ERR` would only
//! confirm which program.
//!
//! # Non-blocking, because there is a frame to draw
//!
//! The daemon is a single loop with a deadline every few tens of milliseconds.
//! The listener is non-blocking and polled once per frame, and an accepted
//! connection gets a short read timeout, so a client that connects and then says
//! nothing stalls itself rather than the animation.

use super::wire::{Command, Reply, MAX_REQUEST};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the socket lives. See `man 8 raba`.
pub const SOCKET: &str = "/run/raba.sock";

/// How long a connection may take to send its one line.
///
/// Short on purpose. The only caller is a local one-shot that has the whole
/// request ready before it connects, and the cost of being generous is a frame
/// that does not get drawn.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// `S_IRUSR | S_IWUSR`: owner reads and writes, nobody else does anything.
const MODE: u32 = 0o600;

/// How much of an over-long request to read and throw away before giving up.
///
/// Nothing is kept, so this costs time and not memory, and the time is already
/// capped by the read timeout.
const DISCARD_LIMIT: usize = 64 * 1024;

/// Where the socket is built before it is published.
///
/// Beside the real path on purpose: `rename` only works within a filesystem,
/// and `/run` and `/tmp` are not guaranteed to be the same one.
fn staging_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    path.with_file_name(name)
}

/// Whether a peer may command us.
///
/// Split out from the socket code so it is testable with made-up identities:
/// the real check runs once per connection and has exactly one input, and a unit
/// test cannot conjure a second user to run as.
#[must_use]
pub const fn permitted(peer: u32, owner: u32) -> bool {
    peer == owner
}

/// The listening socket.
#[derive(Debug)]
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
    /// The user that bound it, and so the only one allowed to talk to it.
    owner: u32,
}

impl Listener {
    /// Bind the control socket, replacing any socket left by a dead instance.
    ///
    /// # Errors
    ///
    /// If the path cannot be bound, which on a normal system means `/run` is not
    /// writable — so not root, or no `/run` yet. The caller carries on without a
    /// socket rather than refusing to draw: a splash nobody can talk to is worth
    /// more than no splash.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Listener> {
        let path = path.as_ref().to_path_buf();
        // The mode has to be right from the instant the socket is reachable at
        // its published path, and `bind` then `set_permissions` leaves a window
        // in which it is whatever the ambient umask allows.
        //
        // The obvious fix is `umask`, and it is wrong: `umask` is per *process*,
        // so setting and restoring it around a bind is a global mutation that
        // races anything else running at the time. The project's own test suite
        // found that — several tests binding at once left the process umask at
        // the saved value of whichever finished last, and directories created
        // afterwards came out without their execute bit.
        //
        // So: bind to a name nobody is told about, set the mode on it, and
        // `rename` it into place. Rename is atomic, the inode is the same one
        // the listener is attached to, and the socket is never reachable at
        // `path` in a mode other than this one. It replaces a stale socket in
        // the same step.
        let staging = staging_path(&path);
        let _ = std::fs::remove_file(&staging);
        let inner = UnixListener::bind(&staging)?;
        let mut permissions = std::fs::metadata(&staging)?.permissions();
        permissions.set_mode(MODE);
        if let Err(error) = std::fs::set_permissions(&staging, permissions)
            .and_then(|()| std::fs::rename(&staging, &path))
        {
            let _ = std::fs::remove_file(&staging);
            return Err(error);
        }
        inner.set_nonblocking(true)?;

        Ok(Listener { inner, path, owner: rustix::process::getuid().as_raw() })
    }

    /// The user this socket will take commands from. Root, at boot.
    #[must_use]
    pub const fn owner(&self) -> u32 {
        self.owner
    }

    /// The path it is bound to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Take the next waiting connection, if there is one.
    ///
    /// Returns `None` when nothing is waiting, which is the usual answer and not
    /// an error, and also when a connection was refused — the caller has nothing
    /// to do about either.
    pub fn accept(&self) -> Option<Connection> {
        let (stream, _) = self.inner.accept().ok()?;
        let peer = rustix::net::sockopt::socket_peercred(&stream).ok()?;
        if !permitted(peer.uid.as_raw(), self.owner) {
            // Dropped, with nothing written. See the module documentation.
            return None;
        }
        stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
        Some(Connection { stream })
    }
}

impl Drop for Listener {
    /// Take the socket file with us.
    ///
    /// Not load-bearing — [`bind`](Self::bind) copes with one left behind, which
    /// it has to, because the case this does not cover is the one that actually
    /// happens: `SIGKILL`.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One accepted, authenticated connection.
#[derive(Debug)]
pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    /// Read the one request line.
    ///
    /// Reads at most [`MAX_REQUEST`] bytes and then stops, so a client that
    /// never sends a newline cannot make this grow. A request that long, or that
    /// is not UTF-8, is a [`Reply::Failed`] rather than an error: the connection
    /// is still good and the client is still owed an answer.
    ///
    /// # Errors
    ///
    /// Only if the socket itself fails or the client sends nothing within the
    /// read timeout.
    pub fn request(&mut self) -> io::Result<Result<Command, super::wire::Rejection>> {
        let mut buffer = Vec::with_capacity(64);
        let mut byte = [0u8; 1];
        loop {
            let read = self.stream.read(&mut byte)?;
            if read == 0 {
                break;
            }
            if byte[0] == b'\n' {
                break;
            }
            if buffer.len() >= MAX_REQUEST {
                self.discard();
                return Ok(Err(super::wire::Rejection::TooLong));
            }
            buffer.push(byte[0]);
        }
        let Ok(line) = std::str::from_utf8(&buffer) else {
            return Ok(Err(super::wire::Rejection::NotUtf8));
        };
        Ok(Command::parse(line))
    }

    /// Read and throw away whatever is left, so the reply gets through.
    ///
    /// Closing a socket that still has unread bytes in its receive buffer sends
    /// a reset, and a reset destroys data already written — so a client that
    /// sent too much would get `ECONNRESET` instead of the `-ERR too long` the
    /// protocol promises it. Draining first is what makes the documented reply
    /// the one that actually arrives.
    ///
    /// Bounded, because this is the path a client that sends a megabyte takes:
    /// nothing is kept, and after [`DISCARD_LIMIT`] bytes we stop reading and
    /// accept that such a client gets a reset. It asked for it.
    fn discard(&mut self) {
        let mut sink = [0u8; 256];
        let mut dropped = 0usize;
        while dropped < DISCARD_LIMIT {
            match self.stream.read(&mut sink) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    dropped = dropped.saturating_add(read);
                    if sink.get(..read).is_some_and(|got| got.contains(&b'\n')) {
                        return;
                    }
                }
            }
        }
    }

    /// Send a reply. The connection stays open until it is dropped.
    ///
    /// # Errors
    ///
    /// If the client has already gone, which is not worth doing anything about.
    pub fn reply(&mut self, reply: &Reply) -> io::Result<()> {
        self.stream.write_all(reply.render().as_bytes())?;
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
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
}
