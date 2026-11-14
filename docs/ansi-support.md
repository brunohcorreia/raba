# Importing ANSI art

`raba-gen convert art.ans -o art.raba`

ANSI art is the art of the bulletin-board era: CP437 bytes with escape sequences
mixed in, meant to be sent straight down a modem to a DOS terminal. There is a
large body of it, it is already the right shape for a console, and almost none of
it was made with anything like RABA in mind — which is exactly why importing it is
worth doing.

## The importer is a terminal

It does not parse the file. It **runs** it: a cursor, a grid of cells, a current
foreground and background, and a loop that feeds the stream in one byte at a time.
Whatever is on the grid at the end is the picture.

That is not a shortcut, it is the only correct reading. The format has no document
structure to parse — a `.ans` is a recording of what was sent to a screen, and the
same cell may be written several times. Art routinely draws a shape and then jumps
the cursor back inside it to colour it in:

```
ESC[1;1H  ....        home, draw four dots
ESC[1;2H  XX          go back and overwrite the middle two
```

Any approach other than imitating a terminal gets `.XX.` wrong.

What is honoured: `SGR` (colour, bold, reverse, reset), `CUU` `CUD` `CUF` `CUB`
`CNL` `CPL` `CHA` `CUP` `HVP` (movement), `ED` `EL` (erase), `SCP` `RCP` (save and
restore the cursor). Anything else is stepped over.

## Three common mistakes

**It is CP437, not UTF-8.** Byte `0xDB` is `█`. Decoded as UTF-8 it is not a
character at all, and the box drawing that every piece of ANSI art is made of
arrives as replacement characters. `crate::cp437` has the table; it is total, so
an `.ans` can never fail to decode the way text can.

**Bytes 0x01 to 0x1F are pictures, not control codes.** CP437 draws a smiley at
`0x01` and a heart at `0x03`, and art uses them. Four bytes keep their control
meaning because the parser needs them — line feed, carriage return, `SUB` (`0x1A`,
the DOS end-of-file marker that terminates the art), and `ESC`. Everything else in
the range is a glyph. This is what `ansilove` does too.

**Bold means bright.** `ESC[1m` followed by `ESC[31m` is bright red on every
terminal this art was drawn for, and it stays bright until something clears it. A
reader that treats SGR 1 as a font weight loses half the palette.

## SAUCE

The last 128 bytes of many `.ans` files are a metadata record: title, author,
group, and — the useful part — the width the art was drawn at, which is otherwise
a guess. It is read rather than merely skipped, but it has to be skipped either
way: left in, its text draws as a line of garbage along the bottom of the picture.
A comment block (`COMNT` plus 64 bytes per line) sits immediately before it and is
stripped with it.

One SAUCE flag matters. **iCE colours** means the file was made for a terminal
where the high background bit is brightness rather than blink. A Linux console is
not such a terminal, so those backgrounds are darkened to their dim counterparts
and a note says so. See [glyphs.md](glyphs.md) for the same bit's other job.

## What does not survive

**Blinking text.** The format has no blink attribute, and neither does `.raba`.
Noted when the art asks for it.

**Bright backgrounds.** A console's high background bit is blink, so there are
eight backgrounds and not sixteen. The hue is kept and the brightness is not,
which is the closest thing to what was asked for.

**Animation.** ANSImation exists — art that moves by redrawing — but it animates
by *baud rate*, with characters appearing as fast as the modem delivered them.
There are no frame boundaries in the file to find. The importer runs the whole
stream and keeps the final state, which is what a terminal shows when it finishes
receiving. If the art clears the screen between scenes, you get the last scene.
A file like that is better re-authored in Durdraw, where frames are real.

## Plain text

`convert` also takes plain text, which is the same thing minus the escapes:

```sh
raba-gen convert banner.txt -o banner.raba --fg 10
```

Tabs expand to eight-column stops, counted in characters rather than bytes so that
an accented line stops in the same place as an unaccented one. UTF-8 is tried
first and CP437 is the fallback, which is unambiguous in both directions: CP437 has
no invalid bytes, so the fallback cannot itself fail, and valid UTF-8 is never
mistaken for it.

**`$1`..`$9` mean nothing here.** Those are a recipe feature, and they are right
for art written *for* RABA — but in a file written for something else a dollar
sign is a dollar sign. So `convert` is literal and `build` reads markers, and which
you get follows from the command you ran. See [../themes/README.md](../themes/README.md).

## Which importer runs

The extension is a hint, not a promise: `.nfo` and `.asc` are sometimes plain
CP437 and sometimes full of escape sequences. `.dur`, `.ans` and `.ansi` settle it;
anything else is examined, and a file containing a CSI introducer is read as ANSI
art. When the extension did not decide, `convert` says which importer it picked,
because reading a file as the wrong format produces a wrong *picture* rather than
an error, and that is much harder to diagnose than a refusal.

`--format dur|ansi|text` overrides all of it.

## See also

- [dur-support.md](dur-support.md) — the other importer, and the one for animation
- [glyphs.md](glyphs.md) — which of CP437 a console can actually draw
- [format.md](format.md) — what the importers produce
