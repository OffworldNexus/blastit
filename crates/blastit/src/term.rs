//! Terminal presentation, decided once per process image (§8.1, `A-4`).
//!
//! Every image blastit starts asks the same two questions about its own
//! descriptors — is stdout a terminal, is stderr — and applies the two variables
//! an operator uses to forbid colour. This is the one place those reads happen:
//! [`crate::logs::Colour`] carries the answer from here to the emitter, the help
//! text and the refusals, and nothing downstream asks again.

use std::io::IsTerminal;

use crate::logs::Colour;

/// The colour decision for this process, read once.
///
/// `std::io::IsTerminal` is safe, and it is asked per descriptor: a redirect
/// changes one stream and not the other, so `blastit >out.txt` still has a
/// terminal for its refusals.
pub fn colour() -> Colour {
    let no_color = std::env::var("NO_COLOR").ok();
    let term = std::env::var("TERM").ok();
    Colour::new(
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
        no_color.as_deref(),
        term.as_deref(),
    )
}
