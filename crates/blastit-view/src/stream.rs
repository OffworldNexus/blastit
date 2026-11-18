//! A worker's two output streams, and the cap on one line of them.
//!
//! `wsgi.errors` and PHP's own `STDOUT`/`STDERR` are diagnostics: they reach
//! the log through the runtime worker's output sink, never the client's response,
//! and the cap is what keeps one `echo` from burying a log line.

/// Which of the worker's two output streams a line came from.
///
/// `wsgi.errors`, and PHP's own `STDOUT`/`STDERR` outside the response, are
/// diagnostics: they are never part of the client's response, and a runtime may
/// interleave them with one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    /// The runtime's standard output.
    Stdout,
    /// The runtime's standard error: tracebacks, Zend notices, `wsgi.errors`.
    Stderr,
}

/// The most bytes of one output line that a view worker keeps.
///
/// An application can print a megabyte on one line; the runtime hands it over
/// in any number of calls, and the sink keeps this prefix and drops the rest
/// rather than let one `echo` bury the log it lands in.
pub const OUTPUT_LINE_CAP: usize = 2048;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_line_cap_is_the_documented_2048() {
        // §4.1 documents `output`'s line cap as 2048 bytes; the WSGI sink
        // `MB.3.24` builds and the PHP sink `MB.3.13` builds must not drift.
        assert_eq!(OUTPUT_LINE_CAP, 2048);
    }
}
