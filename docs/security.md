# Security

> Design note. Threat model and hardening plan for a program that runs as root, early in
> boot, parsing a file.

RABA is cosmetic software with a privileged runtime. That combination deserves an
explicit threat model rather than an assumption that a splash screen cannot matter.

## Posture

| | |
|---|---|
| runs as | root |
| when | initramfs onward, before most of the system exists |
| parses | a binary file from disk |
| listens on | a unix socket |
| writes to | a virtual terminal, `/run` |
| handles secrets | **no** — see [luks.md](luks.md) |

The last row is the most important security decision in the project. Staying out of the
passphrase path removes the only category of secret RABA could leak.

---

## Findings

Issues identified during design, with their mitigations. Each needs a test.

### 1. Control socket — privilege boundary · **high**

`/run/raba.sock` accepts commands. If any local user can connect, they can call
`deactivate`, `quit`, and `message`.

The dangerous one is `message`: it draws attacker-chosen text on a console, as root. A
convincing fake `login:` prompt on a terminal the user trusts is a credential-harvesting
attack, and the splash is the one program on the system whose entire job is drawing over
the console.

> **Mitigation:** socket mode `0600`, owned by root, in a root-owned directory. Verify the
> peer with `SO_PEERCRED` and reject any `uid != 0` — permissions alone are not enough if
> a directory is ever mis-created. Both checks, not either.

### 2. Terminal escape injection · **high**

Any text RABA puts on the terminal — a `message`, a theme name in an error, a log line
echoed to the console — passes through a terminal that interprets escape sequences. The
Linux console's sequence set includes changing modes and remapping the keyboard.

Passing untrusted bytes to a terminal is a well-worn vulnerability class and it would
apply to us by default.

> **Mitigation:** everything written to the terminal goes through one encoder. It accepts
> a grid of validated cells, never a raw string. Text entering from outside — `message`,
> file names in errors — is filtered to printable characters, control bytes dropped, and
> truncated to the canvas width. No path writes caller-supplied bytes to a terminal fd.

### 3. State file is trusted on resume · **medium**

[early-boot.md](early-boot.md) has RABA resume from `/run/raba.state`, which carries a VT
number and a frame index. An attacker who can write it chooses which terminal RABA draws
on — including one with an active session.

> **Mitigation:** same ownership and mode rules as the socket. Independently, validate
> every field on read: VT within range, frame index below the theme's frame count,
> timestamp recent. A state file that fails any check is deleted and treated as a fresh
> start, never partially trusted.

### 4. Symlink attacks on `/run` paths · **medium**

If `/run/raba.log` is pre-created as a symlink, RABA writes through it as root.

> **Mitigation:** `O_NOFOLLOW` everywhere, `O_CREAT|O_EXCL` where the file should be new,
> and never follow a path we did not create. Exposure is small because `/run` is
> root-owned this early, which is a reason to be careful rather than a reason to skip it.

### 5. Malformed or hostile animation file · **medium, structurally mitigated**

We parse a file as root with no supervision.

> **Mitigation:** this is why the format is ours. `.raba` states its total size in a fixed
> header, validated before a single allocation, with hard ceilings on canvas dimensions
> and frame count. The gzip-bomb class of failure cannot be expressed. Every offset is
> bounds-checked once at load; the renderer then indexes without further checks.
> `cargo-fuzz` over the parser, in CI, with the golden fixtures as seed corpus.

### 6. Core dumps · **low**

A root process dumping core writes memory to disk. We hold no secrets, but a dump is
still unnecessary disclosure.

> **Mitigation:** `prctl(PR_SET_DUMPABLE, 0)`. One line.

### 7. Denial of service by holding the terminal · **low**

A RABA that never exits leaves the user staring at art.

> **Mitigation:** `Boot.Timeout`, a watchdog, and `raba.enable=0`.

---

## Hardening

Cheap measures that suit a program with a small, fixed set of syscalls.

**`PR_SET_NO_NEW_PRIVS`** — free. We never `exec`, so setting it costs nothing and closes
escalation through any future accidental exec.

**Drop capabilities.** After acquiring the terminal we need very little. Keeping
`CAP_SYS_TTY_CONFIG` and dropping the rest is straightforward. Dropping to an unprivileged
*user* is not realistic in an initramfs, which usually has only root.

**seccomp allowlist.** RABA's syscall set is small and known at design time: open, read,
write, ioctl, nanosleep, a few more. A `seccomp-bpf` allowlist installed after setup is
strong containment for a root process that parses files, and it is unusually achievable
here because the syscall set genuinely does not grow.

Worth doing in v2. The cost is a dependency or some hand-written BPF, against the
dependency budget in [maintenance.md](maintenance.md).

**`#![forbid(unsafe_code)]`** outside `vt/`, where every `unsafe` block carries a comment
stating the invariant it relies on.

---

## Integrity

RABA does not verify its theme file, and should not invent a scheme for it.

With a Unified Kernel Image, the initramfs — binary and theme included — is inside the
signature. Integrity comes from the existing chain, for free.

In a classic shim + GRUB setup the initramfs is **not** verified at all; GRUB checks the
kernel's signature and not the initrd's. We do not create that gap, but we do add one more
root-privileged binary to the unverified zone, which is the honest cost of existing there.
A signature check by a program the attacker can replace proves nothing.

Full detail, including which TPM measurements we disturb: [secure-boot.md](secure-boot.md).

---

## Reporting

A vulnerability report should go privately first, via GitHub's security advisory form.
Describe the class of problem; do not include a working exploit in a public issue.

Given the posture, the reports that matter most are: anything that gets code or arbitrary
bytes onto a terminal through RABA, anything that makes the parser allocate without bound,
and anything that lets a non-root user influence what RABA does.
