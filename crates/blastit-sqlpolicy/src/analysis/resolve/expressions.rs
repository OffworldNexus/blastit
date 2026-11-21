//! Expression resolution: column references, `*`, function calls and casts.
//!
//! Every read a statement can make through an expression ends here, as a set of
//! base `(table, column)` pairs ([`Reads`](super::super::scope::Reads)) or, for a
//! function or a cast, as a name a grant has to list.

use serde_json::Value;

use super::super::is_safe_cast_type;
use super::super::last_name;
use super::super::scope::{Named, Reads, Rel, Scope};
use super::super::table_key;
use super::super::types::AnalysisError;
use super::super::walk::{Resolver, node_field, node_list, string_node, text, typed_body, unwrap};
use super::helpers::{is_star, names_of, output_name};

impl Resolver {
    /// A target-list or `RETURNING` entry: its output name, and what reading it
    /// needs.
    pub(super) fn target(
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
    pub(crate) fn column_ref(&self, body: &Value, scope: &Scope) -> Result<Reads, AnalysisError> {
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
    pub(super) fn columns_of(&self, rels: &[Named], column: &str) -> Result<Reads, AnalysisError> {
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
    pub(crate) fn func_call(&mut self, body: &Value, scope: &Scope) -> Result<(), AnalysisError> {
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
    pub(crate) fn type_cast(&mut self, body: &Value, scope: &Scope) -> Result<(), AnalysisError> {
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
