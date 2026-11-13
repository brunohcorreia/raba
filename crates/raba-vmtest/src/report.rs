//! Results, on the serial line, in TAP.
//!
//! TAP because it is a format a human can read without a tool and a tool can read
//! without a parser library: `ok 3 - the console reports a size`. The host script
//! greps it; a person watching the boot reads it directly.

use std::io::Write as _;

#[derive(Debug)]
pub struct Report {
    count: u32,
    failed: u32,
}

impl Report {
    pub fn new() -> Report {
        println!("TAP version 13");
        Report { count: 0, failed: 0 }
    }

    /// A report that carries on another one's numbering, without a second
    /// header. The checks run in a child process — see `main` — and the two
    /// halves have to read as one TAP stream.
    pub fn continuing(already_run: u32) -> Report {
        Report { count: already_run, failed: 0 }
    }

    /// How many checks have been recorded, including any inherited.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// How many of them failed.
    pub fn failures(&self) -> u32 {
        self.failed
    }

    /// Run a check and record it. `Err` carries the reason, which goes on the line.
    ///
    /// Takes the body rather than its result so a check can use `?` without the
    /// immediately-invoked-closure dance, and so that a check is never written
    /// and then not run.
    pub fn check<F>(&mut self, name: &str, body: F)
    where
        F: FnOnce() -> Result<(), String>,
    {
        let outcome = body();
        self.count = self.count.saturating_add(1);
        match outcome {
            Ok(()) => println!("ok {} - {name}", self.count),
            Err(reason) => {
                self.failed = self.failed.saturating_add(1);
                println!("not ok {} - {name}", self.count);
                println!("  ---");
                println!("  reason: {reason}");
                println!("  ...");
            }
        }
        // Serial is slow and the machine is about to be switched off. An
        // unflushed buffer is a result that never reaches the host, which reads
        // as a hang rather than as a failure.
        let _ = std::io::stdout().flush();
    }

    /// Anything worth saying that is not a pass or a fail.
    pub fn note(&mut self, text: &str) {
        println!("# {text}");
        let _ = std::io::stdout().flush();
    }

    pub fn failed(&self) -> bool {
        self.failed > 0
    }

    pub fn finish(&self) {
        println!("1..{}", self.count);
        if self.failed > 0 {
            println!("# {} of {} failed", self.failed, self.count);
        }
        let _ = std::io::stdout().flush();
    }
}
