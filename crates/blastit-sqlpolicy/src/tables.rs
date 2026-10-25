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
//! ```toml
//! [route.db.tables]
//! restaurants_restaurant = { select = ["id", "slug", "name"] }
//! restaurants_cuisinetype = { select = "*" }
//! orders_order = { select = "*", insert = "*", update = ["status"], delete = true }
//!
//! [route.db]
//! functions = ["st_distance", "unaccent"]
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
    /// [`TableGrants::to_toml`], which says so in a comment.
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
    /// column it reads, and `"*"` when it reads no column at all (a lock through
    /// a subquery names the table without reading a column of it, so there is no
    /// name to write). A table whose `update` entry a statement's own writes
    /// already filled keeps it: that entry covers the lock too.
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
            grant.update = Some(lock_update(grant.select.as_ref()));
            grant.lock_only = true;
        }
    }

    /// Renders the grants as the TOML a manifest holds, under the section
    /// `prefix` (for example `route.db`).
    ///
    /// The output is the text of one `[<prefix>.tables]` section plus, when
    /// there are any, a `functions` line under `[<prefix>]` — what a manifest
    /// author pastes in to replace a list of exact queries. Tables and columns
    /// are sorted, so the same grants render the same text and a diff is
    /// readable.
    pub fn to_toml(&self, prefix: &str) -> String {
        let mut out = String::new();
        out.push('[');
        out.push_str(prefix);
        out.push_str(".tables]\n");
        for (table, grant) in &self.tables {
            if grant.lock_only {
                let columns = match &grant.update {
                    Some(columns) => columns_toml(columns),
                    None => "[]".to_string(),
                };
                out.push_str(&format!(
                    "# {}: `update = {columns}` only locks its rows (`FOR UPDATE`) — any one \
                     column is enough, as PostgreSQL requires.\n",
                    key_toml(table),
                ));
            }
            let mut operations = Vec::new();
            for (name, columns) in [
                ("select", &grant.select),
                ("insert", &grant.insert),
                ("update", &grant.update),
            ] {
                if let Some(columns) = columns {
                    operations.push(format!("{name} = {}", columns_toml(columns)));
                }
            }
            if grant.delete {
                operations.push("delete = true".to_string());
            }
            out.push_str(&format!(
                "{} = {{ {} }}\n",
                key_toml(table),
                operations.join(", ")
            ));
        }
        if !self.functions.is_empty() {
            out.push('[');
            out.push_str(prefix);
            out.push_str("]\nfunctions = [");
            out.push_str(
                &self
                    .functions
                    .iter()
                    .map(|function| string_toml(function))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push_str("]\n");
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

/// One column set, as TOML.
fn columns_toml(columns: &Columns) -> String {
    match columns {
        Columns::All => "\"*\"".to_string(),
        Columns::List(columns) => format!(
            "[{}]",
            columns
                .iter()
                .map(|column| string_toml(column))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The `update` entry a row lock alone needs: one column the route already
/// reads.
///
/// PostgreSQL wants the UPDATE privilege on any one column of a locked table, so
/// the smallest covering grant names one instead of `"*"`: `id` when the route
/// reads it (a lock's `WHERE` usually goes through the key), otherwise the first
/// column it reads, and `"*"` when it reads none — a lock through a subquery can
/// name a table without reading a column of it, and then there is no name to
/// write.
fn lock_update(select: Option<&Columns>) -> Columns {
    let Some(Columns::List(columns)) = select else {
        return Columns::All;
    };
    let column = columns
        .iter()
        .find(|column| column.as_str() == "id")
        .or_else(|| columns.iter().next());
    match column {
        Some(column) => Columns::list([column.clone()]),
        None => Columns::All,
    }
}

/// A TOML key: bare when it can be, quoted when it cannot.
fn key_toml(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_string()
    } else {
        string_toml(key)
    }
}

/// A TOML basic string.
fn string_toml(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04X}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::analysis::analyze;

    fn checked(grants: &TableGrants, sql: &str) -> Result<(), Denied> {
        grants.check(&analyze(sql).expect("the statement is analysable"))
    }

    fn selecting(table: &str, columns: &[&str]) -> TableGrants {
        TableGrants::new(
            [(
                table.to_string(),
                TableGrant {
                    select: Some(Columns::list(columns.iter().copied())),
                    ..TableGrant::default()
                },
            )],
            [],
        )
    }

    #[test]
    fn a_column_list_allows_exactly_its_columns() {
        let grants = selecting("t", &["id", "name"]);
        assert!(checked(&grants, "SELECT id FROM t").is_ok());
        assert!(checked(&grants, "SELECT name FROM t WHERE id = $1").is_ok());
        let denied = checked(&grants, "SELECT password FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "select on t.password not granted");
    }

    #[test]
    fn a_star_grant_allows_a_whole_row_read_and_the_other_way_round() {
        let star = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&star, "SELECT * FROM t").is_ok());
        assert!(checked(&star, "SELECT id FROM t").is_ok());

        let listed = selecting("t", &["id"]);
        let denied = checked(&listed, "SELECT * FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "select on t.* not granted");
    }

    #[test]
    fn a_column_that_is_only_compared_still_needs_select() {
        let grants = selecting("t", &["id"]);
        let denied = checked(&grants, "SELECT 1 FROM t WHERE secret = $1").expect_err("denied");
        assert_eq!(denied.reason(), "select on t.secret not granted");
    }

    #[test]
    fn insert_update_and_delete_have_their_own_columns() {
        let grants = TableGrants::new(
            [(
                "orders_order".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    insert: Some(Columns::list(["status"])),
                    update: Some(Columns::list(["status", "accepted_at"])),
                    lock_only: false,
                    delete: false,
                },
            )],
            [],
        );
        assert!(checked(&grants, "INSERT INTO orders_order (status) VALUES ($1)").is_ok());
        assert!(
            checked(
                &grants,
                "UPDATE orders_order SET accepted_at = now() WHERE id = $1"
            )
            .is_ok()
        );
        let denied = checked(&grants, "UPDATE orders_order SET total = $1 WHERE id = $2")
            .expect_err("denied");
        assert_eq!(denied.reason(), "update on orders_order.total not granted");
        let denied = checked(&grants, "DELETE FROM orders_order").expect_err("denied");
        assert_eq!(denied.reason(), "delete on orders_order not granted");
        let denied = checked(
            &grants,
            "INSERT INTO orders_order (status, total) VALUES ($1, $2)",
        )
        .expect_err("denied");
        assert_eq!(denied.reason(), "insert on orders_order.total not granted");
    }

    #[test]
    fn an_insert_without_a_column_list_needs_every_column() {
        let grants = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    insert: Some(Columns::list(["id"])),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        let denied = checked(&grants, "INSERT INTO t VALUES ($1)").expect_err("denied");
        assert_eq!(denied.reason(), "insert on t.* not granted");
    }

    #[test]
    fn a_table_that_is_not_granted_at_all_is_refused() {
        let grants = selecting("t", &["id"]);
        let denied = checked(&grants, "SELECT id FROM other").expect_err("denied");
        assert_eq!(denied.reason(), "select on other.id not granted");
    }

    #[test]
    fn public_is_the_same_table_and_another_schema_is_not() {
        let grants = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&grants, "SELECT * FROM public.t").is_ok());
        assert!(checked(&grants, r#"SELECT * FROM "public"."t""#).is_ok());
        let denied = checked(&grants, "SELECT * FROM audit.t").expect_err("denied");
        assert_eq!(denied.reason(), "select on audit.t.* not granted");

        // ... and a grant may spell out another schema.
        let audit = TableGrants::new(
            [(
                "audit.t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&audit, "SELECT * FROM audit.t").is_ok());
        assert!(checked(&audit, "SELECT * FROM t").is_err());
    }

    #[test]
    fn the_system_catalogues_are_never_grantable() {
        // A qualified name is refused by the analysis itself, so the statement
        // cannot be granted by any grant.
        assert!(analyze("SELECT * FROM pg_catalog.pg_class").is_err());
        assert!(analyze("SELECT * FROM information_schema.tables").is_err());
        // ... and so is an unqualified `pg_*` name, which resolves into
        // `pg_catalog`; a grant key for it can never be written at all.
        assert!(analyze("SELECT * FROM pg_class").is_err());
        assert!(analyze("SELECT count(*) FROM pg_type").is_err());
        assert!(analyze("SELECT count(*) FROM tables").is_err());
    }

    #[test]
    fn functions_are_limited_to_the_safe_set_and_the_grant() {
        let grants = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&grants, "SELECT lower(name) FROM t").is_ok());
        let denied = checked(&grants, "SELECT unaccent(name) FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "the function unaccent is not granted");

        let mut with_function = grants.clone();
        with_function.functions.insert("unaccent".to_string());
        assert!(checked(&with_function, "SELECT unaccent(name) FROM t").is_ok());
        // A schema-qualified call to a granted function is the same function.
        assert!(
            checked(&with_function, "SELECT pg_catalog.unaccent(name) FROM t").is_ok(),
            "pg_catalog.unaccent is unaccent"
        );
    }

    #[test]
    fn a_statement_that_touches_nothing_passes_any_table_grant() {
        let grants = selecting("t", &["id"]);
        assert!(checked(&grants, "SELECT 1").is_ok());
        assert!(checked(&grants, "SELECT now()").is_ok());
        assert!(checked(&grants, "VALUES (1)").is_ok());
    }

    #[test]
    fn merging_unions_operations_and_columns() {
        let mut merged = selecting("t", &["id"]);
        merged.merge_from(&TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::list(["name"])),
                    delete: true,
                    ..TableGrant::default()
                },
            )],
            [],
        ));
        assert!(checked(&merged, "SELECT id, name FROM t").is_ok());
        assert!(checked(&merged, "DELETE FROM t").is_ok());
        assert!(checked(&merged, "SELECT password FROM t").is_err());

        // `"*"` absorbs a column list, whichever side it is on.
        let mut merged = selecting("t", &["id"]);
        merged.merge_from(&TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        ));
        assert!(checked(&merged, "SELECT * FROM t").is_ok());
    }

    #[test]
    fn an_empty_grant_is_empty() {
        assert!(TableGrants::default().is_empty());
        assert!(
            TableGrants::new(
                [(
                    "t".to_string(),
                    TableGrant {
                        select: None,
                        insert: None,
                        update: None,
                        lock_only: false,
                        delete: false,
                    },
                )],
                [],
            )
            .is_empty(),
            "a table with no operation grants nothing"
        );
        assert!(!selecting("t", &["id"]).is_empty());
        let mut functions = TableGrants::default();
        functions.functions.insert("unaccent".to_string());
        assert!(!functions.is_empty());
    }

    #[test]
    fn a_covering_grant_is_the_smallest_one_that_fits() {
        let analyses = [
            analyze("SELECT id, name FROM t WHERE id = $1").unwrap(),
            analyze("SELECT name FROM t").unwrap(),
            analyze("INSERT INTO t (id) VALUES ($1)").unwrap(),
            analyze("DELETE FROM t WHERE id = $1").unwrap(),
            analyze("SELECT unaccent(name) FROM t").unwrap(),
        ];
        let covering = TableGrants::covering(analyses.iter());
        assert_eq!(
            covering.to_toml("route.db"),
            "[route.db.tables]\n\
             t = { select = [\"id\", \"name\"], insert = [\"id\"], delete = true }\n\
             [route.db]\n\
             functions = [\"unaccent\"]\n"
        );
        // The covering grant allows every statement it was built from.
        for analysis in &analyses {
            assert!(covering.check(analysis).is_ok(), "{analysis:?}");
        }
        // ... and nothing more.
        assert!(checked(&covering, "SELECT password FROM t").is_err());
        assert!(checked(&covering, "SELECT id FROM t, u").is_err());
    }

    #[test]
    fn a_covering_grant_of_a_whole_row_read_asks_for_every_column() {
        let analyses = [analyze("SELECT * FROM t").unwrap()];
        assert_eq!(
            TableGrants::covering(analyses.iter()).to_toml("route.db"),
            "[route.db.tables]\n\
             t = { select = \"*\" }\n"
        );
    }

    #[test]
    fn covering_an_empty_set_is_empty() {
        let covering = TableGrants::covering(std::iter::empty());
        assert!(covering.is_empty());
        assert_eq!(covering.to_toml("route.db"), "[route.db.tables]\n");
    }

    #[test]
    fn toml_quotes_what_it_has_to() {
        let grants = TableGrants::new(
            [(
                "audit.users".to_string(),
                TableGrant {
                    select: Some(Columns::list([r#"we"ird"#])),
                    ..TableGrant::default()
                },
            )],
            ["my schema.f".to_string()],
        );
        assert_eq!(
            grants.to_toml("route.db"),
            "[route.db.tables]\n\
             \"audit.users\" = { select = [\"we\\\"ird\"] }\n\
             [route.db]\n\
             functions = [\"my schema.f\"]\n"
        );
        let _ = grants;
    }

    #[test]
    fn a_grant_built_from_a_pool_keeps_canonical_names() {
        let grants = TableGrants::new(
            [(
                "public.t".to_string(),
                TableGrant {
                    select: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            ["pg_catalog.lower".to_string()],
        );
        assert!(grants.table("t").is_some());
        assert!(grants.table("public.t").is_some(), "looked up canonically");
        assert!(grants.functions().any(|name| name == "lower"));
        assert!(!grants.is_empty());
    }

    //
    // D7 review round: the rules the first round missed.
    //

    #[test]
    fn a_table_named_without_a_column_read_needs_a_select_entry() {
        // A table with no grant at all refuses even a `count(*)`.
        let denied =
            checked(&TableGrants::default(), "SELECT count(*) FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "select on t not granted");
        // A grant with only an `insert` does too: naming the table is a read.
        let insert_only = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    insert: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&insert_only, "SELECT count(*) FROM t").is_err());
        // Any `select` entry is enough: a column list, even an empty one, or
        // `"*"`.
        assert!(checked(&selecting("t", &["id"]), "SELECT count(*) FROM t").is_ok());
        let mut empty = TableGrants::default();
        empty.tables.insert(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::List(BTreeSet::new())),
                ..TableGrant::default()
            },
        );
        assert!(checked(&empty, "SELECT count(*) FROM t").is_ok());
        // ... but an empty list reads no column.
        let denied = checked(&empty, "SELECT id FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "select on t.id not granted");
    }

    #[test]
    fn a_select_with_an_empty_list_is_not_an_empty_grant() {
        // `select = []` grants naming the table, so the route still needs its
        // listener: the grant is not empty.
        let mut grants = TableGrants::default();
        grants.tables.insert(
            "t".to_string(),
            TableGrant {
                select: Some(Columns::List(BTreeSet::new())),
                ..TableGrant::default()
            },
        );
        assert!(!grants.is_empty());
    }

    #[test]
    fn covering_a_count_only_table_asks_for_an_empty_select() {
        let analyses = [
            analyze("SELECT count(*) FROM t").unwrap(),
            analyze("SELECT id FROM t").unwrap(),
        ];
        // The column read absorbs the naming: `select = ["id"]` covers both.
        assert_eq!(
            TableGrants::covering(analyses.iter()).to_toml("route.db"),
            "[route.db.tables]\n\
             t = { select = [\"id\"] }\n"
        );
        let covering = TableGrants::covering(analyses.iter());
        for analysis in &analyses {
            assert!(covering.check(analysis).is_ok(), "{analysis:?}");
        }

        // A table only ever counted gets the naming grant, and still a listener.
        let counted = [analyze("SELECT count(*) FROM t").unwrap()];
        let covering = TableGrants::covering(counted.iter());
        assert_eq!(
            covering.to_toml("route.db"),
            "[route.db.tables]\n\
             t = { select = [] }\n"
        );
        assert!(!covering.is_empty());
        assert!(covering.check(&counted[0]).is_ok());
    }

    #[test]
    fn locking_a_derived_relation_needs_update_on_its_base_table() {
        let grants = selecting("auth_user", &["id"]);
        // A plain lock needs `update`, on any column of the table. The reason
        // says so: PostgreSQL asks for the UPDATE privilege on at least one
        // column of a locked table.
        let denied = checked(&grants, "SELECT id FROM auth_user FOR UPDATE").expect_err("denied");
        assert_eq!(
            denied.reason(),
            "update on auth_user (any column, to lock its rows) not granted"
        );
        // Through a subquery, too: the statement reads no column of the table,
        // but PostgreSQL locks its rows.
        let denied = checked(
            &grants,
            "SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR UPDATE",
        )
        .expect_err("denied");
        assert_eq!(
            denied.reason(),
            "update on auth_user (any column, to lock its rows) not granted"
        );
        // With the `update` grant, both are allowed — and `"*"` is not the only
        // way: one column is enough, exactly as PostgreSQL requires.
        for update in [
            Columns::All,
            Columns::list(["id"]),
            Columns::list(["status"]),
        ] {
            let mut with_update = grants.clone();
            with_update
                .tables
                .entry("auth_user".to_string())
                .or_default()
                .update = Some(update.clone());
            assert!(
                checked(
                    &with_update,
                    "SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR UPDATE"
                )
                .is_ok(),
                "update = {update:?} locks the rows"
            );
        }
    }

    #[test]
    fn a_lock_is_not_satisfied_by_an_empty_update_list() {
        // `update = []` grants no column, so it grants no lock either: there is
        // nothing to lock the rows *with*.
        let mut grants = selecting("t", &["id"]);
        grants.tables.entry("t".to_string()).or_default().update =
            Some(Columns::List(BTreeSet::new()));
        let denied = checked(&grants, "SELECT id FROM t FOR UPDATE").expect_err("denied");
        assert_eq!(
            denied.reason(),
            "update on t (any column, to lock its rows) not granted"
        );
        // An `insert` entry is a different operation: it does not lock.
        let insert_only = TableGrants::new(
            [(
                "t".to_string(),
                TableGrant {
                    select: Some(Columns::list(["id"])),
                    insert: Some(Columns::All),
                    ..TableGrant::default()
                },
            )],
            [],
        );
        assert!(checked(&insert_only, "SELECT id FROM t FOR UPDATE").is_err());
    }

    #[test]
    fn covering_a_locked_table_asks_for_one_column_not_every_column() {
        // The Django shape (D7b): a join locked with `select_for_update()` reads
        // its columns and writes none, so the covering grant needs no more than
        // `update = ["id"]` per locked table.
        let analyses =
            [
                analyze("SELECT t.id, t.status, u.id FROM t JOIN u ON u.t_id = t.id FOR UPDATE")
                    .unwrap(),
            ];
        let covering = TableGrants::covering(analyses.iter());
        assert_eq!(
            covering.to_toml("route.db"),
            "[route.db.tables]\n\
             # t: `update = [\"id\"]` only locks its rows (`FOR UPDATE`) — any one column is \
             enough, as PostgreSQL requires.\n\
             t = { select = [\"id\", \"status\"], update = [\"id\"] }\n\
             # u: `update = [\"id\"]` only locks its rows (`FOR UPDATE`) — any one column is \
             enough, as PostgreSQL requires.\n\
             u = { select = [\"id\", \"t_id\"], update = [\"id\"] }\n"
        );
        assert!(covering.check(&analyses[0]).is_ok());
        // ... and it is still the smallest one that fits: another column of `t`
        // is not granted.
        assert!(checked(&covering, "SELECT password FROM t").is_err());
    }

    #[test]
    fn covering_a_locked_table_picks_the_column_it_reads() {
        // `id` when the route reads one (the key a lock's `WHERE` goes through)…
        let key = [analyze("SELECT t.status, t.id FROM t FOR UPDATE").unwrap()];
        assert_eq!(
            TableGrants::covering(key.iter()).to_toml("route.db"),
            "[route.db.tables]\n\
             # t: `update = [\"id\"]` only locks its rows (`FOR UPDATE`) — any one column is \
             enough, as PostgreSQL requires.\n\
             t = { select = [\"id\", \"status\"], update = [\"id\"] }\n"
        );
        // … otherwise the first column it reads.
        let first = [analyze("SELECT t.status, t.name FROM t FOR UPDATE").unwrap()];
        assert_eq!(
            TableGrants::covering(first.iter()).to_toml("route.db"),
            "[route.db.tables]\n\
             # t: `update = [\"name\"]` only locks its rows (`FOR UPDATE`) — any one column is \
             enough, as PostgreSQL requires.\n\
             t = { select = [\"name\", \"status\"], update = [\"name\"] }\n"
        );
        // A lock through a subquery reads no column of the table it locks, so
        // there is no name to pick: the suggestion is `"*"`.
        let none = [analyze("SELECT 1 FROM (SELECT 1 FROM t) s FOR UPDATE").unwrap()];
        let covering = TableGrants::covering(none.iter());
        assert_eq!(
            covering.to_toml("route.db"),
            "[route.db.tables]\n\
             # t: `update = \"*\"` only locks its rows (`FOR UPDATE`) — any one column is \
             enough, as PostgreSQL requires.\n\
             t = { select = [], update = \"*\" }\n"
        );
        assert!(covering.check(&none[0]).is_ok());
        // A table whose own `UPDATE` already names columns keeps them: they lock
        // the rows just as well, and no comment claims the entry is only a lock.
        let written = [
            analyze("SELECT id FROM t FOR UPDATE").unwrap(),
            analyze("UPDATE t SET status = $1 WHERE id = $2").unwrap(),
        ];
        assert_eq!(
            TableGrants::covering(written.iter()).to_toml("route.db"),
            "[route.db.tables]\n\
             t = { select = [\"id\"], update = [\"status\"] }\n"
        );
    }

    #[test]
    fn a_cast_that_can_run_code_is_denied_unless_granted() {
        let grants = selecting("t", &["a"]);
        assert!(checked(&grants, "SELECT a::text FROM t").is_ok());
        let denied = checked(&grants, "SELECT a::regclass FROM t").expect_err("denied");
        assert_eq!(denied.reason(), "the function regclass is not granted");
        let denied = checked(&grants, "SELECT a::myschema.mytype FROM t").expect_err("denied");
        assert_eq!(
            denied.reason(),
            "the function myschema.mytype is not granted"
        );

        let mut with_type = grants.clone();
        with_type.functions.insert("regclass".to_string());
        assert!(checked(&with_type, "SELECT a::regclass FROM t").is_ok());
        assert!(checked(&with_type, "SELECT a::regclass[] FROM t").is_ok());
    }
}
