//! What `--learn` may propose, and what a session may replay (R25, R31, ADR-0003).
//!
//! `--learn` runs every statement a request sends and then writes the ones it
//! saw into `blastit.toml`. A statement that cannot be written as a grant must
//! therefore be refused *before* it is proposed, or `--learn` would produce a
//! manifest that does not boot: a redacted `…`, a literal only this request
//! carried, a second statement hidden behind the first, or a statement that
//! reaches the server's filesystem or network.
//!
//! [`check_learnable`] is that one verdict. `blastit check --learn`,
//! `blastit dev --learn` and `blastit-manifest fold` all ask it instead of each
//! re-deriving the rules, so the three can never disagree about what is
//! grantable.
//!
//! [`is_introspection_setup_sql`] is the other half of the same idea for
//! `DynamicCheck`: a connection's setup statements (`SET`, `SHOW`, the two
//! fixed protocol forms and read-only catalogue lookups) are recorded while no
//! route is active and replayed once when the connection first runs under a
//! route, so a driver's session setup does not start failing the moment routing
//! begins (R31).

use serde_json::Value;

use crate::analysis::{one_of, syntax, unwrap};
use crate::lexer::{RegionKind, code_token_end, lex};
use crate::literals::{KNOWN_STATIC_SETUP_SQL, has_quoted_literal, redact_literals};
use crate::normalize::normalize;
use crate::statements::statement_count_hint;

/// The functions `--learn` refuses to propose a grant for.
///
/// Every entry is a file, large-object, network or server-control capability:
/// running it is not a query, and a grant that lists it hands the application
/// the server's filesystem or the network rather than a table. The names are
/// the ones PostgreSQL resolves for an unqualified or `pg_catalog`-qualified
/// call; a function of the same name in another schema is a different function
/// and is not refused by this list.
pub const DANGEROUS_LEARN_FUNCTIONS: &[&str] = &[
    // Files and large objects.
    "lo_export",
    "lo_import",
    "pg_file_rename",
    "pg_file_unlink",
    "pg_file_write",
    "pg_logdir_ls",
    "pg_ls_dir",
    "pg_read_binary_file",
    "pg_read_file",
    "pg_stat_file",
    // Network.
    "dblink",
    "dblink_connect",
    "dblink_connect_u",
    "dblink_disconnect",
    "dblink_exec",
    "dblink_fetch",
    "dblink_get_result",
    "dblink_open",
    "dblink_send_query",
    // Server control.
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_terminate_backend",
];

/// Whether `name` is one of [`DANGEROUS_LEARN_FUNCTIONS`].
///
/// The comparison is exact and case-sensitive, because PostgreSQL resolves
/// `"PG_READ_FILE"` to a different function than `pg_read_file`.
pub fn is_dangerous_learn_function(name: &str) -> bool {
    DANGEROUS_LEARN_FUNCTIONS.contains(&name)
}

/// Why `--learn` refused a statement.
///
/// [`Display`](std::fmt::Display) renders one actionable line, naming the fix
/// (`bind the value with a placeholder`) and never the value itself. In
/// particular, [`RefuseReason::Unparseable`] carries libpg_query's message
/// without the token it choked on: the parser quotes the offending bytes
/// (`syntax error at or near "s3cr3t"`), and the refusal is printed to a human.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefuseReason {
    /// PostgreSQL's own parser rejected the text.
    ///
    /// The message [`check_learnable`] stores is stripped of the token the
    /// parser quoted, so neither [`Display`](std::fmt::Display) nor `Debug` can
    /// show a literal a request carried. [`Display`](std::fmt::Display) strips
    /// it again, so a value built by hand is safe to print too.
    Unparseable(String),
    /// The text holds more than one statement.
    MultipleStatements(usize),
    /// The statement inlines a quoted literal.
    InlineLiteral,
    /// The statement contains `…`, the marker a redacted literal leaves behind.
    RedactedPlaceholder,
    /// `COPY … PROGRAM` runs a command on the server.
    CopyProgram,
    /// The statement calls a [`DANGEROUS_LEARN_FUNCTIONS`] entry.
    DangerousFunction(String),
}

impl std::fmt::Display for RefuseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable(message) => write!(
                f,
                "the statement is not valid SQL: {}",
                sanitize_parser_message(message)
            ),
            Self::MultipleStatements(count) => write!(
                f,
                "the text holds {count} statements, and a grant covers exactly one"
            ),
            Self::InlineLiteral => f.write_str(
                "the statement inlines a quoted literal; bind the value with a placeholder instead",
            ),
            Self::RedactedPlaceholder => f.write_str(
                "the statement contains `…`, a redacted literal; bind the value with a \
                 placeholder and grant the parameterised statement",
            ),
            Self::CopyProgram => {
                f.write_str("`COPY … PROGRAM` runs a command on the server; it cannot be granted")
            }
            Self::DangerousFunction(name) => write!(
                f,
                "`{name}` reaches the server's files or network; it cannot be learned"
            ),
        }
    }
}

impl std::error::Error for RefuseReason {}

/// libpg_query's message with the token it choked on removed.
///
/// PostgreSQL quotes the bytes it rejected — `syntax error at or near
/// "s3cr3t"` — and a `--learn` refusal is printed to a human and pasted into
/// tickets, so the tail from ` at or near ` is dropped. A double-quoted span
/// that survives is replaced by `"…"` as well: PostgreSQL's different wordings
/// (`unterminated quoted string`, `invalid Unicode escape value`, …) all quote
/// the token the same way, and this keeps a future wording from re-introducing
/// the leak. The function is idempotent, so applying it again in
/// [`Display`](std::fmt::Display) is safe.
fn sanitize_parser_message(message: &str) -> String {
    let head = match message.find(" at or near ") {
        Some(index) => &message[..index],
        None => message,
    };
    let mut out = String::with_capacity(head.len());
    let mut quoted = false;
    for c in head.chars() {
        if c == '"' {
            if quoted {
                out.push('…');
            }
            quoted = !quoted;
            out.push('"');
        } else if !quoted {
            out.push(c);
        }
    }
    if quoted {
        // An opening quote with no closing one: everything after it is data.
        out.push('…');
    }
    out
}

/// What `--learn` decided about one statement.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearnVerdict {
    /// The statement is grantable, as the normalised text a grant has to hold.
    Query {
        /// The statement normalised by [`crate::normalize`]: placeholders are
        /// `$?` and there is no literal left to redact.
        normalized: String,
    },
    /// The statement cannot be granted.
    Refused {
        /// The statement normalised and redacted, safe to print.
        redacted: String,
        /// Why it was refused.
        reason: RefuseReason,
    },
}

impl LearnVerdict {
    /// Whether the statement is grantable.
    pub fn is_query(&self) -> bool {
        matches!(self, Self::Query { .. })
    }

    /// The normalised text of a grantable statement.
    pub fn normalized(&self) -> Option<&str> {
        match self {
            Self::Query { normalized } => Some(normalized),
            Self::Refused { .. } => None,
        }
    }

    /// The redacted text of a refused statement.
    pub fn redacted(&self) -> Option<&str> {
        match self {
            Self::Query { .. } => None,
            Self::Refused { redacted, .. } => Some(redacted),
        }
    }

    /// Why the statement was refused.
    pub fn reason(&self) -> Option<&RefuseReason> {
        match self {
            Self::Query { .. } => None,
            Self::Refused { reason, .. } => Some(reason),
        }
    }
}

/// Decides whether `--learn` may propose a grant for `sql`.
///
/// `sql` is the statement as PostgreSQL receives it, with `$1` placeholders —
/// not the normalised form, which `$?` makes unparseable. The returned
/// [`LearnVerdict::Query`] carries the normalised text a grant has to hold.
///
/// The checks run in this order, and every one of them fails closed:
///
/// 1. a `…` in the text is a redacted literal: the value is gone, so the
///    statement can only be granted after it is parameterised
///    ([`RefuseReason::RedactedPlaceholder`]);
/// 2. an exact [`KNOWN_STATIC_SETUP_SQL`] match is grantable as it stands,
///    before the parser runs: `SELECT set_config('TimeZone', $?, false)` holds
///    `$?`, which is not valid SQL;
/// 3. PostgreSQL's own parser must accept the text
///    ([`RefuseReason::Unparseable`]);
/// 4. it must be exactly one statement
///    ([`RefuseReason::MultipleStatements`]);
/// 5. `COPY … PROGRAM` runs a command on the server
///    ([`RefuseReason::CopyProgram`]);
/// 6. no quoted literal may remain ([`RefuseReason::InlineLiteral`]);
/// 7. no [`DANGEROUS_LEARN_FUNCTIONS`] entry may be called
///    ([`RefuseReason::DangerousFunction`]).
///
/// ```
/// # use blastit_sqlpolicy::{LearnVerdict, check_learnable};
/// let verdict = check_learnable(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $1"#);
/// assert_eq!(
///     verdict.normalized(),
///     Some(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#),
/// );
/// assert!(check_learnable("SELECT 'x'").reason().is_some());
/// ```
pub fn check_learnable(sql: &str) -> LearnVerdict {
    let normalized = normalize(sql);
    let refuse = |reason: RefuseReason| LearnVerdict::Refused {
        redacted: redact_literals(&normalized),
        reason,
    };

    if normalized.contains('…') {
        return refuse(RefuseReason::RedactedPlaceholder);
    }
    if KNOWN_STATIC_SETUP_SQL.contains(&normalized.as_str()) {
        return LearnVerdict::Query { normalized };
    }

    let parsed = match pg_query::parse(sql) {
        Ok(parsed) => parsed,
        Err(error) => {
            return refuse(RefuseReason::Unparseable(sanitize_parser_message(&syntax(
                &error,
            ))));
        }
    };
    let statements = &parsed.protobuf.stmts;
    match statements.len() {
        0 => return refuse(no_statement()),
        1 => {}
        count => return refuse(RefuseReason::MultipleStatements(count)),
    }
    let Some(tree) = statement_tree(statements) else {
        return refuse(no_statement());
    };

    if is_copy_program(&tree) {
        return refuse(RefuseReason::CopyProgram);
    }
    if has_quoted_literal(&normalized) {
        return refuse(RefuseReason::InlineLiteral);
    }
    if let Some(name) = find_dangerous_function(&tree) {
        return refuse(RefuseReason::DangerousFunction(name));
    }
    LearnVerdict::Query { normalized }
}

/// Whether `normalized` is a connection-setup statement a session may replay.
///
/// The text is normalised first, so a caller may hand over wire text or the
/// normalised text a log carries. A statement qualifies when it is
///
/// * an exact [`KNOWN_STATIC_SETUP_SQL`] match,
/// * a single `SET` or `SHOW` statement, or
/// * a single read-only `SELECT` that only calls the introspection functions
///   and only reads the introspection catalogues — `SELECT
///   pg_catalog.version()`, `SELECT current_schema()`, `SELECT to_regtype($?)`,
///   a `pg_type` lookup.
///
/// Everything else is false, so a statement is never replayed on the strength
/// of looking like setup. Multi-statement text is always false.
///
/// ```
/// # use blastit_sqlpolicy::is_introspection_setup_sql;
/// assert!(is_introspection_setup_sql("SET search_path = public"));
/// assert!(is_introspection_setup_sql("SHOW standard_conforming_strings"));
/// assert!(is_introspection_setup_sql("SELECT pg_catalog.version()"));
/// assert!(is_introspection_setup_sql("SELECT to_regtype($?)"));
/// assert!(!is_introspection_setup_sql("SELECT * FROM auth_user"));
/// assert!(!is_introspection_setup_sql("SET x; DROP TABLE t"));
/// ```
pub fn is_introspection_setup_sql(normalized: &str) -> bool {
    let text = normalize(normalized);
    if text.is_empty() {
        return false;
    }
    if KNOWN_STATIC_SETUP_SQL.contains(&text.as_str()) {
        return true;
    }
    if statement_count_hint(&text) != 1 {
        return false;
    }
    let parseable = parse_form(&text);
    let Ok(parsed) = pg_query::parse(&parseable) else {
        return false;
    };
    let statements = &parsed.protobuf.stmts;
    if statements.len() != 1 {
        return false;
    }
    let Some(tree) = statement_tree(statements) else {
        return false;
    };
    // `SET …` and `SHOW …` are session setup whatever they set: the statement
    // already ran while no route was active, and replaying it keeps the
    // connection's behaviour identical.
    if tree.get("VariableSetStmt").is_some() || tree.get("VariableShowStmt").is_some() {
        return true;
    }
    is_introspection_select(&tree)
}

/// Whether the tree is a single read-only `SELECT` over the catalogue.
///
/// The walk is stricter than "it parses": a `SELECT … INTO` creates a table, a
/// `FOR UPDATE` locks rows, and a data-modifying `WITH` writes — none of which
/// is setup. Functions and relations are checked by name against
/// [`INTROSPECTION_FUNCTIONS`] and [`INTROSPECTION_TABLES`], so a statement
/// that reads `pg_authid` or calls `pg_read_file` is not replayed.
fn is_introspection_select(tree: &Value) -> bool {
    match one_of(tree) {
        Some(("SelectStmt", _)) => introspection_node(tree, 0),
        _ => false,
    }
}

/// How deep the introspection walk may go before it refuses.
///
/// The parse tree is bounded by PostgreSQL's parser, but the walk is recursive,
/// and a statement that nests deeply enough to exhaust the stack would abort
/// `dbproxy`. Refusing is the safe direction: the statement is simply not
/// replayed.
const MAX_INTROSPECTION_DEPTH: usize = 128;

/// Whether every node of a catalogue `SELECT` is one this build replays.
fn introspection_node(node: &Value, depth: usize) -> bool {
    if depth > MAX_INTROSPECTION_DEPTH {
        return false;
    }
    match node {
        Value::Object(map) => {
            if let Some((kind, body)) = map.iter().next().filter(|_| map.len() == 1) {
                match kind.as_str() {
                    "FuncCall" => {
                        let Some(name) = function_name(body) else {
                            return false;
                        };
                        if !INTROSPECTION_FUNCTIONS.contains(&name.as_str()) {
                            return false;
                        }
                    }
                    "RangeVar" => {
                        if !is_introspection_relation(body) {
                            return false;
                        }
                    }
                    // A `SELECT` that writes: a data-modifying CTE, an `INTO`
                    // table or a lock. None of it is session setup.
                    "InsertStmt" | "UpdateStmt" | "DeleteStmt" | "MergeStmt" | "CopyStmt"
                    | "CreateTableAsStmt" | "RefreshMatViewStmt" | "CallStmt" | "DoStmt" => {
                        return false;
                    }
                    "SelectStmt" => {
                        if body
                            .get("into_clause")
                            .is_some_and(|value| !value.is_null())
                        {
                            return false;
                        }
                        if body.get("locking_clause").is_some_and(|value| {
                            value.as_array().is_some_and(|clauses| !clauses.is_empty())
                        }) {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            map.values()
                .all(|value| introspection_node(value, depth + 1))
        }
        Value::Array(items) => items.iter().all(|item| introspection_node(item, depth + 1)),
        _ => true,
    }
}

/// Whether a `RangeVar` names a catalogue relation in [`INTROSPECTION_TABLES`].
///
/// A qualified name has to match its schema exactly. An unqualified one is
/// resolved through the search path, where only `pg_catalog` is guaranteed to
/// come first: `pg_type` is the catalogue, but `tables` is whatever the route's
/// `public` schema holds, so `information_schema` entries need their schema
/// spelled out.
fn is_introspection_relation(body: &Value) -> bool {
    if body
        .get("catalogname")
        .and_then(Value::as_str)
        .is_some_and(|catalog| !catalog.is_empty())
    {
        return false;
    }
    let schema = body
        .get("schemaname")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(name) = body.get("relname").and_then(Value::as_str) else {
        return false;
    };
    if name.is_empty() {
        return false;
    }
    match schema {
        "pg_catalog" | "information_schema" => INTROSPECTION_TABLES
            .iter()
            .any(|table| *table == format!("{schema}.{name}")),
        // `pg_catalog` is first on every search path, so an unqualified
        // `pg_type` resolves to `pg_catalog.pg_type`. `information_schema` is
        // not on the search path, and `tables`, `columns` and `schemata` are
        // ordinary names a `public` table can take, so an unqualified name is
        // only ever matched against the `pg_catalog` entries.
        "" => INTROSPECTION_TABLES.iter().any(|table| {
            table
                .strip_prefix("pg_catalog.")
                .is_some_and(|bare| bare == name)
        }),
        _ => false,
    }
}

/// The functions a replayable introspection `SELECT` may call.
///
/// Every entry reads the catalogue or the session and nothing else. A function
/// that is not here (`pg_read_file`, `current_setting` on an arbitrary name is
/// still a read, but `pg_proc`-reading helpers are not) keeps the statement
/// out of the replay list, which is the safe direction: a setup statement that
/// is not replayed is one the route's grant has to cover. `pg_get_userbyid` is
/// deliberately absent: it resolves role names out of `pg_authid`, which no
/// route is granted.
const INTROSPECTION_FUNCTIONS: &[&str] = &[
    "col_description",
    "current_database",
    "current_schema",
    "current_schemas",
    "current_setting",
    "current_user",
    "format_type",
    "has_schema_privilege",
    "has_table_privilege",
    "inet_client_addr",
    "inet_server_addr",
    "obj_description",
    "pg_backend_pid",
    "pg_conf_load_time",
    "pg_encoding_to_char",
    "pg_get_constraintdef",
    "pg_get_expr",
    "pg_get_indexdef",
    "pg_get_serial_sequence",
    "pg_get_viewdef",
    "pg_is_in_recovery",
    "pg_postmaster_start_time",
    "pg_relation_size",
    "pg_size_pretty",
    "pg_table_is_visible",
    "pg_total_relation_size",
    "pg_typeof",
    "session_user",
    "to_regclass",
    "to_regnamespace",
    "to_regproc",
    "to_regprocedure",
    "to_regtype",
    "txid_current",
    "user",
    "version",
];

/// The catalogues a replayable introspection `SELECT` may read.
///
/// Deliberately not "every `pg_catalog` table": `pg_authid`, `pg_shadow`,
/// `pg_proc` and `pg_settings` would widen what a route can read, and a replay
/// is only meant to repeat a driver's session setup.
const INTROSPECTION_TABLES: &[&str] = &[
    "information_schema.columns",
    "information_schema.schemata",
    "information_schema.tables",
    "pg_catalog.pg_am",
    "pg_catalog.pg_attribute",
    "pg_catalog.pg_class",
    "pg_catalog.pg_collation",
    "pg_catalog.pg_constraint",
    "pg_catalog.pg_database",
    "pg_catalog.pg_enum",
    "pg_catalog.pg_index",
    "pg_catalog.pg_namespace",
    "pg_catalog.pg_range",
    "pg_catalog.pg_type",
];

/// The one statement a parse produced, as the JSON tree the walkers read.
fn statement_tree(statements: &[pg_query::protobuf::RawStmt]) -> Option<Value> {
    let node = statements.first()?.stmt.as_ref()?.node.as_ref()?;
    serde_json::to_value(node).ok()
}

/// The "no statement" refusal, which is a parse failure as far as a caller is
/// concerned: nothing can be granted.
fn no_statement() -> RefuseReason {
    RefuseReason::Unparseable("there is no statement to learn".to_string())
}

/// Whether the tree holds a `COPY … PROGRAM` node.
fn is_copy_program(node: &Value) -> bool {
    match node {
        Value::Object(map) => {
            if let Some(body) = map.get("CopyStmt")
                && body.get("is_program").and_then(Value::as_bool) == Some(true)
            {
                return true;
            }
            map.values().any(is_copy_program)
        }
        Value::Array(items) => items.iter().any(is_copy_program),
        _ => false,
    }
}

/// The first [`DANGEROUS_LEARN_FUNCTIONS`] call in the tree, if any.
fn find_dangerous_function(node: &Value) -> Option<String> {
    match node {
        Value::Object(map) => {
            if let Some(body) = map.get("FuncCall")
                && let Some(name) = dangerous_function_name(body)
            {
                return Some(name);
            }
            // `CALL p(...)` parses to a `CallStmt` whose `funccall` field holds
            // the `FuncCall` message directly, not the one-key `Node` wrapper
            // the walk below looks for, so it needs reading here.
            if let Some(name) = map
                .get("CallStmt")
                .and_then(|body| body.get("funccall"))
                .and_then(dangerous_function_name)
            {
                return Some(name);
            }
            map.values().find_map(find_dangerous_function)
        }
        Value::Array(items) => items.iter().find_map(find_dangerous_function),
        _ => None,
    }
}

/// The parts of a `FuncCall`'s `funcname`, in order.
fn function_parts(body: &Value) -> Option<Vec<&str>> {
    Some(
        body.get("funcname")?
            .as_array()?
            .iter()
            .filter_map(|part| unwrap(part).get("String")?.get("sval")?.as_str())
            .collect(),
    )
}

/// The name a `FuncCall` resolves to, for an unqualified or `pg_catalog` call.
///
/// A call in another schema is a different function, so it is not reported:
/// `public.pg_read_file` is not the server's `pg_read_file`.
fn function_name(body: &Value) -> Option<String> {
    let parts = function_parts(body)?;
    let name = match parts.as_slice() {
        [name] => *name,
        [schema, name] if *schema == "pg_catalog" => *name,
        _ => return None,
    };
    (!name.is_empty()).then(|| name.to_string())
}

/// The dangerous name a call resolves to, failing closed on a three-part name.
///
/// PostgreSQL accepts `database.schema.function` as well as the one- and
/// two-part forms, and `pg_catalog.pg_catalog.pg_read_file` resolves to the
/// server's function when the database is named `pg_catalog`. A grant is not
/// the place to guess, so any name with more than two parts is refused when its
/// last part is dangerous.
fn dangerous_function_name(body: &Value) -> Option<String> {
    let parts = function_parts(body)?;
    let (name, qualifiers) = parts.split_last()?;
    // One qualifier has to be `pg_catalog`; an unqualified name resolves
    // through the search path, and a three-part `database.schema.function` is
    // refused rather than guessed at.
    let resolved = match qualifiers {
        [schema] => *schema == "pg_catalog",
        _ => true,
    };
    (resolved && !name.is_empty() && is_dangerous_learn_function(name)).then(|| (*name).to_string())
}

/// `normalized` with every `$?` placeholder turned back into `$1`.
///
/// A normalised statement is what a log carries, and `$?` is not valid SQL, so
/// the parser needs the placeholder spelling. Only a `$?` that starts a code
/// token is replaced: one inside a literal, a quoted identifier or an
/// identifier is data, and the lexer never hands those over as code.
fn parse_form(normalized: &str) -> String {
    if !normalized.contains("$?") {
        return normalized.to_string();
    }
    let chars: Vec<char> = normalized.chars().collect();
    let mut out = String::with_capacity(normalized.len());
    for region in lex(&chars) {
        let text = &chars[region.start..region.end];
        if region.kind != RegionKind::Code {
            out.extend(text);
            continue;
        }
        // The code text is consumed one token at a time: `rest` is the part that
        // is left, so every step strictly shortens it and the rewrite cannot
        // stall on a hostile statement.
        let mut rest = text;
        while let Some(first) = rest.first() {
            if *first == '$' && rest.get(1) == Some(&'?') {
                out.push_str("$1");
                // `get(1)` proved the second character exists, so this drops
                // both of them.
                rest = rest.get(2..).unwrap_or_default();
                continue;
            }
            // One whole code token, copied verbatim: `a$?` is a single
            // identifier and keeps its `$`.
            let end = code_token_end(rest, 0).clamp(1, rest.len());
            let Some((token, tail)) = rest.split_at_checked(end) else {
                // Unreachable: `end` is clamped to `1..=rest.len()`. Stopping
                // keeps the rewrite finite rather than panicking on the way out.
                break;
            };
            out.extend(token);
            rest = tail;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameterised_statement_is_grantable() {
        let verdict = check_learnable(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $1"#);
        assert_eq!(
            verdict.normalized(),
            Some(r#"SELECT "t"."id" FROM "t" WHERE "t"."id" = $?"#)
        );
        assert!(verdict.is_query());
        assert!(verdict.reason().is_none());
    }

    #[test]
    fn every_inline_literal_form_is_refused() {
        for sql in [
            "SELECT 'x'",
            "SELECT E'x'",
            "SELECT B'1'",
            "SELECT X'ff'",
            "SELECT N'x'",
            r"SELECT U&'\0041'",
            "SELECT $$x$$",
            "SELECT $t$x$t$",
        ] {
            let verdict = check_learnable(sql);
            assert_eq!(
                verdict.reason(),
                Some(&RefuseReason::InlineLiteral),
                "{sql:?}"
            );
            assert_eq!(verdict.redacted(), Some("SELECT …"), "{sql:?}");
        }
    }

    #[test]
    fn a_redacted_placeholder_is_refused() {
        let verdict = check_learnable("SELECT … FROM t");
        assert_eq!(verdict.reason(), Some(&RefuseReason::RedactedPlaceholder));
    }

    #[test]
    fn unparseable_and_multi_statement_are_refused() {
        assert!(matches!(
            check_learnable("SELEC 1").reason(),
            Some(RefuseReason::Unparseable(_))
        ));
        assert!(matches!(
            check_learnable("").reason(),
            Some(RefuseReason::Unparseable(_))
        ));
        assert_eq!(
            check_learnable("SELECT 1; SELECT 2").reason(),
            Some(&RefuseReason::MultipleStatements(2))
        );
    }

    #[test]
    fn a_parser_message_never_carries_the_token() {
        assert_eq!(
            sanitize_parser_message("Invalid statement: syntax error at or near \"s3cr3t\""),
            "Invalid statement: syntax error"
        );
        assert_eq!(
            sanitize_parser_message("invalid Unicode escape value at or near \"'\\0073'\""),
            "invalid Unicode escape value"
        );
        assert_eq!(
            sanitize_parser_message("invalid input syntax for type integer: \"hunter2\""),
            "invalid input syntax for type integer: \"…\""
        );
        assert_eq!(
            sanitize_parser_message("syntax error at end of input"),
            "syntax error at end of input"
        );
        // The refusal stores the stripped message, so even `Debug` is safe.
        let verdict = check_learnable("SELECT 's3cr3t");
        let debug = format!("{verdict:?}");
        assert!(!debug.contains("s3cr3t"), "{debug:?}");
        let Some(reason) = verdict.reason() else {
            panic!("the statement must be refused: {verdict:?}");
        };
        let rendered = reason.to_string();
        assert!(!rendered.contains("s3cr3t"), "{rendered:?}");
    }

    #[test]
    fn copy_program_is_refused() {
        for sql in [
            "COPY t FROM PROGRAM 'id'",
            "COPY (SELECT 1) TO PROGRAM 'id'",
        ] {
            assert_eq!(
                check_learnable(sql).reason(),
                Some(&RefuseReason::CopyProgram),
                "{sql:?}"
            );
        }
        // `COPY … TO STDOUT` runs no command, so it is only refused for the
        // literal it would need to carry a value.
        assert_eq!(
            check_learnable("COPY t TO STDOUT").reason(),
            None,
            "a plain COPY is grantable"
        );
    }

    #[test]
    fn every_dangerous_function_is_refused() {
        for name in DANGEROUS_LEARN_FUNCTIONS {
            let sql = format!("SELECT {name}($1)");
            let verdict = check_learnable(&sql);
            assert_eq!(
                verdict.reason(),
                Some(&RefuseReason::DangerousFunction((*name).to_string())),
                "{sql:?}"
            );
        }
    }

    #[test]
    fn a_function_of_the_same_name_in_another_schema_is_not_refused() {
        let verdict = check_learnable("SELECT public.pg_read_file($1)");
        assert!(verdict.is_query());
    }

    #[test]
    fn a_call_and_a_three_part_name_are_refused() {
        for sql in [
            "CALL pg_read_file($1)",
            "CALL pg_catalog.pg_read_file($1)",
            "SELECT pg_catalog.pg_catalog.pg_read_file($1)",
        ] {
            let verdict = check_learnable(sql);
            assert!(
                matches!(verdict.reason(), Some(RefuseReason::DangerousFunction(_))),
                "{sql:?} -> {verdict:?}"
            );
        }
    }

    #[test]
    fn known_static_setup_is_grantable_before_the_parser_runs() {
        for sql in KNOWN_STATIC_SETUP_SQL {
            let verdict = check_learnable(sql);
            assert_eq!(verdict.normalized(), Some(*sql), "{sql:?}");
        }
    }

    #[test]
    fn introspection_setup_is_recognised() {
        for sql in [
            "SET TIME ZONE 'UTC'",
            "SET search_path = public",
            "SHOW standard_conforming_strings",
            "SELECT pg_catalog.version()",
            "SELECT current_schema()",
            "SELECT to_regtype($?)",
            "SELECT * FROM pg_catalog.pg_type WHERE oid = $?",
        ] {
            assert!(is_introspection_setup_sql(sql), "{sql:?}");
        }
        for sql in [
            "SELECT * FROM auth_user",
            "SELECT pg_read_file('/etc/passwd')",
            "SELECT pg_get_userbyid($?)",
            "SELECT * FROM tables",
            "SELECT * FROM columns",
            "SELECT * FROM schemata",
            "DROP TABLE t",
            "SET x; DROP TABLE t",
            "SELECT 1; SHOW x",
            "",
        ] {
            assert!(!is_introspection_setup_sql(sql), "{sql:?}");
        }
        assert!(is_introspection_setup_sql(
            "SELECT * FROM information_schema.tables"
        ));
    }

    #[test]
    fn parse_form_only_rewrites_code_placeholders() {
        assert_eq!(parse_form("SELECT $?"), "SELECT $1");
        assert_eq!(parse_form("SELECT '$?'"), "SELECT '$?'");
        assert_eq!(parse_form(r#"SELECT "a$?""#), r#"SELECT "a$?""#);
        assert_eq!(parse_form("SELECT a$?"), "SELECT a$?");
        assert_eq!(parse_form("SELECT 1"), "SELECT 1");
    }
}
