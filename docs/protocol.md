# The control socket protocol

`raba`(8) listens on `/run/raba.sock`. `raba-ctl`(1) connects to it. This document is
the contract between them, and it is written before either end so that the protocol is
something both were built against rather than something the first client happened to do.

It is listed in [maintenance.md](maintenance.md#5-written-down-contracts) as public API:
semantic versioning applies to it the same way it applies to the config file keys.

---

## Shape

A Unix domain `SOCK_STREAM` socket. **One command per connection**: connect, send one
line, read the reply, the server closes.

That is not the most efficient design available and it is the right one here. There is
no session to get out of sync, no state on the server per client, no framing bugs, and
the whole thing is legible with `socat`. The client is a boot-time one-shot that runs
three or four times per boot; it does not need a session.

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
one line by construction rather than by validation.

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
version: 0.0.0
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

An error is a reply; the connection still closes normally afterwards.

## Who may connect

Two independent checks, and both of them, which is the mitigation for
[security.md](security.md) finding 1:

1. The socket is mode `0600`, owned by `root:root`, in a root-owned directory.
2. The server reads `SO_PEERCRED` on every accepted connection and **closes it with no
   reply at all** unless the peer's `uid` is 0.

Permissions alone are not enough if a directory is ever mis-created by a package or an
initramfs generator, and a `uid` check alone would let a mode-`0666` socket be probed.
The second check is silent on purpose: a refused peer learns that something is
listening, which it already knew from `connect`, and nothing else.

### Why this matters more than it looks

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
case and the same answer. The server unlinks and re-creates the socket on startup rather
than trusting one it finds.

## Versioning

`status` reports `protocol: 1`. A client should treat an unknown key in `status` as
something to ignore, and the server will only ever add keys within a protocol version.
A removal or a change of meaning is a new protocol number and a major version of the
package.

## See also

`man 1 raba-ctl` for the command-line surface, `man 8 raba` for the daemon,
[security.md](security.md) for the findings this implements.
