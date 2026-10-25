//! SQL normalisation and allowlist matching for `postgresql` grants
//! (ARCHITECTURE.md §5.2, §6).
//!
//! blastit does not parse SQL. A `postgresql` grant (§5.2) lists the statement
//! texts the app may send, and dbproxy compares them **exactly**, after
//! normalising both the manifest's text and the text on the wire with the same
//! rules (§6, "Matching"): trim, drop trailing `;`, replace every comment by a
//! single space, and collapse every run of whitespace outside quoted strings to
//! one space. Matching the whole normalised text is what makes a listed query
//! followed by a second statement fail to match.
//!
//! # Agreement with PostgreSQL
//!
//! Normalisation follows PostgreSQL 18's `src/backend/parser/scan.l` for every
//! boundary a policy check depends on: which characters are whitespace
//! (`space` = `[ \t\n\r\f\v]`, and nothing else), where a line comment ends
//! (`\n` or `\r`), where a quoted region starts and ends (`''`, E-strings,
//! `B'…'`/`X'…'`, `"…"`, `$tag$…$tag$`), and which `$` opens a dollar quote
//! (`a$b$c` is one identifier). See the `lexer` module for the two deliberate
//! divergences, neither of which can hide code or quoted data.
//!
//! **`standard_conforming_strings = on` is assumed**, which is PostgreSQL's
//! default and what dbproxy must force on every upstream connection (as a
//! startup parameter, or `SET standard_conforming_strings = on`). With it on, a
//! `'…'` literal has no backslash escapes: `'a\'` ends at the second quote,
//! which is how this crate reads it. With it off, PostgreSQL would read the
//! same bytes as an escape string ending later, and a statement could contain
//! text this crate thought was outside the literal.
//!
//! A grant with `transaction_control = true` additionally allows the
//! transaction-control statements (and only those) when the text holds exactly
//! one statement: `BEGIN`, `START TRANSACTION`, `COMMIT`, `END`, `ROLLBACK`,
//! `SAVEPOINT <ident>`, `RELEASE [SAVEPOINT] <ident>` and
//! `ROLLBACK [WORK|TRANSACTION] TO [SAVEPOINT] <ident>`.
//!
//! ```
//! use blastit_sqlpolicy::{Allowlist, Decision};
//!
//! let allow = Allowlist::new(["SELECT 1"], true);
//! assert_eq!(
//!     allow.check("  select  1 ; "),
//!     Decision::Denied { normalized: "select 1".to_string() },
//! );
//! assert_eq!(allow.check("SELECT\n  1;"), Decision::Allowed);
//! assert_eq!(allow.check("BEGIN"), Decision::Allowed);
//! assert_eq!(
//!     allow.check("BEGIN; DROP TABLE t"),
//!     Decision::Denied { normalized: "BEGIN; DROP TABLE t".to_string() },
//! );
//! ```

mod lexer;
mod normalize;
mod statements;

pub use normalize::normalize;

use std::collections::HashSet;

use statements::is_transaction_control;

/// What a [`Allowlist`] decided about one SQL text.
///
/// Not exhaustive: more decision kinds (and more detail in `Denied`) may be
/// added without a breaking change.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The text is allowed.
    Allowed,
    /// The text is not allowed.
    Denied {
        /// The normalised text, ready to log (`sql.denied`, §8.1) and to extend
        /// the manifest with.
        normalized: String,
    },
}

impl Decision {
    /// Whether the text was allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allowed)
    }
}

/// The SQL one `postgresql` grant allows on one (route, proxy) pair.
///
/// Built from the effective queries of a route (§6, "Effective grants") plus
/// the route's `transaction_control` flag.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    queries: HashSet<String>,
    transaction_control: bool,
}

impl Allowlist {
    /// Builds an allowlist from a grant's queries and its `transaction_control`
    /// flag.
    ///
    /// Every query is stored in its normalised form (see [`normalize`]), so
    /// manifest formatting differences are irrelevant when checking. Queries
    /// that normalise to the empty text are ignored: no text is ever allowed
    /// just because the manifest contained an empty string.
    pub fn new<S: AsRef<str>>(
        queries: impl IntoIterator<Item = S>,
        transaction_control: bool,
    ) -> Self {
        let mut set = HashSet::new();
        for query in queries {
            let query = normalize(query.as_ref());
            if !query.is_empty() {
                set.insert(query);
            }
        }
        Self {
            queries: set,
            transaction_control,
        }
    }

    /// The distinct normalised queries the allowlist holds, in no particular
    /// order.
    pub fn queries(&self) -> impl Iterator<Item = &str> {
        self.queries.iter().map(String::as_str)
    }

    /// Decides whether `sql` is allowed.
    ///
    /// The text is allowed when its normalised form is exactly one of the
    /// allowlisted queries, or — with `transaction_control` — when it is a
    /// single transaction-control statement.
    ///
    /// Empty or whitespace-only text is always denied.
    pub fn check(&self, sql: &str) -> Decision {
        let normalized = normalize(sql);
        if normalized.is_empty() {
            return Decision::Denied { normalized };
        }
        if self.queries.contains(&normalized) {
            return Decision::Allowed;
        }
        // The grammar wants the text normalised exactly once, as here.
        if self.transaction_control && is_transaction_control(&normalized) {
            return Decision::Allowed;
        }
        Decision::Denied { normalized }
    }

    /// Whether this allowlist also accepts single transaction-control
    /// statements.
    pub fn transaction_control(&self) -> bool {
        self.transaction_control
    }

    /// How many **distinct normalised** queries the allowlist holds: two
    /// manifest entries that differ only in formatting count once.
    pub fn len(&self) -> usize {
        self.queries.len()
    }

    /// Whether the allowlist holds no query at all. It can then only allow
    /// transaction-control statements, if those are enabled.
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ARCHITECTURE.md §6, `[group.catalog.db]`.
    const CATALOG_QUERY: &str = r#"SELECT "shop_product"."id", "shop_product"."name" FROM "shop_product" WHERE "shop_product"."id" = $1 LIMIT 21"#;
    /// ARCHITECTURE.md §6, `[group.session.db]`.
    const SESSION_QUERY: &str = r#"SELECT "django_session"."session_key", "django_session"."session_data", "django_session"."expire_date" FROM "django_session" WHERE ("django_session"."expire_date" > $1 AND "django_session"."session_key" = $2) LIMIT 21"#;
    /// ARCHITECTURE.md §6, inline grant of the `checkout` route.
    const ORDER_INSERT: &str = r#"INSERT INTO "shop_order" ("product_id", "qty") VALUES ($1, $2) RETURNING "shop_order"."id""#;

    fn allowlist(queries: &[&str], transaction_control: bool) -> Allowlist {
        Allowlist::new(queries.iter().copied(), transaction_control)
    }

    fn denied_reason(decision: Decision) -> String {
        match decision {
            Decision::Allowed => panic!("expected a denial"),
            Decision::Denied { normalized } => normalized,
        }
    }

    #[test]
    fn forbids_a_query_by_default() {
        let allow = allowlist(&[CATALOG_QUERY], false);
        assert_eq!(allow.check(CATALOG_QUERY), Decision::Allowed);
        assert_eq!(
            allow.check("DROP TABLE shop_product"),
            Decision::Denied {
                normalized: "DROP TABLE shop_product".to_string()
            }
        );
        assert_eq!(allow.len(), 1);
        assert!(!allow.is_empty());
        assert!(!allow.transaction_control());
    }

    #[test]
    fn allowlists_the_queries_of_an_empty_grant() {
        let allow = allowlist(&[], false);
        assert!(allow.is_empty());
        assert_eq!(allow.len(), 0);
        assert!(!allow.check("SELECT 1").is_allowed());
    }

    #[test]
    fn ignores_empty_queries_in_a_grant() {
        let allow = allowlist(&["", "   ", ";", "\n"], false);
        assert!(allow.is_empty());
        for sql in ["", " ", ";", "\t\n"] {
            assert!(!allow.check(sql).is_allowed(), "should deny {sql:?}");
        }
    }

    #[test]
    fn django_queries_match_their_reformatted_selves() {
        // The same statements, laid out with a different mix of whitespace and
        // a trailing separator, as an ORM or a hand-edited manifest would.
        let reformatted_catalog = "\n\tSELECT\n\t\t\"shop_product\".\"id\",\n\t\t\"shop_product\".\"name\"\n\tFROM \"shop_product\"\n\tWHERE \"shop_product\".\"id\" = $1\n\tLIMIT 21\n;\n";
        let allow = allowlist(&[CATALOG_QUERY], false);
        assert_eq!(allow.check(reformatted_catalog), Decision::Allowed);

        let reformatted_session = SESSION_QUERY
            .replace(", ", ",\n\t")
            .replace(" FROM ", "\nFROM  ");
        assert!(reformatted_session.contains('\n'), "test fixture is broken");
        let allow = allowlist(&[SESSION_QUERY], true);
        assert_eq!(allow.check(&reformatted_session), Decision::Allowed);
        assert_eq!(
            allow.check(&format!("{SESSION_QUERY}\n;\n")),
            Decision::Allowed
        );
        assert_eq!(
            allow.check(ORDER_INSERT),
            Decision::Denied {
                normalized: normalize(ORDER_INSERT),
            }
        );

        let allow = allowlist(&["SET TIME ZONE 'UTC'"], false);
        assert_eq!(
            allow.check("  SET   TIME\n  ZONE   'UTC'  ;"),
            Decision::Allowed
        );
    }

    #[test]
    fn matching_is_case_sensitive_on_the_query_text() {
        let allow = allowlist(&[CATALOG_QUERY, "SELECT 1"], true);
        assert!(!allow.check("select 1").is_allowed());
        assert!(allow.check("SELECT 1").is_allowed());
        assert!(!allow.check("set time zone 'utc'").is_allowed());
        let allow = allowlist(&["SET TIME ZONE 'UTC'"], false);
        assert!(!allow.check("SET TIME ZONE 'utc'").is_allowed());
    }

    #[test]
    fn keywords_of_transaction_statements_are_case_insensitive() {
        let allow = allowlist(&[CATALOG_QUERY], true);
        for sql in [
            "begin",
            "BeGiN work",
            "start transaction",
            "commit",
            "end",
            "rollback",
            "savepoint s1",
            "release savepoint \"s1_x\"",
        ] {
            assert!(allow.check(sql).is_allowed(), "should allow {sql:?}");
        }
    }

    #[test]
    fn empty_text_is_always_denied() {
        for transaction_control in [false, true] {
            let allow = allowlist(&["SELECT 1"], transaction_control);
            for sql in ["", " ", "\t", "\n  \n", ";", " ; ; "] {
                let decision = allow.check(sql);
                assert!(!decision.is_allowed(), "should deny {sql:?}");
                assert_eq!(denied_reason(decision), "");
            }
        }
    }

    #[test]
    fn transaction_control_is_optional() {
        let allow = allowlist(&[CATALOG_QUERY], false);
        for sql in [
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT s1",
            "START TRANSACTION",
        ] {
            let decision = allow.check(sql);
            assert!(!decision.is_allowed(), "should deny {sql:?}");
            assert_eq!(denied_reason(decision), normalize(sql));
        }
        // ... and a listed transaction statement is allowed even without the
        // flag, because matching is exact.
        let allow = allowlist(&["COMMIT"], false);
        assert_eq!(allow.check("COMMIT;"), Decision::Allowed);
    }

    #[test]
    fn transaction_control_accepts_the_granted_statements() {
        let allow = allowlist(&[CATALOG_QUERY], true);
        for sql in [
            "BEGIN",
            "BEGIN;",
            "  begin  ",
            "BEGIN WORK",
            "BEGIN TRANSACTION",
            "START TRANSACTION",
            "START TRANSACTION READ WRITE",
            "BEGIN ISOLATION LEVEL SERIALIZABLE",
            "BEGIN ISOLATION LEVEL READ COMMITTED, READ ONLY",
            "COMMIT",
            "COMMIT WORK",
            "COMMIT; -- a comment",
            "END",
            "END WORK",
            "END TRANSACTION",
            "ROLLBACK",
            "ROLLBACK WORK",
            "SAVEPOINT s1",
            "SAVEPOINT s123_x1",
            "SAVEPOINT \"s1_x\"",
            "SAVEPOINT \"s1_x\";",
            "RELEASE s1",
            "RELEASE SAVEPOINT s1",
            "RELEASE savepoint \"s1_x\"",
            "ROLLBACK TO s1",
            "ROLLBACK TO SAVEPOINT s1",
            "ROLLBACK WORK TO SAVEPOINT s1",
            "ROLLBACK TRANSACTION TO s1",
            "BEGIN; -- x",
            "BEGIN /* c */",
            "begin  /* c */  ;  ",
        ] {
            assert!(allow.check(sql).is_allowed(), "should allow {sql:?}");
        }
    }

    #[test]
    fn transaction_control_rejects_everything_else() {
        let allow = allowlist(&[CATALOG_QUERY], true);
        for sql in [
            "DROP TABLE t",
            "SELECT 1",
            "SET TIME ZONE 'UTC'",
            "BEGIN READ ONLY EXTRA",
            "BEGIN ISOLATION LEVEL BOGUS",
            "SAVEPOINT",
            "SAVEPOINT 1",
            "RELEASE",
            "ROLLBACK TO",
            "ROLLBACK TO 1",
            "COMMIT WORK EXTRA",
            "END WORK EXTRA",
        ] {
            let decision = allow.check(sql);
            assert!(!decision.is_allowed(), "should deny {sql:?}");
            assert_eq!(denied_reason(decision), normalize(sql));
        }
    }

    #[test]
    fn multi_statement_texts_are_never_allowed_by_transaction_control() {
        let allow = allowlist(&[CATALOG_QUERY], true);
        for sql in [
            "BEGIN; DROP TABLE t",
            "BEGIN; COMMIT",
            "COMMIT; BEGIN",
            "START TRANSACTION; SELECT 1",
            "SAVEPOINT s1; SELECT 1",
            "ROLLBACK; ROLLBACK",
            "BEGIN; SELECT 1 -- ; and a comment",
            "SELECT 1; BEGIN",
        ] {
            assert!(!allow.check(sql).is_allowed(), "should deny {sql:?}");
        }
    }

    #[test]
    fn a_comment_never_hides_a_second_statement() {
        // Reproduction from review: the `--` comment is replaced by a single
        // space, not collapsed together with the newline that ends it, so the
        // `;` on the next line is real code and the stored query no longer
        // matches.
        let allow = allowlist(&["SELECT 1 -- ; DROP TABLE t"], true);
        assert_eq!(
            allow.check("SELECT 1 --\n; DROP TABLE t"),
            Decision::Denied {
                normalized: "SELECT 1 ; DROP TABLE t".to_string()
            }
        );
        // ... and the mirror image: a stored query whose comment ends in a
        // newline no longer matches the one-line spelling of it.
        let allow = allowlist(&["SELECT 1 --\n; DROP TABLE t"], true);
        assert_eq!(
            allow.check("SELECT 1 -- ; DROP TABLE t"),
            Decision::Denied {
                normalized: "SELECT 1".to_string()
            }
        );
        // A separator after any comment starts a second statement again.
        let allow = allowlist(&["SELECT 1"], true);
        for sql in [
            "SELECT 1 -- c\n; DROP TABLE t",
            "SELECT 1 /* c */ ; DROP TABLE t",
            "SELECT 1 /* c */ DROP TABLE t",
            "SELECT 1 -- c\nDROP TABLE t",
        ] {
            assert!(!allow.check(sql).is_allowed(), "should deny {sql:?}");
        }
    }

    #[test]
    fn comment_contents_do_not_matter_but_quoted_markers_do() {
        // Comments normalise to a single space whatever they contain, so two
        // spellings of the same statement with different comments are equal.
        let allow = allowlist(&["SELECT 1 /* ; */ DROP TABLE t"], true);
        assert_eq!(
            allow.check("SELECT 1 /*\nsome\nother\ncomment\n*/ DROP TABLE t"),
            Decision::Allowed
        );
        // A comment marker inside a quoted region is data, not a comment.
        let allow = allowlist(&["SELECT 1"], true);
        for sql in [
            "SELECT '-- 1'",
            "SELECT '/* 1 */'",
            "SELECT \"-- 1\"",
            "SELECT $$-- 1$$",
            "SELECT E'-- 1'",
        ] {
            assert!(!allow.check(sql).is_allowed(), "should deny {sql:?}");
        }
    }

    #[test]
    fn a_carriage_return_ends_a_line_comment() {
        // scan.l `comment` ends at `\n` *or* `\r`, so the `;` on the next line
        // is code again and the comment-only query no longer matches.
        let allow = allowlist(&["Q"], true);
        assert_eq!(
            allow.check("Q --\r; DROP TABLE t"),
            Decision::Denied {
                normalized: "Q ; DROP TABLE t".to_string()
            }
        );
        // The mirror image, as with `\n`: a comment that never ends before the
        // end of the text stands for `Q` and is a different text.
        let allow = allowlist(&["Q -- ; DROP TABLE t"], true);
        assert_eq!(
            allow.check("Q --\r; DROP TABLE t"),
            Decision::Denied {
                normalized: "Q ; DROP TABLE t".to_string()
            }
        );
        // ... and a transaction statement cannot hide one behind a CR.
        let allow = allowlist(&[CATALOG_QUERY], true);
        assert!(!allow.check("BEGIN -- x\r; DROP TABLE t").is_allowed());
        assert!(!allow.check("BEGIN -- x\rDROP TABLE t").is_allowed());
        // A CR-terminated comment on its own is still a fine BEGIN.
        assert!(allow.check("BEGIN -- x\r").is_allowed());
        assert!(allow.check("BEGIN; -- x\r").is_allowed());
    }

    #[test]
    fn dollar_signs_inside_identifiers_are_not_dollar_quotes() {
        // scan.l `ident_cont` includes `$`, so `a$b$c` is one identifier and no
        // dollar quote can start in the middle of it.
        let allow = allowlist(&["SELECT a$b$x y$b$z"], false);
        assert_eq!(allow.check("SELECT a$b$x   y$b$z;"), Decision::Allowed);
        assert_eq!(allow.check("SELECT a$b$x y$b$z"), Decision::Allowed);
        // A real dollar quote still works, and `$1$` is a parameter plus `$`.
        let allow = allowlist(&["SELECT $tag$a  b$tag$"], false);
        assert_eq!(allow.check("SELECT  $tag$a  b$tag$  ;"), Decision::Allowed);
        let allow = allowlist(&["SELECT $1$"], false);
        assert_eq!(allow.check("SELECT  $1$ ;"), Decision::Allowed);
    }

    #[test]
    fn bit_and_hex_strings_are_opaque_without_escapes() {
        // xb/xh are `[^']*`: no backslash escape, so a backslash-escaped quote
        // does not keep the literal open and the spaces after it are code.
        let allow = allowlist(&[r"SELECT B'a\'b c'"], false);
        assert_eq!(allow.check(r"SELECT B'a\'b  c'"), Decision::Allowed);
        assert_eq!(allow.check(r"SELECT   B'a\'b    c'  "), Decision::Allowed);
        // A doubled quote ends the bit string and starts a standard string.
        let allow = allowlist(&["SELECT B'a''b  c'"], false);
        assert_eq!(allow.check("SELECT B'a''b  c'"), Decision::Allowed);
        assert!(!allow.check("SELECT B'a''b c'").is_allowed());
        // Hex strings behave the same, and are not `SELECT X'1F'` plus junk.
        let allow = allowlist(&["SELECT X'1F 2A'"], false);
        assert_eq!(allow.check("SELECT X'1F 2A'"), Decision::Allowed);
        assert!(!allow.check("SELECT X'1F 2A' , 'z'").is_allowed());
    }

    #[test]
    fn non_pg_whitespace_is_not_collapsed() {
        let allow = allowlist(&["SELECT 1"], true);
        for sql in [
            "SELECT\u{a0}1",
            "SELECT\u{2028}1",
            "SELECT\u{2000}1",
            "SELECT\u{85}1",
        ] {
            assert!(!allow.check(sql).is_allowed(), "should deny {sql:?}");
        }
        // ... while scan.l's own whitespace is.
        for sql in [
            "SELECT 1",
            "SELECT\t1",
            "SELECT\r1",
            "SELECT\x0b1",
            "SELECT\x0c1",
            "SELECT\r\n1",
            " SELECT \t\r\n\x0b\x0c 1 ; ",
        ] {
            assert!(allow.check(sql).is_allowed(), "should allow {sql:?}");
        }
    }

    #[test]
    fn a_number_glues_a_following_e_to_itself() {
        // `1E` is one token in PostgreSQL (`integer_junk`), so the quote after
        // it starts a standard string: no backslash escape, which means the
        // literal ends at the second quote and the spaces after it collapse.
        let allow = allowlist(&["SELECT 1"], false);
        assert_eq!(
            allow.check(r"SELECT 1E'a\'b  c'"),
            Decision::Denied {
                normalized: r"SELECT 1E'a\'b c'".to_string()
            }
        );
        // With a space before the E, the E does start its own token, so the
        // backslash escapes the quote and both spaces survive.
        let allow = allowlist(&[r"SELECT E'a\'b  c'"], false);
        assert_eq!(allow.check(r"SELECT   E'a\'b  c'  ;"), Decision::Allowed);
        assert_eq!(
            allow.check(r"SELECT E'a\'b c'"),
            Decision::Denied {
                normalized: r"SELECT E'a\'b c'".to_string()
            }
        );
    }

    #[test]
    fn queried_then_appended_statement_still_denied() {
        // A `;` hidden in a string, an E-string or a dollar quote is not a
        // separator, but it does not let a listed query grow a second one.
        let allow = allowlist(&["SELECT 'a; b'"], true);
        assert_eq!(allow.check("SELECT 'a; b';"), Decision::Allowed);
        assert!(!allow.check("SELECT 'a; b'; DROP TABLE t").is_allowed());
    }

    #[test]
    fn queries_are_stored_normalised_and_distinct() {
        let allow = allowlist(
            &[
                "SELECT   1 ;",
                "SELECT 1",
                "SELECT\t1;",
                "SELECT 1 -- c",
                "SET TIME ZONE 'UTC'",
            ],
            false,
        );
        assert_eq!(allow.len(), 2);
        let mut stored: Vec<&str> = allow.queries().collect();
        stored.sort_unstable();
        assert_eq!(stored, ["SELECT 1", "SET TIME ZONE 'UTC'"]);
        for stored in allow.queries() {
            assert_eq!(normalize(stored), stored);
        }
        // `new` accepts anything `AsRef<str>`: owned strings too.
        let owned = Allowlist::new([String::from("SELECT 1"), "SELECT  1 ;".to_string()], false);
        assert_eq!(owned.len(), 1);
        assert_eq!(owned.check("SELECT 1"), Decision::Allowed);
    }

    #[test]
    fn a_listed_query_followed_by_a_second_statement_is_denied() {
        let allow = allowlist(&[SESSION_QUERY, CATALOG_QUERY, ORDER_INSERT], true);
        for sql in [
            format!("{SESSION_QUERY}; SELECT 1"),
            format!("{SESSION_QUERY}; DROP TABLE django_session"),
            format!("{CATALOG_QUERY}; DELETE FROM shop_product"),
            format!("{ORDER_INSERT}; COMMIT"),
            format!("{ORDER_INSERT} ; SELECT 1"),
            format!("{ORDER_INSERT};{ORDER_INSERT}"),
        ] {
            let decision = allow.check(&sql);
            assert!(!decision.is_allowed(), "should deny {sql:?}");
            assert_eq!(denied_reason(decision), normalize(&sql));
        }
        // A bare trailing separator is *not* a second statement: it is
        // normalised away, so the listed query still matches.
        assert_eq!(allow.check(&format!("{CATALOG_QUERY};")), Decision::Allowed);
        // A semicolon inside a string literal is part of one statement.
        let allow = allowlist(&["SELECT ';'"], false);
        assert_eq!(allow.check("SELECT ';';"), Decision::Allowed);
    }

    #[test]
    fn quoted_parameters_do_not_hide_a_second_statement() {
        let allow = allowlist(&[CATALOG_QUERY], true);
        assert_eq!(
            allow.check("BEGIN; SELECT 'a; b'"),
            Decision::Denied {
                normalized: "BEGIN; SELECT 'a; b'".to_string()
            }
        );
    }

    #[test]
    fn denied_carries_the_normalised_text() {
        let allow = allowlist(&[CATALOG_QUERY], false);
        assert_eq!(
            allow.check("  DELETE\n\tFROM \"shop_product\"  ;  "),
            Decision::Denied {
                normalized: "DELETE FROM \"shop_product\"".to_string()
            }
        );
    }

    #[test]
    fn decision_helper_reports_allowance() {
        assert!(Decision::Allowed.is_allowed());
        assert!(
            !Decision::Denied {
                normalized: String::new()
            }
            .is_allowed()
        );
    }

    #[test]
    fn odd_inputs_never_panic() {
        let odd = [
            "'",
            "\"",
            "$$",
            "$",
            "$1",
            "$tag$",
            "$tag$a",
            "/*",
            "*/",
            "--",
            ";;;",
            "；",
            "SELECT 'héllo  wörld'",
            "\u{0}",
            "SELECT $$;$$;",
            "''",
            "\"\"",
            "BEGIN \"",
            "SAVEPOINT \"s",
            "SELECT 1 -- '",
            "SELECT 'a''b   '  ;",
            "e'\\\\'",
            "🦀",
            "SELECT \"a  b\" FROM \"c;d\" ;",
        ];
        let allow = allowlist(&[CATALOG_QUERY, ORDER_INSERT], true);
        for sql in odd {
            let once = normalize(sql);
            assert_eq!(normalize(&once), once, "not idempotent for {sql:?}");
            let _ = allow.check(sql);
            let _ = allow.check(&once);
        }
    }

    #[test]
    fn exhaustive_short_inputs_never_panic() {
        // Every string of length <= 4 over a hostile alphabet, so quote,
        // E-string, bit/hex-string, comment and dollar-quote handling is
        // exercised together.
        let alphabet = [
            '\'', '"', '$', ';', '-', '/', '*', '\\', ' ', '\n', '\r', '\x0b', 'a', 'b', 'e', 'B',
            'X', '€',
        ];
        let allow = allowlist(&["SELECT 1", "BEGIN"], true);
        for len in 0..=4 {
            let combos = alphabet.len().pow(len);
            for n in 0..combos {
                let mut sql = String::new();
                let mut rest = n;
                for _ in 0..len {
                    sql.push(alphabet[rest % alphabet.len()]);
                    rest /= alphabet.len();
                }
                let once = normalize(&sql);
                assert_eq!(normalize(&once), once, "not idempotent for {sql:?}");
                // Normalisation and the statement splitter share one lexer, so
                // normalising may never change how many statements a text has.
                assert_eq!(
                    crate::statements::statement_count_hint(&once),
                    crate::statements::statement_count_hint(&sql),
                    "statement count changed by normalisation for {sql:?}"
                );
                let _ = allow.check(&sql);
                let _ = allow.check(&once);
            }
        }
    }
}
