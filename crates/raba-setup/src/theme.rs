//! Changing the theme in `raba.conf`, which is one line and three traps.
//!
//! The first trap is that `/etc/raba/raba.conf` is a file a person wrote in,
//! with their comments and their other settings in it. Rewriting it from a
//! parsed structure would throw all of that away, so the edit here is textual
//! and touches exactly the line it has to.
//!
//! The second is that the file belongs to RABA. `docs/packaging.md` forbids
//! editing configuration owned by *another* package — `/etc/default/grub`,
//! `/etc/mkinitcpio.conf`, `/etc/ly/config.lua` — and this is the other side of
//! that rule: our own file, named in the package's `backup=`, is ours to edit
//! when somebody asks us to.
//!
//! The third is the one that cost a review finding. A textual editor needs to
//! know which line the *parser* obeys, and the first version worked that out
//! for itself: it lowercased section names, lowercased keys, and stopped at the
//! first `Theme` it saw. [`raba::config::apply`] does none of those things. It
//! matches `[Boot]` exactly, matches `Theme` exactly, and runs through the whole
//! file so the **last** assignment wins. Every one of those three differences
//! produces the same failure — the command reports success, a line changes, and
//! the boot draws the logo it drew before.
//!
//! So the editor does not decide where the line is. [`raba::config::placement`]
//! does, from inside the parser, over the same scanner `apply` uses.

use raba::config;

/// Put `value` on the `Theme` line of the `[Boot]` section.
///
/// Three cases, in the order they are tried: the file already sets it and the
/// line the parser obeys is replaced in place; the file has a `[Boot]` section
/// and the key is inserted at the top of it; the file has neither and both are
/// appended. Everything else — comments, order, the settings nobody asked about
/// — comes out unchanged.
///
/// Lines that set the key and are *not* the effective one are left exactly as
/// they are. Deleting them would be the editor making a decision nobody asked
/// for about somebody else's file, and the result is the same either way:
/// `apply` reads the last one, which is the one that was just written.
#[must_use]
pub fn rewrite(text: &str, value: &str) -> String {
    let found = config::placement(text, "Boot", "Theme");
    let replace_at = found.effective();
    let insert_after = found.header;

    let mut out = String::with_capacity(text.len().saturating_add(32));
    let mut wrote = false;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        if Some(number) == replace_at {
            out.push_str("Theme=");
            out.push_str(value);
            out.push('\n');
            wrote = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if replace_at.is_none() && Some(number) == insert_after {
            // Straight under the header, where a person reading the file next
            // will see it rather than at the bottom of a section.
            out.push_str("Theme=");
            out.push_str(value);
            out.push('\n');
            wrote = true;
        }
    }

    if !wrote {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str("[Boot]\nTheme=");
        out.push_str(value);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
