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

/// The character offset of the first quoted literal in `sql`, for the
/// `LiteralQuery` refusal's site.
///
/// The offset is the region's **start**, prefix included: `SELECT E'x'` is `7`
/// (the `E`), `SELECT $$x$$` is `7` (the first `$`) — the character [`redact_literals`]
/// replaces, so a squiggle points at the token that carries the value rather
/// than inside it. `None` when there is no literal to point at, in which case
/// the caller falls back to the statement's own origin.
///
/// The count is in `char`s, like every other site in the manifest
/// (`error::Position`), and the name is crate-qualified: `blastit-env` has an
/// unrelated `literal_offset`.
///
/// ```
/// # use blastit_sqlpolicy::literal_offset;
/// assert_eq!(literal_offset("SELECT id FROM t"), None);
/// assert_eq!(literal_offset("SELECT E'x'"), Some(7));
/// assert_eq!(literal_offset(r#"SELECT "a" FROM t WHERE b = 'c'"#), Some(28));
/// ```
pub fn literal_offset(sql: &str) -> Option<u32> {
    let chars: Vec<char> = sql.chars().collect();
    let region = lex(&chars)
        .into_iter()
        .find(|region| region.kind.is_literal())?;
    u32::try_from(region.start).ok()
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

    #[test]
    fn the_literal_offset_is_the_region_start_in_characters() {
        // Every literal form, prefix included, is where `redact_literals`
        // writes its `…`.
        for (sql, offset) in [
            ("SELECT E'x'", 7),
            ("SELECT B'1'", 7),
            ("SELECT X'ff'", 7),
            ("SELECT N'x'", 7),
            (r"SELECT U&'\0041'", 7),
            ("SELECT $$x$$", 7),
            ("SELECT $tag$x$tag$", 7),
            ("SELECT 'x'", 7),
        ] {
            assert_eq!(literal_offset(sql), Some(offset), "for {sql:?}");
            let chars: Vec<char> = sql.chars().collect();
            let region = lex(&chars)
                .into_iter()
                .find(|region| region.kind.is_literal());
            assert_eq!(region.map(|region| region.start), Some(offset as usize));
        }
        // Nothing to point at: no literal, or only a quoted identifier and a
        // comment, which are schema and trivia rather than data.
        for sql in [
            "SELECT id FROM t",
            r#"SELECT "a" FROM "t" -- 'x'"#,
            "SELECT $1 FROM t",
        ] {
            assert_eq!(literal_offset(sql), None, "for {sql:?}");
        }
        // Characters, not bytes: `café` before the literal costs one offset per
        // character, so the byte position is one larger.
        let sql = r#"SELECT "café" AS name FROM t WHERE a = 'x'"#;
        let byte = sql.find('\'').unwrap_or_default();
        let character = sql.get(..byte).map_or(0, |head| head.chars().count());
        assert!(
            byte > character,
            "the fixture must hold a non-ASCII character"
        );
        assert_eq!(literal_offset(sql), Some(character as u32));
    }
}
