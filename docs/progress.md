# Progress indication

> Design note. Not implemented.

Drawing a progress bar is trivial — it is a row of cells. Knowing the progress is the
hard problem, and it is where this feature goes wrong in practice.

## There is no reliable source of boot progress

Nothing in a Linux boot knows how far along it is. The kernel does not, the initramfs
does not, and systemd knows only how many jobs it has queued *so far*, which grows as
it discovers more. Any progress bar at boot is an estimate, a guess, or a lie.

Four approaches, with what each actually costs:

### 1. Pushed from outside (`raba-ctl progress`)

```sh
raba-ctl progress 0.4
raba-ctl progress --advance 0.1
```

RABA draws what it is told and measures nothing. Whoever knows something — an initramfs
hook, a systemd unit, a user script — reports it.

Honest, trivial to implement, and init-agnostic: a runit script can call it as easily as
a systemd unit. It also fits the project's stated philosophy, since measuring boot
progress is emphatically not "one thing".

The cost is that out of the box nothing calls it, so the bar does not move unless
someone wires it up.

### 2. Time-based estimate against a cached previous boot

This is Plymouth's approach. It records how long the last boot took in
`/var/lib/plymouth/boot-duration` and interpolates against it next time.

It works, and its failure modes are well documented:

- If `/` and `/var` are separate filesystems, the file is not readable when the splash
  starts, because `/var` is not mounted yet. The bar crawls and stalls — a reported bug
  with a bind-mount workaround, open for years.
- With no cache file at all, Plymouth caps progress at 25%, so a first boot shows a bar
  that stops a quarter of the way across.
- The file is written after boot completes, so a boot that fails never updates it, and a
  system whose boot time changes takes several boots to converge.

For RABA the problem is worse. RABA starts in the initramfs and exits at handoff; it
never sees a mounted `/var`. Options are to bake the last known duration into the
initramfs at build time — stale, refreshing only on kernel updates — or to survive the
`switch_root` the way Plymouth does, which is a significant complexity increase for a
project whose first requirement is not breaking boots.

### 3. Indeterminate (pulse)

A bar that sweeps back and forth, or fills and resets. Shows activity, claims no
knowledge. Zero infrastructure, never wrong, never informative. It is a spinner shaped
like a bar.

### 4. Querying the init system

Subscribing to systemd's D-Bus to count jobs gives real numbers. It also means a D-Bus
client inside a boot splash, and it works on exactly one init system — which contradicts
the requirement that RABA not care what you run.

Rejected.

## Recommendation

Ship **3** as the default and **1** as the capable path. Treat **2** as optional, late,
and off by default.

```ini
[Progress]
Enable=false
Style=pulse        # pulse | bar | percent
Source=ctl         # ctl | time | none
Position=below
Width=40
```

With `Source=ctl` and nothing reporting, the bar stays at zero — so `Style=pulse`
remains the sane default until something is actually wired up.

## This breaks the pure `.dur` model

Worth stating plainly, because it is an architectural decision rather than a feature.

A progress bar changes between frames in response to external events. It cannot be baked
into a `.raba` file, which is a fixed sequence. So RABA cannot be only a `.dur`
player; it needs a **compositing layer** that draws widgets over the animation.

The good news is that this layer is required anyway:

| feature | needs an overlay |
|---|---|
| progress bar | yes |
| LUKS passphrase prompt | yes |
| `raba-ctl message` | yes |
| the animation itself | no |

Three requirements converge on the same mechanism. Building it for the passphrase
prompt — which is not optional — makes the progress bar nearly free.

The design stays clean if the rule is explicit:

> The `.raba` file is the background. Overlays are drawn on top, by RABA, in response to
> runtime state. A theme never contains an overlay.

Keeping that boundary means themes stay pure data, `raba-gen` keeps validating them
statically, and the art stays portable — anything else that can draw the same background
sees exactly what RABA shows minus the overlays.

## Cost

A bar is a few dozen cells redrawn per frame. Against a full-canvas animation this is
noise. The compositing layer is perhaps 150 lines. Neither is a reason to say no.

The reason to be careful is not performance. It is that a progress bar is a promise to
the user about how long they will wait, and the sources above show how easily that
promise becomes a bar that stops at 25% and sits there.
