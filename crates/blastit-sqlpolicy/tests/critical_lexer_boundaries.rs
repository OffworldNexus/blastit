//! Protected critical tests for the lexer's region and numeric-token boundaries
//! (ARCHITECTURE.md §6, "Matching"; `scan.l`).
//!
//! The lexer is the one place that decides where a quoted region starts and
//! ends, and where a numeric literal ends and the next token begins. Both are
//! policy boundaries: a `;` the lexer leaves inside a quoted region can never
//! become a second statement, and a character it wrongly folds into a number
//! stops being the `$1` a grant's text was written with. `docs/testing/AUDIT-MB.1.md`
//! `F4` found this oracle — six tests pinning those boundaries and 200 KB of
//! hostile input — in the unprotected standard tier (`tests/lexer_progress.rs`,
//! still at 85.2 % killed on `lexer.rs`), where the file and the check it pins
//! could be deleted together with `tools/critical_guard.py` staying green.
//!
//! Every case is a pair: the spelling that must be read one way, and the twin
//! that must be read the other. A scanner that widens (a quote read as code, a
//! `;` read inside a string, a number that swallows a parameter) fails the
//! denied half exactly as one that narrows fails the allowed half.
//!
//! The cases below are the MB.1.3 cases moved here unchanged (none dropped),
//! plus the numeric-extent cases MB.1.14 added; the exact numeric extents, and
//! the `lex` loop, are pinned at the source level in `src/lexer.rs`.

use blastit_sqlpolicy::{
    RefuseReason, check_learnable, has_quoted_literal, is_introspection_setup_sql, normalize,
    redact_literals,
};

/// The redaction marker: one character, never a literal.
const ELLIPSIS: &str = "…";

#[test]
fn unterminated_quoting_is_one_opaque_region_to_the_end() {
    // Nothing here closes, so the whole tail is one region: a `;` inside it can
    // never become a second statement.
    for sql in [
        "SELECT 'a",
        "SELECT E'a",
        "SELECT B'a",
        "SELECT X'a",
        "SELECT N'a",
        r"SELECT U&'a",
        "SELECT $$a",
        "SELECT $tag$a",
    ] {
        assert!(has_quoted_literal(sql), "{sql:?} carries a literal");
        assert_eq!(
            redact_literals(sql),
            format!("SELECT {ELLIPSIS}"),
            "{sql:?}"
        );
        assert_eq!(normalize(sql), sql, "{sql:?} normalises to itself");
    }
    // An unterminated quoted *identifier* is not a literal, and is kept.
    assert!(!has_quoted_literal("SELECT \"a"));
    assert_eq!(redact_literals("SELECT \"a"), "SELECT \"a");

    // The `;` of a literal is data, not a statement separator: the statement is
    // refused as a literal, never counted as two.
    let verdict = check_learnable("SELECT 'a;DROP TABLE t'");
    assert_eq!(verdict.reason(), Some(&RefuseReason::InlineLiteral));

    // Unterminated comments run to the end and are dropped, and an unterminated
    // block comment that has already nested still ends.
    assert_eq!(normalize("SELECT 1 --a"), "SELECT 1");
    assert_eq!(normalize("SELECT 1 /*a"), "SELECT 1");
    assert_eq!(normalize("SELECT 1 /* a /* b */ c"), "SELECT 1");
}

#[test]
fn each_quoted_form_closes_at_its_own_boundary() {
    // `''` and `""` are escaped quotes: the region continues past them.
    assert_eq!(redact_literals("SELECT 'it''s'"), "SELECT …");
    assert_eq!(redact_literals("SELECT 'a', 'b'"), "SELECT …, …");
    assert_eq!(redact_literals(r#"SELECT "a""b""#), r#"SELECT "a""b""#);
    // `E'…'` takes a backslash escape; `B'…'` and `X'…'` take none at all, so
    // the quote after `\'` closes them and a doubled quote starts a second
    // literal (two adjacent literals, with no separator between them).
    assert_eq!(redact_literals(r"SELECT E'a\'b'"), "SELECT …");
    assert_eq!(redact_literals(r"SELECT B'a''b'"), "SELECT ……");
    assert_eq!(redact_literals(r"SELECT X'f''a'"), "SELECT ……");
    // `N'…'` and `U&'…'` escape only by doubling.
    assert_eq!(redact_literals("SELECT N'it''s'"), "SELECT …");
    assert_eq!(redact_literals(r"SELECT U&'a''b'"), "SELECT …");
    // A dollar quote closes on its own tag and on nothing else; without a match
    // it runs to the end of the text.
    assert_eq!(redact_literals("SELECT $tag$a$tag$"), "SELECT …");
    assert_eq!(redact_literals("SELECT $tag$a$other$"), "SELECT …");
    assert_eq!(redact_literals("SELECT $$a$$, $$b$$"), "SELECT …, …");
    // A tag may use the scan.l alphabet above ASCII, and a digit may not start
    // one: `$1$` is the placeholder `$1` and a lone `$`.
    assert_eq!(redact_literals("SELECT $€1_$a$€1_$"), "SELECT …");
    assert_eq!(redact_literals("SELECT $1$"), "SELECT $1$");
    assert!(!has_quoted_literal("SELECT $1$"));
    // A quote inside a comment never opens a literal.
    assert_eq!(redact_literals("SELECT '--x' -- 'y'"), "SELECT … -- 'y'");
    assert_eq!(
        redact_literals("SELECT 'x' /* 'y' */"),
        "SELECT … /* 'y' */"
    );
}

#[test]
fn placeholders_and_identifiers_split_where_scan_l_splits_them() {
    // `\${decdigit}+` ends after its last digit, whatever follows it.
    assert_eq!(normalize("SELECT $1, $23, $007"), "SELECT $?, $?, $?");
    assert_eq!(normalize("SELECT $1$"), "SELECT $?$");
    // `$1E` is the placeholder `$1` and the operator `E`, so the quote after it
    // is a plain literal, not an E-string.
    assert_eq!(normalize("SELECT $1E'a'"), "SELECT $?E'a'");
    // A `$` that is part of an identifier is data: `ident_cont` includes it, so
    // `a$1` never becomes a placeholder.
    assert_eq!(normalize("SELECT a$1, a$b$c, x$"), "SELECT a$1, a$b$c, x$");
    // ... and after a number the `$` is a fresh token, so the placeholder does
    // start there.
    assert_eq!(normalize("SELECT 1$1"), "SELECT 1$?");
    // A lone `$` is not a placeholder, and neither is `$a`.
    assert_eq!(normalize("SELECT $a, $?"), "SELECT $a, $?");
}

#[test]
fn numeric_literal_extent_decides_where_the_next_token_starts() {
    // `{hex,oct,bin}integer` = `( _? digit )+`: the literal ends after its last
    // digit, so the `$1` behind it is a fresh token — a bind parameter.
    assert_eq!(normalize("SELECT 0x1F$1"), "SELECT 0x1F$?");
    assert_eq!(normalize("SELECT 0x1_2$1"), "SELECT 0x1_2$?");
    assert_eq!(normalize("SELECT 0o17$1"), "SELECT 0o17$?");
    assert_eq!(normalize("SELECT 0b11$1"), "SELECT 0b11$?");
    // The denied twin: an identifier glued to the number is *one* token
    // (`integer_junk`), so the `$1` inside it is identifier data and stays.
    assert_eq!(normalize("SELECT 0x1Fg$1"), "SELECT 0x1Fg$1");

    // The `…fail` form keeps exactly one separator, which is what puts the `$`
    // on a token start — a second one starts an identifier instead.
    assert_eq!(normalize("SELECT 0x_$1"), "SELECT 0x_$?");
    assert_eq!(normalize("SELECT 0x__$1"), "SELECT 0x__$1");

    // `{decinteger}` = `{decdigit}(_?{decdigit})*`: one `_` between two digits
    // belongs to the number, and nothing else does.
    assert_eq!(normalize("SELECT 1_0$1"), "SELECT 1_0$?");
    assert_eq!(normalize("SELECT 1_$1"), "SELECT 1_$1");
    assert_eq!(normalize("SELECT 1__0$1"), "SELECT 1__0$1");

    // `1..2` is `1`, `..`, `2` (`numericfail`), while `1.2` is one number. The
    // `E` shows where the number ended: at a token start it opens an E-string,
    // whose `\'` escape keeps both spaces.
    assert_eq!(normalize(r"SELECT 1..E'a\'b  c'"), r"SELECT 1..E'a\'b  c'");
    assert_eq!(
        normalize(r"SELECT 0x1F.E'a\'b  c'"),
        r"SELECT 0x1F.E'a\'b  c'"
    );
    // ... and glued to the number it is `{numeric}_junk`, so the string is
    // plain and its whitespace collapses: `1.E` is one error token, exactly
    // like `1E`, `0x1E` and `.5E`.
    assert_eq!(normalize(r"SELECT 1E'a\'b  c'"), r"SELECT 1E'a\'b c'");
    assert_eq!(normalize(r"SELECT 1.E'a\'b  c'"), r"SELECT 1.E'a\'b c'");
    assert_eq!(normalize(r"SELECT 0x1E'a\'b  c'"), r"SELECT 0x1E'a\'b c'");
    assert_eq!(normalize(r"SELECT .5E'a\'b  c'"), r"SELECT .5E'a\'b c'");

    // `{real}` needs its digits: `1e+2` is one number, and the `$1` after it is
    // a parameter; a failed exponent form glues the identifier instead.
    assert_eq!(normalize("SELECT 1e+2$1"), "SELECT 1e+2$?");
    assert_eq!(normalize("SELECT 1e-2$1"), "SELECT 1e-2$?");
    assert_eq!(normalize("SELECT 12e34$1"), "SELECT 12e34$?");
    assert_eq!(normalize("SELECT 1e$1"), "SELECT 1e$1");

    // The same statement written both ways is still one normalised text: a
    // grant written from one run matches the next.
    assert_eq!(normalize("SELECT 0x1F$1"), normalize("SELECT 0x1F$9"));
    assert_ne!(normalize("SELECT 0x1F$1"), normalize("SELECT 0x1Fg$1"));
}

#[test]
fn comments_end_at_their_terminator_and_nest() {
    // scan.l's `comment` ends at `\n` or `\r`, and the terminator is part of it.
    assert_eq!(normalize("SELECT 1 --c\nFROM t"), "SELECT 1 FROM t");
    assert_eq!(normalize("SELECT 1 --c\rFROM t"), "SELECT 1 FROM t");
    assert_eq!(normalize("SELECT 1 --c\r\nFROM t"), "SELECT 1 FROM t");
    // A block comment nests, so the first `*/` only closes the inner one; an
    // unbalanced close leaves the rest as code. A comment stands for one space,
    // which is why the comma that follows keeps one in front of it.
    assert_eq!(normalize("SELECT 1 /* /* */ */, 2"), "SELECT 1 , 2");
    assert_eq!(normalize("SELECT 1 /* a */ , 2"), "SELECT 1 , 2");
    assert_eq!(normalize("SELECT 1 */ 2"), "SELECT 1 */ 2");
}

#[test]
fn a_large_hostile_text_is_scanned_in_one_pass() {
    // Each shape is one a scan that never advanced would spin on, and each is
    // large enough that a quadratic scan would not finish: an unterminated
    // literal of every quoting family, an identifier with no end, a line
    // comment and a block comment with no terminator, and a dollar tag that is
    // never repeated.
    let body = "a".repeat(200_000);
    for sql in [
        format!("SELECT '{body}"),
        format!("SELECT E'{body}"),
        format!("SELECT B'{body}"),
        format!(r#"SELECT "{body}"#),
        format!("SELECT {body}"),
        format!("SELECT $tag${body}"),
    ] {
        assert_eq!(normalize(&sql), sql, "a {}-byte statement", sql.len());
    }
    for sql in [format!("SELECT --{body}"), format!("SELECT /*{body}")] {
        assert_eq!(normalize(&sql), "SELECT");
    }
    // The literal shapes stay opaque at that size, so the statement is refused
    // for its literal rather than parsed.
    assert_eq!(
        check_learnable(&format!("SELECT '{body}'")).reason(),
        Some(&RefuseReason::InlineLiteral)
    );
    // A 200 KB numeric literal is still one token, and the `$1` behind it is
    // still a parameter: the extent is counted, never walked by a cursor.
    let digits = "9".repeat(200_000);
    assert_eq!(
        normalize(&format!("SELECT {digits}$1")),
        format!("SELECT {digits}$?")
    );
}

#[test]
fn learn_rewrites_a_placeholder_in_code_only() {
    // `parse_form` turns the `$?` a log carries back into `$1` so the statement
    // parses, and the rewrite is a token walk: a `$?` glued to an identifier is
    // that identifier's data, not a placeholder.
    assert!(is_introspection_setup_sql("SELECT to_regtype($?)"));
    assert!(is_introspection_setup_sql(
        "SELECT to_regtype($?) FROM pg_type"
    ));
    assert!(!is_introspection_setup_sql(
        "SELECT to_regtype($?); DROP TABLE t"
    ));
    assert!(!is_introspection_setup_sql(
        r#"SELECT "a$?" FROM auth_user"#
    ));

    // A `$` that opens no placeholder is left alone: the statement stays
    // unparseable, and so is never replayed, instead of having `$a` repaired
    // into a parameter.
    assert!(!is_introspection_setup_sql(
        "SELECT pg_catalog.version(), $a, $?"
    ));
    // The twin: the same statement with real placeholders is replayable.
    assert!(is_introspection_setup_sql(
        "SELECT pg_catalog.version(), $?, $?"
    ));
}
