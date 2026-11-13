//! Changing the theme in `raba.conf`, which is one line and two traps.
//!
//! The first trap is that `/etc/raba/raba.conf` is a file a person wrote in,
//! with their comments and their other settings in it. Rewriting it from a
//! parsed structure would throw all of that away, so the edit here is textual
//! and touches exactly the line it has to.
//!
//! The second is that the file belongs to RABA. `docs/packaging.md` forbids
//! editing configuration owned by *another* package — `/etc/default/grub`,
//! `/etc/mkinitcpio.conf`, `/etc/ly/config.lua` — and this is the other side of
//! that rule: our own file, named in `backup=`, is ours to edit when somebody
//! asks us to.

/// Put `value` on the `Theme` line of the `[Boot]` section.
///
/// Three cases, in the order they are tried: there is already a `Theme` key in
/// `[Boot]` and it is replaced in place; there is a `[Boot]` section and the key
/// is inserted at the top of it; there is no section at all and one is appended.
/// Everything else in the file — comments, order, the settings nobody asked
/// about — comes out unchanged.
///
/// Keys outside `[Boot]` are left alone, which matters because `Outro.Theme` is
/// a different setting with the same key name.
pub fn rewrite(text: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(32));
    let mut section = String::new();
    let mut done = false;
    let mut boot_header_at: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            out.push_str(line);
            out.push('\n');
            if section == "boot" {
                boot_header_at = Some(out.len());
            }
            continue;
        }
        if !done && section == "boot" && key_of(trimmed).is_some_and(|key| key == "theme") {
            out.push_str("Theme=");
            out.push_str(value);
            out.push('\n');
            done = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    if !done {
        match boot_header_at {
            // Straight under the header, where a person reading the file next
            // will see it rather than at the bottom of a section.
            Some(at) => out.insert_str(at, &format!("Theme={value}\n")),
            None => {
                if !out.is_empty() && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                out.push_str(&format!("[Boot]\nTheme={value}\n"));
            }
        }
    }
    out
}

/// The key of `Key=value`, lowercased, or `None` for a comment or a blank line.
fn key_of(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    Some(key.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::rewrite;

    #[test]
    fn an_existing_key_is_replaced_where_it_stands() {
        let before = "# mine\n[Boot]\nTheme=auto\nTimeout=30\n";
        assert_eq!(rewrite(before, "arch"), "# mine\n[Boot]\nTheme=arch\nTimeout=30\n");
    }

    #[test]
    fn spacing_and_case_in_the_key_do_not_hide_it() {
        let before = "[boot]\n  theme  =  auto  \n";
        assert_eq!(rewrite(before, "arch"), "[boot]\nTheme=arch\n");
    }

    #[test]
    fn the_same_key_in_another_section_is_left_alone() {
        let before = "[Outro]\nTheme=auto-outro\n\n[Boot]\nTheme=auto\n";
        assert_eq!(rewrite(before, "arch"), "[Outro]\nTheme=auto-outro\n\n[Boot]\nTheme=arch\n");
    }

    #[test]
    fn a_boot_section_with_no_theme_gets_one_at_the_top() {
        let before = "[Boot]\nTimeout=30\n";
        assert_eq!(rewrite(before, "arch"), "[Boot]\nTheme=arch\nTimeout=30\n");
    }

    #[test]
    fn a_file_with_no_boot_section_gets_one() {
        let before = "[Outro]\nEnable=true\n";
        assert_eq!(rewrite(before, "arch"), "[Outro]\nEnable=true\n\n[Boot]\nTheme=arch\n");
    }

    #[test]
    fn an_empty_file_becomes_the_smallest_valid_one() {
        assert_eq!(rewrite("", "arch"), "[Boot]\nTheme=arch\n");
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
    }

    /// The edit is idempotent, which is what makes running it twice harmless.
    #[test]
    fn setting_the_same_theme_twice_changes_nothing_the_second_time() {
        let once = rewrite("[Boot]\nTheme=auto\n", "arch");
        assert_eq!(rewrite(&once, "arch"), once);
    }
}
