//! The write half of the walker: `INSERT`, `UPDATE`, `DELETE` and their clauses.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::scope::{Derived, Frame, Named, Scope};
use super::super::table_key;
use super::super::types::{AnalysisError, Op};
use super::super::walk::{Resolver, node_field, node_list, text, typed_body, unwrap};
use super::helpers::{insert_columns, res_target_name, star_target, target_table};

impl Resolver {
    /// An `INSERT`, including its `ON CONFLICT` clause.
    pub(super) fn insert(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
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
    pub(super) fn update(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
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
    pub(super) fn delete(&mut self, body: &Value, scope: &Scope) -> Result<Derived, AnalysisError> {
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
}
