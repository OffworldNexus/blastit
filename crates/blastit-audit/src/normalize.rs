//! The SQL text rules a grant is compared with (`ARCHITECTURE.md` §6).
//!
//! These are [`blastit_sqlpolicy`]'s own functions, re-exported rather than
//! re-implemented: `fold.py`, the pytest plugin and `blastit check` must see
//! exactly the text the policy compares, and a second lexer is the kind of
//! copy that drifts from the first (`docs/knowledge/LESSONS.md`).
//!
//! * [`normalize`] — trim, drop a trailing `;`, collapse comments and
//!   whitespace, and rewrite every placeholder to `$?`, so a grant fits any
//!   page size;
//! * [`has_quoted_literal`] — whether a text holds a `'…'`, `E'…'`, `B'…'`,
//!   `X'…'`, `N'…'`, `U&'…'`, `$$…$$` or `$tag$…$tag$` span (the backslash-aware
//!   spelling of `MB.2.3.1`);
//! * [`redact_literals`] — the same text with every such span replaced by `…`,
//!   which is what a report may carry.

pub use blastit_sqlpolicy::{has_quoted_literal, normalize, redact_literals};
