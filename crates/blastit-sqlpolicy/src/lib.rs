//! SQL normalisation and allowlist matching for `postgresql` grants
//! (ARCHITECTURE.md §5.2, §6).
//!
//! blastit does not parse SQL. A `postgresql` grant (§5.2) lists the statement
//! texts the app may send, and dbproxy compares them **exactly**, after
//! normalising both the manifest's text and the text on the wire with the same
//! rules (§6, "Matching"): trim, drop trailing `;`, replace every comment by a
//! single space, collapse every run of whitespace outside quoted strings to
//! one space, and rewrite the *shapes* a real app can never keep still — every
//! bind placeholder becomes `$?`, a list of placeholders after `IN` becomes
//! `IN ($?, ...)`, and a run of identical `VALUES` rows becomes
//! `(row), ...`. Matching the whole normalised text is what makes a listed
//! query followed by a second statement fail to match.
//!
//! # Agreement with PostgreSQL
//!
//! Normalisation follows PostgreSQL 18's `src/backend/parser/scan.l` for every
//! boundary a policy check depends on: which characters are whitespace
//! (`space` = `[ \t\n\r\f\v]`, and nothing else), where a line comment ends
//! (`\n` or `\r`), where a quoted region starts and ends (`''`, E-strings,
//! `B'…'`/`X'…'`, `N'…'`, `U&'…'`, `"…"`, `$tag$…$tag$`), and which `$` opens a
//! dollar quote (`a$b$c` is one identifier). See the `lexer` module for the two
//! deliberate divergences, neither of which can hide code or quoted data.
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
//! [`Allowlist`] also carries the grant's [`TableGrants`] (§6, D7): a statement
//! whose exact text is not listed is allowed when it parses to one `SELECT`,
//! `INSERT`, `UPDATE` or `DELETE` that only touches granted tables, columns and
//! functions (see [`analyze`] and [`Allowlist::with_tables`]).
//!
//! [`Allowlist::unrestricted`] is the other extreme: a route a manifest marks
//! `unrestricted = true` may run any statement on one proxy (§6), and its
//! allowlist allows everything without normalising or comparing anything.
//!
//! ```
//! use blastit_sqlpolicy::{Allowlist, Decision};
//!
//! let allow = Allowlist::new(["SELECT 1"], true);
//! assert_eq!(
//!     allow.check("  select  1 ; "),
//!     Decision::Denied {
//!         normalized: "select 1".to_string(),
//!         reason: "the statement is not granted".to_string(),
//!     },
//! );
//! assert_eq!(allow.check("SELECT\n  1;"), Decision::Allowed);
//! assert_eq!(allow.check("BEGIN"), Decision::Allowed);
//! assert_eq!(
//!     allow.check("BEGIN; DROP TABLE t"),
//!     Decision::Denied {
//!         normalized: "BEGIN; DROP TABLE t".to_string(),
//!         reason: "the statement is not granted".to_string(),
//!     },
//! );
//!
//! // A grant survives the shapes a Django app produces.
//! let allow = Allowlist::new(["SELECT id FROM t WHERE owner_id IN ($1)"], false);
//! assert_eq!(
//!     allow.check("SELECT id FROM t WHERE owner_id IN ($1, $2, $3)"),
//!     Decision::Allowed,
//! );
//! ```

mod analysis;
mod learn;
mod lexer;
mod literals;
mod lru;
mod normalize;
mod statements;
mod tables;
mod template;

pub use analysis::{
    Access, Analysis, AnalysisError, Classified, Op, SAFE_FUNCTIONS, StatementClass, StatementKind,
    analyze, cached_analyses, classify, is_safe_function, is_system_table,
};
pub use learn::{
    DANGEROUS_LEARN_FUNCTIONS, LearnVerdict, RefuseReason, check_learnable,
    is_dangerous_learn_function, is_introspection_setup_sql,
};
pub use literals::{KNOWN_STATIC_SETUP_SQL, has_quoted_literal, literal_offset, redact_literals};
pub use normalize::{normalize, restore_parameters};
pub use tables::{Columns, Denied, TableGrant, TableGrants};
pub use template::{
    MAX_TEMPLATE_CHARS, MAX_TEMPLATE_PARAMS, MAX_TEMPLATE_PARSES, TemplateError, TemplateKind,
    check_template, parameterize,
};

use std::borrow::Cow;
use std::collections::HashSet;

use analysis::AnalysisCache;
use statements::is_transaction_control;

/// Why a statement is refused when no exact grant lists it and the grant has no
/// table grants to check it against.
const NOT_GRANTED: &str = "the statement is not granted";

/// What a [`Allowlist`] decided about one SQL text.
///
/// Not exhaustive: more decision kinds may be added without a breaking change.
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
        /// Why it was refused, in one line: `select on auth_user.password not
        /// granted` for a table grant, `the statement is not granted` for a
        /// text no grant lists. It names tables, columns and functions, never a
        /// parameter value (§8.1).
        reason: String,
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
/// Built from the effective queries of a route (§6, "Effective grants"), its
/// [`TableGrants`] and the route's `transaction_control` flag, or with
/// [`Allowlist::unrestricted`] for a route a manifest marked
/// `unrestricted = true`.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    queries: HashSet<String>,
    transaction_control: bool,
    unrestricted: bool,
    tables: TableGrants,
    /// The bounded analysis memo this allowlist checks statements against.
    ///
    /// It is a handle to a shared cache, not process-wide state read behind the
    /// allowlist's back: `Allowlist::new` and `Default` hand over the
    /// per-process memo blastit has always used, and [`Allowlist::check`]
    /// analyses against exactly the handle the allowlist holds.
    analysis: AnalysisCache,
}

impl Allowlist {
    /// Builds an allowlist from a grant's queries and its `transaction_control`
    /// flag.
    ///
    /// Every query is stored in its normalised form (see [`normalize`]), so
    /// manifest formatting differences are irrelevant when checking. Queries
    /// that normalise to the empty text are ignored: no text is ever allowed
    /// just because the manifest contained an empty string.
    ///
    /// The grant's table grants are added with [`Allowlist::with_tables`]; a
    /// grant that has none allows only what its queries list.
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
            unrestricted: false,
            tables: TableGrants::default(),
            analysis: AnalysisCache::default(),
        }
    }

    /// The same allowlist with the grant's table and column grants (§6, D7).
    ///
    /// A statement that is not listed exactly is then analysed
    /// ([`analyze`]) and allowed when every table, column and function it
    /// touches is granted. An exact match still wins: it is checked first and
    /// costs no parse, which is what keeps a route that lists its statements
    /// unchanged.
    pub fn with_tables(mut self, tables: TableGrants) -> Self {
        self.tables = tables;
        self
    }

    /// The grant's table and column grants.
    pub fn tables(&self) -> &TableGrants {
        &self.tables
    }

    /// An allowlist that allows every statement.
    ///
    /// This is what a manifest's `unrestricted = true` means for one
    /// (route, proxy) pair (§6): the route may run anything on that proxy,
    /// still through dbproxy and still without ever seeing the credentials.
    /// [`Allowlist::check`] then allows without normalising anything, and
    /// dbproxy logs no denial — there is nothing to deny.
    pub fn unrestricted() -> Self {
        Self {
            queries: HashSet::new(),
            transaction_control: false,
            unrestricted: true,
            tables: TableGrants::default(),
            analysis: AnalysisCache::default(),
        }
    }

    /// The distinct normalised queries the allowlist holds, in no particular
    /// order.
    ///
    /// Always empty for an [`Allowlist::unrestricted`] one, which allows by
    /// not comparing.
    pub fn queries(&self) -> impl Iterator<Item = &str> {
        self.queries.iter().map(String::as_str)
    }

    /// Decides whether `sql` is allowed.
    ///
    /// An [`Allowlist::unrestricted`] one allows everything without looking at
    /// the text at all, empty text included: PostgreSQL answers an empty
    /// statement with an `EmptyQueryResponse` and runs nothing.
    ///
    /// Every other allowlist allows the text when its normalised form is exactly
    /// one of the allowlisted quarries, when — with `transaction_control` — it is
    /// a single transaction-control statement, or, with table grants, when it
    /// analyses ([`analyze`]) to a statement of its grant's tables, columns and
    /// functions. It denies empty or whitespace-only text.
    pub fn check(&self, sql: &str) -> Decision {
        if self.unrestricted {
            return Decision::Allowed;
        }
        let normalized = normalize(sql);
        let deny = |reason: &str| Decision::Denied {
            normalized: normalized.clone(),
            reason: reason.to_string(),
        };
        if normalized.is_empty() {
            return deny(NOT_GRANTED);
        }
        if self.queries.contains(&normalized) {
            return Decision::Allowed;
        }
        // The grammar wants the text normalised exactly once, as here.
        if self.transaction_control && is_transaction_control(&normalized) {
            return Decision::Allowed;
        }
        if self.tables.is_empty() {
            // Nothing to analyse against: the grant is a list of statements and
            // this is not one of them.
            return deny(NOT_GRANTED);
        }
        // The analysis is cached by this normalised text, which is also the key
        // an exact grant is compared against (§6). It *parses* the text, and an
        // observation carries `blastit_dbproxy`'s spelling — `$?` placeholders
        // and `…` where a literal was redacted — which no parser accepts. So a
        // text in that spelling is restored to one of the same shape first: the
        // access set a table grant decides on is identical, and the cache key
        // stays the normalised text (`MB.40.2`).
        let analysable: Cow<'_, str> =
            if normalized.contains("$?") || normalized.contains('\u{2026}') {
                Cow::Owned(restore_parameters(sql))
            } else {
                Cow::Borrowed(sql)
            };
        match self
            .analysis
            .analyze_cached(&analysable, &normalized)
            .as_ref()
        {
            Ok(analysis) => match self.tables.check(analysis) {
                Ok(()) => Decision::Allowed,
                Err(denied) => Decision::Denied {
                    normalized,
                    reason: denied.to_string(),
                },
            },
            Err(error) => Decision::Denied {
                normalized,
                reason: error.to_string(),
            },
        }
    }

    /// Whether this allowlist also accepts single transaction-control
    /// statements.
    ///
    /// Always false for an [`Allowlist::unrestricted`] one, which needs no
    /// such flag: it allows transaction control along with everything else.
    pub fn transaction_control(&self) -> bool {
        self.transaction_control
    }

    /// Whether every statement is allowed, whatever it is.
    pub fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// How many **distinct normalised** queries the allowlist holds: two
    /// manifest entries that differ only in formatting count once.
    pub fn len(&self) -> usize {
        self.queries.len()
    }

    /// Whether the allowlist can allow nothing at all: no listed query, no table
    /// grant and no extra function. It can then only allow transaction-control
    /// statements, if those are enabled.
    ///
    /// An [`Allowlist::unrestricted`] one is never empty: it allows everything,
    /// which is the opposite of allowing nothing, and it reads the same way on
    /// both sides of the policy (`PgGrant::is_empty` in `blastit-manifest`).
    pub fn is_empty(&self) -> bool {
        !self.unrestricted && self.queries.is_empty() && self.tables.is_empty()
    }
}
