//! The ANSI importer against arbitrary bytes.
//!
//! Every `.ans` is somebody else's file, often decades old and often truncated by
//! whatever moved it around. The importer runs at build time rather than at boot,
//! so a panic here does not stop a machine booting — but it does stop a theme
//! being built, with a backtrace instead of a message, and that is worth not
//! having. There is no input it may crash on and none it may hang on.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let options = raba_gen::ansi::Options::default();
    if let Ok(imported) = raba_gen::ansi::parse(data, &options) {
        // What came out has to be a movie the format accepts, or the importer has
        // produced something no one can play.
        let movie = &imported.movie;
        assert!(movie.columns > 0 && movie.lines > 0);
        let bytes = raba_core::encode(movie);
        assert!(raba_core::decode(&bytes).is_ok(), "our own output must decode");
    }

    // Plain text takes the same bytes and must be just as unflappable.
    let _ = raba_gen::text::parse(data, &raba_gen::text::Options::default());
});
