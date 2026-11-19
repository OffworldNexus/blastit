//! The `Blastfile`'s lowered model, and the one writer that edits it.
//!
//! `fold` and the pytest plugin both have to know what a `Blastfile` declares
//! before they can plan a grant or map a database alias, and both have to write
//! the edited document back. They must not parse or splice it themselves —
//! there is one reader and one writer (§15.2, §14.5) — so this module is the
//! bridge: [`blastfile_model`] returns the lowered model as JSON, and
//! [`blastfile_apply_edits`] applies a `BlastfileEdits` document through the
//! manifest crate's verified editor.

use std::collections::BTreeMap;

use blastit_manifest::blastfile::{BlastfileEdits, apply_edits};
use blastit_manifest::{Grant, Manifest, ProxyName};
use serde_json::{Map, Value, json};

use crate::error::AuditError;
/// The `Blastfile`'s model, as JSON: its stores, traits and views.
///
/// `fold` and the pytest plugin both have to know what a `Blastfile` declares
/// before they can plan a grant or map a database alias. They must not parse one
/// themselves — there is one reader (§15.2) — so this binding hands over the
/// lowered model instead: the store names and their input slots, every trait's
/// grants by store, and every view's name, patterns, methods, attached traits
/// and grants.
///
/// It is [`Manifest::from_blastfile_str`]: parse and lower, **without**
/// `validate`, exactly as the Python reader it replaces did. A caller that wants
/// validation runs [`crate::audit`], which parses with the same reader and validates.
///
/// ```json
/// {
///   "stores": [{"name": "db", "protocol": "postgresql",
///               "url": "DATABASE_URL", "inputs": {"url": "DATABASE_URL"}}],
///   "traits": {"session": {"db": {"queries": ["SELECT 40"], "unrestricted": false,
///                                 "transactions": true, "tables": {}, "functions": []}}},
///   "views": [{"name": "notes", "methods": ["GET", "HEAD"], "patterns": ["/notes/.*"],
///              "use": ["session"], "egress": false,
///              "grants": {"db": {"queries": ["SELECT 10"], …}}}]
/// }
/// ```
///
/// # Errors
///
/// [`AuditError`] with the parser's or lowerer's one-line reason when the text is
/// not a `Blastfile`; no partial model is ever returned.
pub fn blastfile_model(source: &str) -> Result<String, AuditError> {
    let manifest =
        Manifest::from_blastfile_str(source).map_err(|errors| AuditError::manifest(&errors))?;
    let stores: Vec<Value> = manifest
        .proxies()
        .iter()
        .map(|(name, proxy)| {
            json!({
                "name": name.as_str(),
                "protocol": proxy.protocol().as_str(),
                "url": proxy.input().get("url"),
                "inputs": proxy.input(),
            })
        })
        .collect();
    let traits: Map<String, Value> = manifest
        .groups()
        .iter()
        .map(|(name, group)| (name.clone(), grants_json(group.grants())))
        .collect();
    let views: Vec<Value> = manifest
        .routes()
        .iter()
        .map(|route| {
            json!({
                "name": route.name(),
                "methods": route.methods(),
                "patterns": route.path(),
                "use": route.groups(),
                "egress": route.egress(),
                "grants": grants_json(route.grants()),
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({
        "stores": stores,
        "traits": traits,
        "views": views,
    }))
    .map_err(|error| AuditError::Report(error.to_string()))
}

/// One map of grants, by store, as the model document spells it.
fn grants_json(grants: &BTreeMap<ProxyName, Grant>) -> Value {
    let by_store: Map<String, Value> = grants
        .iter()
        .map(|(proxy, grant)| {
            let value = match grant {
                Grant::Postgresql(grant) => json!({
                    "queries": grant.queries(),
                    "unrestricted": grant.is_unrestricted(),
                    "transactions": grant.transaction_control(),
                    "tables": tables_json(grant.tables()),
                    "functions": grant.tables().functions,
                }),
            };
            (proxy.as_str().to_owned(), value)
        })
        .collect();
    Value::Object(by_store)
}

/// A table scope as a `Blastfile` holds it: only the operations it grants.
///
/// `TableGrant`'s own serde shape carries every field (`insert: null`,
/// `delete: false` included) because the report does; a model of a file should
/// read like the file.
fn tables_json(tables: &blastit_sqlpolicy::TableGrants) -> Value {
    let by_table: Map<String, Value> = tables
        .tables
        .iter()
        .map(|(table, grant)| {
            let mut value = Map::new();
            for (operation, columns) in [
                ("select", &grant.select),
                ("insert", &grant.insert),
                ("update", &grant.update),
            ] {
                if let Some(columns) = columns {
                    value.insert(operation.to_owned(), json!(columns));
                }
            }
            if grant.delete {
                value.insert("delete".to_owned(), Value::Bool(true));
            }
            (table.clone(), Value::Object(value))
        })
        .collect();
    Value::Object(by_table)
}

/// Applies an edits document to a `Blastfile` (§14.5's editor, the one writer).
///
/// `edits_json` is a [`BlastfileEdits`] document — the shape `--learn` plans and
/// `fold` builds — and `source` is the file it is applied to. The editor parses
/// the document into the one tree, splices the proposals, and re-parses,
/// re-lowers and compares the result with the model the edits intended, so a
/// returned string is already proved; an edit that cannot reproduce the document
/// is an error and nothing is written.
///
/// `fold` calls this instead of carrying its own writer, which is what makes
/// `blastit-manifest fold`, `blastit check --learn` and `blastit dev --learn` one
/// editor rather than three.
///
/// # Errors
///
/// The edits not being a [`BlastfileEdits`] document, the source not parsing,
/// and every refusal of §14.5's table (a literal or redacted statement, an
/// unwritable table/column/function name, a store the file does not declare, a
/// failed postcondition). The messages name the view and the store, never a
/// statement.
///
/// # Returns
///
/// The edited source, byte-identical outside the spliced lines.
pub fn blastfile_apply_edits(source: &str, edits_json: &str) -> Result<String, String> {
    let edits: BlastfileEdits = serde_json::from_str(edits_json)
        .map_err(|error| format!("the edits are not a BlastfileEdits document: {error}"))?;
    apply_edits(source, &edits).map_err(|error| error.to_string())
}
