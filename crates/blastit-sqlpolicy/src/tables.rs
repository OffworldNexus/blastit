//! Table and column grants: which tables a route may read and write, and which
//! functions it may call (§6, D7).
//!
//! This is the second way to allow a statement. An exact grant
//! ([`crate::Allowlist`]'s `queries`) lists statement *texts*, which a real app
//! cannot keep still: a list endpoint's filters, prefetches and joins produce
//! hundreds of near-duplicate `SELECT`s, and a test suite never covers every
//! combination. A table grant says instead which **tables and columns** a route
//! may touch, and every statement is checked against it by
//! [`crate::analysis`], which parses it with PostgreSQL's own parser.
//!
//! ```text
//! view catalog GET "/products/{id:int}" {
//!     postgres {
//!         functions st_distance, unaccent
//!         select restaurants_restaurant(id, slug, name)
//!         select restaurants_cuisinetype
//!         select orders_order
//!         insert orders_order
//!         update orders_order(status)
//!         delete orders_order
//!     }
//! }
//! ```
//!
//! * a table key is the name a grant and the analysis agree on: `public.t` is
//!   `t`, any other schema keeps its qualified spelling (`audit.users`), and
//!   `pg_catalog`/`information_schema` can never be granted at all;
//! * `select`, `insert` and `update` take a column list, or the string `"*"` for
//!   every column; `delete` is a boolean. An operation that is not written at
//!   all is not granted;
//! * a `*` read, an `INSERT` with no column list and a `DELETE` need the
//!   whole-table form (`select = "*"`, `insert = "*"`, `delete = true`);
//! * a locking clause (`FOR UPDATE`, `FOR SHARE`, …) needs an `update` entry on
//!   every base table it locks, through derived relations — but on **any one**
//!   column, because that is all PostgreSQL asks for to lock a row: a route that
//!   only reads the rows it locks can be granted `update = ["id"]`.
//!   [`TableGrants::covering`] writes exactly that instead of `"*"`;
//! * naming a table is touching it: `SELECT count(*) FROM t`, `SELECT 1 FROM t`
//!   and `EXISTS (SELECT 1 FROM t)` read no column but still learn that the
//!   table exists, so they need a `select` entry — any columns. The entry a
//!   covering grant writes for a table only ever counted is `select = []`, and a
//!   written operation counts even when its list is empty (so the route keeps its
//!   listener); a write target (`INSERT INTO t`, `UPDATE t`, `DELETE FROM t`) is
//!   named by its own operation instead;
//! * names are compared exactly as PostgreSQL resolved them. An unquoted
//!   identifier in SQL arrives lowercased (`SELECT ID` is `id`), so a grant for
//!   it is written `id`; `"Id"` in the SQL is the different name `Id`.
//!
//! The whole set merges like a grant's queries do (§6, "Effective grants"): a
//! route's tables are the union of its own, its groups' and the global groups',
//! with `"*"` absorbing a column list.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::analysis::{Access, Analysis, Extent, Op, is_safe_function};

/// The columns one operation of one table grant may touch.
///
/// Spelled the way a manifest spells it: the string `"*"` for every column, or
/// a list of column names. The JSON a report or dbproxy's configuration carries
/// uses the same two spellings, so what a report shows is what a manifest
/// holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Columns {
    /// `select = "*"`: every column of the table, including ones added later.
    All,
    /// `select = ["id", "slug"]`: exactly these, by the names PostgreSQL
    /// resolved.
    List(BTreeSet<String>),
}

impl Columns {
    /// The list form, from any set of names.
    pub fn list<S: Into<String>>(columns: impl IntoIterator<Item = S>) -> Self {
        Self::List(columns.into_iter().map(Into::into).collect())
    }

    /// Whether the grant covers `column`.
    pub fn contains(&self, column: &str) -> bool {
        match self {
            Self::All => true,
            Self::List(columns) => columns.contains(column),
        }
    }

    /// Whether the grant covers no column at all.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::All => false,
            Self::List(columns) => columns.is_empty(),
        }
    }

    /// The union of two column grants: `"*"` absorbs a list.
    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::List(mut left), Self::List(right)) => {
                left.extend(right);
                Self::List(left)
            }
        }
    }
}

impl Serialize for Columns {
    /// `"*"`, or a list of names.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str("*"),
            Self::List(columns) => columns.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Columns {
    /// Reads `"*"` or a list of names — the two spellings a manifest and a
    /// report use.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColumnsVisitor;

        impl<'de> serde::de::Visitor<'de> for ColumnsVisitor {
            type Value = Columns;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("the string \"*\" or a list of column names")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Columns, E> {
                match value {
                    "*" => Ok(Columns::All),
                    other => Err(E::invalid_value(serde::de::Unexpected::Str(other), &self)),
                }
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Columns, A::Error> {
                let mut columns = BTreeSet::new();
                while let Some(column) = sequence.next_element::<String>()? {
                    columns.insert(column);
                }
                Ok(Columns::List(columns))
            }
        }

        deserializer.deserialize_any(ColumnsVisitor)
    }
}

/// What one route may do to one table (§6).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableGrant {
    /// The columns the route may read, `select = …`.
    #[serde(default)]
    pub select: Option<Columns>,
    /// The columns the route may write in a new row, `insert = …`.
    #[serde(default)]
    pub insert: Option<Columns>,
    /// The columns the route may change in an existing row, `update = …`.
    #[serde(default)]
    pub update: Option<Columns>,
    /// Whether `update` is only there to lock rows (`FOR UPDATE`), not to write
    /// them — a hint [`TableGrants::covering`] leaves for
    /// [`TableGrants::to_blastfile`], which says so in a comment.
    ///
    /// A lock needs the UPDATE privilege on **any one** column of the table
    /// (PostgreSQL's rule), so the covering grant names the smallest column the
    /// route already reads instead of `"*"`. It changes nothing about what the
    /// grant allows; a manifest never writes it, and it is not serialised.
    #[serde(skip)]
    pub lock_only: bool,
    /// Whether the route may delete rows, `delete = true`.
    #[serde(default)]
    pub delete: bool,
}

impl TableGrant {
    /// Whether this grant covers one access.
    fn allows(&self, op: Op, extent: &Extent) -> bool {
        let granted = match op {
            Op::Select => &self.select,
            Op::Insert => &self.insert,
            Op::Update => &self.update,
            // `delete` is about rows, not columns: it takes the whole table.
            Op::Delete => return self.delete,
        };
        match extent {
            // The table is only *named*: any `select` entry covers it, even an
            // empty list. What the statement learns is that the table exists,
            // not any column of it.
            Extent::Named => granted.is_some(),
            // A row lock (`FOR UPDATE`): PostgreSQL asks for the UPDATE
            // privilege on *at least one* column of a locked table, so any
            // `update` entry with a column in it covers the lock. An empty
            // list does not: it grants no column to update.
            Extent::Any => match granted {
                Some(Columns::All) => true,
                Some(Columns::List(columns)) => !columns.is_empty(),
                None => false,
            },
            // A whole-row access (`*`, an `INSERT` with no column list) is not
            // covered by a list of columns: it reads or writes them all.
            Extent::Whole => matches!(granted, Some(Columns::All)),
            Extent::Column(column) => match granted {
                Some(Columns::All) => true,
                Some(Columns::List(columns)) => columns.contains(column),
                None => false,
            },
        }
    }

    /// Unions another grant on the same table into this one.
    fn merge_from(&mut self, other: &Self) {
        for (slot, other_slot) in [
            (&mut self.select, &other.select),
            (&mut self.insert, &other.insert),
            (&mut self.update, &other.update),
        ] {
            if let Some(other) = other_slot {
                *slot = Some(match slot.take() {
                    Some(own) => own.union(other.clone()),
                    None => other.clone(),
                });
            }
        }
        self.delete |= other.delete;
        self.lock_only |= other.lock_only;
    }

    /// Whether the grant allows nothing at all.
    ///
    /// A written operation counts even when its column list is empty: `select =
    /// []` grants *naming* the table (a `count(*)`, an `EXISTS`), which is
    /// exactly what a covering grant writes for a table a route only counts.
    fn is_empty(&self) -> bool {
        !self.delete && self.select.is_none() && self.insert.is_none() && self.update.is_none()
    }
}

/// A route's table and column grants, and the extra functions it may call (§6).
///
/// Built from a grant's `tables` and `functions` (§6), or with
/// [`TableGrants::covering`] from what a test suite was observed to do — which
/// is what the pytest plugin reports so that a manifest can be rewritten from
/// exact queries to table grants (D7).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableGrants {
    /// The table grants, by the name a grant and the analysis agree on.
    #[serde(default)]
    pub tables: BTreeMap<String, TableGrant>,
    /// The functions beyond [`crate::SAFE_FUNCTIONS`] the route may call.
    #[serde(default)]
    pub functions: BTreeSet<String>,
}

impl TableGrants {
    /// Builds table grants from manifest values.
    ///
    /// Keys and function names are canonicalised the way the analysis spells
    /// them: a leading `public.` is dropped from a table and a leading
    /// `pg_catalog.` from a function, so `public.t` and `t` — and
    /// `pg_catalog.lower` and `lower` — are one entry each, never two that
    /// disagree.
    pub fn new(
        tables: impl IntoIterator<Item = (String, TableGrant)>,
        functions: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            tables: tables
                .into_iter()
                .map(|(table, grant)| (canonical_table(&table), grant))
                .collect(),
            functions: functions
                .into_iter()
                .map(|function| canonical_function(&function))
                .collect(),
        }
    }

    /// Whether the grant allows nothing at all: no table, no function.
    ///
    /// A grant that is otherwise empty (`queries = []`, no
    /// `transaction_control`) then gives the route no dbproxy listener (§2.2).
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.tables.values().all(TableGrant::is_empty)
    }

    /// The table grants, by name.
    pub fn tables(&self) -> &BTreeMap<String, TableGrant> {
        &self.tables
    }

    /// One table's grant, by the name the analysis would use.
    pub fn table(&self, table: &str) -> Option<&TableGrant> {
        self.tables.get(&canonical_table(table))
    }

    /// The extra functions the route may call, beyond
    /// [`crate::SAFE_FUNCTIONS`].
    pub fn functions(&self) -> impl Iterator<Item = &str> {
        self.functions.iter().map(String::as_str)
    }

    /// Unions another set of grants into this one (§6, "Effective grants").
    pub fn merge_from(&mut self, other: &Self) {
        for (table, grant) in &other.tables {
            self.tables
                .entry(table.clone())
                .or_default()
                .merge_from(grant);
        }
        self.functions.extend(other.functions.iter().cloned());
    }

    /// Checks one analysed statement against the grants.
    ///
    /// The *first* access the grants do not cover is the reason: a statement is
    /// allowed only when every access is, and a denial tells the app author what
    /// to add.
    ///
    /// # Errors
    ///
    /// [`Denied`], whose message names the missing access
    /// (`select on auth_user.password not granted`) or function — never a
    /// parameter value, which never reaches a policy check (§8.1).
    pub fn check(&self, analysis: &Analysis) -> Result<(), Denied> {
        for access in analysis.accesses() {
            let allowed = self
                .tables
                .get(access.table())
                .is_some_and(|grant| grant.allows(access.op(), access.extent()));
            if !allowed {
                return Err(Denied::new(format!("{access} not granted")));
            }
        }
        for function in analysis.functions() {
            if !is_safe_function(function) && !self.functions.contains(function) {
                return Err(Denied::new(format!(
                    "the function {function} is not granted"
                )));
            }
        }
        Ok(())
    }

    /// The smallest grant that covers every one of `analyses`.
    ///
    /// This is what the pytest plugin reports for a route: the tables, columns
    /// and functions its statements were observed to touch, so that a manifest
    /// written from exact queries can be rewritten as a table grant (D7). A
    /// table a statement only locks (`FOR UPDATE`) gets an `update` entry on the
    /// smallest column the route already reads — any one column is enough for
    /// PostgreSQL — never `"*"`.
    pub fn covering<'a>(analyses: impl IntoIterator<Item = &'a Analysis>) -> Self {
        let mut covering = Self::default();
        let mut locked = BTreeSet::new();
        for analysis in analyses {
            for access in analysis.accesses() {
                if access.extent() == &Extent::Any {
                    locked.insert(access.table().to_string());
                }
                covering.cover(access);
            }
            for function in analysis.extra_functions() {
                covering.functions.insert(function.to_string());
            }
        }
        covering.settle_locks(&locked);
        covering
    }

    /// Adds one access to the covering grant.
    fn cover(&mut self, access: &Access) {
        let grant = self.tables.entry(access.table().to_string()).or_default();
        if access.op() == Op::Delete {
            grant.delete = true;
            return;
        }
        let slot = match access.op() {
            Op::Select => &mut grant.select,
            Op::Insert => &mut grant.insert,
            Op::Update => &mut grant.update,
            Op::Delete => return,
        };
        *slot = Some(match (slot.take(), access.extent()) {
            // A whole-row access covers every column, so it absorbs a list.
            (Some(Columns::All), _) | (_, Extent::Whole) => Columns::All,
            // A row lock names no column of its own: it needs `update` on any
            // one column, which `settle_locks` fills in at the end — after
            // every statement has been seen, so it can pick a column the route
            // already reads. Keep whatever a written `UPDATE` asked for.
            (Some(current), Extent::Any) => current,
            (None, Extent::Any) => Columns::List(BTreeSet::new()),
            // A written list — even an empty one — already covers naming the
            // table; an empty one is exactly what a table only counted gets.
            (Some(columns @ Columns::List(_)), Extent::Named) => columns,
            (None, Extent::Named) => Columns::List(BTreeSet::new()),
            (Some(Columns::List(mut columns)), Extent::Column(column)) => {
                columns.insert(column.to_string());
                Columns::List(columns)
            }
            (None, Extent::Column(column)) => Columns::list([column]),
        });
    }

    /// Fills in the `update` entry that a row lock alone asks for.
    ///
    /// It runs once every statement has been covered, so the column it picks is
    /// one the route already reads: `id` when it reads one, otherwise the first
    /// column it reads. A table whose `update` entry a statement's own writes
    /// already filled keeps it: that entry covers the lock too.
    ///
    /// When there is no column to name ([`lock_update`] answers `None`) the
    /// entry is left empty, and the grant does **not** allow the lock — the
    /// statement has to stay an exact `queries` entry, and
    /// [`TableGrants::locks_without_a_column`] is how a caller finds out.
    fn settle_locks(&mut self, locked: &BTreeSet<String>) {
        for table in locked {
            let Some(grant) = self.tables.get_mut(table) else {
                continue;
            };
            let covered = match &grant.update {
                Some(Columns::All) => true,
                Some(Columns::List(columns)) => !columns.is_empty(),
                None => false,
            };
            if covered {
                continue;
            }
            grant.update = Some(
                lock_update(grant.select.as_ref())
                    .unwrap_or_else(|| Columns::List(BTreeSet::new())),
            );
            grant.lock_only = true;
        }
    }

    /// The tables whose row lock this grant cannot name a column for (D3c).
    ///
    /// A statement that locks a table without reading a column of it
    /// (`SELECT 1 FROM t FOR UPDATE`, `SELECT * FROM t FOR UPDATE`) leaves the
    /// `update` entry empty: a table grant never takes `"*"` for a lock, because
    /// `"*"` would hand out the UPDATE privilege on every column of the table
    /// (§6). Such a table is *named* without being granted, the statement is not
    /// allowed by this grant, and a caller that offered the grant as a
    /// replacement for a route's exact queries has to say so and keep the
    /// statement as an exact `queries` entry.
    pub fn locks_without_a_column(&self) -> impl Iterator<Item = &str> {
        self.tables
            .iter()
            .filter(|(_, grant)| {
                grant.lock_only
                    && matches!(&grant.update, Some(Columns::List(columns)) if columns.is_empty())
            })
            .map(|(table, _)| table.as_str())
    }

    /// Names one column for every lock left without one, from what *this* grant
    /// reads.
    ///
    /// It is what makes the covering grant of several statements better than any
    /// one of them: a statement may lock a table it reads no column of while
    /// another statement of the same route reads one, and PostgreSQL asks for
    /// the UPDATE privilege on any single column of a locked table. A lock with
    /// no column to name in the whole grant is left alone.
    pub fn pick_locking_columns(&mut self) {
        let picks: Vec<(String, Columns)> = self
            .tables
            .iter()
            .filter(|(_, grant)| {
                grant.lock_only
                    && matches!(&grant.update, Some(Columns::List(columns)) if columns.is_empty())
            })
            .filter_map(|(table, grant)| {
                lock_update(grant.select.as_ref()).map(|columns| (table.clone(), columns))
            })
            .collect();
        for (table, columns) in picks {
            if let Some(grant) = self.tables.get_mut(&table) {
                grant.update = Some(columns);
            }
        }
    }

    /// Renders the grants as the directives a view's `postgres` block holds,
    /// under `locator` (for example `view catalog postgres`, `view catalog
    /// postgres warehouse`, or `trait connection`).
    ///
    /// The output is what a manifest author pastes in to replace a list of exact
    /// queries: one `# <locator>` comment saying where it goes, a `functions …`
    /// line when the grant calls one beyond the safe set, then
    /// `select`/`insert`/`update`/`delete` for every table. Tables, columns and
    /// functions are sorted, so the same grants render the same text and a diff
    /// is readable. A grant with no directive at all renders as the empty
    /// string: a bare header would say nothing.
    ///
    /// A lock whose column this grant cannot name is written as a comment
    /// saying so, and the table is left without an `update` entry: it is the one
    /// thing a manifest author has to finish by hand, because a lock needs a
    /// column and a table grant has no `"*"` (§6, D3c).
    ///
    /// A column list that is empty — a table the statements merely *named*
    /// (`SELECT count(*) FROM t`) — has no spelling either: the grammar's
    /// `table_grant` needs at least one word, and `select t` would grant every
    /// column. So the entry is left out and the author is told to name a column,
    /// exactly as for a lock. Every line this function emits parses, and
    /// `crates/blastit-manifest/tests/grant_fragment.rs` proves it by pasting the
    /// value into a `view` and reading it back.
    ///
    /// A name the reader cannot write — a relation outside the default schema
    /// keeps its `schema.` prefix, and `audit.users` is not a table name the
    /// grammar takes — is never interpolated: written raw it is a refusal
    /// (`` `.` is only part of a qualified function name ``), and a name
    /// carrying a newline would end the line and become a directive of its own.
    /// The table is left out with one `#` comment naming it, like the two shapes
    /// above, so everything this function emits parses. `is_word` here is the
    /// reader's own rule.
    pub fn to_blastfile(&self, locator: &str) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        out.push_str(&format!("# {locator}\n"));
        // A function name is `Word ("." Word)*`, so a qualified name is fine and
        // anything else (a space, a quote, a control byte) is not: the writable
        // ones are one line, the rest one comment.
        let (functions, unwritable): (Vec<&String>, Vec<&String>) = self
            .functions
            .iter()
            .partition(|function| is_function_name(function));
        if !functions.is_empty() {
            out.push_str("functions ");
            out.push_str(
                &functions
                    .iter()
                    .map(|function| function.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push('\n');
        }
        for function in unwritable {
            out.push_str(&format!(
                "# functions: {function:?} is not a name a Blastfile writes (a name, or names \
                 joined by `.`), so it is left out — grant it in the block by hand.\n"
            ));
        }
        for (table, grant) in &self.tables {
            // The names come from the analysis, so a qualified relation reaches
            // here exactly as the analysis spells it. Rendering it raw is the
            // refusal above; a name carrying a newline would even become a
            // directive of its own. Comment instead, and never interpolate a name
            // that is not a word (the comment quotes it, so it stays escaped).
            let bad_column = [&grant.select, &grant.insert, &grant.update]
                .into_iter()
                .flatten()
                .filter_map(|columns| match columns {
                    Columns::All => None,
                    Columns::List(columns) => columns.iter().find(|column| !is_word(column)),
                })
                .next();
            if !is_word(table) || bad_column.is_some() {
                match bad_column {
                    Some(column) => out.push_str(&format!(
                        "# {table:?}: the column {column:?} is not a name a Blastfile writes \
                         (one word: letters, digits, `_` and an inner `-`), so no directive \
                         names it — write that grant by hand, or keep the statement an exact \
                         `query`.\n"
                    )),
                    None => out.push_str(&format!(
                        "# {table:?}: a table name is one word (letters, digits, `_` and an \
                         inner `-`), so no directive can name it — write that grant by hand, \
                         or keep the statement an exact `query`.\n"
                    )),
                }
                continue;
            }
            let unnamable = grant.lock_only
                && matches!(&grant.update, Some(Columns::List(columns)) if columns.is_empty());
            if grant.lock_only && !unnamable {
                let directive = match &grant.update {
                    Some(columns) => table_directive("update", table, columns),
                    None => format!("update {table}"),
                };
                out.push_str(&format!(
                    "# {table}: `{directive}` only locks its rows (`FOR UPDATE`) — any one \
                     column is enough, as PostgreSQL requires.\n"
                ));
            }
            if unnamable {
                out.push_str(&format!(
                    "# {table}: its rows are locked (`FOR UPDATE`) without reading a column of \
                     it, so name one here — its primary key, say: a table grant never takes \
                     `\"*\"` for a lock, and without the entry the statement is refused.\n"
                ));
            }
            // An operation whose column list is empty names the table and reads
            // no column. `unnamable` already explains why there is none, so its
            // own sentence is the advice and no second one repeats it.
            let nameless: Vec<&str> = if unnamable {
                Vec::new()
            } else {
                [
                    ("select", &grant.select),
                    ("insert", &grant.insert),
                    ("update", &grant.update),
                ]
                .into_iter()
                .filter(|(_, columns)| {
                    matches!(columns, Some(Columns::List(columns)) if columns.is_empty())
                })
                .map(|(operation, _)| operation)
                .collect()
            };
            if !nameless.is_empty() {
                out.push_str(&format!(
                    "# {table}: the statements only name it, so there is no column to write for \
                     {} — a table grant takes at least one, so name one here (its primary key, \
                     say): without the entry the statement is refused.\n",
                    nameless
                        .iter()
                        .map(|operation| format!("`{operation}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ));
            }
            for (operation, columns) in [
                ("select", &grant.select),
                ("insert", &grant.insert),
                ("update", &grant.update),
            ] {
                let Some(columns) = columns else {
                    continue;
                };
                if operation == "update" && unnamable {
                    continue;
                }
                // No grammar spelling for "names it, reads no column": the
                // advice above is the entry's whole rendering.
                if matches!(columns, Columns::List(columns) if columns.is_empty()) {
                    continue;
                }
                out.push_str(&table_directive(operation, table, columns));
                out.push('\n');
            }
            if grant.delete {
                out.push_str(&format!("delete {table}\n"));
            }
        }
        out
    }
}

/// Why a table grant refused a statement (§6, §8.1).
///
/// The message names the access that is missing, or the function — an app
/// author's next step — and never a parameter value: values are not part of a
/// policy check at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denied {
    reason: String,
}

impl Denied {
    /// A denial with its reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// The reason, as a `sql.denied` event's `reason` field carries it.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for Denied {}

impl From<Denied> for String {
    fn from(denied: Denied) -> Self {
        denied.reason
    }
}

/// The spelling of a table the analysis and a grant agree on.
pub(crate) fn canonical_table(table: &str) -> String {
    match table.strip_prefix("public.") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => table.to_string(),
    }
}

/// The spelling of a function the analysis and a grant agree on.
pub(crate) fn canonical_function(function: &str) -> String {
    match function.strip_prefix("pg_catalog.") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => function.to_string(),
    }
}

/// One `select`/`insert`/`update` directive: `select t(id, name)`, or `select t`
/// when the grant is every column.
fn table_directive(operation: &str, table: &str, columns: &Columns) -> String {
    match columns {
        Columns::All => format!("{operation} {table}"),
        Columns::List(columns) => format!("{operation} {table}({})", column_names(columns)),
    }
}

/// Whether `name` is one `Blastfile` word: the reader's own rule (§5.1).
///
/// A word is a non-empty run of ASCII letters, digits and `_`, where a `-` is
/// accepted only after one of those — which is exactly what
/// `crates/blastit-manifest/src/blastfile/lexer.rs`'s `word_run` consumes. A
/// name that fails it (a `.`, a space, a quote, a control byte) has no directive
/// spelling: `table_grant` takes one word and nothing else.
fn is_word(name: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in name.chars() {
        let accepted = match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => true,
            '-' => matches!(previous, Some(previous) if previous.is_ascii_alphanumeric()),
            _ => false,
        };
        if !accepted {
            return false;
        }
        previous = Some(character);
    }
    previous.is_some()
}

/// Whether `name` is a `Blastfile` function name: `Word ('.' Word)*`.
fn is_function_name(name: &str) -> bool {
    name.split('.').all(is_word)
}

/// A grant's column names, as a directive's `(…)` list holds them.
///
/// `Columns::List` is empty only for a table the statements merely *named*
/// (`SELECT count(*) FROM t`), and [`TableGrants::to_blastfile`] never calls this
/// for one: the grammar takes at least one column, so that entry is a comment.
fn column_names(columns: &BTreeSet<String>) -> String {
    columns.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// The `update` entry a row lock alone needs: one column the route already
/// reads.
///
/// PostgreSQL wants the UPDATE privilege on any one column of a locked table, so
/// the smallest covering grant names one instead of `"*"`: `id` when the route
/// reads it (a lock's `WHERE` usually goes through the key), otherwise the first
/// column it reads.
///
/// `None` when there is no column to name — the statement locked the table
/// without reading a column of it (`SELECT 1 FROM t FOR UPDATE`, or a `SELECT *`
/// that names none), or reads every column with `*`. A table grant has no `"*"`
/// for a lock: `"*"` would grant the UPDATE privilege on every column of the
/// table, which is exactly the privilege a lock does not need (§6). `None` means
/// the covering grant does not allow the statement, and a caller that offered to
/// write the grant has to say so instead.
fn lock_update(select: Option<&Columns>) -> Option<Columns> {
    let Some(Columns::List(columns)) = select else {
        return None;
    };
    let column = columns
        .iter()
        .find(|column| column.as_str() == "id")
        .or_else(|| columns.iter().next());
    column.map(|column| Columns::list([column.clone()]))
}
