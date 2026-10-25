//! What a statement touches: tables, columns and functions (§6, D7).
//!
//! A `postgresql` grant can allow a statement two ways: because its exact
//! normalised text is listed (§6, "Matching"), or because **every table and
//! column it touches** is granted by [`crate::TableGrants`]. This module is the
//! second half: it turns the text on the wire into the accesses a grant has to
//! cover.
//!
//! # PostgreSQL's parser, not a second one
//!
//! The statement is parsed by `libpg_query`, PostgreSQL server's own parser
//! compiled as a library. Re-implementing the grammar (with `sqlparser`, say)
//! would be a *parser differential*: any text the two read differently is a
//! bypass, and the text that matters is the one **PostgreSQL** reads. Using the
//! server's parser is what makes the analysis below a statement about what the
//! server will run.
//!
//! # Deny on anything not understood
//!
//! The analysis is deliberately conservative, and the absence of an access is
//! never assumed:
//!
//! * exactly one statement, of kind `SELECT`, `INSERT`, `UPDATE` or `DELETE`
//!   (a `WITH` clause included). Everything else — DDL, `SET`, `COPY`, `LOCK`,
//!   `DO`, `CALL`, `EXPLAIN`, `PREPARE`/`EXECUTE`, transaction control — is left
//!   to the exact-match grants and to `transaction_control`;
//! * every column reference is resolved through the range table and the scope
//!   (a correlated subquery sees the outer scopes) to a `(table, column)` pair.
//!   Reading a column *anywhere* — only in a `WHERE`, an `ORDER BY`, a join
//!   condition — needs `select` on it, because a value that is compared or
//!   sorted is a value the statement can learn;
//! * `*` and `t.*` need `select = "*"` on every base table they expand to,
//!   through derived relations; `count(*)` reads no column at all;
//! * a function call this build does not know is denied unless the grant lists
//!   it ([`SAFE_FUNCTIONS`] is the built-in set), and a cast to a type that can
//!   run code — a `reg*` type, a domain, any type outside the built-in scalar
//!   set — is denied the same way, under the type's name;
//! * every base table a statement *names* needs a `select` grant, even when it
//!   reads no column of it: `SELECT count(*) FROM t` and `SELECT 1 FROM t` both
//!   leak the row count, and `EXISTS (SELECT 1 FROM t …)` its emptiness, so
//!   naming `t` is touching it (`select on t` in a denial, covered by any
//!   `select = […]` entry). A write target (`INSERT INTO t`, `UPDATE t`,
//!   `DELETE FROM t`) is named by its own `insert`/`update`/`delete` grant, not
//!   by this rule.
//!
//! A derived relation carries the base tables its definition **names**, not only
//! the ones it read a column from: that is what makes a locking clause require
//! `update` on the tables under a subquery or a CTE — `SELECT 1 FROM (SELECT 1
//! FROM auth_user) s FOR UPDATE` locks `auth_user` rows, so the grant has to say
//! `update` on `auth_user`.
//!
//! Two mechanisms keep that honest as the code grows. Every node type the
//! resolver handles explicitly (and consumes itself, like a `JOIN` or an
//! `ON CONFLICT` clause) is registered as *visited* when it is handled, and the
//! whole parse tree is scanned afterwards for nodes of those types: a node the
//! traversal never reached — a clause field someone forgot to read — refuses the
//! statement instead of silently contributing no access. And the walk has a
//! depth limit, so a pathological statement is refused rather than allowed to
//! exhaust dbproxy's stack.
//!
//! # The cache
//!
//! Parsing is expensive and an app runs the same statements over and over, so
//! analyses are cached by the statement's **normalised** text (§6): the shape
//! the policy compares, not the exact bytes. Two texts that normalise alike
//! differ only in the number of placeholders, of `IN` list entries or of
//! identical `VALUES` rows, and none of those changes which table, column or
//! function a statement touches — which is why the shape decides what an exact
//! grant matches as well. The cache is bounded ([`CACHE_CAPACITY`] entries,
//! least recently used first), because its keys come from an untrusted process.
//!
//! # Example
//!
//! ```
//! use blastit_sqlpolicy::{Access, Op, analyze};
//!
//! let analysis = analyze(
//!     r#"SELECT "shop_product"."id" FROM "shop_product" WHERE "shop_product"."name" = $1"#,
//! )
//! .expect("a plain SELECT is analysed");
//! let accesses: Vec<&Access> = analysis.accesses().collect();
//! assert_eq!(
//!     accesses,
//!     [
//!         &Access::new(Op::Select, "shop_product", Some("id")),
//!         &Access::new(Op::Select, "shop_product", Some("name")),
//!     ],
//! );
//! assert_eq!(analysis.functions().count(), 0);
//!
//! // A statement the analysis does not understand is refused, never allowed.
//! assert!(analyze("SET search_path = public").is_err());
//! ```

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use crate::lru::Lru;

#[cfg(test)]
mod tests;

/// How many analyses one process keeps.
///
/// A real route sends on the order of a hundred distinct statements (a Django
/// admin can send a few hundred); the cache is a per-process memo, and 512
/// entries of a few hundred bytes each is well under a megabyte.
pub(crate) const CACHE_CAPACITY: usize = 512;

/// How deep the walked parse tree may be before the statement is refused.
///
/// PostgreSQL bounds a parse tree by its own stack limit, and no real statement
/// comes close to this; the limit is here so that a hostile statement cannot
/// exhaust dbproxy's stack (dbproxy is the process holding the credentials).
const MAX_DEPTH: usize = 128;

/// The functions a statement may call without listing them in a grant (§6).
///
/// Every entry is a function that reads nothing outside its arguments: an
/// aggregate, a string or arithmetic function, or a set-returning function of
/// its input. What is *not* here is the point of the list: `nextval`,
/// `set_config`, `current_setting`, `pg_read_file`, `pg_ls_dir`, `lo_*`,
/// `dblink`, `pg_sleep`, `pg_terminate_backend` and every other function that
/// touches something beyond its arguments. A grant's own `functions` list
/// ([`crate::TableGrants`]) extends this set for one route.
///
/// A few entries exist because PostgreSQL's *parser* generates them: `SIMILAR TO`
/// becomes `pg_catalog.similar_to_escape`, `TRIM(x)` `pg_catalog.btrim`,
/// `EXTRACT(…)` `pg_catalog.extract`, `x AT TIME ZONE y` `pg_catalog.timezone`,
/// and `SUBSTRING`/`POSITION`/`OVERLAY` similarly. Nobody can write those names
/// in a grant, so leaving them out would only deny standard SQL. The
/// `pg_catalog.` prefix is dropped before the lookup, so `pg_catalog.lower` and
/// `lower` are one name.
pub const SAFE_FUNCTIONS: &[&str] = &[
    // Aggregates.
    "array_agg",
    "avg",
    "bool_and",
    "bool_or",
    "count",
    "json_agg",
    "json_build_object",
    "jsonb_agg",
    "jsonb_build_object",
    "max",
    "min",
    "string_agg",
    "sum",
    // Conditions. (`coalesce`, `nullif`, `greatest` and `least` are parse-tree
    // nodes of their own; listing them keeps the set the one §6 names.)
    "coalesce",
    "greatest",
    "least",
    "nullif",
    // Strings.
    "btrim",
    "concat",
    "left",
    "length",
    "lower",
    "ltrim",
    "overlay",
    "position",
    "replace",
    "right",
    "rtrim",
    "similar_to_escape",
    "substr",
    "substring",
    "trim",
    "upper",
    // Numbers.
    "abs",
    "ceil",
    "floor",
    "round",
    // Dates and times.
    "age",
    "date_trunc",
    "extract",
    "make_interval",
    "now",
    "timezone",
    "to_char",
    // Sets and arrays.
    "array_length",
    "cardinality",
    "generate_series",
    "unnest",
    // Window functions.
    "dense_rank",
    "rank",
    "row_number",
    // Window and ordered-set functions: they read their arguments and the
    // window, and nothing else.
    "lag",
    "lead",
    "ntile",
    "cume_dist",
    "percent_rank",
    "first_value",
    "last_value",
    "nth_value",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "mode",
    "percentile_cont",
    "percentile_disc",
    // Full-text search and JSON extraction: pure functions of their arguments.
    // (`unaccent` is deliberately absent: it reads a rule file, so a grant has
    // to name it in `functions`.)
    "to_tsvector",
    "plainto_tsquery",
    "websearch_to_tsquery",
    "ts_rank",
    "jsonb_extract_path_text",
    "json_extract_path_text",
    "jsonb_array_length",
];

/// Whether a function call needs no entry in a grant's `functions` list.
///
/// The name is compared as [`Analysis::functions`] reports it, with the
/// `pg_catalog.` prefix dropped. The comparison is exact, so a call to
/// `"Lower"` — a different function, quoted in the SQL — is not `lower`.
pub fn is_safe_function(name: &str) -> bool {
    SAFE_FUNCTIONS.contains(&name)
}

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
/// column list, a `DELETE`, or the `FOR UPDATE` lock of a table. The table is
/// spelled the way a grant has to spell it: `public.t` is `t`, any other schema
/// keeps its qualified name.
///
/// There is a third, weaker shape: a table the statement only *names* —
/// `SELECT count(*) FROM t` — needs some `select` entry, but no particular
/// column. [`Access::named`] is that access, and it is what
/// [`Op::Select`] on a table with no column read turns into.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Access {
    op: Op,
    table: String,
    extent: Extent,
}

/// How much of a table one [`Access`] needs.
///
/// Ordered so that a `Named` access sorts before a whole-row one and a column
/// one: the first access a denial reports is the first thing a grant author
/// would have to add.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Extent {
    /// The table is named, but no column of it is read: any `select` entry
    /// covers it.
    Named,
    /// Every column: `select = "*"`, `insert = "*"`, a `DELETE`.
    Whole,
    /// One column, by the name PostgreSQL resolved.
    Column(String),
}

impl Access {
    /// One access of a statement.
    ///
    /// `column` of `None` is a whole-row access (`select on t.*`), `Some` a
    /// column one (`select on t.id`); [`Access::named`] is the table-only form.
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

    /// The privilege this access needs.
    pub fn op(&self) -> Op {
        self.op
    }

    /// The table's name, as a grant spells it.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The column, or `None` for a whole-table or table-only access.
    pub fn column(&self) -> Option<&str> {
        match &self.extent {
            Extent::Column(column) => Some(column),
            Extent::Named | Extent::Whole => None,
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
    /// to say; a table-only access is spelled by the table's name alone.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.extent, self.op) {
            (Extent::Column(column), _) => write!(f, "{} on {}.{}", self.op, self.table, column),
            (_, Op::Delete) => write!(f, "delete on {}", self.table),
            (Extent::Named, _) => write!(f, "{} on {}", self.op, self.table),
            (Extent::Whole, _) => write!(f, "{} on {}.*", self.op, self.table),
        }
    }
}

/// Every table, column and function one statement touches (§6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    kind: StatementKind,
    accesses: BTreeSet<Access>,
    functions: BTreeSet<String>,
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
    /// and `lower` are one name — and one [`SAFE_FUNCTIONS`] entry.
    pub fn functions(&self) -> impl Iterator<Item = &str> {
        self.functions.iter().map(String::as_str)
    }

    /// The function calls a grant has to list: everything in
    /// [`Analysis::functions`] that is not in [`SAFE_FUNCTIONS`] — what
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

/// Analyses one statement (§6).
///
/// Returns what the statement reads and writes, table by table and column by
/// column, plus the functions it calls — or why it cannot be analysed.
///
/// # Errors
///
/// [`AnalysisError`] when the text is not exactly one statement of a kind this
/// build analyses, or when it uses a construct the analysis refuses. Nothing is
/// guessed at: a statement this function refuses is one a table grant can never
/// allow.
pub fn analyze(sql: &str) -> Result<Analysis, AnalysisError> {
    analyze_cached(sql, &crate::normalize(sql)).as_ref().clone()
}

/// [`analyze`], cached by the statement's normalised text.
///
/// The cache is process-wide: several routes, and several connections of one
/// route, send the same statements. A statement that fails to analyse is cached
/// too — reaching a refusal costs a parse as well, and a view that keeps sending
/// it should not cost one every time.
///
/// **`sql` is the text that is parsed**, so it is the statement as PostgreSQL
/// receives it, with `$n` placeholders: the normalised form (`$?`) is not valid
/// SQL and would be refused. `normalized` is only the key, which is why two
/// texts that normalise alike — one `IN ($1)` and one `IN ($1, $2, $3)` — share
/// an entry, and why the *first* of them decides what the others get: the shape
/// is what the accesses depend on (§6, D7). Every caller inside blastit hands
/// over the wire text, so this only matters to a caller of the public
/// [`analyze`].
pub(crate) fn analyze_cached(sql: &str, normalized: &str) -> Cached {
    if let Some(hit) = cache()
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(&normalized.to_string()).cloned())
    {
        return hit;
    }
    let analysis = Arc::new(resolve(sql));
    if let Ok(mut cache) = cache().lock() {
        cache.insert(normalized.to_string(), Arc::clone(&analysis));
    }
    analysis
}

/// How many analyses the process is currently holding.
///
/// This is the size of a process-wide cache, not of one route's, and it exists
/// so that a test can see the bound hold.
pub fn cached_analyses() -> usize {
    cache().lock().map(|cache| cache.len()).unwrap_or_default()
}

/// One statement's analysis, shared so the cache can hand it out without
/// cloning it.
type Cached = Arc<Result<Analysis, AnalysisError>>;

/// The process-wide cache, built on first use.
fn cache() -> &'static Mutex<Lru<String, Cached>> {
    static CACHE: OnceLock<Mutex<Lru<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Lru::new(CACHE_CAPACITY)))
}

/// Parses and analyses one statement.
fn resolve(sql: &str) -> Result<Analysis, AnalysisError> {
    let parsed = pg_query::parse(sql).map_err(|error| AnalysisError::Syntax(syntax(&error)))?;
    let statements = &parsed.protobuf.stmts;
    match statements.len() {
        0 => return Err(AnalysisError::Empty),
        1 => {}
        count => return Err(AnalysisError::MultipleStatements(count)),
    }
    let statement = statements[0].stmt.as_ref().ok_or(AnalysisError::Empty)?;
    let node = statement.node.as_ref().ok_or(AnalysisError::Empty)?;
    let tree = serde_json::to_value(node)
        .map_err(|error| AnalysisError::NotUnderstood(format!("readable SQL: {error}")))?;

    let kind = match one_of(&tree).map(|(kind, _)| kind) {
        Some("SelectStmt") => StatementKind::Select,
        Some("InsertStmt") => StatementKind::Insert,
        Some("UpdateStmt") => StatementKind::Update,
        Some("DeleteStmt") => StatementKind::Delete,
        Some(other) => {
            return Err(AnalysisError::NotUnderstood(format!(
                "{}: only SELECT, INSERT, UPDATE and DELETE can be",
                statement_label(other)
            )));
        }
        None => return Err(AnalysisError::Empty),
    };

    let mut resolver = Resolver::default();
    resolver.statement(&tree, &Scope::default())?;
    resolver.check_coverage(&tree)?;
    // A table the statement names but reads no column of — `SELECT count(*) FROM
    // t`, `EXISTS (SELECT 1 FROM t)` — still tells the statement whether the
    // table exists and roughly how many rows it has, so naming it needs a grant
    // (§6, D7). A table with a `select` access already needs one, so only the
    // tables left without any get the weaker, table-only access.
    let named = std::mem::take(&mut resolver.named);
    for table in named {
        let read = resolver
            .accesses
            .iter()
            .any(|access| access.op() == Op::Select && access.table() == table);
        if !read {
            resolver.accesses.insert(Access::named(Op::Select, table));
        }
    }
    Ok(Analysis {
        kind,
        accesses: resolver.accesses,
        functions: resolver.functions,
    })
}

/// A one-line rendering of a parser error: `libpg_query` reports the position and
/// the message on separate lines, and an event is one line (§8.1).
///
/// The message is also cut short: a deeply nested statement makes the parser
/// report its whole path back to the root, and a `sql.denied` event's `reason`
/// is not the place for a kilobyte of it.
fn syntax(error: &pg_query::Error) -> String {
    const LIMIT: usize = 200;
    let text = error.to_string();
    let message = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("LINE ") && !line.starts_with('^'))
        .collect::<Vec<_>>()
        .join(" ");
    let message = message.trim().trim_end_matches(':').trim();
    let message = match message.char_indices().nth(LIMIT) {
        Some((at, _)) => format!("{}…", &message[..at]),
        None => message.to_string(),
    };
    if message.is_empty() {
        "invalid SQL".to_string()
    } else {
        message
    }
}

/// How a statement kind is named in a refusal.
fn statement_label(kind: &str) -> &str {
    match kind {
        "TransactionStmt" => "a transaction-control statement",
        "VariableSetStmt" => "SET",
        "VariableShowStmt" => "SHOW",
        "CopyStmt" => "COPY",
        "LockStmt" => "LOCK",
        "DoStmt" => "DO",
        "CallStmt" => "CALL",
        "ExplainStmt" => "EXPLAIN",
        "PrepareStmt" => "PREPARE",
        "ExecuteStmt" => "EXECUTE",
        "DeallocateStmt" => "DEALLOCATE",
        "DeclareCursorStmt" => "DECLARE CURSOR",
        "FetchStmt" => "FETCH",
        "ClosePortalStmt" => "CLOSE",
        "ListenStmt" => "LISTEN",
        "NotifyStmt" => "NOTIFY",
        "DiscardStmt" => "DISCARD",
        "MergeStmt" => "MERGE",
        "TruncateStmt" => "TRUNCATE",
        "GrantStmt" => "GRANT",
        "RevokeStmt" => "REVOKE",
        other => other,
    }
}

//
// The walker.
//

/// The node types the resolver handles itself (and therefore *must* visit).
///
/// The whole tree is scanned for nodes of these types after the walk, and a node
/// the walk never reached refuses the statement. That is what turns "a clause
/// field nobody reads" from a silent hole into a denial: any node of a handled
/// type somewhere the walker does not look is a bug, and a bug in a security
/// boundary has to fail closed.
const HANDLED: &[&str] = &[
    "CoalesceExpr",
    "ColumnRef",
    "CommonTableExpr",
    "DeleteStmt",
    "FuncCall",
    "InsertStmt",
    "IntoClause",
    "JoinExpr",
    "JsonAggConstructor",
    "JsonArgument",
    "JsonArrayAgg",
    "JsonArrayConstructor",
    "JsonArrayQueryConstructor",
    "JsonBehavior",
    "JsonConstructorExpr",
    "JsonExpr",
    "JsonFuncExpr",
    "JsonIsPredicate",
    "JsonKeyValue",
    "JsonObjectAgg",
    "JsonObjectConstructor",
    "JsonParseExpr",
    "JsonScalarExpr",
    "JsonSerializeExpr",
    "JsonValueExpr",
    "LockingClause",
    "MergeStmt",
    "MinMaxExpr",
    "OnConflictClause",
    "RangeFunction",
    "RangeSubselect",
    "RangeTableSample",
    "RangeVar",
    "SelectStmt",
    "SqlvalueFunction",
    "SubLink",
    "TypeCast",
    "UpdateStmt",
    "WithClause",
    "XmlExpr",
    "XmlSerialize",
];

/// What reading a column (or every column) of a relation needs.
///
/// The three parts are what makes `*` through a derived relation work: a named
/// column is an exact `(table, column)` pair, a table in `whole` is read
/// entirely whatever the statement asked for (`t.*`), and a table in `wildcard`
/// stands for whichever column the statement asked for — how a column read
/// through a derived relation whose output nobody can name is recorded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Reads {
    /// `(table, column)` pairs the statement reads by name.
    columns: BTreeSet<(String, String)>,
    /// Tables read entirely: `*`, a whole-row reference to a relation, or a
    /// part of a derived relation whose columns come from a `*`.
    whole: BTreeSet<String>,
    /// Tables where the column is not known: a part of a derived relation whose
    /// names come from a branch nobody can name.
    wildcard: BTreeSet<String>,
}

impl Reads {
    /// Counts one `(table, column)` read.
    fn column(&mut self, table: &str, column: &str) {
        self.columns.insert((table.to_string(), column.to_string()));
    }

    /// Counts one "read every column of `table`" (`t.*`).
    fn whole_table(&mut self, table: &str) {
        self.whole.insert(table.to_string());
    }

    /// Merges another reading (the two branches of a `UNION`, say).
    fn merge(&mut self, other: &Self) {
        self.columns.extend(other.columns.iter().cloned());
        self.whole.extend(other.whole.iter().cloned());
        self.wildcard.extend(other.wildcard.iter().cloned());
    }

    /// The same read, but where every wholly-read table becomes "the column the
    /// statement asked for".
    ///
    /// Reading a named column of a derived relation whose output is open (`s.x`
    /// where `s` is `SELECT * FROM t`) has to touch every table that output
    /// could come from; the column the statement named is the only one it can be
    /// held to, so the whole-table reads become wildcards — the `t.*` the
    /// definition itself needed is recorded there, not here.
    fn as_unknown_column(&self) -> Self {
        let mut reads = self.clone();
        reads.wildcard.extend(reads.whole.iter().cloned());
        reads.whole.clear();
        reads
    }

    /// The accesses this needs, read either as named columns, as whole rows
    /// (`*`) or, for a wildcard, as the name the statement used.
    fn accesses(&self, name: Option<&str>) -> Vec<Access> {
        let mut accesses =
            Vec::with_capacity(self.columns.len() + self.whole.len() + self.wildcard.len());
        for (table, column) in &self.columns {
            accesses.push(Access::new(Op::Select, table.clone(), Some(column)));
        }
        for table in &self.whole {
            accesses.push(Access::new(Op::Select, table.clone(), None));
        }
        for table in &self.wildcard {
            match name {
                Some(column) => accesses.push(Access::new(Op::Select, table.clone(), Some(column))),
                None => accesses.push(Access::new(Op::Select, table.clone(), None)),
            }
        }
        accesses
    }
}

/// The output columns of a derived relation: a subquery, a CTE, a `VALUES` list
/// or a set-returning function.
///
/// Reading one of its columns needs what the definition needed, which was
/// recorded when the definition was analysed; the only thing the output has to
/// carry is which base `(table, column)` pairs each output column stands for, so
/// that a rename (`SELECT password AS p`) cannot hide one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Derived {
    /// Output columns whose name is known.
    named: BTreeMap<String, Reads>,
    /// The part of the output whose names are not known — a `*` expansion, a
    /// set-returning function's columns, a computed expression. `None` when
    /// every output column is named, so a name that is not there is an unknown
    /// column and the statement is refused.
    open: Option<Reads>,
    /// The output columns in order, when they can be counted; `None` after a `*`
    /// expansion, whose column count is not known. Used to apply an
    /// `AS alias(a, b)` column list.
    positions: Option<Vec<(Option<String>, Reads)>>,
    /// The base tables the relation's definition **names**, whether or not it
    /// reads a column of them (§6, D7). That is what a locking clause has to
    /// cover: `SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR UPDATE` locks
    /// `auth_user` rows even though the outer statement reads no column at all.
    names: BTreeSet<String>,
}

impl Derived {
    /// The builder every statement handler fills in: a known count of columns.
    fn new() -> Self {
        Self {
            named: BTreeMap::new(),
            open: None,
            positions: Some(Vec::new()),
            names: BTreeSet::new(),
        }
    }

    /// A relation whose columns read nothing: a set-returning function's output,
    /// or the self-reference of a recursive CTE.
    fn passthrough() -> Self {
        Self {
            named: BTreeMap::new(),
            open: Some(Reads::default()),
            positions: None,
            names: BTreeSet::new(),
        }
    }

    /// Adds one output column.
    fn push(&mut self, name: Option<String>, reads: Reads) {
        match &name {
            Some(name) => {
                self.named.insert(name.clone(), reads.clone());
            }
            None => self.open.get_or_insert_with(Reads::default).merge(&reads),
        }
        if let Some(positions) = &mut self.positions {
            positions.push((name, reads));
        }
    }

    /// The base tables the relation's definition names.
    fn named_tables(&self) -> &BTreeSet<String> {
        &self.names
    }

    /// Forgets that the output's columns can be counted: a `*` expansion stands
    /// for however many columns the table has.
    fn unbounded(&mut self) {
        self.positions = None;
    }

    /// What reading every column of this relation needs.
    fn whole_row(&self) -> Reads {
        let mut reads = Reads::default();
        for column in self.named.values() {
            reads.merge(column);
        }
        if let Some(open) = &self.open {
            reads.merge(open);
        }
        reads
    }

    /// Renames the output columns positionally, as `AS v(a, b)` does. `None`
    /// when the output has no countable column list or a different length.
    fn rename(&self, names: &[String]) -> Option<Self> {
        let positions = self.positions.as_ref()?;
        if positions.len() != names.len() {
            return None;
        }
        let mut renamed = Self::new();
        renamed.names = self.names.clone();
        for (name, (_, reads)) in names.iter().zip(positions) {
            renamed.push(Some(name.clone()), reads.clone());
        }
        Some(renamed)
    }
}

/// A relation in scope: a base table, or something derived from one.
#[derive(Clone, Debug)]
enum Rel {
    /// A base table, named by the key a grant uses.
    Base { table: String },
    /// A relation built by the statement itself.
    Derived(Box<Derived>),
}

/// A relation in scope, and the name the statement uses for it.
#[derive(Clone, Debug)]
struct Named {
    /// The alias, or the table's own name. Empty for something unreferencable.
    name: String,
    rel: Rel,
}

impl Named {
    /// A base table under its own name.
    fn base(name: String, table: String) -> Self {
        Self {
            name,
            rel: Rel::Base { table },
        }
    }

    /// A derived relation under an alias.
    fn derived(name: String, derived: Derived) -> Self {
        Self {
            name,
            rel: Rel::Derived(Box::new(derived)),
        }
    }

    /// Whether an unqualified column name could be this relation's.
    ///
    /// A base table's columns are not known (there is no schema here), so it
    /// always could be; a derived relation only provides what its definition
    /// named, or anything at all when its output is open.
    fn provides(&self, column: &str) -> bool {
        match &self.rel {
            Rel::Base { .. } => true,
            Rel::Derived(derived) => derived.named.contains_key(column) || derived.open.is_some(),
        }
    }

    /// What reading every column of this relation needs.
    fn whole_row(&self) -> Reads {
        match &self.rel {
            Rel::Base { table } => {
                let mut reads = Reads::default();
                reads.whole_table(table);
                reads
            }
            Rel::Derived(derived) => derived.whole_row(),
        }
    }

    /// The base tables this relation's definition names, whether or not it
    /// reads a column of them.
    ///
    /// A base table is itself; a derived relation carries the set its definition
    /// collected. This is what a locking clause covers.
    fn named_tables(&self) -> BTreeSet<String> {
        match &self.rel {
            Rel::Base { table } => BTreeSet::from([table.clone()]),
            Rel::Derived(derived) => derived.named_tables().clone(),
        }
    }
}

/// One scope level: the relations and CTEs a statement can refer to.
#[derive(Clone, Debug, Default)]
struct Frame {
    rels: Vec<Named>,
    /// The CTEs this level defines, by name (§6: a CTE name shadows a table).
    ctes: BTreeMap<String, Derived>,
}

/// The nested scopes of one statement: innermost last, so a correlated subquery
/// can see the outer ones.
#[derive(Clone, Debug, Default)]
struct Scope {
    frames: Vec<Frame>,
}

impl Scope {
    /// The same scope with one more level on top.
    fn push(&self, frame: Frame) -> Self {
        let mut frames = self.frames.clone();
        frames.push(frame);
        Self { frames }
    }

    /// The innermost level, if there is one.
    fn innermost(&self) -> Option<&Frame> {
        self.frames.last()
    }

    /// The relation a name refers to, searched from the innermost scope out.
    fn relation(&self, name: &str) -> Option<&Named> {
        self.frames
            .iter()
            .rev()
            .flat_map(|frame| frame.rels.iter().rev())
            .find(|rel| rel.name == name)
    }

    /// The CTE a name refers to, searched from the innermost scope out.
    fn cte(&self, name: &str) -> Option<&Derived> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.ctes.get(name))
    }

    /// The innermost level that could provide an unqualified column name, with
    /// the relations that provide it.
    fn level_for(&self, column: &str) -> Option<&[Named]> {
        self.frames
            .iter()
            .rev()
            .map(|frame| frame.rels.as_slice())
            .find(|rels| rels.iter().any(|rel| rel.provides(column)))
    }
}

/// The statement walker.
#[derive(Default)]
struct Resolver {
    accesses: BTreeSet<Access>,
    functions: BTreeSet<String>,
    /// Every base table the statement names, whether or not it reads a column
    /// of it. A table in here with no `select` access gets a
    /// [`Access::named`] one, so naming a table is enough to need a grant for it
    /// (§6, D7).
    named: BTreeSet<String>,
    /// The nodes of [`HANDLED`] types the walk actually handled, by address, so
    /// that [`Resolver::check_coverage`] can prove nothing was skipped.
    visited: HashSet<usize>,
    depth: usize,
}

impl Resolver {
    /// Counts one access.
    fn access(&mut self, op: Op, table: &str, column: Option<&str>) {
        self.accesses
            .insert(Access::new(op, table.to_string(), column));
    }

    /// Remembers that the statement names a base table, even if it reads no
    /// column of it.
    fn name_table(&mut self, table: &str) {
        self.named.insert(table.to_string());
    }

    /// Counts every access a reading needs.
    fn accesses_of(&mut self, reads: &Reads, name: Option<&str>) {
        self.accesses.extend(reads.accesses(name));
    }

    /// Remembers that a node of a handled type was handled.
    fn mark(&mut self, node: &Value) {
        self.visited.insert(address(node));
    }

    /// Refuses the statement when the walk did not reach a node the tree holds.
    ///
    /// Every node of a [`HANDLED`] type contributes an access, or a table
    /// reference, that the walk is responsible for; one the walk never reached
    /// means a clause this build does not read, and a clause it does not read
    /// could hold a read it does not count.
    fn check_coverage(&self, tree: &Value) -> Result<(), AnalysisError> {
        let mut missing: Option<&str> = None;
        let mut stack = vec![tree];
        while let Some(value) = stack.pop() {
            let value = unwrap(value);
            if let Some((kind, body)) = one_of(value) {
                if HANDLED.contains(&kind) && !self.visited.contains(&address(value)) {
                    missing = missing.or(Some(kind));
                }
                if let Some(map) = body.as_object() {
                    stack.extend(map.values());
                }
            } else {
                match value {
                    Value::Object(map) => stack.extend(map.values()),
                    Value::Array(items) => stack.extend(items),
                    _ => {}
                }
            }
        }
        match missing {
            None => Ok(()),
            Some(kind) => Err(AnalysisError::NotUnderstood(format!(
                "this statement: it holds a {kind} the analysis does not read"
            ))),
        }
    }

    /// Walks one node, bounding the walk's depth.
    fn dispatch(&mut self, node: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        if self.depth >= MAX_DEPTH {
            return Err(AnalysisError::NotUnderstood(
                "a statement nested this deeply".to_string(),
            ));
        }
        self.depth += 1;
        let result = self.dispatch_here(node, scope);
        self.depth -= 1;
        result
    }

    /// Walks one node: the handled types explicitly, everything else by
    /// structure.
    fn dispatch_here(&mut self, node: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        let node = unwrap(node);
        let Some((kind, body)) = one_of(node) else {
            // A message that is not a `Node` — a `WindowDef` in `over`, an
            // `OnConflictClause`, a `RangeVar` in a DML target — is walked
            // field by field: it cannot be *named* here, but whatever it holds is
            // still walked, so nothing inside it is skipped.
            return self.walk(node, scope);
        };
        match kind {
            "ColumnRef" => {
                self.mark(node);
                let reads = self.column_ref(body, scope)?;
                let name = last_name(body);
                self.accesses_of(&reads, name.as_deref());
                Ok(())
            }
            "FuncCall" => {
                self.mark(node);
                self.func_call(body, scope)
            }
            "TypeCast" => {
                self.mark(node);
                self.type_cast(body, scope)
            }
            // PostgreSQL's parser builds these nodes for SQL that is not a
            // `FuncCall`: `CURRENT_TIMESTAMP`, `greatest`/`least`, `coalesce`,
            // `JSON_OBJECT`, `xmlelement`. They are classified here by hand, so
            // that a node whose operation nobody looked at is a refusal instead
            // of a silently-walked hole.
            //
            // `SqlvalueFunction` (a session value), `MinMaxExpr`
            // (`greatest`/`least`), `CoalesceExpr` and the SQL/JSON family are
            // pure: they read their arguments and nothing else, so walking their
            // children is the whole of their check.
            "SqlvalueFunction"
            | "MinMaxExpr"
            | "CoalesceExpr"
            | "JsonExpr"
            | "JsonConstructorExpr"
            | "JsonValueExpr"
            | "JsonKeyValue"
            | "JsonObjectConstructor"
            | "JsonArrayConstructor"
            | "JsonArrayQueryConstructor"
            | "JsonAggConstructor"
            | "JsonObjectAgg"
            | "JsonArrayAgg"
            | "JsonFuncExpr"
            | "JsonParseExpr"
            | "JsonScalarExpr"
            | "JsonSerializeExpr"
            | "JsonIsPredicate"
            | "JsonArgument"
            | "JsonBehavior" => {
                self.mark(node);
                self.walk(body, scope)
            }
            // XML is denied by default: this build does not model what
            // `xmlelement`, `xpath` or `IS DOCUMENT` do with their arguments, so
            // a grant cannot say what it is allowing.
            "XmlExpr" | "XmlSerialize" => {
                self.mark(node);
                Err(AnalysisError::NotUnderstood(
                    "an XML expression".to_string(),
                ))
            }
            "SubLink" => {
                self.mark(node);
                self.expression(body, "testexpr", scope)?;
                if let Some(subselect) = node_field(body, "subselect") {
                    self.statement(subselect, scope)?;
                }
                Ok(())
            }
            "SelectStmt" | "InsertStmt" | "UpdateStmt" | "DeleteStmt" => {
                self.mark(node);
                self.statement(node, scope).map(|_| ())
            }
            "MergeStmt" => {
                self.mark(node);
                Err(AnalysisError::NotUnderstood("MERGE".to_string()))
            }
            // These belong to a clause of a statement, never to an expression:
            // reaching one here means the walker is somewhere it does not know.
            other @ ("RangeVar" | "RangeSubselect" | "RangeFunction" | "RangeTableSample"
            | "JoinExpr" | "CommonTableExpr" | "WithClause" | "IntoClause"
            | "LockingClause" | "OnConflictClause") => {
                self.mark(node);
                Err(AnalysisError::NotUnderstood(format!(
                    "this statement: it holds a {other} the analysis does not read"
                )))
            }
            // Anything else is walked structurally: a node this build does not
            // name cannot hide a read, because every read is a `ColumnRef`, a
            // `FuncCall` or a nested statement, and those are named.
            _ => self.walk(body, scope),
        }
    }

    /// Walks the values of a node this build does not name.
    fn walk(&mut self, value: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        match value {
            Value::Object(map) => {
                // A `Node` message holds exactly one field, `node`.
                if map.len() == 1
                    && let Some(inner) = map.get("node")
                    && !inner.is_null()
                {
                    return self.dispatch(inner, scope);
                }
                for value in map.values() {
                    self.walk(value, scope)?;
                }
                Ok(())
            }
            Value::Array(items) => {
                for item in items {
                    self.walk(item, scope)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Walks a clause that is one expression, if it is there at all.
    fn expression(
        &mut self,
        body: &Value,
        field: &str,
        scope: &Scope,
    ) -> Result<(), AnalysisError> {
        match node_field(body, field) {
            Some(node) => self.dispatch(node, scope),
            None => Ok(()),
        }
    }

    /// Walks a clause that is either one expression or a list of them.
    ///
    /// A clause an attacker can make both shapes is walked for both: the field is
    /// a list in some statement kinds and a single expression in others
    /// (`limit_count` is one node; `group_clause` is a list).
    fn expressions(
        &mut self,
        body: &Value,
        field: &str,
        scope: &Scope,
    ) -> Result<(), AnalysisError> {
        match body.get(field) {
            Some(Value::Array(_)) => {
                for node in node_list(body, field) {
                    self.dispatch(node, scope)?;
                }
                Ok(())
            }
            _ => self.expression(body, field, scope),
        }
    }

    //
    // Statements.
    //

    /// The statement a node holds: the entry point, and what a CTE body and a
    /// subquery go through. Returns the statement's output columns, which a
    /// derived relation is built from.
    fn statement(&mut self, node: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let node = unwrap(node);
        let Some((kind, body)) = one_of(node) else {
            return Err(AnalysisError::Empty);
        };
        self.mark(node);
        match kind {
            "SelectStmt" => self.select(body, scope),
            "InsertStmt" => self.insert(body, scope),
            "UpdateStmt" => self.update(body, scope),
            "DeleteStmt" => self.delete(body, scope),
            "MergeStmt" => Err(AnalysisError::NotUnderstood("MERGE".to_string())),
            other => Err(AnalysisError::NotUnderstood(format!(
                "{}: only SELECT, INSERT, UPDATE and DELETE can be",
                statement_label(other)
            ))),
        }
    }

    /// The CTEs a `WITH` clause defines, as a scope that sees them.
    ///
    /// A CTE may refer to the ones written before it, and — with `RECURSIVE` —
    /// to itself: its own name is in scope as a relation whose columns read
    /// nothing, because what its columns come from *is* its body, which is
    /// analysed on its own.
    fn with_clause(&mut self, body: &Value, scope: &Scope) -> Result<Scope, AnalysisError> {
        let Some(clause) = node_field(body, "with_clause") else {
            return Ok(scope.clone());
        };
        self.mark(clause);
        let Some(clause) = typed_body(clause, "WithClause") else {
            return Err(AnalysisError::NotUnderstood("a WITH clause".to_string()));
        };
        let mut defined = BTreeMap::new();
        for cte in node_list(clause, "ctes") {
            self.mark(cte);
            let Some(cte) = typed_body(cte, "CommonTableExpr") else {
                return Err(AnalysisError::NotUnderstood("a WITH clause".to_string()));
            };
            let name = text(cte, "ctename").unwrap_or_default().to_string();
            let mut ctes = defined.clone();
            ctes.insert(name.clone(), Derived::passthrough());
            let inner = scope.push(Frame {
                rels: Vec::new(),
                ctes,
            });
            let query = node_field(cte, "ctequery")
                .ok_or_else(|| AnalysisError::NotUnderstood("an empty CTE".to_string()))?;
            let output = self.statement(query, &inner)?;
            let names = names_of(cte.get("aliascolnames"));
            let output = match names.is_empty() {
                true => output,
                false => output.rename(&names).ok_or_else(|| {
                    AnalysisError::NotUnderstood(
                        "a CTE whose column list does not match its body".to_string(),
                    )
                })?,
            };
            defined.insert(name, output);
        }
        Ok(scope.push(Frame {
            rels: Vec::new(),
            ctes: defined,
        }))
    }

    /// A `SELECT`, or a bare `VALUES`.
    fn select(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let scope = self.with_clause(body, scope)?;
        if node_field(body, "into_clause").is_some() {
            return Err(AnalysisError::NotUnderstood(
                "SELECT ... INTO: it creates a table".to_string(),
            ));
        }

        // A set operation (`UNION`/`INTERSECT`/`EXCEPT`) is a `SelectStmt` whose
        // two sides are statements of their own.
        if set_operation(body) {
            let left = self.branch(body, "larg", &scope)?;
            let right = self.branch(body, "rarg", &scope)?;
            let output = merge_outputs(left, right);
            // `ORDER BY`/`LIMIT` after a set operation name the *output* columns,
            // so they are resolved against the merged output.
            let scope = scope.push(Frame {
                rels: vec![Named::derived(String::new(), output.clone())],
                ctes: BTreeMap::new(),
            });
            self.expressions(body, "distinct_clause", &scope)?;
            for field in ["sort_clause", "limit_offset", "limit_count"] {
                self.expressions(body, field, &scope)?;
            }
            self.locking(body, &scope)?;
            return Ok(output);
        }

        let rels = self.collect_from(body, "from_clause", &scope)?;
        // The base tables the statement names, from its range table: what a
        // locking clause over this query has to cover, and what a derived
        // relation built from it carries (§6, D7).
        let names: BTreeSet<String> = rels.iter().flat_map(Named::named_tables).collect();
        let scope = scope.push(Frame {
            rels,
            ctes: BTreeMap::new(),
        });

        self.expressions(body, "distinct_clause", &scope)?;

        let mut output = Derived::new();
        output.names = names;
        for target in node_list(body, "target_list") {
            let (name, reads) = self.target(target, &scope)?;
            // A `*` expansion stands for as many columns as the table has, so
            // the output can no longer be counted.
            if name.is_none() && star_target(target) {
                output.unbounded();
            }
            output.push(name, reads);
        }

        let rows = node_list(body, "values_lists");
        if !rows.is_empty() {
            // A bare `VALUES (…), (…)`: what its expressions read is counted
            // here, and its columns are named by the alias of the `FROM` item it
            // ends up as, if it has one.
            let mut width = 0;
            for row in &rows {
                let Some(list) = typed_body(row, "List") else {
                    return Err(AnalysisError::NotUnderstood("a VALUES row".to_string()));
                };
                let items = node_list(list, "items");
                width = width.max(items.len());
                for item in items {
                    self.dispatch(item, &scope)?;
                }
            }
            for _ in 0..width {
                output.push(None, Reads::default());
            }
        }

        for field in ["where_clause", "having_clause"] {
            self.expression(body, field, &scope)?;
        }
        for field in [
            "group_clause",
            "window_clause",
            "sort_clause",
            "limit_offset",
            "limit_count",
        ] {
            self.expressions(body, field, &scope)?;
        }
        self.locking(body, &scope)?;

        Ok(output)
    }

    /// One side of a set operation.
    ///
    /// The two sides of a `UNION` are the only statements the parse tree keeps
    /// *bare* — not wrapped in a `Node` — so they are read directly.
    fn branch(
        &mut self,
        body: &Value,
        field: &str,
        scope: &Scope,
    ) -> Result<Derived, AnalysisError> {
        let node = node_field(body, field)
            .ok_or_else(|| AnalysisError::NotUnderstood("a set operation".to_string()))?;
        match one_of(unwrap(node)) {
            Some(("SelectStmt", branch)) => self.select(branch, scope),
            Some(_) => Err(AnalysisError::NotUnderstood(
                "a set operation this build does not read".to_string(),
            )),
            None => self.select(node, scope),
        }
    }

    /// The relations a `FROM` clause — or a `DELETE`'s `USING` list, which is
    /// the same thing — puts in scope, in order.
    ///
    /// Each item is built with the previous ones already in scope, so a `LATERAL`
    /// subquery can see them. A `JOIN` contributes both of its sides to the same
    /// level.
    fn collect_from(
        &mut self,
        body: &Value,
        field: &str,
        scope: &Scope,
    ) -> Result<Vec<Named>, AnalysisError> {
        let mut rels = Vec::new();
        for item in node_list(body, field) {
            let lateral = scope.push(Frame {
                rels: rels.clone(),
                ctes: BTreeMap::new(),
            });
            let mut found = self.build_relation(item, &lateral)?;
            rels.append(&mut found);
        }
        Ok(rels)
    }

    /// One `FROM` item: a table, a subquery, a function, a join or a sample.
    fn build_relation(&mut self, node: &Value, scope: &Scope) -> Result<Vec<Named>, AnalysisError> {
        let node = unwrap(node);
        let Some((kind, body)) = one_of(node) else {
            return Err(AnalysisError::NotUnderstood(
                "an empty FROM item".to_string(),
            ));
        };
        self.mark(node);
        match kind {
            "RangeVar" => Ok(vec![self.range_var(body, scope)?]),
            "RangeSubselect" => {
                let inner = scope.push(Frame::default());
                let subquery = node_field(body, "subquery")
                    .ok_or_else(|| AnalysisError::NotUnderstood("an empty subquery".to_string()))?;
                let output = self.statement(subquery, &inner)?;
                let output = self.alias_columns(body, output)?;
                Ok(vec![Named::derived(
                    alias_name(body).unwrap_or_default(),
                    output,
                )])
            }
            "RangeFunction" => {
                self.expressions(body, "functions", scope)?;
                for node in node_list(body, "coldeflist") {
                    self.dispatch(node, scope)?;
                }
                Ok(vec![Named::derived(
                    alias_name(body).unwrap_or_default(),
                    Derived::passthrough(),
                )])
            }
            "JoinExpr" => self.join(body, scope),
            "RangeTableSample" => {
                self.expressions(body, "args", scope)?;
                self.expression(body, "repeatable", scope)?;
                let relation = node_field(body, "relation").ok_or_else(|| {
                    AnalysisError::NotUnderstood("an empty TABLESAMPLE".to_string())
                })?;
                self.build_relation(relation, scope)
            }
            other => Err(AnalysisError::NotUnderstood(format!(
                "this statement: it holds a {other} the analysis does not read"
            ))),
        }
    }

    /// A `JOIN`: both sides in one level, then the condition.
    fn join(&mut self, body: &Value, scope: &Scope) -> Result<Vec<Named>, AnalysisError> {
        if flag(body, "is_natural") {
            return Err(AnalysisError::NotUnderstood(
                "a NATURAL JOIN: its columns are the ones both sides have".to_string(),
            ));
        }
        let left_node = node_field(body, "larg")
            .ok_or_else(|| AnalysisError::NotUnderstood("an empty JOIN".to_string()))?;
        let left = self.build_relation(left_node, scope)?;
        let left_scope = scope.push(Frame {
            rels: left.clone(),
            ctes: BTreeMap::new(),
        });
        let right_node = node_field(body, "rarg")
            .ok_or_else(|| AnalysisError::NotUnderstood("an empty JOIN".to_string()))?;
        let right = self.build_relation(right_node, &left_scope)?;

        let mut rels = left.clone();
        rels.extend(right.iter().cloned());
        let scope = scope.push(Frame {
            rels: rels.clone(),
            ctes: BTreeMap::new(),
        });
        self.expression(body, "quals", &scope)?;
        // `USING (a, b)` reads both sides' columns: they are the ones compared.
        for name in names_of(body.get("using_clause")) {
            for side in [&left, &right] {
                let reads = self.columns_of(side, &name)?;
                self.accesses_of(&reads, Some(&name));
            }
        }
        if let Some(alias) = alias_name(body) {
            // `(a JOIN b) AS j`: the alias stands for every column of both
            // sides, under whatever name the statement asks for.
            let reads = rels.iter().fold(Reads::default(), |mut reads, rel| {
                reads.merge(&rel.whole_row());
                reads
            });
            // The alias also stands for both sides' tables: a `FOR UPDATE OF j`
            // locks them, and a derived relation carries the tables it names.
            let names = rels.iter().flat_map(Named::named_tables).collect();
            let mut derived = Derived::passthrough_named(reads);
            derived.names = names;
            rels.push(Named::derived(alias, derived));
        }
        Ok(rels)
    }

    /// A `RangeVar`: a CTE's name, or a base table.
    ///
    /// A base table is *named* whatever the statement does with it, so it is
    /// recorded even when the statement reads no column of it.
    fn range_var(&mut self, body: &Value, scope: &Scope) -> Result<Named, AnalysisError> {
        let name = text(body, "relname").unwrap_or_default().to_string();
        let schema = text(body, "schemaname").unwrap_or_default();
        if !names_of(body.get("alias").and_then(|alias| alias.get("colnames"))).is_empty() {
            return Err(AnalysisError::NotUnderstood(
                "a column list on a table (only a subquery or a view has named columns)"
                    .to_string(),
            ));
        }
        let used = alias_name(body).unwrap_or_else(|| name.clone());
        if schema.is_empty()
            && let Some(cte) = scope.cte(&name)
        {
            return Ok(Named::derived(used, cte.clone()));
        }
        let table = table_key(schema, &name)?;
        // The statement names this table, whether or not it reads a column of
        // it: a `count(*)`, a bare `SELECT 1` or an `EXISTS` still tells the
        // statement whether the table exists (§6, D7).
        self.name_table(&table);
        Ok(Named::base(used, table))
    }

    /// Applies a `FROM` item's `AS alias(a, b)` column list to its output.
    fn alias_columns(&self, body: &Value, output: Derived) -> Result<Derived, AnalysisError> {
        let names = names_of(body.get("alias").and_then(|alias| alias.get("colnames")));
        if names.is_empty() {
            return Ok(output);
        }
        output.rename(&names).ok_or_else(|| {
            AnalysisError::NotUnderstood(
                "a column list on an output whose columns cannot be counted".to_string(),
            )
        })
    }

    /// An `INSERT`, including its `ON CONFLICT` clause.
    fn insert(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let scope = self.with_clause(body, scope)?;
        let target = target_table(body)?;
        let relation = self.write_target(body, &scope)?;

        let columns = insert_columns(body);
        if columns.is_empty() {
            // No column list at all (`INSERT INTO t VALUES …`,
            // `INSERT INTO t DEFAULT VALUES`): every column is written.
            self.access(Op::Insert, &target, None);
        } else {
            for name in &columns {
                self.access(Op::Insert, &target, Some(name));
            }
        }
        for column in node_list(body, "cols") {
            let Some(column) = typed_body(column, "ResTarget") else {
                return Err(AnalysisError::NotUnderstood(
                    "a column list this build does not read".to_string(),
                ));
            };
            self.expression(column, "val", &scope)?;
        }

        if let Some(source) = node_field(body, "select_stmt") {
            self.statement(source, &scope)?;
        }
        // The proposed row and the target's own row are the only ones an
        // `ON CONFLICT` clause and a `RETURNING` list can read.
        let scope = scope.push(Frame {
            rels: vec![relation.clone()],
            ctes: BTreeMap::new(),
        });
        if let Some(clause) = node_field(body, "on_conflict_clause") {
            self.mark(clause);
            self.on_conflict(clause, &scope, &target, &relation)?;
        }
        let mut output = self.returning(body, &scope)?;
        output.names.insert(target);
        Ok(output)
    }

    /// The `ON CONFLICT` clause: the conflict target, and what `DO UPDATE`
    /// writes.
    fn on_conflict(
        &mut self,
        clause: &Value,
        outer: &Scope,
        target: &str,
        relation: &Named,
    ) -> Result<(), AnalysisError> {
        // The proposed row is `excluded`, and the target table keeps its own
        // name: both are readable in the `SET` list and the `WHERE`.
        let scope = outer.push(Frame {
            rels: vec![
                relation.clone(),
                Named::derived("excluded".to_string(), Derived::passthrough()),
            ],
            ctes: BTreeMap::new(),
        });
        if let Some(infer) = node_field(clause, "infer") {
            self.mark(infer);
            let infer = unwrap(infer);
            for element in node_list(infer, "index_elems") {
                self.dispatch(element, &scope)?;
            }
            self.expression(infer, "where_clause", &scope)?;
        }
        let assignments = node_list(clause, "target_list");
        for assignment in assignments {
            let Some(target_body) = typed_body(assignment, "ResTarget") else {
                return Err(AnalysisError::NotUnderstood(
                    "an ON CONFLICT whose SET columns cannot be read from the statement"
                        .to_string(),
                ));
            };
            let Some(name) = res_target_name(target_body) else {
                return Err(AnalysisError::NotUnderstood(
                    "an ON CONFLICT whose SET columns cannot be read from the statement"
                        .to_string(),
                ));
            };
            self.access(Op::Update, target, Some(&name));
            self.expression(target_body, "val", &scope)?;
        }
        self.expression(clause, "where_clause", &scope)?;
        Ok(())
    }

    /// An `UPDATE`: the `SET` columns written, everything else read.
    fn update(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let scope = self.with_clause(body, scope)?;
        let target = target_table(body)?;
        let relation = self.write_target(body, &scope)?;

        let mut rels = vec![relation];
        rels.extend(self.collect_from(body, "from_clause", &scope)?);
        let scope = scope.push(Frame {
            rels,
            ctes: BTreeMap::new(),
        });

        for assignment in node_list(body, "target_list") {
            let Some(assignment) = typed_body(assignment, "ResTarget") else {
                return Err(AnalysisError::NotUnderstood(
                    "an UPDATE whose SET columns cannot be read from the statement".to_string(),
                ));
            };
            let Some(name) = res_target_name(assignment) else {
                return Err(AnalysisError::NotUnderstood(
                    "an UPDATE whose SET columns cannot be read from the statement".to_string(),
                ));
            };
            self.access(Op::Update, &target, Some(&name));
            self.expression(assignment, "val", &scope)?;
        }
        self.expression(body, "where_clause", &scope)?;
        let mut output = self.returning(body, &scope)?;
        output.names.insert(target);
        Ok(output)
    }

    /// A `DELETE`: the table it empties, and what it reads to find the rows.
    fn delete(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let scope = self.with_clause(body, scope)?;
        let target = target_table(body)?;
        let relation = self.write_target(body, &scope)?;
        self.access(Op::Delete, &target, None);

        let mut rels = vec![relation];
        // `USING` is a `DELETE`'s `FROM`: the relations it reads to find the
        // rows, in the same scope as the target.
        rels.extend(self.collect_from(body, "using_clause", &scope)?);
        let scope = scope.push(Frame {
            rels,
            ctes: BTreeMap::new(),
        });
        self.expression(body, "where_clause", &scope)?;
        let mut output = self.returning(body, &scope)?;
        output.names.insert(target);
        Ok(output)
    }

    /// The base table a statement writes to.
    ///
    /// A write target is always a real table: a CTE of the same name cannot be
    /// updated, so the name is resolved as a table here and nowhere else.
    fn write_target(&mut self, body: &Value, scope: &Scope) -> Result<Named, AnalysisError> {
        let relation = node_field(body, "relation")
            .ok_or_else(|| AnalysisError::NotUnderstood("a statement with no table".to_string()))?;
        self.mark(relation);
        let relation = unwrap(relation);
        let name = text(relation, "relname")
            .ok_or_else(|| AnalysisError::NotUnderstood("a table name".to_string()))?;
        let schema = text(relation, "schemaname").unwrap_or_default();
        let table = table_key(schema, name)?;
        let _ = scope;
        Ok(Named::base(name.to_string(), table))
    }

    /// A statement's `RETURNING` list, as the statement's output columns.
    fn returning(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
        let mut output = Derived::new();
        for target in node_list(body, "returning_list") {
            let (name, reads) = self.target(target, scope)?;
            if name.is_none() && star_target(target) {
                output.unbounded();
            }
            output.push(name, reads);
        }
        Ok(output)
    }

    /// `FOR UPDATE`/`FOR SHARE`: locking a row is reading it to change it, so the
    /// locked tables need `update` (§6).
    ///
    /// A relation can be derived — a subquery, a CTE, a join — and PostgreSQL
    /// still locks the base tables under it, so the requirement follows the set
    /// of tables the relation's definition *names*, not the columns the outer
    /// statement happened to read: `SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR
    /// UPDATE` locks `auth_user` rows and needs `update` on `auth_user`.
    fn locking(&mut self, body: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        for clause in node_list(body, "locking_clause") {
            self.mark(clause);
            let Some(("LockingClause", clause)) = one_of(clause) else {
                return Err(AnalysisError::NotUnderstood("a locking clause".to_string()));
            };
            let named = node_list(clause, "locked_rels");
            let locked: Vec<Named> = if named.is_empty() {
                // `FOR UPDATE` without a list: every relation the statement
                // reads.
                scope
                    .innermost()
                    .map(|frame| frame.rels.clone())
                    .unwrap_or_default()
            } else {
                let mut rels = Vec::new();
                for node in named {
                    rels.push(self.relation_reference(node, scope)?);
                }
                rels
            };
            for rel in locked {
                for table in rel.named_tables() {
                    self.access(Op::Update, &table, None);
                }
            }
        }
        Ok(())
    }

    /// The relation a `RangeVar` names, from the scope (`FOR UPDATE OF t`).
    fn relation_reference(&mut self, node: &Value, scope: &Scope) -> Result<Named, AnalysisError> {
        let Some(body) = typed_body(node, "RangeVar") else {
            return Err(AnalysisError::NotUnderstood(
                "a locked relation this build does not read".to_string(),
            ));
        };
        self.mark(node);
        let name = text(body, "relname").unwrap_or_default();
        if let Some(rel) = scope.relation(name) {
            return Ok(rel.clone());
        }
        let schema = text(body, "schemaname").unwrap_or_default();
        let table = table_key(schema, name)?;
        if let Some(frame) = scope.innermost()
            && let Some(rel) = frame
                .rels
                .iter()
                .find(|rel| matches!(&rel.rel, Rel::Base { table: own } if *own == table))
        {
            return Ok(rel.clone());
        }
        Err(AnalysisError::NotUnderstood(format!(
            "this statement: `{name}` is locked but not read"
        )))
    }

    //
    // Expressions.
    //

    /// A target-list or `RETURNING` entry: its output name, and what reading it
    /// needs.
    fn target(
        &mut self,
        node: &Value,
        scope: &Scope,
    ) -> Result<(Option<String>, Reads), AnalysisError> {
        let node = unwrap(node);
        let Some(body) = typed_body(node, "ResTarget") else {
            return Err(AnalysisError::NotUnderstood("a result column".to_string()));
        };
        let value = node_field(body, "val");
        let reads = match value {
            Some(value) => {
                let value = unwrap(value);
                self.dispatch(value, scope)?;
                match typed_body(value, "ColumnRef") {
                    // The output column stands for exactly what the reference
                    // reads, so a rename cannot hide it.
                    Some(ref_body) => {
                        let name = last_name(ref_body);
                        let reads = self.column_ref(ref_body, scope)?;
                        self.accesses_of(&reads, name.as_deref());
                        reads
                    }
                    None => Reads::default(),
                }
            }
            None => Reads::default(),
        };
        let name = match text(body, "name") {
            Some(name) if !name.is_empty() => Some(name.to_string()),
            _ => value
                .map(|value| output_name(unwrap(value)))
                .unwrap_or(None),
        };
        Ok((name, reads))
    }

    /// A column reference: what reading it needs.
    fn column_ref(&self, body: &Value, scope: &Scope) -> Result<Reads, AnalysisError> {
        let fields = node_list(body, "fields");
        if fields.is_empty() {
            return Err(AnalysisError::NotUnderstood(
                "an empty column reference".to_string(),
            ));
        }
        if let Some(index) = fields.iter().position(|field| is_star(field)) {
            if index + 1 != fields.len() {
                return Err(AnalysisError::NotUnderstood(
                    "a column reference after `*`".to_string(),
                ));
            }
            return match index {
                0 => self.star(scope),
                1 => {
                    let name = string_node(fields[0]).ok_or_else(|| {
                        AnalysisError::NotUnderstood("a `*` on something unnamed".to_string())
                    })?;
                    let rel = scope.relation(name).ok_or_else(|| {
                        AnalysisError::NotUnderstood(format!(
                            "this statement: `{name}` is not in its FROM clause"
                        ))
                    })?;
                    Ok(rel.whole_row())
                }
                _ => Err(AnalysisError::NotUnderstood(
                    "a `*` on a qualified name".to_string(),
                )),
            };
        }

        let names: Vec<&str> = match fields
            .iter()
            .map(|field| string_node(field))
            .collect::<Option<Vec<&str>>>()
        {
            Some(names) => names,
            None => {
                return Err(AnalysisError::NotUnderstood(
                    "a column reference this build does not read".to_string(),
                ));
            }
        };

        match names.len() {
            1 => {
                // A single name that is a relation in scope is a whole-row
                // reference to it (`SELECT t FROM t`). PostgreSQL only reads it
                // that way when no column of that name exists, and this build
                // does not know the columns, so the stricter of the two — a
                // whole-row read — is what it requires.
                if let Some(rel) = scope.relation(names[0]) {
                    return Ok(rel.whole_row());
                }
                self.unqualified(names[0], scope)
            }
            2 => {
                let rel = scope.relation(names[0]).ok_or_else(|| {
                    AnalysisError::NotUnderstood(format!(
                        "this statement: `{}` is not in its FROM clause",
                        names[0]
                    ))
                })?;
                self.column_of(rel, names[1])
            }
            _ => {
                // `schema.table.column`: the first part is a relation in scope
                // when it is an alias, and a schema otherwise — which is how a
                // statement reaches a table outside `public`.
                if let Some(rel) = scope.relation(names[0]) {
                    return self.column_of(rel, names[1]);
                }
                let schema = names[..names.len() - 2].join(".");
                let table = table_key(&schema, names[names.len() - 2])?;
                let mut reads = Reads::default();
                reads.column(&table, names[names.len() - 1]);
                Ok(reads)
            }
        }
    }

    /// An unqualified column name: every relation in the innermost level that
    /// could provide it has to grant it — no schema is known here, so the
    /// statement could be about any of them (§6).
    fn unqualified(&self, name: &str, scope: &Scope) -> Result<Reads, AnalysisError> {
        let rels = scope.level_for(name).ok_or_else(|| {
            AnalysisError::NotUnderstood(format!(
                "this statement: `{name}` is not a column of anything in its FROM clause"
            ))
        })?;
        self.columns_of(rels, name)
    }

    /// A named column of every relation in `rels` that could provide it.
    fn columns_of(&self, rels: &[Named], column: &str) -> Result<Reads, AnalysisError> {
        let mut reads = Reads::default();
        for rel in rels {
            if rel.provides(column) {
                reads.merge(&self.column_of(rel, column)?);
            }
        }
        Ok(reads)
    }

    /// A named column of one relation.
    fn column_of(&self, rel: &Named, column: &str) -> Result<Reads, AnalysisError> {
        match &rel.rel {
            Rel::Base { table } => {
                let mut reads = Reads::default();
                reads.column(table, column);
                Ok(reads)
            }
            Rel::Derived(derived) => match derived.named.get(column) {
                Some(reads) => Ok(reads.clone()),
                None => match &derived.open {
                    // The output's names are not known, so reading `column` of
                    // it touches whatever column of the base tables the
                    // definition read: an open part that was a `*` becomes "the
                    // column the statement named", which is as specific as this
                    // build can be.
                    Some(reads) => Ok(reads.as_unknown_column()),
                    None => Err(AnalysisError::NotUnderstood(format!(
                        "this statement: `{}` has no column `{column}`",
                        rel.name
                    ))),
                },
            },
        }
    }

    /// A bare `*`: every relation of the innermost level, whole.
    fn star(&self, scope: &Scope) -> Result<Reads, AnalysisError> {
        let frame = scope.innermost().ok_or_else(|| {
            AnalysisError::NotUnderstood("a `*` with no table in scope".to_string())
        })?;
        if frame.rels.is_empty() {
            return Err(AnalysisError::NotUnderstood(
                "a `*` with no table in scope".to_string(),
            ));
        }
        Ok(frame.rels.iter().fold(Reads::default(), |mut reads, rel| {
            reads.merge(&rel.whole_row());
            reads
        }))
    }

    /// A function call: its name, its arguments, and its window.
    fn func_call(&mut self, body: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        let mut parts = Vec::new();
        for part in node_list(body, "funcname") {
            parts.push(
                string_node(part)
                    .ok_or_else(|| AnalysisError::NotUnderstood("a function name".to_string()))?
                    .to_string(),
            );
        }
        // `pg_catalog.lower` is `lower`: the schema is only there because the
        // parser put it there.
        let name = match parts.len() {
            2.. if parts[parts.len() - 2] == "pg_catalog" => parts[parts.len() - 1].clone(),
            _ => parts.join("."),
        };
        if !name.is_empty() {
            self.functions.insert(name);
        }
        self.expressions(body, "args", scope)?;
        self.expressions(body, "agg_order", scope)?;
        self.expression(body, "agg_filter", scope)?;
        self.expression(body, "over", scope)?;
        Ok(())
    }

    /// A cast: what its argument reads, and whether its *target type* is one
    /// that can run code (§6, D7).
    ///
    /// A cast is a function call PostgreSQL writes for you: `x::int4` parses
    /// like `int4(x)`, and a domain or a `reg*` type can run arbitrary code — or
    /// read the catalogue — at input. The built-in scalar types below have no
    /// such input function, so a cast to one stays free; every other type is
    /// reported like a function call under its own name, so a grant has to list
    /// it in `functions`.
    fn type_cast(&mut self, body: &Value, scope: &Scope) -> Result<(), AnalysisError> {
        self.expression(body, "arg", scope)?;
        let Some(type_name) = node_field(body, "type_name") else {
            return Err(AnalysisError::NotUnderstood(
                "a cast with no target type".to_string(),
            ));
        };
        let names = names_of(type_name.get("names"));
        if names.is_empty() {
            return Err(AnalysisError::NotUnderstood(
                "a cast to an unnamed type".to_string(),
            ));
        }
        let name = match names.len() {
            2.. if names[names.len() - 2] == "pg_catalog" => names[names.len() - 1].clone(),
            _ => names.join("."),
        };
        if !is_safe_cast_type(&name) {
            self.functions.insert(name);
        }
        Ok(())
    }
}

impl Derived {
    /// A derived relation whose columns all read `reads`, under any name.
    fn passthrough_named(reads: Reads) -> Self {
        Self {
            named: BTreeMap::new(),
            open: Some(reads),
            positions: None,
            names: BTreeSet::new(),
        }
    }
}

/// The built-in scalar types a cast to which runs no user code (§6, D7).
///
/// An array of one of these is safe too: `int[]::text[]` has the same element
/// input functions, and the parse tree gives one name either way. Every other
/// type — a `reg*` type (which looks a name up in the catalogue), a domain, a
/// user type, a type with a C input function — is reported like a function call
/// under its own name, so a grant has to list it in `functions`.
const SAFE_CAST_TYPES: &[&str] = &[
    // Integers and floating point.
    "int2",
    "int4",
    "int8",
    "float4",
    "float8",
    "numeric",
    // Strings.
    "text",
    "varchar",
    "bpchar",
    // Booleans, dates and times.
    "bool",
    "date",
    "timestamp",
    "timestamptz",
    // Identifiers and JSON.
    "uuid",
    "json",
    "jsonb",
    // PostGIS's own types: pure, and the ones a real app casts geometry to.
    "geometry",
    "geography",
];

/// Whether a cast to `name` needs no entry in a grant's `functions` list.
fn is_safe_cast_type(name: &str) -> bool {
    SAFE_CAST_TYPES.contains(&name)
}

/// The `information_schema` views PostgreSQL defines, by name.
///
/// They describe everything else in the database, so a table grant naming one
/// (unqualified) could only read as a capability it is not; the list is the one
/// `information_schema.tables` reports for its own schema.
const INFORMATION_SCHEMA_VIEWS: &[&str] = &[
    "_pg_foreign_data_wrappers",
    "_pg_foreign_servers",
    "_pg_foreign_table_columns",
    "_pg_foreign_tables",
    "_pg_user_mappings",
    "administrable_role_authorizations",
    "applicable_roles",
    "attributes",
    "character_sets",
    "check_constraint_routine_usage",
    "check_constraints",
    "collation_character_set_applicability",
    "collations",
    "column_column_usage",
    "column_domain_usage",
    "column_options",
    "column_privileges",
    "column_udt_usage",
    "columns",
    "constraint_column_usage",
    "constraint_table_usage",
    "data_type_privileges",
    "domain_constraints",
    "domain_udt_usage",
    "domains",
    "element_types",
    "enabled_roles",
    "foreign_data_wrapper_options",
    "foreign_data_wrappers",
    "foreign_server_options",
    "foreign_servers",
    "foreign_table_options",
    "foreign_tables",
    "information_schema_catalog_name",
    "key_column_usage",
    "parameters",
    "referential_constraints",
    "role_column_grants",
    "role_routine_grants",
    "role_table_grants",
    "role_udt_grants",
    "role_usage_grants",
    "routine_column_usage",
    "routine_privileges",
    "routine_routine_usage",
    "routine_sequence_usage",
    "routine_table_usage",
    "routines",
    "schemata",
    "sequences",
    "sql_features",
    "sql_implementation_info",
    "sql_parts",
    "sql_sizing",
    "table_constraints",
    "table_privileges",
    "tables",
    "transforms",
    "triggered_update_columns",
    "triggers",
    "udt_privileges",
    "usage_privileges",
    "user_defined_types",
    "user_mapping_options",
    "user_mappings",
    "view_column_usage",
    "view_routine_usage",
    "view_table_usage",
    "views",
];

/// Whether a bare relation name could be one of PostgreSQL's system catalogues.
///
/// `pg_catalog` is always first on the search path, so an unqualified name
/// starting with `pg_` resolves into it — into `pg_class`, `pg_proc` or one of
/// their views — and a relation the statement reaches is a relation a grant
/// cannot describe. A name that is one of the `information_schema` views is
/// refused too: a table grant for it could only read as a capability it is not.
///
/// Called with the **key** a table grant would carry, so it is only ever asked
/// about an unqualified name: `audit.users` keeps its schema and is a table like
/// any other. (§6, D7.)
pub fn is_system_table(name: &str) -> bool {
    name.starts_with("pg_") || INFORMATION_SCHEMA_VIEWS.contains(&name)
}

/// The table key a grant uses for a schema-qualified name (§6).
///
/// `public.t` is `t`, because `public` is on every search path a view has; any
/// other schema keeps its name, so `audit.auth_user` can only be read through a
/// grant that spells it out. `pg_catalog` and `information_schema` are never
/// readable through a table grant — their contents describe everything else in
/// the database — and neither is an unqualified name that would resolve into
/// them (`pg_class`, `tables`), because `pg_catalog` is always first on the
/// search path and an unqualified name is a name a grant cannot pin down.
fn table_key(schema: &str, table: &str) -> Result<String, AnalysisError> {
    match schema {
        "" | "public" if is_system_table(table) => Err(AnalysisError::NotUnderstood(format!(
            "{table}: the system catalogues are never readable through a table grant"
        ))),
        "" | "public" => Ok(table.to_string()),
        "pg_catalog" | "information_schema" => Err(AnalysisError::NotUnderstood(format!(
            "{schema}.{table}: the system catalogues are never readable through a table grant"
        ))),
        other => Ok(format!("{other}.{table}")),
    }
}

/// The output of a set operation: two branches positionally, or — when a branch
/// has columns nobody can count — everything the two read.
fn merge_outputs(left: Derived, right: Derived) -> Derived {
    let (Some(left_positions), Some(right_positions)) = (&left.positions, &right.positions) else {
        return union_of(&[left, right]);
    };
    if left_positions.len() != right_positions.len() {
        return union_of(&[left, right]);
    }
    let names: BTreeSet<String> = left.names.union(&right.names).cloned().collect();
    let mut output = Derived::new();
    for ((name, left_reads), (_, right_reads)) in left_positions.iter().zip(right_positions) {
        let mut reads = left_reads.clone();
        reads.merge(right_reads);
        // The first branch names the output column, as PostgreSQL does.
        output.push(name.clone(), reads);
    }
    output.names = names;
    output
}

/// Every column of every branch, as one open output.
fn union_of(branches: &[Derived]) -> Derived {
    let mut reads = Reads::default();
    let mut names = BTreeSet::new();
    for derived in branches {
        for column in derived.named.values() {
            reads.merge(column);
        }
        if let Some(open) = &derived.open {
            reads.merge(open);
        }
        names.extend(derived.names.iter().cloned());
    }
    let mut output = Derived::passthrough_named(reads);
    output.names = names;
    output
}

/// The name of the last `String` field of a `ColumnRef`.
fn last_name(body: &Value) -> Option<String> {
    node_list(body, "fields")
        .last()
        .and_then(|field| string_node(field))
        .map(str::to_string)
}

/// Whether a target list entry is a `*` expansion.
fn star_target(node: &Value) -> bool {
    let Some(target) = typed_body(node, "ResTarget") else {
        return false;
    };
    let Some(value) = node_field(target, "val") else {
        return false;
    };
    match typed_body(value, "ColumnRef") {
        Some(fields) => node_list(fields, "fields")
            .first()
            .is_some_and(|field| is_star(field)),
        None => false,
    }
}

/// The output name PostgreSQL would give a target with no alias.
///
/// Only the cases a derived relation's columns depend on: a plain column
/// reference is that column's name, and a function call is the function's. Any
/// other expression has no name this build can know, and a derived relation's
/// unknown output is read through its `open` part instead.
fn output_name(node: &Value) -> Option<String> {
    match one_of(unwrap(node)) {
        Some(("ColumnRef", body)) => last_name(body),
        Some(("FuncCall", body)) => node_list(body, "funcname")
            .last()
            .and_then(|part| string_node(part))
            .map(str::to_string),
        _ => None,
    }
}

/// The column a `SET`/`ON CONFLICT` target body names, if it names one.
fn res_target_name(body: &Value) -> Option<String> {
    text(body, "name")
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// The column names an `INSERT`'s `cols` list writes.
fn insert_columns(body: &Value) -> Vec<String> {
    node_list(body, "cols")
        .into_iter()
        .filter_map(|column| typed_body(column, "ResTarget"))
        .filter_map(res_target_name)
        .collect()
}

/// The `RangeVar` a statement writes to: its `relation` field.
fn target_table(body: &Value) -> Result<String, AnalysisError> {
    let relation = node_field(body, "relation")
        .ok_or_else(|| AnalysisError::NotUnderstood("a statement with no table".to_string()))?;
    let relation = unwrap(relation);
    let name = text(relation, "relname")
        .ok_or_else(|| AnalysisError::NotUnderstood("a table name".to_string()))?;
    table_key(text(relation, "schemaname").unwrap_or_default(), name)
}

/// The alias a `FROM` item's relation carries, if it has one.
fn alias_name(body: &Value) -> Option<String> {
    let alias = body.get("alias")?.as_object()?;
    let name = alias.get("aliasname")?.as_str()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// A list of `String` nodes as owned names (`aliascolnames`, `using_clause`).
fn names_of(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| string_node(item))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `SelectStmt` is a set operation (`UNION`, `INTERSECT`, `EXCEPT`).
fn set_operation(body: &Value) -> bool {
    body.get("op")
        .and_then(Value::as_i64)
        .is_some_and(|op| op != 1)
}

/// A column reference's star, if it has one.
fn is_star(node: &Value) -> bool {
    matches!(one_of(unwrap(node)), Some(("AStar", _)))
}

/// The name of a leaf `String` node.
fn string_node(node: &Value) -> Option<&str> {
    match one_of(unwrap(node)) {
        Some(("String", body)) => text(body, "sval"),
        _ => None,
    }
}

//
// JSON helpers.
//

/// The address of a JSON value, which is stable for as long as the tree lives.
fn address(value: &Value) -> usize {
    value as *const Value as usize
}

/// The node a JSON value stands for.
///
/// A `Node` message is `{"node": <the node>}` and holds nothing else; a message
/// body is `{"Type": body}`. Only a *pure* wrapper is unwrapped, so a message
/// whose body happens to have a `node` field (a `SortBy`, say) keeps it.
fn unwrap(value: &Value) -> &Value {
    let mut current = value;
    for _ in 0..8 {
        let Some(map) = current.as_object() else {
            return current;
        };
        if map.len() != 1 {
            return current;
        }
        match map.get("node") {
            Some(inner) if !inner.is_null() => current = inner,
            _ => return current,
        }
    }
    current
}

/// The type and body of a node: `{"SelectStmt": { … }}`.
fn one_of(value: &Value) -> Option<(&str, &Value)> {
    let map = value.as_object()?;
    if map.len() != 1 {
        return None;
    }
    let (kind, body) = map.iter().next()?;
    if !body.is_object() {
        return None;
    }
    Some((kind.as_str(), body))
}

/// A field of a message body that holds part of the parse tree, if it is there
/// and not null.
///
/// Some fields hold a `Node` (`{"node": { … }}`) and others hold the message
/// itself — a `RangeVar` in a DML statement's `relation`, an `OnConflictClause`,
/// a `FuncCall`'s `WindowDef` in `over`. Both are returned; the caller walks or
/// reads them, and [`Resolver::dispatch`] walks a message that is not a `Node`
/// field by field.
/// The body of a field the caller knows the type of.
///
/// The parse tree wraps a node in a `Node` in some places and not in others:
/// `SelectStmt.larg`/`rarg` are bare `SelectStmt` messages, `with_clause` is a
/// bare `WithClause`, a DML statement's `relation` a bare `RangeVar`. Both forms
/// are accepted — a `Node` of the expected type is unwrapped, and a message that
/// is not in a `Node` at all *is* the body. A `Node` of another type is not.
fn typed_body<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    let node = unwrap(node);
    match one_of(node) {
        Some((found, body)) => (found == kind).then_some(body),
        None if node.is_object() => Some(node),
        None => None,
    }
}

fn node_field<'a>(body: &'a Value, field: &str) -> Option<&'a Value> {
    let value = body.get(field)?;
    if value.is_null() {
        return None;
    }
    let value = unwrap(value);
    match value.is_object() {
        true => Some(value),
        false => None,
    }
}

/// A list of `Node`s, unwrapped, skipping empty entries.
fn node_list<'a>(body: &'a Value, field: &str) -> Vec<&'a Value> {
    body.get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(unwrap)
                .filter(|item| one_of(item).is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// A string field of a message body.
fn text<'a>(body: &'a Value, field: &str) -> Option<&'a str> {
    body.get(field)?.as_str()
}

/// A boolean field of a message body; absent counts as false.
fn flag(body: &Value, field: &str) -> bool {
    body.get(field).and_then(Value::as_bool).unwrap_or(false)
}
