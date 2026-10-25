//! The `blastit` binary: the standalone install (`ARCHITECTURE.md` §2.1, §4).
//!
//! Everything it does lives in the library, so that a wheel install — where the
//! same code is called from the interpreter that imported
//! `blastit._blastit` — plays exactly the same roles. This program is the only
//! one that re-executes `/proc/self/exe` (§4), which is what
//! [`blastit::Host::binary`] says.

use std::process::ExitCode;

use blastit::{Host, cli};

/// Where the exit code comes from: init's own status.
fn main() -> ExitCode {
    ExitCode::from(cli::main(
        Host::binary(),
        std::env::args().skip(1).collect(),
        None,
    ))
}
