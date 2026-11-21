//! The walk over PostgreSQL's parse tree: dispatch, coverage, depth.
//!
//! The tree is walked by structure — every node type the resolver handles is
//! named here, and everything else is descended into field by field — so a node
//! type this build does not name cannot hide a read. Two mechanisms keep that
//! honest as the code grows: every node the walk handles is registered as
//! *visited*, and the whole tree is scanned afterwards for nodes of those types
//! ([`Resolver::check_coverage`]); and the walk has a depth limit ([`MAX_DEPTH`]),
//! so a pathological statement is refused rather than allowed to exhaust
//! dbproxy's stack.
//!
//! What a statement *means* is resolved in [`super::resolve`]; this module is
//! the traversal and its parse-tree accessors.

use std::collections::{BTreeSet, HashSet};

use serde_json::Value;

use super::classify::{statement_label, syntax};
use super::scope::{Reads, Scope};
use super::types::{Access, Analysis, AnalysisError, Op, StatementKind};

/// How deep the walked parse tree may be before the statement is refused.
///
/// PostgreSQL bounds a parse tree by its own stack limit, and no real statement
/// comes close to this; the limit is here so that a hostile statement cannot
/// exhaust dbproxy's stack (dbproxy is the process holding the credentials).
const MAX_DEPTH: usize = 128;

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

/// The statement walker.
#[derive(Default)]
pub(super) struct Resolver {
    pub(super) accesses: BTreeSet<Access>,
    pub(super) functions: BTreeSet<String>,
    /// Every base table the statement names, whether or not it reads a column
    /// of it. A table in here with no `select` access gets a
    /// [`Access::named`] one, so naming a table is enough to need a grant for it
    /// (§6, D7).
    pub(super) named: BTreeSet<String>,
    /// The nodes of [`HANDLED`] types the walk actually handled, by address, so
    /// that [`Resolver::check_coverage`] can prove nothing was skipped.
    visited: HashSet<usize>,
    depth: usize,
}

/// Parses and analyses one statement.
pub(super) fn resolve(sql: &str) -> Result<Analysis, AnalysisError> {
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

impl Resolver {
    /// Counts one access.
    pub(super) fn access(&mut self, op: Op, table: &str, column: Option<&str>) {
        self.accesses
            .insert(Access::new(op, table.to_string(), column));
    }

    /// Remembers that the statement names a base table, even if it reads no
    /// column of it.
    pub(super) fn name_table(&mut self, table: &str) {
        self.named.insert(table.to_string());
    }

    /// Counts every access a reading needs.
    pub(super) fn accesses_of(&mut self, reads: &Reads, name: Option<&str>) {
        self.accesses.extend(reads.accesses(name));
    }

    /// Remembers that a node of a handled type was handled.
    pub(super) fn mark(&mut self, node: &Value) {
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
    pub(super) fn dispatch(&mut self, node: &Value, scope: &Scope) -> Result<(), AnalysisError> {
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
                let name = super::last_name(body);
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
    pub(super) fn expression(
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
    pub(super) fn expressions(
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
pub(crate) fn unwrap(value: &Value) -> &Value {
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
pub(crate) fn one_of(value: &Value) -> Option<(&str, &Value)> {
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
pub(super) fn typed_body<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    let node = unwrap(node);
    match one_of(node) {
        Some((found, body)) => (found == kind).then_some(body),
        None if node.is_object() => Some(node),
        None => None,
    }
}

pub(super) fn node_field<'a>(body: &'a Value, field: &str) -> Option<&'a Value> {
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
pub(super) fn node_list<'a>(body: &'a Value, field: &str) -> Vec<&'a Value> {
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
pub(super) fn text<'a>(body: &'a Value, field: &str) -> Option<&'a str> {
    body.get(field)?.as_str()
}

/// A boolean field of a message body; absent counts as false.
pub(super) fn flag(body: &Value, field: &str) -> bool {
    body.get(field).and_then(Value::as_bool).unwrap_or(false)
}

/// The name of a leaf `String` node.
pub(super) fn string_node(node: &Value) -> Option<&str> {
    match one_of(unwrap(node)) {
        Some(("String", body)) => text(body, "sval"),
        _ => None,
    }
}
