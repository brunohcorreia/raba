//! Code page 437, the character set ANSI art is written in.
//!
//! Every `.ans`, `.nfo` and `.asc` file in existence is a stream of CP437 bytes,
//! because the format predates Unicode and the art is made of characters that only
//! that code page has. Decoding one as UTF-8 gives mojibake where the box drawing
//! should be, which is the single most common way to get this wrong.
//!
//! This lives in `raba-gen` and not in `raba-core`: it is needed to *read* art at
//! build time and never to draw it at boot.
//!
//! ## The control range
//!
//! Bytes 0x01 to 0x1F are pictures in CP437 — a smiley, card suits, arrows — and
//! art uses them. They are not control codes here, with four exceptions the parser
//! must see: line feed, carriage return, SUB (0x1A, the DOS end-of-file marker
//! that terminates the art), and ESC (0x1B, which begins every escape sequence).
//! Those four keep their ASCII meaning; everything else in the range is a glyph.
//! 0x00 is a blank, which is how it was always displayed.
//!
//! This matches what `ansilove` and every other ANSI renderer does.

/// Byte to character. Index is the CP437 byte.
pub const TABLE: [char; 256] = [
    ' ', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '\u{a}', '♂', '♀', '\u{d}', '♫', '☼', '►',
    '◄', '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '\u{1a}', '\u{1b}', '∟', '↔', '▲', '▼', ' ', '!',
    '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2', '3', '4',
    '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G',
    'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    '[', '\\', ']', '^', '_', '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm',
    'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '⌂', 'Ç',
    'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô',
    'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª',
    'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣',
    '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠',
    '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±', '≥',
    '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', ' ',
];

/// Decode a CP437 byte to the character it draws.
///
/// Note that this is total: every one of the 256 bytes is a character, which is
/// why ANSI art can never fail to decode the way UTF-8 can.
#[must_use]
pub const fn decode(byte: u8) -> char {
    // The index is a `u8` and the table has 256 entries, so this cannot be out of
    // range; `#[allow]` rather than `.get()` because a `const fn` cannot unwrap,
    // and a lookup that is total by construction should read like one.
    #[allow(clippy::indexing_slicing)]
    TABLE[byte as usize]
}

/// Decode a whole buffer, control bytes and all.
#[must_use]
pub fn decode_all(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| decode(*byte)).collect()
}

/// The byte that draws `ch`, if CP437 has one.
///
/// Used when writing art back out, where a character the code page does not have
/// has to become something, and `?` is the honest answer.
#[must_use]
pub fn encode(ch: char) -> Option<u8> {
    // Linear over 256 entries, which is nothing at build time and keeps the table
    // the single source of truth rather than having a second one to disagree with.
    TABLE.iter().position(|candidate| *candidate == ch).and_then(|i| u8::try_from(i).ok())
}
