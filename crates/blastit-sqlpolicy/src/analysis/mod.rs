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
//! `update` on `auth_user`. **Any** `update` entry is enough for a lock, on any
//! one column: PostgreSQL only asks for the UPDATE privilege on a single column
//! of a locked table (and `select` for the columns read), so a route that locks
//! rows it only reads needs no more than `update = ["id"]`.
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
//! # Layout
//!
//! * [`types`] — the values a policy decides on: statement kinds and classes,
//!   accesses and analyses, and the refusals;
//! * [`scope`] — the relations a statement puts in scope and what reading a
//!   derived relation's column is held to;
//! * [`classify`] — what PostgreSQL's parser says a statement is, plus the
//!   function set a grant may leave unnamed;
//! * [`walk`] — the traversal itself: dispatch, coverage and depth;
//! * `resolve` — what the statements *mean*: the read side, the write side,
//!   expressions and the shared parse-tree accessors;
//! * [`cache`] — the bounded analysis cache and the manifest-only free
//!   functions.
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

mod cache;
mod classify;
mod resolve;
mod scope;
mod types;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use serde_json::Value;

pub use self::cache::{analyze, cached_analyses};
pub use self::classify::{SAFE_FUNCTIONS, classify, is_safe_function};
pub use self::types::{
    Access, Analysis, AnalysisError, Classified, Op, StatementClass, StatementKind,
};

pub(crate) use self::cache::AnalysisCache;
/// Re-exported for `tests::the_cache_stays_bounded`, which pins the bound
/// through `super::*`; no production code names the capacity outside the cache
/// itself, so the lib build does not carry the import.
#[cfg(test)]
pub(crate) use self::cache::CACHE_CAPACITY;
pub(crate) use self::classify::syntax;
pub(crate) use self::types::Extent;
pub(crate) use self::walk::{one_of, unwrap};

use self::scope::{Derived, Reads};
use self::walk::{node_list, string_node};

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
pub(crate) fn is_safe_cast_type(name: &str) -> bool {
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
pub(crate) fn table_key(schema: &str, table: &str) -> Result<String, AnalysisError> {
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
pub(in crate::analysis) fn merge_outputs(left: Derived, right: Derived) -> Derived {
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
pub(crate) fn last_name(body: &Value) -> Option<String> {
    node_list(body, "fields")
        .last()
        .and_then(|field| string_node(field))
        .map(str::to_string)
}
