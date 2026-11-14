# 3D rotating logos

> Future work. Not scheduled. Written down so the decision is recorded rather than
> rediscovered.

[`areofyl/fetch`](https://github.com/areofyl/fetch) (ISC) renders a distro logo as a
rotating 3D relief in a terminal, donut.c style. The technique is well documented in its
README, and the question is whether RABA could do the same during boot.

## Feasibility: a generator feature, not a runtime one

The answer follows from a rule the project already has:

> **Authoring time may depend on anything. Run time depends on nothing.**

3D rotation is **a `raba-gen` feature, not a `raba` feature.** The generator does the
mathematics at build time and bakes the frames into a `.raba`; the boot binary stays what
it is, a thing that plays pre-rendered frames.

That is not a compromise. It is the same split that keeps `serde_json` and `flate2` out of
the boot path, applied to floating point and trigonometry.

### Why not compute it at boot

The alternative — rotating at runtime — fails on the project's own stated requirements:
smallest possible footprint in storage, RAM **and CPU**. It would add floating point or
fixed-point trigonometry, per-frame computation, and a new class of bug to the one program
that must never fail. For an animation most people see for three seconds.

Rejected, and not narrowly.

## The algorithm

From `fetch`'s own description, which is clear enough to implement from:

1. **Heightmap.** Each character gets a weight by visual density — `@` heavy, `.` light,
   `█` full, `░` thin. Weight becomes Z, turning flat art into relief.
2. **Point cloud.** Sample the heightmap. Interior cells get several Z layers for a solid
   extrusion; edge cells get only front and back faces, which keeps outlines clean.
3. **Normals.** Finite differences on the height gradient.
4. **Rotate and project.** Per frame, rotate around X/Y, perspective-project onto the cell
   grid, z-buffer for occlusion.
5. **Shade.** Blinn-Phong, diffuse plus specular, mapped onto a ramp.

A detail worth keeping: `fetch` auto-scales depth for logos with low height variance, so
art made of uniform characters does not come out flat.

## What the console allows

The glyph rule from [glyphs.md](glyphs.md) decides which of its three shading modes can
survive a boot — and this is a good test of that validator, since the answer is not
uniform:

| mode | glyphs | at boot |
|---|---|---|
| `ascii` — `.,-~:;=!*#$@` | pure ASCII | **yes** |
| `blocks` — 2×2 quadrants | `▘▝▖▗▚▞` and friends | **probably**, font dependent |
| `sextants` — 2×3 | Symbols for Legacy Computing, U+1FB00 | **no** |

Sextants are a 2019 Unicode addition. `fetch`'s own README says they need kitty, Ghostty,
foot or WezTerm — none of which is a Linux virtual console. They would render as blanks.

`raba-gen`'s glyph validator already rejects all three sub-cell sets, which is the first
external confirmation that the check earns its place — the full blocks `█▀▄▌▐` are CP437
and safe, the quadrants right beside them in Unicode are not.

The ASCII ramp is the one that matters, and it is also the original donut.c look.

## The cost: delta encoding stops paying

**Delta encoding stops working.** A 3D rotation changes nearly every cell every frame, so
the format's main size optimisation — a spinner over a static logo costing a few cells per
frame — contributes nothing here.

Rough arithmetic at 12 bytes per op:

| canvas | frames | size |
|---|---|---|
| 40×20 | 24 | ~230 KB |
| 60×30 | 30 | ~650 KB |
| 60×30 | 60 | ~1.3 MB |

All within the 16 MiB ceiling, none of it free in an initramfs. A conventional theme is a
few kilobytes.

### The format change this would justify

Not now, but recorded as the trigger: ops currently carry an explicit `cell` index, which
costs 4 bytes that a dense frame does not need. A **dense frame** encoding — all cells in
order, 6 bytes each for codepoint, fg and bg — would nearly halve the above.

That is a `.raba` version 2 feature, and the condition for adding it is "3D logos turn out
to be something people actually want", not "it might be useful".

## Whether it suits a boot splash

A separate question from whether it is possible, and worth asking before building it.

A boot splash is on screen for two to five seconds and the user is waiting for something
else. A slowly rotating 3D logo is striking in a fetch tool you invoke deliberately; it may
read as noise in a sequence whose job is to get out of the way.

Worth prototyping with `raba --preview` before committing. That is cheap, which is the
whole reason `--preview` is the first milestone.

## Licensing

`fetch` is ISC, which is compatible with GPL-3.0 — code could be incorporated with
attribution. The better path is implementing from the described algorithm, which is
standard graphics work, and crediting the inspiration either way.

## What applies today

One line from their README is useful immediately rather than eventually:

> the entire frame is written in a single `write()` syscall to avoid flicker

That belongs in M0, not here. A renderer that emits escape sequences incrementally will
tear and flicker on a console; building the whole frame in one buffer and issuing one write
is both faster and visually correct.

Noted against [milestones.md](milestones.md) M0, issue 6.
