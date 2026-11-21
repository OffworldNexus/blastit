//! The command line's help text, and the one shape a refusal takes (`A-8`, `A-14`).
//!
//! Every byte a person reads about the command line lives here: the five usage
//! blocks, `blastit help [COMMAND]`, and the refusal that carries them. The
//! colour of each is the caller's: the help text goes to stdout and takes
//! stdout's answer, a refusal goes to stderr and takes stderr's — a
//! `blastit >out.txt` run has a terminal on the other one.

use crate::logs::Colour;

use super::{FAILURE, USAGE_FAILURE};

/// A refused stage-0 command line, and the exit status the refusal deserves.
///
/// One mistake in argv is one exit status, but the status is not always the
/// same: what an operator typed deserves `2` (`A-12`), while a malformed
/// invocation of the re-exec grammar — `--stage=1 …`, which stage 0 writes
/// itself — keeps the `1` the protected CLI matrix pins
/// (`critical_config_and_presets.rs:466-470`).
#[derive(Debug)]
pub(super) struct Refusal {
    /// The sentence a person reads, naming the argument and the fix.
    pub(super) message: String,
    /// What `main` exits with.
    pub(super) exit: u8,
}

impl Refusal {
    /// A mistake in what a person typed: `2`, with the fix in the sentence.
    pub(super) fn usage(message: String) -> Self {
        Self {
            message,
            exit: USAGE_FAILURE,
        }
    }

    /// A malformed internal invocation: `1`, like every other boot refusal.
    pub(super) fn internal(message: String) -> Self {
        Self {
            message,
            exit: FAILURE,
        }
    }
}

/// How to use the program, printed with `--help` and a command-line error.
///
/// The presets are listed with their one-line descriptions: `--preset` is the
/// one argument whose valid values an operator cannot guess (E1).
///
/// `coloured` is the answer for **the stream this text is about to be written
/// to**, never a fixed one: [`refuse`] passes stderr's, `--help` passes stdout's.
pub(super) fn usage(coloured: bool) -> String {
    let (bold, cyan, reset) = if coloured {
        ("\u{1b}[1m", "\u{1b}[36m", "\u{1b}[0m")
    } else {
        ("", "", "")
    };
    format!(
        "{bold}{cyan}usage:{reset}\n{USAGE}\n\n\
         {bold}{cyan}presets{reset} (`--preset NAME`, or `BLASTIT_PRESET=NAME`; \
         every BLASTIT_* variable that is set wins over one):\n{}",
        crate::config::preset_help()
    )
}

/// The commands and their arguments; [`usage`] adds the header and the presets.
///
/// The re-exec grammar stage 0 uses for stage 1 and `--role=dbproxy` is
/// deliberately absent: it is plumbing between two images of this same program,
/// and listing it beside `dev` sold an operator a flag they must never type
/// (A-8). Both are still parsed — [`super::parse::parse`] accepts every one of
/// their tokens.
const USAGE: &str = "  blastit [--preset NAME]                    production: stage 0, the real environment\n  blastit dev     [--port N] [--host ADDR] [--learn] [--no-reload]\n                                             unprivileged single-UID simulation (no sudo)\n  blastit check   [--json] [--learn [--dry-run]] [-- ARGS...]\n                                             audit a pytest suite (alias: test)\n  blastit fmt     [--check] [PATH]           rewrite a Blastfile in the canonical layout\n  blastit lsp                                speak LSP on stdin/stdout (editors)\n  blastit help [dev|check|fmt|lsp]           print one command's help\n\n  -h, --help      print this help\n  -V, --version   print the version";

/// The text `blastit help [COMMAND]` prints (`A-14`).
///
/// `help` is the word a person types first, and answering it with
/// "``help`` is not ``--key=value``" taught them nothing. `docs/reference/cli.md`
/// spells every command here, so the two can never drift apart.
///
/// `blastit` alone is the same text as no argument; `serve` is not a command
/// this build has, so `blastit help serve` is refused exactly as
/// `blastit serve` is.
///
/// # Errors
///
/// A one-line message naming an unknown command, and the fix.
pub(super) fn help_for(command: Option<&str>, coloured: bool) -> Result<String, String> {
    match command {
        None | Some("blastit") => Ok(usage(coloured)),
        Some("dev") => Ok(DEV_USAGE.to_string()),
        Some("check" | "test") => Ok(CHECK_USAGE.to_string()),
        Some("fmt") => Ok(fmt_usage()),
        Some("lsp") => Ok(LSP_USAGE.to_string()),
        Some(other) => Err(format!(
            "unknown subcommand '{other}' — run 'blastit --help'"
        )),
    }
}

/// `blastit dev --help` (§1.1).
pub(super) const DEV_USAGE: &str = "\
usage: blastit dev [--port N] [--host ADDR] [--learn] [--no-reload]

  --port N      port to bind, 1..=65535 (default 8080, or PORT when set)
  --host ADDR   IP address to bind (default 127.0.0.1); a non-loopback
                address needs BLASTIT_DEV_EXPOSE=1
  --learn       observe requests and SQL and propose manifest additions
  --no-reload   turn live reload off: nothing is watched, and no
                `dev.reload.unavailable` warning is printed
  -h, --help    print this help

`blastit dev` runs the real five-role tree as the current uid: no sudo, no
capabilities, not a sandbox. Use `blastit` for production.";

/// `blastit check --help` (§1.1).
pub(super) const CHECK_USAGE: &str = "\
usage: blastit check [--json] [--learn [--dry-run]] [-- ARGS...]
       blastit test  ...                    exact alias of `check`

  --json        write at most one JSON document to stdout; everything else,
                including pytest output, goes to stderr
  --learn       observe requests and SQL and propose manifest additions
  --dry-run     with --learn, report the proposal without writing it
  -- ARGS...    passed to `pytest -q` verbatim
  -h, --help    print this help

Exit codes: 0 clean, 1 completeness findings, 2 CLI/config error, 3 pytest
failed (including exit 5, no tests collected).";

/// `blastit fmt --help` (§14.4).
///
/// `{env}` is [`crate::config::MANIFEST_ENV`], filled in by [`fmt_usage`] so the help
/// text and the variable a boot reads can never spell it differently.
const FMT_USAGE: &str = "\
usage: blastit fmt [--check] [PATH]

  --check       change nothing; name every file that is not in canonical
                layout and exit 1
  PATH          the file to format (default ./Blastfile, or {env})
  -h, --help    print this help

`blastit fmt` rewrites the file in the canonical layout: one directive per line,
four spaces per level, one blank line between sections. Comments are kept, and
without --check the rewrite is atomic and keeps the file's mode.

Exit codes: 0 clean or rewritten, 1 --check found a file that is not canonical,
2 the file could not be read, parsed or written.";

/// [`FMT_USAGE`] with the manifest variable named from its one spelling.
pub(super) fn fmt_usage() -> String {
    FMT_USAGE.replace("{env}", crate::config::MANIFEST_ENV)
}

/// `blastit lsp --help` (`docs/reference/cli.md` `## blastit lsp`).
pub(super) const LSP_USAGE: &str = "\
usage: blastit lsp

Reads LSP messages on stdin and answers on stdout, for an editor to spawn:
`blastit lsp` takes no argument and no file, reads no `BLASTIT_*` variable,
opens no path and holds no privilege. Diagnostics, formatting and hover go to
the editor, not to a terminal, so there is nothing else to print.

  -h, --help    print this help

Exit codes: 0 after the client said `shutdown`, 1 if it disappeared without
one, 2 a command-line error.";

/// Prints one command-line refusal on stderr, with the usage when it has one.
///
/// The sentence — and the usage block under it — is coloured by **stderr**'s
/// answer and never by stdout's: `blastit --help >help.txt` still deserves a
/// coloured refusal on its terminal, and `blastit 2>err.txt` must not get
/// escape codes in the file (`A-4`). That is why the usage text is rendered
/// here instead of being passed in already coloured.
pub(super) fn refuse(prefix: &str, message: &str, with_usage: bool, colour: Colour) {
    let (red, reset) = if colour.stderr {
        ("\u{1b}[31m", "\u{1b}[0m")
    } else {
        ("", "")
    };
    match with_usage {
        true => eprintln!("{red}{prefix}: {message}{reset}\n{}", usage(colour.stderr)),
        false => eprintln!("{red}{prefix}: {message}{reset}"),
    }
}
