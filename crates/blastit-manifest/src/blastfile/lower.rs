//! CST → `Manifest`: the v1 capability map of §7, and the `body`, trait,
//! entrypoint and grant rules of §10–§13.
//!
//! One pass, in §7.3's order: `app`/`entrypoint`, then stores, then traits
//! (resolved against the stores built a step earlier, so a trait may be written
//! before its store), then views, then mounts, then the routes' header lines.
//! `validate` runs afterwards, in the caller, so every whole-file rule keeps its
//! one home.
//!
//! The tree is read through `syntax`'s typed accessors — there is no second
//! AST (§4.2, §6.4) — and the values come from `tokens`' two decoders, so the
//! one escape table and the one pattern compiler are shared with the parser.
//!
//! Three things this module deliberately does **not** do:
//!
//! * it never reads the environment: `$DB_HOST` becomes the string `"DB_HOST"`
//!   and whether that variable exists is `blastit-env`'s answer at boot (§7.4,
//!   §17.2, R20);
//! * it never writes a per-view `static` or `php` block, because Blastfile has
//!   no spelling for either — a `view` lowers to a route with both absent
//!   (§7.1, §12.3);
//! * it never attaches a group globally: `global_groups` is always empty, and a
//!   view that wants a trait's grants writes `use <trait>` (§7.1, `ADR-0012`).

use std::collections::{BTreeMap, BTreeSet};

use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_sqlpolicy::{Columns, TableGrant, TableGrants, normalize};

use super::parser::line_starts;
use super::pattern::normalise_methods;
use super::syntax::{Cst, Element, NodeId, NodeKind};
use super::tokens::{TokenKind, raw_value, str_value};
use crate::error::{Location, ManifestError, Position};
use crate::model::{
    Grant, Group, Manifest, PgGrant, PhpPolicy, Protocol, Proxy, ProxyName, Route, StaticMount,
    StaticPolicy, UnresolvedGrant,
};

/// The name the unnamed `postgres $DATABASE_URL` takes (§7.3).
pub(crate) const DEFAULT_STORE: &str = "db";

/// The file a mount answers a directory with unless it says otherwise (§9.1).
const DEFAULT_INDEX: &str = "index.html";

// The rules a mount's `root` and `index` break (§9.1). They are `validate`'s
// phrases, word for word, so a developer reads one sentence whichever section
// the mistake is in: `check_static_root`/`check_static_index` check the same two
// values on a route's own block, and a `Blastfile` mount has no route. Moving
// these two checks into `validate::check_mounts` (which already takes a
// `Location`) is the follow-up that deletes this copy.
const RELATIVE_PATH: &str = "must be a relative path";
const NO_PARENT_SEGMENT: &str = "must not contain ..";
const NO_BACKSLASH: &str = "must not contain \\";
const NO_CONTROL_BYTE: &str = "must not contain a control byte";
const ONE_OR_MORE_SEGMENTS: &str = "must be one or more segments";
const NO_DOTFILE_SEGMENT: &str = "must not contain a dotfile segment";
const MUST_END_IN_PHP: &str = "must end in .php";

/// Lowers one parsed tree into the `Manifest` the rest of blastit reads (§7.3).
///
/// `source` is the text the tree was built from; it is used for the `line:col`
/// index only, because every value comes from the tree's own tokens.
pub(crate) fn lower(cst: &Cst, source: &str) -> Result<Manifest, crate::error::ManifestErrors> {
    Lowering::new(cst, source).run().map_err(Into::into)
}

/// One lowering: the tree, the position index, and the four lists being built.
struct Lowering<'a> {
    /// The tree being read.
    cst: &'a Cst,
    /// The source the tree spans, for `line:col` (§18).
    text: &'a str,
    /// The byte offset of the start of every line, so a position is a binary
    /// search rather than a scan (`LESSONS.md`, `MB.21.3.2`).
    line_starts: Vec<u32>,
    /// Step 2's store declarations, by name.
    proxies: BTreeMap<ProxyName, Proxy>,
    /// Step 3's traits, by name.
    groups: BTreeMap<String, Group>,
    /// Step 4's routes, in file order — which is matching order.
    routes: Vec<Route>,
    /// Step 6's `view` header line per route.
    route_lines: Vec<u32>,
    /// Step 5's mounts, in file order.
    mounts: Vec<StaticMount>,
    /// Grants that named a store nothing declares, for `validate` to report.
    unresolved: Vec<UnresolvedGrant>,
}

/// What one `view` contributes to every route it lowers to (§7.3, §10.3).
struct RouteParts {
    /// The view's name, shared by all of its routes.
    name: String,
    /// The view's index in file order, which is its sandbox.
    sandbox: u32,
    /// The line of the `view` header, which every route of the view carries.
    line: u32,
    /// The traits the view `use`s, in written order, deduplicated.
    groups: Vec<String>,
    /// Whether the view writes `egress any`.
    egress: bool,
    /// Whether the view writes `prefork`.
    prefork: bool,
    /// The view's inline grants, by store.
    grants: BTreeMap<ProxyName, Grant>,
}

/// Which grant clause of a `grant_block` the walk is inside (§13.1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clause {
    /// No clause yet: the block's first token, or just after a `transactions`.
    None,
    /// `functions …`
    Functions,
    /// `select …`
    Select,
    /// `insert …`
    Insert,
    /// `update …`
    Update,
    /// `delete …`
    Delete,
    /// `query …`
    Query,
}

impl<'a> Lowering<'a> {
    /// A lowering over one tree.
    fn new(cst: &'a Cst, source: &'a str) -> Self {
        Self {
            cst,
            text: source,
            line_starts: line_starts(source),
            proxies: BTreeMap::new(),
            groups: BTreeMap::new(),
            routes: Vec::new(),
            route_lines: Vec::new(),
            mounts: Vec::new(),
            unresolved: Vec::new(),
        }
    }

    /// §7.3's seven steps, in order.
    fn run(mut self) -> Result<Manifest, ManifestError> {
        // 1. `app`/`entrypoint` → `name`, `app`, `preload`, `php`.
        let name = self.app()?;
        let (app, preload, php) = self.entrypoint()?;
        // 2. stores → `proxies`.
        self.stores()?;
        // 3. traits → `groups`, resolved against step 2.
        self.traits()?;
        // 4. views → `routes`.
        self.views()?;
        // 5. statics → `mounts`.
        self.statics()?;
        // 6. `route_lines` was filled beside step 4.
        Ok(Manifest::assemble(
            1,
            // 7.1's dropped row: a global attach has no keyword, so a lowered
            // manifest never carries one.
            Vec::new(),
            self.proxies,
            self.groups,
            self.routes,
            self.unresolved,
        )
        .with_app(app)
        .with_name(name)
        .with_preload(preload)
        .with_php(php)
        .with_mounts(self.mounts)
        .with_route_lines(self.route_lines))
    }

    // ---- positions ------------------------------------------------------

    /// The 1-based position of a byte offset, `col` counted in `char`s (§18).
    fn position_of(&self, offset: u32) -> Position {
        let end = (offset as usize).min(self.text.len());
        let line = self
            .line_starts
            .partition_point(|start| *start as usize <= end)
            .max(1);
        let start = self
            .line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let col = self
            .text
            .get(start as usize..end)
            .map_or(1, |tail| tail.chars().count() as u32 + 1);
        Position::at(line as u32, col)
    }

    /// The position of a node's first token.
    fn position(&self, node: NodeId) -> Position {
        self.cst
            .first_token(node)
            .and_then(|token| self.cst.token_span(token))
            .map_or(Position::NONE, |span| self.position_of(span.start))
    }

    /// The line a node starts on.
    fn line(&self, node: NodeId) -> u32 {
        self.position(node).line()
    }

    /// The position of one token.
    fn token_position(&self, token: super::syntax::TokenId) -> Position {
        self.cst
            .token_span(token)
            .map_or(Position::NONE, |span| self.position_of(span.start))
    }

    /// A refusal for a tree shape the parser cannot produce.
    ///
    /// The grammar guarantees every slot this module reads, so this is a
    /// broken invariant rather than a document mistake — but it is still a
    /// refusal, never a panic (§18).
    fn broken(&self, node: NodeId, expected: &'static str) -> ManifestError {
        ManifestError::Unexpected {
            at: self.position(node),
            expected,
            found: "the end of the section".to_owned(),
        }
    }

    // ---- values ---------------------------------------------------------

    /// The first `"…"` string directly under `node`, decoded (§5.1).
    fn string(&self, node: NodeId) -> Result<String, ManifestError> {
        self.cst
            .strings(node)
            .next()
            .map(str_value)
            .ok_or_else(|| self.broken(node, "a `\"…\"` string"))
    }

    /// The first `$IDENT` directly under `node`, without its `$` (§7.4).
    ///
    /// The name is the value: this never reads the environment (§17.2).
    fn variable(&self, node: NodeId) -> Result<String, ManifestError> {
        self.cst
            .envs(node)
            .next()
            .map(str::to_owned)
            .ok_or_else(|| self.broken(node, "a `$IDENT`"))
    }

    /// The `Method` words directly under `node`, with their positions (§8.4).
    fn methods(&self, node: NodeId) -> Result<Option<Vec<String>>, ManifestError> {
        let mut written: Vec<(&str, Position)> = Vec::new();
        for token in self.cst.child_tokens(node) {
            if !matches!(self.cst.token_kind(token), Some(TokenKind::Word)) {
                continue;
            }
            written.push((self.cst.token_text(token), self.token_position(token)));
        }
        normalise_methods(&written)
    }

    /// The compiled fragment of every `pattern` directly under `node` (§8.1).
    fn patterns(&self, node: NodeId) -> Result<Vec<String>, ManifestError> {
        let mut fragments = Vec::new();
        for pattern in self.cst.patterns(node) {
            let compiled = self
                .cst
                .pattern(pattern)
                .ok_or_else(|| self.broken(pattern, "a compiled pattern"))?;
            fragments.push(compiled.fragment().to_owned());
        }
        Ok(fragments)
    }

    // ---- 1. app and entrypoint (§12) ------------------------------------

    /// `app "shop"` → `Manifest.name` (§4.3, §7.1).
    fn app(&self) -> Result<Option<String>, ManifestError> {
        match self.cst.app() {
            Some(node) => Ok(Some(self.string(node)?)),
            None => Ok(None),
        }
    }

    /// `entrypoint` → the callable, the preload list and the front controller.
    ///
    /// `entrypoint python "…"` fills `app` and `preload` in written order with
    /// duplicates dropped (§12.1); `entrypoint php "…"` fills the one
    /// `PhpPolicy`, with `workers` always `None` (§12.2, §12.3).
    #[allow(clippy::type_complexity)]
    fn entrypoint(
        &self,
    ) -> Result<(Option<String>, Vec<String>, Option<PhpPolicy>), ManifestError> {
        let Some(node) = self.cst.entrypoint() else {
            return Ok((None, Vec::new(), None));
        };
        match self.cst.words(node).nth(1) {
            Some("php") => {
                let entry = self.string(node)?;
                self.check_php_entry(&entry, node)?;
                Ok((
                    None,
                    Vec::new(),
                    Some(PhpPolicy {
                        entry,
                        // §12.3: the per-view cap is gone, so the field is
                        // always `None` and the scheduler owns parallelism.
                        workers: None,
                    }),
                ))
            }
            Some("python") => {
                let app = self.string(node)?;
                let mut preload: Vec<String> = Vec::new();
                for block in self.cst.of_kind(node, NodeKind::Preload) {
                    for text in self.cst.strings(block) {
                        let module = str_value(text);
                        if !preload.contains(&module) {
                            preload.push(module);
                        }
                    }
                }
                Ok((Some(app), preload, None))
            }
            _ => Err(self.broken(node, "`python` or `php`")),
        }
    }

    /// `check_php_entry`'s rules, applied to the one front controller (§12.2).
    ///
    /// The four rules the design keeps are checked here because the entry is no
    /// longer declared per view: `validate`'s `check_php` walks routes, and a
    /// `Blastfile` `entrypoint` is not a route. The path is never echoed — a
    /// pasted token can end up in that slot (§18).
    fn check_php_entry(&self, entry: &str, node: NodeId) -> Result<(), ManifestError> {
        let rule = if entry.is_empty() || entry.starts_with('/') {
            Some(RELATIVE_PATH)
        } else if entry.contains('\\') {
            Some(NO_BACKSLASH)
        } else if entry.split('/').any(|segment| segment == "..") {
            Some(NO_PARENT_SEGMENT)
        } else if entry.bytes().any(|byte| byte < 0x20) {
            Some(NO_CONTROL_BYTE)
        } else if !entry.to_ascii_lowercase().ends_with(".php") {
            Some(MUST_END_IN_PHP)
        } else {
            None
        };
        match rule {
            Some(rule) => Err(ManifestError::PhpEntrypointInvalid {
                at: self.position(node),
                rule,
            }),
            None => Ok(()),
        }
    }

    // ---- 2. stores (§7.4) -----------------------------------------------

    /// Every `postgres` section → one `Proxy` with `output` empty (§7.5).
    fn stores(&mut self) -> Result<(), ManifestError> {
        let stores: Vec<NodeId> = self.cst.stores().collect();
        for store in stores {
            let name = match self.cst.words(store).nth(1) {
                Some(name) => ProxyName::from(name),
                None => ProxyName::from(DEFAULT_STORE),
            };
            let mut input: BTreeMap<String, String> = BTreeMap::new();
            let slots: Vec<NodeId> = self.cst.store_slots(store).collect();
            if slots.is_empty() {
                // `postgres $DATABASE_URL`: the one-line form is the `url` slot.
                input.insert("url".to_owned(), self.variable(store)?);
            } else {
                for slot in slots {
                    let keyword = self
                        .cst
                        .words(slot)
                        .next()
                        .ok_or_else(|| self.broken(slot, "a store slot"))?
                        .to_owned();
                    input.insert(keyword, self.variable(slot)?);
                }
            }
            if self.proxies.contains_key(&name) {
                return Err(ManifestError::DuplicateProxyName {
                    at: self.position(store),
                    name: name.to_string(),
                });
            }
            self.proxies.insert(
                name.clone(),
                Proxy::new(
                    name,
                    Protocol::Postgresql,
                    input,
                    // §7.5: the retired `output` rename map is dropped, so the
                    // protocol's default templates apply.
                    BTreeMap::new(),
                ),
            );
        }
        Ok(())
    }

    // ---- 3. traits (§11) ------------------------------------------------

    /// Every `trait` → one `Group` (§11.1).
    ///
    /// A trait's grants are resolved against the stores step 2 declared, so a
    /// trait may be written before its store; a store nothing declares is
    /// recorded for `validate` to report as `GrantOnUnknownProxy` (§7.3,
    /// §11.3).
    fn traits(&mut self) -> Result<(), ManifestError> {
        let traits: Vec<NodeId> = self.cst.traits().collect();
        for node in traits {
            let name = self
                .cst
                .words(node)
                .nth(1)
                .ok_or_else(|| self.broken(node, "a trait name"))?
                .to_owned();
            let location = Location::Trait(name.clone());
            let grants = self.grants_of(node, &location)?;
            self.groups.insert(name.clone(), Group::new(name, grants));
        }
        Ok(())
    }

    /// The `store_grant`s directly under `node`, merged per store.
    fn grants_of(
        &mut self,
        node: NodeId,
        location: &Location,
    ) -> Result<BTreeMap<ProxyName, Grant>, ManifestError> {
        let mut grants: BTreeMap<ProxyName, Grant> = BTreeMap::new();
        let nodes: Vec<NodeId> = self.cst.grants(node).collect();
        for grant in nodes {
            if let Some((proxy, grant)) = self.grant(grant, location)? {
                match grants.get_mut(&proxy) {
                    Some(existing) => existing.merge_from(&grant),
                    None => {
                        grants.insert(proxy, grant);
                    }
                }
            }
        }
        Ok(grants)
    }

    /// One `store_grant` → one `(store, Grant)` (§13.1).
    ///
    /// `None` when the grant names a store nothing declares: the grant is
    /// recorded in `unresolved` and `validate` reports it, exactly as the
    /// retired reader's group message does (§7.3, §11.3, §13.1).
    fn grant(
        &mut self,
        node: NodeId,
        location: &Location,
    ) -> Result<Option<(ProxyName, Grant)>, ManifestError> {
        let mut store: Option<&str> = None;
        let mut unrestricted = false;
        for word in self.cst.words(node).skip(1) {
            match word {
                "unrestricted" => unrestricted = true,
                name if store.is_none() => store = Some(name),
                _ => {}
            }
        }
        let proxy = match store {
            Some(name) => ProxyName::from(name),
            None => ProxyName::from(DEFAULT_STORE),
        };
        let Some(protocol) = self.proxies.get(&proxy).map(Proxy::protocol) else {
            self.unresolved
                .push(UnresolvedGrant::new(proxy, location.clone()));
            return Ok(None);
        };
        let grant = match protocol {
            Protocol::Postgresql => {
                if unrestricted {
                    PgGrant::unrestricted()
                } else {
                    self.pg_grant(node)?
                }
            }
        };
        Ok(Some((proxy, Grant::Postgresql(grant))))
    }

    /// A `grant_block` → `PgGrant` (§13.1).
    ///
    /// The block's children are walked in source order, one clause at a time:
    /// a clause keyword starts a clause at the start of a statement, and its
    /// items follow until the next statement. A `Newline` right after a `,`
    /// continues the list (§6.6) and stays in the tree, so it is skipped for
    /// structure and never ends the clause.
    fn pg_grant(&self, node: NodeId) -> Result<PgGrant, ManifestError> {
        let mut queries: Vec<String> = Vec::new();
        let mut transaction_control = false;
        let mut tables: BTreeMap<String, TableGrant> = BTreeMap::new();
        let mut functions: Vec<String> = Vec::new();

        let Some(block) = self.cst.first_of_kind(node, NodeKind::GrantBlock) else {
            return Ok(PgGrant::new(Vec::new(), false));
        };

        let mut clause = Clause::None;
        let mut at_start = true;
        let mut after_comma = false;
        // `functions a.b` is one name: a `.` joins the word before it to the one
        // after it (§6.1's `function → Word ('.' Word)*`).
        let mut joined = false;
        for (child, element) in self.cst.child_elements(block) {
            match element {
                Element::Trivia(_) => {}
                Element::Token(token) => match self.cst.token_kind(token) {
                    Some(TokenKind::Newline | TokenKind::Semi) => {
                        joined = false;
                        if after_comma {
                            after_comma = false;
                        } else {
                            at_start = true;
                            clause = Clause::None;
                        }
                    }
                    Some(TokenKind::Comma) => {
                        joined = false;
                        after_comma = true;
                    }
                    Some(TokenKind::Dot) => joined = true,
                    Some(TokenKind::Word) => {
                        let text = self.cst.token_text(token);
                        if at_start {
                            at_start = false;
                            after_comma = false;
                            joined = false;
                            clause = match text {
                                "transactions" => {
                                    transaction_control = true;
                                    Clause::None
                                }
                                "functions" => Clause::Functions,
                                "select" => Clause::Select,
                                "insert" => Clause::Insert,
                                "update" => Clause::Update,
                                "delete" => Clause::Delete,
                                "query" => Clause::Query,
                                _ => return Err(self.broken(block, "a grant")),
                            };
                        } else {
                            after_comma = false;
                            match clause {
                                Clause::Functions if joined => match functions.last_mut() {
                                    Some(last) => {
                                        last.push('.');
                                        last.push_str(text);
                                    }
                                    None => functions.push(text.to_owned()),
                                },
                                Clause::Functions => functions.push(text.to_owned()),
                                Clause::Delete => {
                                    tables.entry(text.to_owned()).or_default().delete = true;
                                }
                                _ => return Err(self.broken(block, "a grant item")),
                            }
                            joined = false;
                        }
                    }
                    _ => {}
                },
                Element::Node { .. } => {
                    after_comma = false;
                    match self.cst.kind(child) {
                        Some(NodeKind::TableGrant) => {
                            let (table, columns) = self.table_grant(child)?;
                            let entry = tables.entry(table).or_default();
                            let slot = match clause {
                                Clause::Select => &mut entry.select,
                                Clause::Insert => &mut entry.insert,
                                Clause::Update => &mut entry.update,
                                _ => {
                                    return Err(
                                        self.broken(child, "a `select`, `insert` or `update`")
                                    );
                                }
                            };
                            *slot = Some(merge_columns(slot.take(), columns));
                        }
                        Some(NodeKind::Query) => {
                            let text =
                                self.cst.strings(child).next().ok_or_else(|| {
                                    self.broken(child, "a `'…'` or `\"…\"` string")
                                })?;
                            // §13.1: either spelling is accepted and stored
                            // without its quotes, normalised by the one
                            // `blastit_sqlpolicy::normalize`.
                            let sql = if text.starts_with('\'') {
                                raw_value(text).to_owned()
                            } else {
                                str_value(text)
                            };
                            let sql = normalize(&sql);
                            if !queries.contains(&sql) {
                                queries.push(sql);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(PgGrant::new(queries, transaction_control)
            .with_tables(TableGrants::new(tables, functions)))
    }

    /// One `table_grant` → its table and its columns (§13.1).
    ///
    /// A table with no `(…)` list is `Columns::All`; one with a list is exactly
    /// that list, in a set so a repeated column is one entry.
    fn table_grant(&self, node: NodeId) -> Result<(String, Columns), ManifestError> {
        let mut words = self.cst.words(node);
        let table = words
            .next()
            .ok_or_else(|| self.broken(node, "a table name"))?
            .to_owned();
        let columns: BTreeSet<String> = words.map(str::to_owned).collect();
        let columns = if columns.is_empty() {
            Columns::All
        } else {
            Columns::List(columns)
        };
        Ok((table, columns))
    }

    // ---- 4. views (§7.3, §8.4, §10.3) -----------------------------------

    /// Every `view` → one route per endpoint, all sharing the view's sandbox.
    fn views(&mut self) -> Result<(), ManifestError> {
        let views: Vec<NodeId> = self.cst.views().collect();
        for (index, view) in views.iter().copied().enumerate() {
            let name = self
                .cst
                .words(view)
                .nth(1)
                .ok_or_else(|| self.broken(view, "a view name"))?
                .to_owned();
            let location = Location::View {
                index,
                name: name.clone(),
            };
            let parts = RouteParts {
                sandbox: u32::try_from(index).unwrap_or(u32::MAX),
                line: self.line(view),
                groups: self.used_traits(view, &name)?,
                egress: self.cst.first_of_kind(view, NodeKind::Egress).is_some(),
                prefork: self.cst.first_of_kind(view, NodeKind::Prefork).is_some(),
                grants: self.grants_of(view, &location)?,
                name,
            };
            // §10.3: `body` outside an endpoint applies to all of them.
            let view_body = match self.cst.first_of_kind(view, NodeKind::Body) {
                Some(node) => Some(self.ingress(node)?),
                None => None,
            };
            match self.cst.first_of_kind(view, NodeKind::ViewHeader) {
                Some(header) => {
                    let methods = self.methods(header)?;
                    let path = self.patterns(header)?;
                    self.push_route(&parts, path, methods, view_body);
                }
                None => {
                    let endpoints: Vec<NodeId> = self.cst.endpoints(view).collect();
                    if endpoints.is_empty() {
                        return Err(self.broken(view, "an endpoint"));
                    }
                    for endpoint in endpoints {
                        let methods = self.methods(endpoint)?;
                        let path = self.patterns(endpoint)?;
                        let ingress = match self.endpoint_body(endpoint) {
                            Some(node) => Some(self.ingress(node)?),
                            None => view_body,
                        };
                        self.push_route(&parts, path, methods, ingress);
                    }
                }
            }
        }
        Ok(())
    }

    /// The `body` of one endpoint: inside its `endpoint_block` (§6.1).
    fn endpoint_body(&self, endpoint: NodeId) -> Option<NodeId> {
        self.cst
            .first_of_kind(endpoint, NodeKind::EndpointBlock)
            .and_then(|block| self.cst.first_of_kind(block, NodeKind::Body))
    }

    /// The traits a view `use`s, in written order, deduplicated (§11.1).
    ///
    /// §11.2: a trait may be written after the view that uses it, so this is
    /// resolved once step 3 has built the trait map — an unknown name is
    /// [`ManifestError::UnknownTrait`] at the name's own position.
    fn used_traits(&self, view: NodeId, view_name: &str) -> Result<Vec<String>, ManifestError> {
        let mut used: Vec<String> = Vec::new();
        let uses: Vec<NodeId> = self.cst.of_kind(view, NodeKind::Use).collect();
        for node in uses {
            for token in self.cst.child_tokens(node) {
                if !matches!(self.cst.token_kind(token), Some(TokenKind::Word)) {
                    continue;
                }
                let name = self.cst.token_text(token);
                if name == "use" {
                    continue;
                }
                if !self.groups.contains_key(name) {
                    return Err(ManifestError::UnknownTrait {
                        at: self.token_position(token),
                        view: view_name.to_owned(),
                        name: name.to_owned(),
                    });
                }
                if !used.iter().any(|listed| listed == name) {
                    used.push(name.to_owned());
                }
            }
        }
        Ok(used)
    }

    /// Appends one route and its header line (§7.3 step 6).
    fn push_route(
        &mut self,
        parts: &RouteParts,
        path: Vec<String>,
        methods: Option<Vec<String>>,
        ingress: Option<IngressPolicy>,
    ) {
        self.routes.push(
            Route::new(
                parts.name.clone(),
                path,
                methods,
                parts.groups.clone(),
                parts.egress,
                parts.prefork,
                parts.grants.clone(),
            )
            .with_ingress(ingress)
            .with_sandbox(parts.sandbox),
        );
        // Two routes of one view carry the same line: the `view` header's.
        self.route_lines.push(parts.line);
    }

    /// `body <codec> [unvalidated]` → `IngressPolicy` (§10.1).
    fn ingress(&self, node: NodeId) -> Result<IngressPolicy, ManifestError> {
        let mut words = self.cst.words(node);
        let _keyword = words.next();
        let codec = words
            .next()
            .ok_or_else(|| self.broken(node, "a codec"))?
            .to_owned();
        let body = match codec.as_str() {
            "json" => BodyCodec::Json,
            "form" => BodyCodec::Form,
            "none" => BodyCodec::None,
            "raw" => BodyCodec::Raw,
            other => {
                return Err(ManifestError::UnknownCodec {
                    at: self.position(node),
                    codec: other.to_owned(),
                });
            }
        };
        // §10.1: `unvalidated` is a sub-token of `body`, and the parser admits
        // it after `raw` only.
        let allow_raw = words.any(|word| word == "unvalidated");
        Ok(IngressPolicy { body, allow_raw })
    }

    // ---- 5. mounts (§9.1) -----------------------------------------------

    /// Every `static` block → one `StaticMount` (§9.1, §9.3).
    fn statics(&mut self) -> Result<(), ManifestError> {
        let statics: Vec<NodeId> = self.cst.statics().collect();
        for node in statics {
            let prefix = self.string(node)?;
            let at = self.position(node);
            let mut root: Option<String> = None;
            let mut index: Option<String> = None;
            let mut fallback: Option<String> = None;
            let mut immutable: Vec<String> = Vec::new();
            let mut seen: BTreeMap<String, u32> = BTreeMap::new();

            let directives: Vec<NodeId> =
                self.cst.of_kind(node, NodeKind::StaticDirective).collect();
            for directive in directives {
                let keyword = self
                    .cst
                    .words(directive)
                    .next()
                    .ok_or_else(|| self.broken(directive, "a `static` directive"))?
                    .to_owned();
                let position = self.position(directive);
                if let Some(first) = seen.get(&keyword).copied() {
                    return Err(ManifestError::DuplicateStaticDirective {
                        at: position,
                        directive: keyword,
                        first,
                    });
                }
                seen.insert(keyword.clone(), position.line());
                match keyword.as_str() {
                    "root" => root = Some(self.string(directive)?),
                    "index" => index = Some(self.string(directive)?),
                    "fallback" => fallback = Some(self.string(directive)?),
                    "immutable" => {
                        for text in self.cst.strings(directive) {
                            immutable.push(str_value(text));
                        }
                    }
                    _ => return Err(self.broken(directive, "a `static` directive")),
                }
            }

            // §9.1: `root` is required, and a mount with no directory answers
            // nothing — defaulting it would hide the omission.
            let root = root.ok_or(ManifestError::StaticRootMissing { at })?;
            let location = Location::Mount {
                index: self.mounts.len(),
                prefix: prefix.clone(),
            };
            if let Some(rule) = root_rule(&root) {
                return Err(ManifestError::StaticRootInvalid {
                    location,
                    root,
                    rule,
                });
            }
            let index = index.unwrap_or_else(|| DEFAULT_INDEX.to_owned());
            if let Some(rule) = index_rule(&index) {
                return Err(ManifestError::StaticIndexInvalid {
                    location,
                    index,
                    rule,
                });
            }
            self.mounts.push(StaticMount {
                prefix,
                policy: StaticPolicy {
                    root,
                    index,
                    fallback,
                    immutable,
                },
            });
        }
        Ok(())
    }
}

/// The union of two column grants: `"*"` absorbs a list (§13.1).
///
/// `blastit-sqlpolicy`'s own union is private to that crate, and a `select`
/// written twice for one table in one block must not drop the first list.
fn merge_columns(existing: Option<Columns>, other: Columns) -> Columns {
    match (existing, other) {
        (Some(Columns::All), _) | (_, Columns::All) => Columns::All,
        (Some(Columns::List(mut left)), Columns::List(right)) => {
            left.extend(right);
            Columns::List(left)
        }
        (None, other) => other,
    }
}

/// The first rule a mount's `root` breaks, or `None` (§9.1).
fn root_rule(root: &str) -> Option<&'static str> {
    if root.is_empty() || root.starts_with('/') {
        Some(RELATIVE_PATH)
    } else if root.contains('\\') {
        Some(NO_BACKSLASH)
    } else if root.split('/').any(|segment| segment == "..") {
        Some(NO_PARENT_SEGMENT)
    } else if root.bytes().any(|byte| byte < 0x20) {
        Some(NO_CONTROL_BYTE)
    } else {
        None
    }
}

/// A `/`-separated segment that starts with `.` and is not exactly `.` (§9.1).
///
/// The definition is the design's: `.` is allowed (it names the root), `..` is
/// refused above, and everything else that starts with `.` is a dotfile.
fn is_dotfile_segment(segment: &str) -> bool {
    segment.starts_with('.') && segment != "."
}

/// The first rule a mount's `index` breaks, or `None` (§9.1).
fn index_rule(index: &str) -> Option<&'static str> {
    if index.starts_with('/') {
        Some(RELATIVE_PATH)
    } else if index.is_empty() || index.split('/').any(|segment| segment.is_empty()) {
        Some(ONE_OR_MORE_SEGMENTS)
    } else if index.split('/').any(|segment| segment == "..") {
        Some(NO_PARENT_SEGMENT)
    } else if index.split('/').any(is_dotfile_segment) {
        Some(NO_DOTFILE_SEGMENT)
    } else if index.contains('\\') {
        Some(NO_BACKSLASH)
    } else if index.bytes().any(|byte| byte < 0x20) {
        Some(NO_CONTROL_BYTE)
    } else {
        None
    }
}
