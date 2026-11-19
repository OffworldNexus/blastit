//! The statements, accesses and refusals an analysis reports (§6, D7).
//!
//! These are the values the policy decides on: what PostgreSQL's parser says a
//! statement is ([`StatementClass`]), what a grant can cover ([`StatementKind`],
//! [`Access`], [`Analysis`]) and why a statement cannot be covered at all
//! ([`AnalysisError`]). The walk that produces them is [`super::walk`], and the
//! relations it resolves against are [`super::scope`].

use std::collections::BTreeSet;
use std::fmt;

use super::classify::is_safe_function;

/// The kind of statement a table grant understood.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatementKind {
    /// `SELECT`, `VALUES`, and a `WITH` clause around either.
    #[default]
    Select,
    /// `INSERT`, including its `ON CONFLICT` clause.
    Insert,
    /// `UPDATE`.
    Update,
    /// `DELETE`.
    Delete,
}

impl StatementKind {
    /// The lowercase spelling, as a grant's operation keys use it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl fmt::Display for StatementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What PostgreSQL's own parser says a statement is (D3c).
///
/// [`analyze`](super::analyze) answers only for the four kinds a table grant can
/// cover, because that is all a table grant can allow. A tool that has to decide
/// whether a statement read out of a log may be granted *at all* — `blastit-manifest
/// fold` — needs an answer for every kind: a `SET`, a `COPY`, a `CREATE TABLE`, the
/// transaction commands. That is what [`classify`](super::classify) gives it,
/// without a second parser and without guessing from the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatementClass {
    /// `SELECT`, `VALUES`, and a `WITH` clause around either.
    Select,
    /// `INSERT`, including its `ON CONFLICT` clause.
    Insert,
    /// `UPDATE`.
    Update,
    /// `DELETE`.
    Delete,
    /// Anything else: DDL, `SET`, `COPY`, `DO`, `CALL`, `LOCK`, `GRANT`,
    /// `TRUNCATE`, `VACUUM`, `LISTEN`, the transaction commands — every kind a
    /// grant is not an allowlist for.
    Other,
}

impl StatementClass {
    /// The lowercase spelling, the same word a grant's operation keys use.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Other => "other",
        }
    }

    /// Whether the four plain kinds a table grant covers include this one.
    pub const fn is_plain(self) -> bool {
        !matches!(self, Self::Other)
    }
}

impl fmt::Display for StatementClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One statement's kind, and the name to say it by (D3c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub(super) class: StatementClass,
    pub(super) label: String,
    pub(super) statements: usize,
}

impl Classified {
    /// The kind, in the words a grant and a report use.
    pub const fn class(&self) -> StatementClass {
        self.class
    }

    /// Whether a `queries` entry — and a table grant — may hold this statement.
    ///
    /// False for everything a grant is not an allowlist for, and false for a
    /// text that holds more than one statement: a grant is one statement, so a
    /// caller has to split the text rather than write it (§6, D3c).
    pub const fn is_plain(&self) -> bool {
        self.class.is_plain() && self.statements == 1
    }

    /// How many statements the text holds. One for everything a grant can hold.
    pub const fn statements(&self) -> usize {
        self.statements
    }

    /// The statement's own name, for a message: `SELECT`, `SET`, `CREATE TABLE`.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The privilege one access needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Op {
    /// Reading a column: `select` on it.
    Select,
    /// Writing a column of a new row: `insert` on it.
    Insert,
    /// Writing a column of an existing row: `update` on it.
    Update,
    /// Removing rows: `delete` on the table.
    Delete,
}

impl Op {
    /// The lowercase spelling, the same word a grant uses (`select = […]`,
    /// `delete = true`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing a statement does to one table (§6).
///
/// A `column` of `None` is a whole-table access: a `*` read, an `INSERT` with no
/// column list, or a `DELETE`. The table is spelled the way a grant has to spell
/// it: `public.t` is `t`, any other schema keeps its qualified name.
///
/// Two weaker shapes exist. A table the statement only *names* —
/// `SELECT count(*) FROM t` — needs some `select` entry, but no particular
/// column ([`Access::named`]). A table whose rows are *locked* (`FOR UPDATE`)
/// needs some `update` entry on any one column, which is what PostgreSQL asks
/// for and what [`Access::lock`] is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Access {
    op: Op,
    table: String,
    extent: Extent,
}

/// How much of a table one [`Access`] needs.
///
/// Ordered so that a `Named` access sorts before an any-column one, a whole-row
/// one and a column one: the first access a denial reports is the first thing a
/// grant author would have to add.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Extent {
    /// The table is named, but no column of it is read: any `select` entry
    /// covers it.
    Named,
    /// Any one column of the table, the one PostgreSQL needs for a row lock
    /// (`FOR UPDATE`): a non-empty `update` list or `update = "*"` covers it,
    /// but an empty list does not — it grants no column to update.
    Any,
    /// Every column: `select = "*"`, `insert = "*"`, a `DELETE`.
    Whole,
    /// One column, by the name PostgreSQL resolved.
    Column(String),
}

impl Access {
    /// One access of a statement.
    ///
    /// `column` of `None` is a whole-row access (`select on t.*`), `Some` a
    /// column one (`select on t.id`); [`Access::named`] is the table-only form
    /// and [`Access::lock`] the row-lock one.
    pub fn new(op: Op, table: impl Into<String>, column: Option<&str>) -> Self {
        let extent = match column {
            Some(column) => Extent::Column(column.to_string()),
            None => Extent::Whole,
        };
        Self {
            op,
            table: table.into(),
            extent,
        }
    }

    /// An access that only *names* a table: no column of it is read, so any
    /// `select` entry covers it (D7).
    ///
    /// `SELECT count(*) FROM t`, `SELECT 1 FROM t` and `EXISTS (SELECT 1 FROM t)`
    /// all read no column, but they tell the statement whether `t` exists and how
    /// few or many rows it has, so a grant has to name `t`.
    pub fn named(op: Op, table: impl Into<String>) -> Self {
        Self {
            op,
            table: table.into(),
            extent: Extent::Named,
        }
    }

    /// An access that locks a table's rows: `SELECT … FOR UPDATE` (and `FOR NO
    /// KEY UPDATE`, `FOR SHARE`, `FOR KEY SHARE`).
    ///
    /// PostgreSQL asks for the UPDATE privilege on **at least one column** of
    /// every locked table, so any `update` entry covers this — a non-empty
    /// column list or `"*"` — while an empty one does not, since it grants no
    /// column. The denial says so: `update on t (any column, to lock its rows)
    /// not granted`.
    pub fn lock(table: impl Into<String>) -> Self {
        Self {
            op: Op::Update,
            table: table.into(),
            extent: Extent::Any,
        }
    }

    /// The privilege this access needs.
    pub fn op(&self) -> Op {
        self.op
    }

    /// The table's name, as a grant spells it.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The column, or `None` for a whole-table, table-only or any-column
    /// access. The last is a row lock: it names no column, and every `update`
    /// entry with at least one column covers it.
    pub fn column(&self) -> Option<&str> {
        match &self.extent {
            Extent::Column(column) => Some(column),
            Extent::Named | Extent::Any | Extent::Whole => None,
        }
    }

    /// How much of the table the access needs, for the grant check.
    pub(crate) fn extent(&self) -> &Extent {
        &self.extent
    }
}

impl fmt::Display for Access {
    /// `select on auth_user.password`, `delete on orders_order`, `select on
    /// auth_user`: the form a denial's `reason` uses, and the one a human reads
    /// in a log. A whole-table access is spelled `t.*`, which is what a grant has
    /// to say; a table-only access is spelled by the table's name alone; a row
    /// lock says what a grant has to add for it (`any column`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.extent, self.op) {
            (Extent::Column(column), _) => write!(f, "{} on {}.{}", self.op, self.table, column),
            (_, Op::Delete) => write!(f, "delete on {}", self.table),
            (Extent::Named, _) => write!(f, "{} on {}", self.op, self.table),
            (Extent::Any, _) => write!(
                f,
                "{} on {} (any column, to lock its rows)",
                self.op, self.table
            ),
            (Extent::Whole, _) => write!(f, "{} on {}.*", self.op, self.table),
        }
    }
}

/// Every table, column and function one statement touches (§6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub(super) kind: StatementKind,
    pub(super) accesses: BTreeSet<Access>,
    pub(super) functions: BTreeSet<String>,
}

impl Analysis {
    /// The kind of statement that was analysed.
    ///
    /// [`Analysis::default`] is a `SELECT` that touches nothing: an empty
    /// analysis can never authorise anything.
    pub fn kind(&self) -> StatementKind {
        self.kind
    }

    /// The accesses, sorted, without duplicates.
    pub fn accesses(&self) -> impl DoubleEndedIterator<Item = &Access> {
        self.accesses.iter()
    }

    /// The accesses themselves, for a caller that wants the set.
    pub fn access_set(&self) -> &BTreeSet<Access> {
        &self.accesses
    }

    /// Every function call the statement makes, as a grant has to spell it:
    /// `lower`, or `myschema.func` when the call named another schema. The
    /// `pg_catalog.` prefix a call may carry is dropped, so `pg_catalog.lower`
    /// and `lower` are one name — and one [`SAFE_FUNCTIONS`](super::SAFE_FUNCTIONS)
    /// entry.
    pub fn functions(&self) -> impl Iterator<Item = &str> {
        self.functions.iter().map(String::as_str)
    }

    /// The function calls a grant has to list: everything in
    /// [`Analysis::functions`] that is not in
    /// [`SAFE_FUNCTIONS`](super::SAFE_FUNCTIONS) — what
    /// [`crate::TableGrants::covering`] writes into `functions = […]`.
    pub fn extra_functions(&self) -> impl Iterator<Item = &str> {
        self.functions().filter(|name| !is_safe_function(name))
    }

    /// Whether the statement touches nothing at all: no table, no column and no
    /// function outside the safe set. Such a statement (`SELECT 1`,
    /// `SELECT now()`) is allowed by any table grant.
    pub fn touches_nothing(&self) -> bool {
        self.accesses.is_empty() && self.extra_functions().next().is_none()
    }
}

/// Why a statement could not be analysed — and so cannot be allowed by a table
/// grant.
///
/// Every variant is a *refusal*: the exact-match grants are the only way such a
/// statement may run (§6). [`Display`](fmt::Display) renders the one line a
/// `sql.denied` event carries as its `reason`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    /// The text is not valid SQL, as PostgreSQL's own parser reports it.
    Syntax(String),
    /// The text holds no statement at all.
    Empty,
    /// The text holds more than one statement.
    MultipleStatements(usize),
    /// The statement is something this build does not analyse: another statement
    /// kind, a construct it does not understand, or nesting deeper than it will
    /// walk.
    NotUnderstood(String),
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the statement is not valid SQL: {message}"),
            Self::Empty => f.write_str("there is no statement to analyse"),
            Self::MultipleStatements(count) => write!(
                f,
                "the text holds {count} statements, and a table grant covers exactly one"
            ),
            Self::NotUnderstood(message) => write!(f, "a table grant cannot cover {message}"),
        }
    }
}

impl std::error::Error for AnalysisError {}
