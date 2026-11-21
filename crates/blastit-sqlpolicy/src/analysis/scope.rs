//! The relations a statement puts in scope, and what reading them needs.
//!
//! A `FROM` item is either a base table or something the statement derives from
//! one — a subquery, a CTE, a `VALUES` list, a set-returning function. Reading a
//! column of a derived relation has to be held to the base `(table, column)`
//! pairs its definition read, so each relation carries them; a nested scope (a
//! correlated subquery) sees the outer ones.

use std::collections::{BTreeMap, BTreeSet};

use super::types::{Access, Op};

/// What reading a column (or every column) of a relation needs.
///
/// The three parts are what makes `*` through a derived relation work: a named
/// column is an exact `(table, column)` pair, a table in `whole` is read
/// entirely whatever the statement asked for (`t.*`), and a table in `wildcard`
/// stands for whichever column the statement asked for — how a column read
/// through a derived relation whose output nobody can name is recorded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Reads {
    /// `(table, column)` pairs the statement reads by name.
    pub(super) columns: BTreeSet<(String, String)>,
    /// Tables read entirely: `*`, a whole-row reference to a relation, or a
    /// part of a derived relation whose columns come from a `*`.
    pub(super) whole: BTreeSet<String>,
    /// Tables where the column is not known: a part of a derived relation whose
    /// names come from a branch nobody can name.
    pub(super) wildcard: BTreeSet<String>,
}

impl Reads {
    /// Counts one `(table, column)` read.
    pub(super) fn column(&mut self, table: &str, column: &str) {
        self.columns.insert((table.to_string(), column.to_string()));
    }

    /// Counts one "read every column of `table`" (`t.*`).
    pub(super) fn whole_table(&mut self, table: &str) {
        self.whole.insert(table.to_string());
    }

    /// Merges another reading (the two branches of a `UNION`, say).
    pub(super) fn merge(&mut self, other: &Self) {
        self.columns.extend(other.columns.iter().cloned());
        self.whole.extend(other.whole.iter().cloned());
        self.wildcard.extend(other.wildcard.iter().cloned());
    }

    /// The same read, but where every wholly-read table becomes "the column the
    /// statement asked for".
    ///
    /// Reading a named column of a derived relation whose output is open (`s.x`
    /// where `s` is `SELECT * FROM t`) has to touch every table that output
    /// could come from; the column the statement named is the only one it can be
    /// held to, so the whole-table reads become wildcards — the `t.*` the
    /// definition itself needed is recorded there, not here.
    pub(super) fn as_unknown_column(&self) -> Self {
        let mut reads = self.clone();
        reads.wildcard.extend(reads.whole.iter().cloned());
        reads.whole.clear();
        reads
    }

    /// The accesses this needs, read either as named columns, as whole rows
    /// (`*`) or, for a wildcard, as the name the statement used.
    pub(super) fn accesses(&self, name: Option<&str>) -> Vec<Access> {
        let mut accesses =
            Vec::with_capacity(self.columns.len() + self.whole.len() + self.wildcard.len());
        for (table, column) in &self.columns {
            accesses.push(Access::new(Op::Select, table.clone(), Some(column)));
        }
        for table in &self.whole {
            accesses.push(Access::new(Op::Select, table.clone(), None));
        }
        for table in &self.wildcard {
            match name {
                Some(column) => accesses.push(Access::new(Op::Select, table.clone(), Some(column))),
                None => accesses.push(Access::new(Op::Select, table.clone(), None)),
            }
        }
        accesses
    }
}

/// The output columns of a derived relation: a subquery, a CTE, a `VALUES` list
/// or a set-returning function.
///
/// Reading one of its columns needs what the definition needed, which was
/// recorded when the definition was analysed; the only thing the output has to
/// carry is which base `(table, column)` pairs each output column stands for, so
/// that a rename (`SELECT password AS p`) cannot hide one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Derived {
    /// Output columns whose name is known.
    pub(super) named: BTreeMap<String, Reads>,
    /// The part of the output whose names are not known — a `*` expansion, a
    /// set-returning function's columns, a computed expression. `None` when
    /// every output column is named, so a name that is not there is an unknown
    /// column and the statement is refused.
    pub(super) open: Option<Reads>,
    /// The output columns in order, when they can be counted; `None` after a `*`
    /// expansion, whose column count is not known. Used to apply an
    /// `AS alias(a, b)` column list.
    pub(super) positions: Option<Vec<(Option<String>, Reads)>>,
    /// The base tables the relation's definition **names**, whether or not it
    /// reads a column of them (§6, D7). That is what a locking clause has to
    /// cover: `SELECT 1 FROM (SELECT 1 FROM auth_user) s FOR UPDATE` locks
    /// `auth_user` rows even though the outer statement reads no column at all.
    pub(super) names: BTreeSet<String>,
}

impl Derived {
    /// The builder every statement handler fills in: a known count of columns.
    pub(super) fn new() -> Self {
        Self {
            named: BTreeMap::new(),
            open: None,
            positions: Some(Vec::new()),
            names: BTreeSet::new(),
        }
    }

    /// A relation whose columns read nothing: a set-returning function's output,
    /// or the self-reference of a recursive CTE.
    pub(super) fn passthrough() -> Self {
        Self {
            named: BTreeMap::new(),
            open: Some(Reads::default()),
            positions: None,
            names: BTreeSet::new(),
        }
    }

    /// A derived relation whose columns all read `reads`, under any name.
    pub(super) fn passthrough_named(reads: Reads) -> Self {
        Self {
            named: BTreeMap::new(),
            open: Some(reads),
            positions: None,
            names: BTreeSet::new(),
        }
    }

    /// Adds one output column.
    pub(super) fn push(&mut self, name: Option<String>, reads: Reads) {
        match &name {
            Some(name) => {
                self.named.insert(name.clone(), reads.clone());
            }
            None => self.open.get_or_insert_with(Reads::default).merge(&reads),
        }
        if let Some(positions) = &mut self.positions {
            positions.push((name, reads));
        }
    }

    /// The base tables the relation's definition names.
    pub(super) fn named_tables(&self) -> &BTreeSet<String> {
        &self.names
    }

    /// Forgets that the output's columns can be counted: a `*` expansion stands
    /// for however many columns the table has.
    pub(super) fn unbounded(&mut self) {
        self.positions = None;
    }

    /// What reading every column of this relation needs.
    pub(super) fn whole_row(&self) -> Reads {
        let mut reads = Reads::default();
        for column in self.named.values() {
            reads.merge(column);
        }
        if let Some(open) = &self.open {
            reads.merge(open);
        }
        reads
    }

    /// Renames the output columns positionally, as `AS v(a, b)` does. `None`
    /// when the output has no countable column list or a different length.
    pub(super) fn rename(&self, names: &[String]) -> Option<Self> {
        let positions = self.positions.as_ref()?;
        if positions.len() != names.len() {
            return None;
        }
        let mut renamed = Self::new();
        renamed.names = self.names.clone();
        for (name, (_, reads)) in names.iter().zip(positions) {
            renamed.push(Some(name.clone()), reads.clone());
        }
        Some(renamed)
    }
}

/// A relation in scope: a base table, or something derived from one.
#[derive(Clone, Debug)]
pub(super) enum Rel {
    /// A base table, named by the key a grant uses.
    Base { table: String },
    /// A relation built by the statement itself.
    Derived(Box<Derived>),
}

/// A relation in scope, and the name the statement uses for it.
#[derive(Clone, Debug)]
pub(super) struct Named {
    /// The alias, or the table's own name. Empty for something unreferencable.
    pub(super) name: String,
    pub(super) rel: Rel,
}

impl Named {
    /// A base table under its own name.
    pub(super) fn base(name: String, table: String) -> Self {
        Self {
            name,
            rel: Rel::Base { table },
        }
    }

    /// A derived relation under an alias.
    pub(super) fn derived(name: String, derived: Derived) -> Self {
        Self {
            name,
            rel: Rel::Derived(Box::new(derived)),
        }
    }

    /// Whether an unqualified column name could be this relation's.
    ///
    /// A base table's columns are not known (there is no schema here), so it
    /// always could be; a derived relation only provides what its definition
    /// named, or anything at all when its output is open.
    pub(super) fn provides(&self, column: &str) -> bool {
        match &self.rel {
            Rel::Base { .. } => true,
            Rel::Derived(derived) => derived.named.contains_key(column) || derived.open.is_some(),
        }
    }

    /// What reading every column of this relation needs.
    pub(super) fn whole_row(&self) -> Reads {
        match &self.rel {
            Rel::Base { table } => {
                let mut reads = Reads::default();
                reads.whole_table(table);
                reads
            }
            Rel::Derived(derived) => derived.whole_row(),
        }
    }

    /// The base tables this relation's definition names, whether or not it
    /// reads a column of them.
    ///
    /// A base table is itself; a derived relation carries the set its definition
    /// collected. This is what a locking clause covers.
    pub(super) fn named_tables(&self) -> BTreeSet<String> {
        match &self.rel {
            Rel::Base { table } => BTreeSet::from([table.clone()]),
            Rel::Derived(derived) => derived.named_tables().clone(),
        }
    }
}

/// One scope level: the relations and CTEs a statement can refer to.
#[derive(Clone, Debug, Default)]
pub(super) struct Frame {
    pub(super) rels: Vec<Named>,
    /// The CTEs this level defines, by name (§6: a CTE name shadows a table).
    pub(super) ctes: BTreeMap<String, Derived>,
}

/// The nested scopes of one statement: innermost last, so a correlated subquery
/// can see the outer ones.
#[derive(Clone, Debug, Default)]
pub(super) struct Scope {
    frames: Vec<Frame>,
}

impl Scope {
    /// The same scope with one more level on top.
    pub(super) fn push(&self, frame: Frame) -> Self {
        let mut frames = self.frames.clone();
        frames.push(frame);
        Self { frames }
    }

    /// The innermost level, if there is one.
    pub(super) fn innermost(&self) -> Option<&Frame> {
        self.frames.last()
    }

    /// The relation a name refers to, searched from the innermost scope out.
    pub(super) fn relation(&self, name: &str) -> Option<&Named> {
        self.frames
            .iter()
            .rev()
            .flat_map(|frame| frame.rels.iter().rev())
            .find(|rel| rel.name == name)
    }

    /// The CTE a name refers to, searched from the innermost scope out.
    pub(super) fn cte(&self, name: &str) -> Option<&Derived> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.ctes.get(name))
    }

    /// The innermost level that could provide an unqualified column name, with
    /// the relations that provide it.
    pub(super) fn level_for(&self, column: &str) -> Option<&[Named]> {
        self.frames
            .iter()
            .rev()
            .map(|frame| frame.rels.as_slice())
            .find(|rels| rels.iter().any(|rel| rel.provides(column)))
    }
}
