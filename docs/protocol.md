# The control socket protocol

`raba`(8) listens on `/run/raba.sock`; `raba-ctl`(1) connects to it. This document is
the contract between them and is authoritative: where an implementation differs from
what is written here, the implementation is the defect.

It is listed in [maintenance.md](maintenance.md#6-written-down-contracts) as public API:
semantic versioning applies to it the same way it applies to the config file keys.

---

## Shape

A Unix domain `SOCK_STREAM` socket. **One command per connection**: connect, send one
line, read the reply, the server closes.

Not the most efficient arrangement available, and the right one for this client: no
session to fall out of sync, no per-client state on the server, no framing, and the
whole exchange is legible with `socat`. `raba-ctl` is a boot-time one-shot that runs
three or four times per boot.

```
$ socat - UNIX-CONNECT:/run/raba.sock
ping
+OK
```

## Requests

One line, UTF-8, terminated by a single `\n`. **At most 1024 bytes including the
newline**, and the server never buffers more than that — a client that sends more gets
`-ERR too long` and the connection is closed, which is a bound on memory and not a
politeness.

The command word is up to the first space. For every command but `message`, the rest of
the line is parsed as space-separated arguments. For `message` the rest of the line is
**the message, verbatim**, spaces and all — so no quoting rules exist to get wrong.

A request cannot contain a newline, because a newline ends it. A message is therefore
one line by construction rather than by validation. `raba-ctl` refuses to send one as
well, so a message with a line break in it is a usage error on the near side of the
socket rather than a silently truncated command on the far side.

**Send one line and nothing after it.** The server reads up to the newline and stops;
bytes after it are never read, and closing a socket with unread bytes in it sends a
reset, which can destroy the reply that was already written. A client that sends two
commands on one connection may not get an answer to either.

| request | meaning |
|---|---|
| `ping` | are you there |
| `status` | report state |
| `quit` | play the outro, restore the terminal, exit |
| `quit --wait` | the same, but hold this connection open until the process is gone |
| `deactivate` | stop drawing and release the terminal, keep running |
| `reactivate` | draw again |
| `message <text>` | show `<text>` under the animation |
| `progress <fraction>` | set the progress indicator, `0.0` to `1.0` |
| `progress --advance <fraction>` | add to it |

## Replies

Lines, UTF-8, each terminated by `\n`. The **first** line is the result:

```
+OK
+OK <text>
-ERR <reason>
```

`status` follows its `+OK` with `key: value` lines, one per line, in a stable order:

```
+OK
protocol: 1
version: 0.1.0
phase: boot
terminal: 2
previous: 1
theme: /usr/share/raba/themes/arch.raba
frame: 431
uptime_ms: 8620
```

Then the server closes the connection. **EOF is part of the protocol**, not an
accident: it is how a reply of unknown length ends, and it is how `quit --wait` reports
that the process is gone. A client that wants to wait for exit does not poll a pid file
and does not race — it reads until EOF, and the kernel closes the socket when the
process it belonged to stops existing.

### Errors

| reply | when |
|---|---|
| `-ERR unknown command` | the command word is not one of the above |
| `-ERR too long` | the request exceeded 1024 bytes |
| `-ERR not utf-8` | the request was not valid UTF-8 |
| `-ERR missing argument` | a command that needs one did not get one |
| `-ERR bad argument` | a `progress` fraction that is not a number in `0.0..=1.0` |
| `-ERR not drawing` | `message` or `progress` while deactivated |
| `-ERR not configured` | `progress` without `Progress.Source=ctl` |

An error is a reply; the connection still closes normally afterwards.

## Who may connect

Two independent checks, and both of them, which is the mitigation for
[security.md](security.md) finding 1:

1. The socket is mode `0600`, owned by whoever runs RABA — `root:root` at boot — in a
   root-owned directory.
2. The server reads `SO_PEERCRED` on every accepted connection and **closes it with no
   reply at all** unless the peer is that same user.

Permissions alone are not enough if a directory is ever mis-created by a package or an
initramfs generator, and an ownership check alone would let a mode-`0666` socket be
probed. The second check is silent on purpose: a refused peer learns that something is
listening, which it already knew from `connect`, and nothing else.

[security.md](security.md) words the rule as "reject any `uid != 0`". *The same user* is
that rule where it matters — at boot RABA is root, so root is the only peer it admits —
and it is the right rule in the case that sentence did not have in mind, somebody
running the binary as themselves. A hardcoded zero would make a user's own instance
controllable by root and not by the user. It is also what makes the check reachable from
the test suite without a second account, and an access check nobody can run is an access
check nobody knows works.

The mode is set before the socket is reachable, not after. Binding and then `chmod`
leaves a window; `umask` closes the window but is per *process*, so setting and
restoring it around a `bind` races everything else in the program — RABA's own test
suite caught exactly that. The socket is therefore bound at `raba.sock.new`, chmodded,
and `rename`d into place, which is atomic, keeps the same inode the listener is attached
to, and replaces a socket left behind by a dead instance in the same step.

### Why the access check matters

`message` puts caller-chosen text on a console, as root, in the one program on the
system whose job is drawing over the console. A fake `login:` prompt on a terminal the
user trusts is a credential-harvesting attack. The socket's access control is the first
line; [message sanitisation](#message-sanitisation) is the second, and neither is
allowed to be the only one.

## Message sanitisation

Text from the socket is **never written to a terminal as bytes**. It goes through
`raba_core::message::sanitise`, which produces a string that:

- contains no C0 control character, no `DEL`, and no C1 control character — including
  `0x9B`, which is a single-byte CSI on a console in 8-bit mode, so dropping `ESC` alone
  would not be enough;
- contains no character the console cannot draw, so no bidirectional overrides and no
  zero-width characters that could rearrange what the user reads;
- is no longer than the canvas is wide.

The result is then placed into a grid of cells like any other text, and the renderer's
own encoder is what reaches the terminal. There is no path from a socket byte to a
terminal write.

## If nothing is listening

`connect` fails with `ENOENT` or `ECONNREFUSED`. `raba-ctl` exits **0** and says
nothing: its callers are boot-time units that must not fail because the splash is
absent, which is the normal case on a machine where RABA is not installed in the
initramfs.

A socket file left behind by a dead instance gives `ECONNREFUSED`, which is the same
case and the same answer. The server replaces the socket on startup rather than trusting
one it finds.

`ping` is the exception to all of this: reporting whether an instance is running is the
entire command, so it exits non-zero when nothing is listening.

## Versioning

`status` reports `protocol: 1`. A client should treat an unknown key in `status` as
something to ignore, and the server will only ever add keys within a protocol version.
A removal or a change of meaning is a new protocol number and a major version of the
package.

## See also

`man 1 raba-ctl` for the command-line surface, `man 8 raba` for the daemon,
[security.md](security.md) for the findings this implements.
