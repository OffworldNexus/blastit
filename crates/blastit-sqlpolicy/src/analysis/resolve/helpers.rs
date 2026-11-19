//! The parse-tree accessors the statement resolvers share: names, aliases and
//! the shapes a `ResTarget` or a write target takes.

use serde_json::Value;

use super::super::last_name;
use super::super::table_key;
use super::super::types::AnalysisError;
use super::super::walk::{node_field, node_list, one_of, string_node, text, typed_body, unwrap};

/// Whether a target list entry is a `*` expansion.
pub(super) fn star_target(node: &Value) -> bool {
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
pub(super) fn output_name(node: &Value) -> Option<String> {
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
pub(super) fn res_target_name(body: &Value) -> Option<String> {
    text(body, "name")
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// The column names an `INSERT`'s `cols` list writes.
pub(super) fn insert_columns(body: &Value) -> Vec<String> {
    node_list(body, "cols")
        .into_iter()
        .filter_map(|column| typed_body(column, "ResTarget"))
        .filter_map(res_target_name)
        .collect()
}

/// The `RangeVar` a statement writes to: its `relation` field.
pub(super) fn target_table(body: &Value) -> Result<String, AnalysisError> {
    let relation = node_field(body, "relation")
        .ok_or_else(|| AnalysisError::NotUnderstood("a statement with no table".to_string()))?;
    let relation = unwrap(relation);
    let name = text(relation, "relname")
        .ok_or_else(|| AnalysisError::NotUnderstood("a table name".to_string()))?;
    table_key(text(relation, "schemaname").unwrap_or_default(), name)
}

/// The alias a `FROM` item's relation carries, if it has one.
pub(super) fn alias_name(body: &Value) -> Option<String> {
    let alias = body.get("alias")?.as_object()?;
    let name = alias.get("aliasname")?.as_str()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// A list of `String` nodes as owned names (`aliascolnames`, `using_clause`).
pub(super) fn names_of(value: Option<&Value>) -> Vec<String> {
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
pub(super) fn set_operation(body: &Value) -> bool {
    body.get("op")
        .and_then(Value::as_i64)
        .is_some_and(|op| op != 1)
}

/// A column reference's star, if it has one.
pub(super) fn is_star(node: &Value) -> bool {
    matches!(one_of(unwrap(node)), Some(("AStar", _)))
}
