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
mod tests {
    use super::rewrite;
    use raba::config::{self, Config};
    use std::path::Path;

    /// What `raba`(8) would actually draw after this edit.
    ///
    /// Every test below asserts through this rather than on the text alone,
    /// because the bug this module was rewritten for was an edit that looked
    /// perfect and changed nothing the parser read.
    fn effective(text: &str) -> String {
        let mut config = Config::default();
        let _ = config::apply(text, Path::new("test"), &mut config);
        match config.boot.theme {
            config::Theme::Auto => "auto".to_owned(),
            config::Theme::Named(name) => name,
            config::Theme::Path(path) => path.display().to_string(),
        }
    }

    #[test]
    fn an_existing_key_is_replaced_where_it_stands() {
        let before = "# mine\n[Boot]\nTheme=auto\nTimeout=30\n";
        assert_eq!(rewrite(before, "arch"), "# mine\n[Boot]\nTheme=arch\nTimeout=30\n");
        assert_eq!(effective(&rewrite(before, "arch")), "arch");
    }

    /// The last assignment is the one `apply` obeys, so it is the one to edit.
    /// The first version of this module changed the first and reported success
    /// while the boot kept drawing the old logo.
    #[test]
    fn with_two_lines_the_one_the_parser_obeys_is_the_one_that_changes() {
        let before = "[Boot]\nTheme=generic\nTheme=arch\n";
        let after = rewrite(before, "cachyos");
        assert_eq!(after, "[Boot]\nTheme=generic\nTheme=cachyos\n");
        assert_eq!(effective(&after), "cachyos");
    }

    /// `[boot]` is not `[Boot]`: `SECTIONS` is matched exactly, so the parser
    /// skips everything under it. Editing the line there would edit something
    /// no boot reads, so a `[Boot]` section is added instead and it wins by
    /// being the only one the parser can see.
    #[test]
    fn a_section_the_parser_does_not_recognise_is_not_the_section_to_edit() {
        let before = "[boot]\ntheme = auto\n";
        let after = rewrite(before, "arch");
        assert!(after.contains("[boot]\ntheme = auto"), "{after}");
        assert_eq!(effective(&after), "arch");
    }

    /// An unterminated header leaves the parser in an unknown section, and
    /// everything under it is skipped. Same reasoning, different typo.
    #[test]
    fn an_unterminated_header_is_not_a_section_either() {
        let before = "[Boot\nTheme=auto\n";
        assert_eq!(effective(&rewrite(before, "arch")), "arch");
    }

    #[test]
    fn the_same_key_in_another_section_is_left_alone() {
        let before = "[Outro]\nTheme=auto-outro\n\n[Boot]\nTheme=auto\n";
        let after = rewrite(before, "arch");
        assert_eq!(after, "[Outro]\nTheme=auto-outro\n\n[Boot]\nTheme=arch\n");
        assert_eq!(effective(&after), "arch");
    }

    #[test]
    fn a_boot_section_with_no_theme_gets_one_at_the_top() {
        let before = "[Boot]\nTimeout=30\n";
        assert_eq!(rewrite(before, "arch"), "[Boot]\nTheme=arch\nTimeout=30\n");
        assert_eq!(effective(&rewrite(before, "arch")), "arch");
    }

    #[test]
    fn a_file_with_no_boot_section_gets_one() {
        let before = "[Outro]\nEnable=true\n";
        assert_eq!(rewrite(before, "arch"), "[Outro]\nEnable=true\n\n[Boot]\nTheme=arch\n");
        assert_eq!(effective(&rewrite(before, "arch")), "arch");
    }

    #[test]
    fn an_empty_file_becomes_the_smallest_valid_one() {
        assert_eq!(rewrite("", "arch"), "[Boot]\nTheme=arch\n");
        assert_eq!(effective("[Boot]\nTheme=arch\n"), "arch");
    }

    #[test]
    fn comments_and_every_other_setting_survive() {
        let before = "# RABA\n# two lines of it\n\n[Boot]\n# which logo\nTheme=auto\n\
                      ShowDelay=0.2\n\n[Progress]\nEnable=true\n";
        let after = rewrite(before, "cachyos");
        assert!(after.contains("# two lines of it"), "{after}");
        assert!(after.contains("# which logo"), "{after}");
        assert!(after.contains("ShowDelay=0.2"), "{after}");
        assert!(after.contains("[Progress]\nEnable=true"), "{after}");
        assert_eq!(after.matches("Theme=").count(), 1, "{after}");
        assert_eq!(effective(&after), "cachyos");
    }

    /// Idempotent, which is what makes running it twice harmless.
    #[test]
    fn setting_the_same_theme_twice_changes_nothing_the_second_time() {
        let once = rewrite("[Boot]\nTheme=auto\n", "arch");
        assert_eq!(rewrite(&once, "arch"), once);
    }

    /// A commented-out key is a comment. The parser skips whole-line comments,
    /// so an editor that treated one as the line to change would write into a
    /// comment and leave the real setting alone.
    #[test]
    fn a_commented_out_theme_is_not_the_line_to_edit() {
        let before = "[Boot]\n#Theme=auto\nTimeout=30\n";
        let after = rewrite(before, "arch");
        assert!(after.contains("#Theme=auto"), "{after}");
        assert_eq!(effective(&after), "arch");
    }
}
