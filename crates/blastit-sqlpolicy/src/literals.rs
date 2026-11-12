//! Literal detection and redaction (R24, ADR-0003).
//!
//! A statement can carry a value its author never meant to publish: a password
//! typed into a form, an e-mail address, a session token. `sql.denied` events,
//! the internal `obs_fd` records `--learn` writes and the grants it proposes all
//! have to be safe to paste into a ticket or commit to a repository, so every
//! quoted literal is replaced by `…` before the text leaves the process.
//!
//! The replacement is the **whole** token, prefix included: `SELECT E'\n'`
//! becomes `SELECT …`, not `SELECT E…`, because a half-literal is neither a
//! statement nor a shape a grant author can read. Quoted identifiers are not
//! literals — `"shop_product"."id"` is schema, not data — and a bind
//! placeholder (`$1`, `$?`) is not a literal either: it is exactly what the
//! grant has to keep.
//!
//! Two fixed statements blastit sends itself are exempt. They are constants of
//! the protocol, not values an application supplied, and a grant has to be able
//! to spell them:
//!
//! ```
//! # use blastit_sqlpolicy::{redact_literals, has_quoted_literal};
//! assert_eq!(redact_literals(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#),
//!            r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#);
//! assert_eq!(redact_literals("SELECT E'\\n'"), "SELECT …");
//! assert_eq!(redact_literals("SELECT B'1'"), "SELECT …");
//! assert_eq!(redact_literals(r"SELECT U&'\0041'"), "SELECT …");
//! assert_eq!(redact_literals("SELECT $t$x$t$"), "SELECT …");
//! assert_eq!(redact_literals("SET TIME ZONE 'UTC'"), "SET TIME ZONE 'UTC'");
//! assert!(has_quoted_literal("SELECT 'x'"));
//! assert!(!has_quoted_literal(r#"SELECT "x""#));
//! ```

use crate::lexer::lex;

/// The fixed protocol setup statements blastit itself sends.
///
/// They are normalised exactly as [`crate::normalize`] writes them, and they may
/// keep their literals: a grant has to be able to spell
/// `SET TIME ZONE 'UTC'`, and the timezone name is a constant of the protocol,
/// not a value an application supplied (ADR-0003).
pub const KNOWN_STATIC_SETUP_SQL: &[&str] = &[
    "SET TIME ZONE 'UTC'",
    "SELECT set_config('TimeZone', $?, false)",
];

/// Whether `sql` contains a quoted literal.
///
/// Every literal form PostgreSQL 18 accepts counts: `'…'`, `E'…'`, `B'…'`,
/// `X'…'`, `N'…'`, `U&'…'`, `$$…$$` and `$tag$…$tag$`. Quoted identifiers
/// (`"…"`, `U&"…"`) do not: they name schema, not data. A quote inside a
/// comment is not a literal either, because the lexer never hands a comment's
/// contents over as anything but a comment.
///
/// ```
/// # use blastit_sqlpolicy::has_quoted_literal;
/// assert!(has_quoted_literal("SELECT N'x'"));
/// assert!(has_quoted_literal(r"SELECT U&'\0041'"));
/// assert!(!has_quoted_literal("SELECT \"x\" -- 'y'"));
/// ```
pub fn has_quoted_literal(sql: &str) -> bool {
    let chars: Vec<char> = sql.chars().collect();
    lex(&chars).iter().any(|region| region.kind.is_literal())
}

/// Replaces every quoted literal in `sql` with `…`, prefix included.
///
/// Code, comments and quoted identifiers are copied verbatim, so the result is
/// still the statement's shape: a grant author can read it, and a caller can
/// compare it with what a grant holds. An exact match of
/// [`KNOWN_STATIC_SETUP_SQL`] is returned unchanged, because those two
/// statements are protocol constants rather than user data.
///
/// The pass is idempotent — `…` is not a literal, so replacing again changes
/// nothing — and it never copies a byte from inside a literal into the result,
/// so no user-derived value can survive it.
///
/// ```
/// # use blastit_sqlpolicy::redact_literals;
/// assert_eq!(redact_literals("SELECT 'a', \"b\" FROM t"), "SELECT …, \"b\" FROM t");
/// assert_eq!(redact_literals("SELECT 'a'"), redact_literals(&redact_literals("SELECT 'a'")));
/// ```
pub fn redact_literals(sql: &str) -> String {
    if KNOWN_STATIC_SETUP_SQL.contains(&sql) {
        return sql.to_string();
    }
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    for region in lex(&chars) {
        if region.kind.is_literal() {
            out.push('…');
        } else {
            out.extend(&chars[region.start..region.end]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_literal_form_redacts_to_one_ellipsis() {
        for sql in [
            "SELECT 'x'",
            "SELECT E'x'",
            "SELECT B'1'",
            "SELECT X'ff'",
            "SELECT N'x'",
            r"SELECT U&'\0041'",
            "SELECT $$x$$",
            "SELECT $tag$x$tag$",
        ] {
            assert_eq!(redact_literals(sql), "SELECT …", "for {sql:?}");
        }
    }

    #[test]
    fn identifiers_placeholders_and_comments_survive() {
        for sql in [
            r#"SELECT "a""b" FROM "t""#,
            r#"SELECT U&"ident" FROM "t""#,
            "SELECT $1, $?",
            "SELECT 1 -- 'x'",
            "SELECT 1 /* 'x' */",
        ] {
            assert_eq!(redact_literals(sql), sql, "for {sql:?}");
            assert!(!has_quoted_literal(sql), "for {sql:?}");
        }
    }

    #[test]
    fn known_static_setup_sql_is_kept_whole() {
        for sql in KNOWN_STATIC_SETUP_SQL {
            assert_eq!(redact_literals(sql), *sql);
            assert!(has_quoted_literal(sql), "{sql:?} still holds its literal");
        }
    }

    #[test]
    fn redaction_is_idempotent_and_leaks_nothing() {
        for sql in [
            "SELECT 's3cr3t' FROM t WHERE id = $1",
            r"SELECT E'a\'s3cr3t'",
            "SELECT N's3cr3t', \"col\"",
            r"SELECT U&'\0073\0063\0072\0074'",
            "SELECT $t$s3cr3t$t$",
            "SELECT 'a', 'b', 'c'",
        ] {
            let once = redact_literals(sql);
            assert_eq!(redact_literals(&once), once, "not idempotent for {sql:?}");
            assert!(
                !once.contains("s3cr3t"),
                "leaked a literal byte in {once:?}"
            );
            assert!(!once.contains("\\0073"), "leaked an escape in {once:?}");
        }
    }
}
