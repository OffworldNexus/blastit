//! What a validated command line actually runs (`R2`, `R3`, `R19`, `MB.22.3`).
//!
//! Each entry point is one delegation: the grammar decided *what*; these hand
//! the options to the stage, engine or session that owns the work, and return
//! the exit status a caller (the binary, or the wheel's console script) exits
//! with.

use crate::host::Host;
use crate::logs::Colour;

use super::{CheckOptions, DevOptions, FMT_FAILURE, FMT_NOT_CANONICAL, FmtOptions, SUCCESS};

/// The `blastit dev` supervisor's entry (`MA.2.3.3`).
///
/// The grammar above decided *what* to run; this hands the options to stage 0's
/// unprivileged mode, which never returns: it either `execve`s stage 1 or exits
/// non-zero. `host` is the installation, exactly as it is for production.
pub(super) fn run_dev(host: &Host, options: &DevOptions, interpreter: bool, colour: Colour) -> u8 {
    crate::stage0::run_dev(host, options, interpreter, colour)
}

/// The `blastit check` engine (`MA.2.3.5`).
///
/// The grammar above decided *what* to audit; this hands the options to
/// [`crate::check::run`], which owns the transaction: validate, start dbproxy,
/// run pytest, audit, and — under `--learn` only — write (`R3..R5`, `R30`).
pub(super) fn run_check(host: &Host, options: &CheckOptions, colour: Colour) -> u8 {
    crate::check::run(host, options, colour)
}

// The `blastit lsp` session lives in `cli.rs`, beside the dispatcher: a protected
// test reads that file and asserts the capability check precedes the session
// (`MB.22.3`, `critical_lsp.rs`).

/// The `blastit fmt` command (§14.4).
///
/// Discovery is [`crate::config::manifest_path`]'s when no `PATH` names a file:
/// the same function the boot, `dev` and `check` call, so `fmt` can never format
/// a different file than the one those three read (design §15.1). A `PATH`
/// argument wins over both, because it is the caller naming the file to format.
///
/// `--check` reads, formats in memory and writes nothing at all — not even a
/// temporary file — and prints `<path>: not in canonical layout` followed by a
/// unified diff of what would change (`--- <path>`, `+++ <path>`, `@@` hunks)
/// on stdout. Without `--check` the rewrite goes through
/// [`blastit_manifest::edit::write_atomic`], so the file keeps its mode and a
/// failed write leaves the original in place.
pub(super) fn run_fmt(options: &FmtOptions) -> u8 {
    let path = match options.path.clone().map_or_else(
        || crate::config::manifest_path().map_err(|error| error.to_string()),
        Ok,
    ) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            return FMT_FAILURE;
        }
    };
    let text = match blastit_manifest::blastfile::read(&path) {
        Ok(text) => text,
        Err(errors) => {
            // §15.1: a file that is not there is the one missing-file sentence,
            // produced exactly once in `config::missing_manifest` — the same
            // words `blastit`, `dev` and `check` use. `read`'s own wording for
            // an unreadable file stays for every other refusal.
            if std::fs::metadata(&path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                eprintln!("{}", crate::config::missing_manifest(&path));
                return FMT_FAILURE;
            }
            return report_manifest_errors(&path, &errors);
        }
    };
    let formatted = match blastit_manifest::blastfile::fmt(&text) {
        Ok(formatted) => formatted,
        Err(errors) => return report_manifest_errors(&path, &errors),
    };
    if formatted == text {
        return SUCCESS;
    }
    if options.check {
        // `AGENTS.md` §4: a developer who is told the file is not canonical is
        // shown the change, not just the verdict — the same text an editor or
        // `patch` reads. Nothing is written, not even a temporary file.
        let shown = crate::config::display_path(&path);
        println!("{shown}: not in canonical layout");
        print!("{}", crate::unified::unified(&shown, &text, &formatted));
        return FMT_NOT_CANONICAL;
    }
    match blastit_manifest::edit::write_atomic(&path, &formatted) {
        Ok(()) => SUCCESS,
        Err(error) => {
            eprintln!(
                "{}: the file could not be written: {}",
                crate::config::display_path(&path),
                blastit_manifest::clipped(&error.to_string())
            );
            FMT_FAILURE
        }
    }
}

/// Prints one line per refusal, each prefixed with the manifest path exactly
/// once (§18), and returns `fmt`'s failure status.
///
/// The position comes from [`blastit_manifest::ManifestError::position`], not
/// from the rendered line: a positioned refusal renders `<line>:<col>: <message>`,
/// so the path goes straight before it — `./Blastfile:2:10: …`; a position-less
/// one takes the `<path>: <message>` spelling.
fn report_manifest_errors(path: &std::path::Path, errors: &blastit_manifest::ManifestErrors) -> u8 {
    let path = crate::config::display_path(path);
    for error in errors.errors() {
        let rendered = error.to_string();
        if error.position().is_none() {
            eprintln!("{path}: {rendered}");
        } else {
            eprintln!("{path}:{rendered}");
        }
    }
    FMT_FAILURE
}

/// Whether an install that runs inside an interpreter may play the role (§2.2).
///
/// dbproxy holds the real credentials and must run no interpreter at all (§4),
/// so a wheel install — where blastit *is* a Python process — hands the role to
/// the Python-free `blastit-dbproxy` binary the wheel ships instead of playing
/// it in the process that imported the extension.
///
/// # Errors
///
/// A one-line message when the caller is in an interpreter and asked for
/// dbproxy.
pub(super) fn role_is_allowed(role: &str, hosted: bool) -> Result<(), String> {
    if hosted && role == "dbproxy" {
        return Err(
            "--role=dbproxy runs no interpreter; use the `blastit-dbproxy` binary this \
             installation ships"
                .to_string(),
        );
    }
    Ok(())
}
