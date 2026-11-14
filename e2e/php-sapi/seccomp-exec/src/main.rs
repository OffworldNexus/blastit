//! Install the real `SeccompRole::View { egress: false }` filter, then exec.
//!
//! `blastit-php-sapi` is zero-dependency by design, so it cannot install the
//! filter itself. This tiny e2e harness links **the real one** from
//! `crates/blastit-sys` (the same `install()` a view worker calls before
//! `php_module_startup`, `MB.3.13`) and then `execve`s the probe, which inherits
//! the filter. That is what makes `make php-sapi-e2e` able to say something the
//! INI half cannot: *this* outbound attempt was refused by the kernel, with
//! `allow_url_fopen` deliberately set to `1` in one of the runs.
//!
//! Usage: `php-sapi-seccomp-exec <program> [args...]`.

use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _program = args.next();
    let Some(program) = args.next() else {
        eprintln!("usage: php-sapi-seccomp-exec <program> [args...]");
        return ExitCode::FAILURE;
    };

    // The same role a `[route.php]` route without `egress` installs. `install`
    // refuses a process that already has a second thread, which is exactly why
    // this runs before the probe (and its PHP runtime) starts.
    let role = blastit_sys::SeccompRole::View { egress: false };
    if let Err(error) = blastit_sys::install(role) {
        eprintln!(
            "php-sapi-seccomp-exec: installing the {} filter failed: {error}",
            role.name()
        );
        return ExitCode::FAILURE;
    }

    // `exec` only returns on failure; the filter travels across `execve`, which
    // is the whole point: the probe starts already sandboxed.
    let error = Command::new(&program).args(args).exec();
    eprintln!(
        "php-sapi-seccomp-exec: exec {}: {error}",
        program.to_string_lossy()
    );
    ExitCode::FAILURE
}
