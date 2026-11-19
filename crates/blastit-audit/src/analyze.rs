//! One statement, as PostgreSQL's own parser sees it.
//!
//! Three renderings of the same analysis, for the three callers that need it:
//!
//! * [`analyze`] — the smallest table grant one statement needs, as JSON;
//! * [`covering`] — the same for a *set* of statements, taken at once, which is
//!   what makes a row lock ask for `update` on a column the route already reads
//!   instead of on `"*"` (`fold --prefer-tables`);
//! * [`classify`] — what the parser says a statement *is*, for every statement,
//!   not only the four a table grant covers.
//!
//! Nothing here classifies by text: [`blastit_sqlpolicy`] owns the parser, and
//! its message is the reason a `sql.denied` event carries.

use blastit_sqlpolicy::TableGrants;
use serde_json::json;
/// Analyses one statement and renders it as the report's `table_grants` shape
/// (D7).
///
/// This is [`blastit_sqlpolicy::analyze`] — the same analysis the policy runs —
/// rendered as JSON, so `import blastit._blastit; _blastit.analyze(sql)` shows
/// exactly what a table grant would have to cover.
///
/// # Errors
///
/// [`blastit_sqlpolicy::AnalysisError`], whose message is the reason a
/// `sql.denied` event carries.
pub fn analyze(sql: &str) -> Result<String, String> {
    let analysis = blastit_sqlpolicy::analyze(sql).map_err(|error| error.to_string())?;
    let mut document = serde_json::to_value(TableGrants::covering(std::iter::once(&analysis)))
        .map_err(|error| error.to_string())?;
    if let Some(map) = document.as_object_mut() {
        // The grant's own shape, plus the kind of statement it came from.
        map.insert("kind".to_string(), json!(analysis.kind().as_str()));
    }
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}
/// Why a statement that locks a table without reading a column of it cannot be
/// covered by a table grant (§6, D3c).
///
/// PostgreSQL asks for the UPDATE privilege on any *one* column of a locked
/// table, and a table grant has no `"*"` for a lock: `"*"` would hand out the
/// UPDATE privilege on every column. So a lock with no column to name — the
/// statement reads none of them (`SELECT 1 FROM t FOR UPDATE`, `SELECT * FROM t
/// FOR UPDATE`) — keeps the statement an exact query, and the table grant is
/// left for a manifest author to finish by naming a column.
pub const LOCK_NEEDS_A_COLUMN: &str = "its rows are locked (`FOR UPDATE`) without reading \
     a column of it, so name one in the table's `update` entry — its primary key, say: a table \
     grant never takes `\"*\"` for a lock, and without the entry the statement is refused";

/// The smallest table grant that covers a set of statements (D3c).
///
/// This is `blastit._blastit.covering`, the function `blastit-manifest fold
/// --prefer-tables` uses to turn the statements a log denied into the grant a
/// manifest should hold: every statement is analysed, and the grant is the one
/// [`TableGrants::covering`] derives from *all of them at once* — which is what
/// makes a row lock (`FOR UPDATE`) ask for `update` on a column the route
/// already reads, instead of on `"*"`.
///
/// The statements are handed over as they went on the wire or as the log
/// recorded them; each one is normalised by the analysis itself.
///
/// # Returns
///
/// Pretty-printed JSON:
///
/// ```json
/// {
///   "tables": {"shop_product": {"select": ["id", "name"]}},
///   "functions": ["unaccent"],
///   "blastfile": "# view shop postgres\nfunctions unaccent\nselect shop_product(id, name)\n",
///   "unanalysable": [{"sql": "SET TIME ZONE 'UTC'",
///                     "reason": "only SELECT, INSERT, UPDATE and DELETE…"}]
/// }
/// ```
///
/// `locator` is the `Blastfile` node the fragment belongs in — `view <name>
/// postgres`, `view <name> postgres <store>`, or `trait <name>` — and becomes
/// the value's one leading `#` comment.
///
/// `unanalysable` lists the statements a table grant cannot describe at all —
/// they have to stay exact `queries`, and the reason is the same one a
/// `sql.denied` event carries.
///
/// # Errors
///
/// Only a JSON rendering failure, which cannot happen in practice: a statement
/// this build cannot analyse is data, not an error.
pub fn covering(statements: &[String], locator: &str) -> Result<String, String> {
    let mut analyses = Vec::with_capacity(statements.len());
    let mut unanalysable = Vec::new();
    for sql in statements {
        match blastit_sqlpolicy::analyze(sql) {
            Ok(analysis) => {
                // A statement that locks a table without reading a column of it
                // cannot be covered by a table grant, and no other statement of
                // the set changes that: it stays an exact `queries` entry (D3c).
                if TableGrants::covering(std::iter::once(&analysis))
                    .locks_without_a_column()
                    .next()
                    .is_some()
                {
                    unanalysable.push(json!({"sql": sql, "reason": LOCK_NEEDS_A_COLUMN}));
                    continue;
                }
                analyses.push(analysis);
            }
            Err(error) => unanalysable.push(json!({"sql": sql, "reason": error.to_string()})),
        }
    }
    let tables = TableGrants::covering(analyses.iter());
    let mut document = serde_json::to_value(&tables).map_err(|error| error.to_string())?;
    if let Some(map) = document.as_object_mut() {
        map.insert("unanalysable".to_string(), json!(unanalysable));
        map.insert("blastfile".to_string(), json!(tables.to_blastfile(locator)));
    }
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}

/// Classifies one statement and renders it as JSON (D3c).
///
/// This is `blastit._blastit.classify`: what PostgreSQL's own parser says a
/// statement is, for *every* statement, not only the four a table grant covers.
/// Nothing here classifies by text: the parser decides, and the caller gets its
/// verdict as data — which kind, how many statements the text holds, and whether
/// a grant may hold it at all.
///
/// # Errors
///
/// The reason a `sql.denied` event carries: the text is not valid SQL, or holds
/// no statement at all. A text that holds several is *not* an error: it is
/// `"statements": 3` in the verdict, and never plain.
pub fn classify(sql: &str) -> Result<String, String> {
    let classified = blastit_sqlpolicy::classify(sql).map_err(|error| error.to_string())?;
    let document = json!({
        "class": classified.class().as_str(),
        "label": classified.label(),
        "plain": classified.is_plain(),
        "statements": classified.statements(),
    });
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}
