#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::process;

/// The portable binary: reads its own argv and hands it to
/// [`blastit_lsp::run_binary`] (`MB.42.12`) — with no argument that is the
/// session `blastit lsp` runs too.
///
/// `std::env` and `std::process` are named in this file and nowhere in the
/// library, which is the text tool an editor's input reaches: the library's
/// critical suite refuses an environment or process module in it (R3).
fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    process::exit(i32::from(blastit_lsp::run_binary(&args)));
}
