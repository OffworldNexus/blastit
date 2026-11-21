//! What PostgreSQL's parser says a statement is, and what a grant may leave
//! unnamed (§6, D7).
//!
//! [`classify`] answers for *every* statement `libpg_query` can parse — a tool
//! that has to decide whether a statement read out of a log may be granted at
//! all (`blastit-manifest fold`) needs that — while
//! [`analyze`](super::analyze) answers only for the four kinds a table grant can
//! cover. [`is_safe_function`] is the built-in function set a grant does not
//! have to spell out.

use serde_json::Value;

use super::types::{AnalysisError, Classified, StatementClass};
use super::walk::one_of;

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
/// The name is compared as [`Analysis::functions`](super::Analysis::functions)
/// reports it, with the `pg_catalog.` prefix dropped. The comparison is exact,
/// so a call to `"Lower"` — a different function, quoted in the SQL — is not
/// `lower`.
pub fn is_safe_function(name: &str) -> bool {
    SAFE_FUNCTIONS.contains(&name)
}

/// Classifies the one statement in `sql`, with PostgreSQL's own parser (D3c).
///
/// Unlike [`analyze`](super::analyze) this succeeds for *every* statement
/// libpg_query can parse, and says what it is; the four kinds a table grant can
/// cover come back as [`StatementClass::Select`] and friends, everything else as
/// [`StatementClass::Other`] with its own name in [`Classified::label`], and a
/// text that holds several statements carries how many — a grant is one
/// statement, so a caller has to refuse it rather than pick one.
///
/// # Errors
///
/// [`AnalysisError::Syntax`] when the text is not valid SQL, and
/// [`AnalysisError::Empty`] when it holds no statement at all.
pub fn classify(sql: &str) -> Result<Classified, AnalysisError> {
    let parsed = pg_query::parse(sql).map_err(|error| AnalysisError::Syntax(syntax(&error)))?;
    let statements = &parsed.protobuf.stmts;
    if statements.is_empty() {
        return Err(AnalysisError::Empty);
    }
    let statement = statements[0].stmt.as_ref().ok_or(AnalysisError::Empty)?;
    let node = statement.node.as_ref().ok_or(AnalysisError::Empty)?;
    let tree = serde_json::to_value(node)
        .map_err(|error| AnalysisError::NotUnderstood(format!("readable SQL: {error}")))?;
    let kind = one_of(&tree)
        .map(|(kind, _)| kind)
        .ok_or(AnalysisError::Empty)?;
    let class = match kind {
        "SelectStmt" => StatementClass::Select,
        "InsertStmt" => StatementClass::Insert,
        "UpdateStmt" => StatementClass::Update,
        "DeleteStmt" => StatementClass::Delete,
        _ => StatementClass::Other,
    };
    let label = match statements.len() {
        1 => classify_label(&tree, kind),
        count => format!("{count} statements"),
    };
    Ok(Classified {
        class,
        label,
        statements: statements.len(),
    })
}

/// What to call a statement, node kinds that share a name included.
///
/// `RESET` and `SET ROLE` are both a `VariableSetStmt` to PostgreSQL's parser,
/// and a person reading "RESET ALL is a SET" would look for the wrong thing, so
/// the node's own `kind` field decides between them.
fn classify_label(tree: &Value, node: &str) -> String {
    match node {
        "VariableSetStmt" if is_reset(tree) => "RESET".to_string(),
        "GrantStmt" if is_revoke(tree) => "REVOKE".to_string(),
        _ => statement_label(node).to_string(),
    }
}

/// Whether a `GrantStmt` is a `REVOKE` (`is_grant` false) rather than a `GRANT`.
fn is_revoke(tree: &Value) -> bool {
    let Some((kind, body)) = one_of(tree) else {
        return false;
    };
    kind == "GrantStmt" && body.get("is_grant") == Some(&Value::Bool(false))
}

/// Whether a `VariableSetStmt` is a `RESET` (`VAR_RESET`, `VAR_RESET_ALL`)
/// rather than a `SET`.
fn is_reset(tree: &Value) -> bool {
    use pg_query::protobuf::VariableSetKind;

    let Some((_, body)) = one_of(tree) else {
        return false;
    };
    let Some(kind) = body.get("kind").and_then(Value::as_i64) else {
        return false;
    };
    kind == VariableSetKind::VarReset as i64 || kind == VariableSetKind::VarResetAll as i64
}

/// How a statement kind is named in a refusal.
pub(super) fn statement_label(kind: &str) -> &str {
    match kind {
        "SelectStmt" => "SELECT",
        "InsertStmt" => "INSERT",
        "UpdateStmt" => "UPDATE",
        "DeleteStmt" => "DELETE",
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
        // DDL: a statement a grant is not an allowlist for, named the way a
        // person reading a denial expects (`CREATE TABLE`, not `CreateStmt`).
        "CreateStmt" => "CREATE TABLE",
        "CreateTableAsStmt" => "CREATE TABLE AS",
        "AlterTableStmt" => "ALTER TABLE",
        "AlterDomainStmt" => "ALTER DOMAIN",
        "AlterDatabaseStmt" => "ALTER DATABASE",
        "AlterDatabaseSetStmt" => "ALTER DATABASE SET",
        "AlterRoleStmt" => "ALTER ROLE",
        "AlterRoleSetStmt" => "ALTER ROLE SET",
        "AlterOwnerStmt" => "ALTER … OWNER TO",
        "CreateRoleStmt" => "CREATE ROLE",
        "DropRoleStmt" => "DROP ROLE",
        "IndexStmt" => "CREATE INDEX",
        "ViewStmt" => "CREATE VIEW",
        "DropStmt" => "DROP",
        "RenameStmt" => "ALTER … RENAME",
        "RefreshMatViewStmt" => "REFRESH MATERIALIZED VIEW",
        "ClusterStmt" => "CLUSTER",
        "CreateFunctionStmt" => "CREATE FUNCTION",
        "CreateExtensionStmt" => "CREATE EXTENSION",
        "CreateSchemaStmt" => "CREATE SCHEMA",
        "CreateSeqStmt" => "CREATE SEQUENCE",
        "CreateTrigStmt" => "CREATE TRIGGER",
        "CreatePolicyStmt" => "CREATE POLICY",
        "VacuumStmt" => "VACUUM",
        "DefineStmt" => "CREATE",
        "CommentStmt" => "COMMENT",
        "RuleStmt" => "CREATE RULE",
        "SecLabelStmt" => "SECURITY LABEL",
        "DefElem" => "a definition",
        other => other,
    }
}

/// A one-line rendering of a parser error: `libpg_query` reports the position and
/// the message on separate lines, and an event is one line (§8.1).
///
/// The message is also cut short: a deeply nested statement makes the parser
/// report its whole path back to the root, and a `sql.denied` event's `reason`
/// is not the place for a kilobyte of it.
pub(crate) fn syntax(error: &pg_query::Error) -> String {
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
