//! The `blastit` binary: the universal install (`ARCHITECTURE.md` §2.1, §4).
//!
//! Everything it does lives in the library, so `blastit check`, `fmt` and `fold`
//! reach the same code whether they are started from `$PATH` or from a package's
//! console script — which `exec`s this binary rather than importing anything.

use std::process::ExitCode;

use blastit::{Host, cli};

/// Where the exit code comes from: init's own status.
fn main() -> ExitCode {
    ExitCode::from(cli::main(
        Host::binary(),
        std::env::args().skip(1).collect(),
    ))
}
