//! Compile the vendored fastfetch logo art into raba-gen.
//!
//! The art is data, not code: `vendor/fastfetch-logos` holds it at a pinned
//! commit and `scripts/fastfetch-manifest.py` turns fastfetch's C declarations
//! into the manifest read here. Embedding it rather than reading it at run time
//! keeps raba-gen a single file that works out of a checkout, a package or a
//! `cargo install`, with no data directory to find.
//!
//! raba-gen is build-time only, so the ~330 KB this adds never goes near an
//! initramfs. See docs/maintenance.md.

// Panicking is the correct failure mode for a build script: a missing or
// malformed manifest means the vendored art is not there, and producing a
// raba-gen that silently knows no logos would be worse than not building.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::Path;

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let vendor = Path::new(&manifest_dir).join("../../vendor/fastfetch-logos");
    let manifest = vendor.join("manifest.tsv");
    let pinned = vendor.join("PINNED");

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", pinned.display());

    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| {
        panic!("{}: {e}\nrun scripts/vendor-fastfetch-logos.sh", manifest.display())
    });

    let mut tag = String::new();
    let mut commit = String::new();
    if let Ok(pin) = std::fs::read_to_string(&pinned) {
        for line in pin.lines() {
            match line.split_once(' ') {
                Some(("tag", value)) => tag = value.to_owned(),
                Some(("commit", value)) => commit = value.to_owned(),
                _ => {}
            }
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "pub const FASTFETCH_TAG: &str = \"{}\";", escape(&tag));
    let _ = writeln!(out, "pub const FASTFETCH_COMMIT: &str = \"{}\";", escape(&commit));
    let _ = writeln!(out, "pub static LOGOS: &[Logo] = &[");

    let mut count = 0usize;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(name), Some(aliases), Some(file)) = (fields.next(), fields.next(), fields.next())
        else {
            panic!("malformed manifest line: {line}");
        };
        let colors = fields.next().unwrap_or("");

        let art = vendor.join(file);
        println!("cargo:rerun-if-changed={}", art.display());
        assert!(art.is_file(), "{}: art file missing", art.display());

        let alias_list = aliases
            .split(',')
            .filter(|a| !a.is_empty())
            .map(|a| format!("\"{}\"", escape(a)))
            .collect::<Vec<_>>()
            .join(", ");
        let colour_list = colors
            .split(',')
            .filter(|c| !c.is_empty())
            .map(|c| format!("\"{}\"", escape(c)))
            .collect::<Vec<_>>()
            .join(", ");

        let _ = writeln!(
            out,
            "    Logo {{ name: \"{}\", aliases: &[{}], art: include_str!(r\"{}\"), colors: &[{}] }},",
            escape(name),
            alias_list,
            art.display(),
            colour_list
        );
        count = count.saturating_add(1);
    }
    let _ = writeln!(out, "];");
    let _ = writeln!(out, "pub const LOGO_COUNT: usize = {count};");

    let dest = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("fastfetch_logos.rs");
    std::fs::write(&dest, out).unwrap_or_else(|e| panic!("{}: {e}", dest.display()));
}
