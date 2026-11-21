//! Resolving what a statement reads: `FROM`, joins, CTEs and locking.
//!
//! This is the read half of the walker: the statement entry point, the scope a
//! `WITH` clause or a `FROM` item builds, and the locking clauses that follow
//! it. The write half is [`write`], expression and column resolution is
//! [`expressions`], and the parse-tree accessors the three share are in
//! [`helpers`].

mod expressions;
mod helpers;
mod write;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::classify::statement_label;
use super::merge_outputs;
use super::scope::{Derived, Frame, Named, Reads, Rel, Scope};
use super::table_key;
use super::types::{Access, AnalysisError};
use super::walk::{Resolver, flag, node_field, node_list, one_of, text, typed_body, unwrap};

use helpers::{alias_name, names_of, set_operation, star_target};

impl Resolver {
    /// The statement a node holds: the entry point, and what a CTE body and a
    /// subquery go through. Returns the statement's output columns, which a
    /// derived relation is built from.
    pub(super) fn statement(
        &mut self,
        node: &Value,
        scope: &Scope,
    ) -> Result<Derived, AnalysisError> {
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

    /// `FOR UPDATE`/`FOR SHARE`: locking a row is reading it to change it, so the
    /// locked tables need `update` (§6) — on **any one column**, which is what
    /// PostgreSQL asks for: the UPDATE privilege on at least one column of every
    /// locked table (the columns read need `select` as usual).
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
                    self.accesses.insert(Access::lock(table));
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
}
