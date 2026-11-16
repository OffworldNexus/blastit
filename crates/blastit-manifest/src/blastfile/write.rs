//! The surgical editor (§14.5): splice one statement into a parsed tree, then
//! prove the result.
//!
//! `--learn` writes into a file a person wrote by hand, so the editor never
//! re-serialises the document. It parses the `Blastfile` once into the one tree
//! of §14.1, resolves the edit to a byte offset that the tree names (the line a
//! `view` starts on, the `}` that closes a `postgres` block), splices whole
//! lines there, and re-parses the result. Every other byte — comments,
//! ordering, quoting, blank lines — is the author's, because it is never
//! touched: the spliced text is the only new text, and the tree's spans are the
//! only offsets the editor uses.
//!
//! The seven rules of §14.5 are the whole editor:
//!
//! * a new view goes immediately above the first method-unrestricted catch-all
//!   view (a comment run directly above it stays above it), else at the end;
//! * a new view's shape is `view <name> <METHOD[, …]> "<pattern>"[, …] { … }`,
//!   methods in the order `--learn` saw them;
//! * a grant lands in the first `postgres [store] { … }` block of that view,
//!   else in a new block at the end of the view, one block per store;
//! * `query '…'` when the statement holds no `'`, `query "…"` (escaped) when it
//!   does, so the writer never emits a file its own reader refuses (R25);
//! * a comment above the insertion point stays above the inserted line,
//!   because comments are elements of the tree, not text the editor reflows.
//!
//! P4 is the postcondition, and it is not optional: [`apply`] re-parses and
//! re-lowers the edited text and compares it with the model the edit intended
//! ([`expected`], which is also what [`super::BlastfileEdits::apply_to`]
//! returns) using [`Manifest::equivalent`]. A mismatch is
//! [`EditError::Verification`] and `apply` returns `Err`, so the caller writes
//! nothing. The comparison covers every field — `static` mounts and the
//! `entrypoint` included — so an edit that cannot reproduce a block refuses
//! instead of dropping it (`MB.3.21`).
//!
//! Nothing here writes `body`: [`super::ProposedView`] and
//! [`super::ProposedGrant`] have no field for it, so "`--learn` never
//! writes `body`" is a property of the types rather than a rule the writer has
//! to remember (§10.1, R11).

use std::collections::{BTreeMap, BTreeSet};

use blastit_sqlpolicy::{KNOWN_STATIC_SETUP_SQL, has_quoted_literal, normalize};

use super::BlastfileEdits;
use super::lower::{self, DEFAULT_STORE};
use super::parser;
use super::pattern::{self, normalise_methods};
use super::syntax::{Cst, Element, NodeId, NodeKind};
use super::tokens::TokenKind;
use crate::edit::EditError;
use crate::error::Position;
use crate::model::{Grant, Manifest, PgGrant, ProxyName, Route};

/// Applies `edits` to `source` and proves the result (§14.5, P4).
///
/// The source is parsed and lowered once, the edit is resolved against that
/// tree, the spliced text is parsed and lowered again, and the two models are
/// compared with [`Manifest::equivalent`]. An edits set with nothing to add
/// returns the source unchanged, which is what makes repeated learning runs
/// byte-identical.
///
/// # Errors
///
/// [`EditError::Source`] when the document does not parse or lower,
/// [`EditError::Verification`] when the splice cannot reproduce the model the
/// edit intended, and the refusal of the rule the edit broke (§14.5's table).
pub(crate) fn apply(source: &str, edits: &BlastfileEdits) -> Result<String, EditError> {
    let cst = parser::parse(source)?;
    let before = lower::lower(&cst, source)?;
    let plan = Plan::build(&before, edits)?;
    if plan.is_noop() {
        return Ok(source.to_string());
    }

    let edited = plan.render(&cst, source)?;
    let after_cst = parser::parse(&edited).map_err(|_| EditError::Verification {
        section: "the document syntax",
    })?;
    let after = lower::lower(&after_cst, &edited).map_err(|_| EditError::Verification {
        section: "the document syntax",
    })?;
    let expected = expected(edits, &before);
    if !after.equivalent(&expected) {
        return Err(EditError::Verification {
            section: first_difference(&after, &expected),
        });
    }
    Ok(edited)
}

/// The manifest `edits` intends, built from `manifest` (§14.5, P4).
///
/// This is the library's own model edit, so the writer and its tests share one
/// oracle instead of re-implementing lowering: the routes are grouped by the
/// view they came from (one `view` with several endpoints is several routes in
/// one sandbox), the grants are merged per store, the new views are inserted at
/// the catch-all's index, and the sandboxes are renumbered by view index, which
/// is what `lower` will do to the edited text.
///
/// A statement `apply` refuses contributes nothing: `apply_edits` validates
/// before it calls this, so an unreachable statement is the only one that can
/// disappear here.
pub(crate) fn expected(edits: &BlastfileEdits, manifest: &Manifest) -> Manifest {
    let mut groups = view_groups(manifest);

    for grant in &edits.grants {
        let store_name = grant.store.as_deref().unwrap_or(DEFAULT_STORE);
        let Ok(proposed) = normalise_queries(&grant.view, store_name, &grant.queries) else {
            continue;
        };
        if proposed.is_empty() {
            continue;
        }
        let store = ProxyName::from(store_name);
        for group in &mut groups {
            if !group
                .first()
                .is_some_and(|route| route.name() == grant.view)
            {
                continue;
            }
            for route in group.iter_mut() {
                let written: Vec<String> = route
                    .grant(&store)
                    .and_then(Grant::as_postgresql)
                    .map(|grant| grant.queries().to_vec())
                    .unwrap_or_default();
                let fresh = fresh_queries(&written, &proposed);
                if fresh.is_empty() {
                    continue;
                }
                let mut merged = route
                    .grant(&store)
                    .and_then(Grant::as_postgresql)
                    .cloned()
                    .unwrap_or_else(|| PgGrant::new(Vec::new(), false));
                merged.merge_from(&PgGrant::new(fresh, false));
                *route = with_grant(route, store.clone(), Grant::Postgresql(merged));
            }
        }
    }

    let mut inserted: Vec<Vec<Route>> = Vec::new();
    for view in &edits.views {
        if view_routes(manifest, &view.name).is_some() {
            continue;
        }
        let mut grants: BTreeMap<ProxyName, Grant> = BTreeMap::new();
        for grant in &edits.grants {
            if grant.view != view.name {
                continue;
            }
            let store_name = grant.store.as_deref().unwrap_or(DEFAULT_STORE);
            let Ok(proposed) = normalise_queries(&grant.view, store_name, &grant.queries) else {
                continue;
            };
            if proposed.is_empty() {
                continue;
            }
            let store = ProxyName::from(store_name);
            let mut merged = grants
                .get(&store)
                .and_then(Grant::as_postgresql)
                .cloned()
                .unwrap_or_else(|| PgGrant::new(Vec::new(), false));
            merged.merge_from(&PgGrant::new(proposed, false));
            grants.insert(store, Grant::Postgresql(merged));
        }
        let path: Vec<String> = view
            .patterns
            .iter()
            .filter_map(|pattern| {
                pattern::compile(pattern, Position::NONE)
                    .ok()
                    .map(|compiled| compiled.fragment().to_owned())
            })
            .collect();
        inserted.push(vec![Route::new(
            view.name.clone(),
            path,
            normalised_methods(&view.methods),
            Vec::new(),
            false,
            false,
            grants,
        )]);
    }

    let at = catch_all_view(manifest)
        .unwrap_or(groups.len())
        .min(groups.len());
    let tail = groups.split_off(at);
    groups.extend(inserted);
    groups.extend(tail);

    let routes: Vec<Route> = groups
        .into_iter()
        .enumerate()
        .flat_map(|(index, group)| {
            let sandbox = u32::try_from(index).unwrap_or(u32::MAX);
            group
                .into_iter()
                .map(move |route| route.with_sandbox(sandbox))
        })
        .collect();

    // `from_parts` is the crate's one model builder; every top-level field the
    // edit does not touch is carried across explicitly, so a manifest with an
    // `app`, a `preload`, mounts or an `entrypoint` compares equal after the
    // edit instead of losing them to a default.
    Manifest::from_parts(
        manifest.version(),
        manifest.global_groups().to_vec(),
        manifest.proxies().clone(),
        manifest.groups().clone(),
        routes,
    )
    .with_app(manifest.app().map(str::to_owned))
    .with_name(manifest.name().map(str::to_owned))
    .with_preload(manifest.preload().to_vec())
    .with_mounts(manifest.mounts().to_vec())
    .with_php(manifest.php().cloned())
}

/// The routes of `manifest`, grouped by the view that produced them.
///
/// `lower` numbers one sandbox per view and pushes that view's routes
/// consecutively, so a run of routes with one sandbox is one view.
fn view_groups(manifest: &Manifest) -> Vec<Vec<Route>> {
    let mut groups: Vec<Vec<Route>> = Vec::new();
    for route in manifest.routes() {
        let same = groups
            .last()
            .and_then(|group| group.first())
            .is_some_and(|first| first.sandbox() == route.sandbox());
        if same {
            if let Some(group) = groups.last_mut() {
                group.push(route.clone());
            }
        } else {
            groups.push(vec![route.clone()]);
        }
    }
    groups
}

/// The same route with one grant replaced, carrying every other field.
///
/// A rebuild that forgets one of these chains answers a different question than
/// the writer asked: `Route::new` starts `ingress`, `static_policy` and `php` at
/// `None`, so a route with a `static` policy or an `entrypoint` would compare
/// unequal to the parsed one and refuse every edit (`MB.3.21`).
fn with_grant(route: &Route, store: ProxyName, grant: Grant) -> Route {
    let mut grants = route.grants().clone();
    grants.insert(store, grant);
    Route::new(
        route.name().to_owned(),
        route.path().to_vec(),
        route.methods().map(<[String]>::to_vec),
        route.groups().to_vec(),
        route.egress(),
        route.prefork(),
        grants,
    )
    .with_ingress(route.ingress())
    .with_static(route.static_policy().cloned())
    .with_php(route.php().cloned())
    .with_sandbox(route.sandbox())
}

/// Which section of the model moved, for the postcondition's message.
///
/// The pair is the model the edited text lowered to and the model the edit
/// intended: the arms name where the two diverge, so a splice that lost a mount
/// says "the static mounts" rather than blaming the views every time.
fn first_difference(edited: &Manifest, expected: &Manifest) -> &'static str {
    if edited.routes() != expected.routes() {
        "the views"
    } else if edited.groups() != expected.groups() {
        "the traits"
    } else if edited.proxies() != expected.proxies() {
        "the stores"
    } else if edited.mounts() != expected.mounts() {
        "the static mounts"
    } else if edited.php() != expected.php() {
        "the entrypoint"
    } else {
        "the top-level keys"
    }
}

/// The routes one view lowered to, or `None` when no view has that name.
fn view_routes<'a>(manifest: &'a Manifest, name: &str) -> Option<Vec<&'a Route>> {
    let routes: Vec<&Route> = manifest
        .routes()
        .iter()
        .filter(|route| route.name() == name)
        .collect();
    (!routes.is_empty()).then_some(routes)
}

/// The index of the view that is the method-unrestricted catch-all, if any.
fn catch_all_view(manifest: &Manifest) -> Option<usize> {
    let route = manifest
        .routes()
        .iter()
        .find(|route| route.is_unrestricted_catch_all())?;
    usize::try_from(route.sandbox()).ok()
}

/// The normalised methods a proposal writes, or `None` for "every method".
fn normalised_methods(methods: &[String]) -> Option<Vec<String>> {
    let written: Vec<(&str, Position)> = methods
        .iter()
        .map(|method| (method.as_str(), Position::NONE))
        .collect();
    normalise_methods(&written).ok().flatten()
}

/// A view name is `[a-z0-9][a-z0-9_-]*` (§6.7).
fn valid_view_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    bytes.all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
    })
}

/// A method is one upper-case word (§8.4), so the written header parses.
fn valid_method(method: &str) -> bool {
    method
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase())
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Normalises proposed statements and refuses the two kinds a grant can never
/// hold (`R24`), without putting either in the message.
fn normalise_queries(
    view: &str,
    store: &str,
    queries: &[String],
) -> Result<Vec<String>, EditError> {
    let mut out: Vec<String> = Vec::new();
    for (index, query) in queries.iter().enumerate() {
        let normalized = normalize(query);
        if normalized.is_empty() {
            return Err(EditError::EmptyQuery {
                route: view.to_owned(),
                proxy: store.to_owned(),
                index,
            });
        }
        if normalized.contains('…') {
            return Err(EditError::RedactedQuery {
                route: view.to_owned(),
                proxy: store.to_owned(),
                index,
            });
        }
        if has_quoted_literal(&normalized) && !KNOWN_STATIC_SETUP_SQL.contains(&normalized.as_str())
        {
            return Err(EditError::LiteralQuery {
                route: view.to_owned(),
                proxy: store.to_owned(),
                index,
            });
        }
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

/// The statements that are not already granted, in proposal order.
fn fresh_queries(written: &[String], proposed: &[String]) -> Vec<String> {
    let already: BTreeSet<String> = written.iter().map(|query| normalize(query)).collect();
    proposed
        .iter()
        .filter(|query| !already.contains(query.as_str()))
        .cloned()
        .collect()
}

/// Where the new views go (§14.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Insertion {
    /// Immediately above the view at this index: the first catch-all.
    Above(usize),
    /// At the end of the document.
    End,
}

/// The `queries` one store's block gains.
struct GrantTarget {
    /// The store's name, or `None` for the unnamed `postgres { … }` block.
    store: Option<String>,
    queries: Vec<String>,
}

/// A `view` block to insert.
struct NewView {
    name: String,
    methods: Vec<String>,
    patterns: Vec<String>,
    grants: Vec<GrantTarget>,
}

/// A grant to merge into a view the document already declares.
struct Patch {
    view: String,
    /// The store's name, or `None` for the unnamed `postgres { … }` block.
    store: Option<String>,
    queries: Vec<String>,
}

/// Everything an edits set resolves to against one document.
struct Plan {
    insert: Insertion,
    new_views: Vec<NewView>,
    patches: Vec<Patch>,
}

impl Plan {
    /// Whether the edits have nothing to add, which makes the run a no-op.
    fn is_noop(&self) -> bool {
        self.new_views.is_empty() && self.patches.is_empty()
    }

    /// Resolves `edits` against the lowered document.
    fn build(manifest: &Manifest, edits: &BlastfileEdits) -> Result<Self, EditError> {
        let mut new_views: Vec<NewView> = Vec::new();
        let mut names: BTreeSet<String> = BTreeSet::new();

        for proposed in &edits.views {
            if !valid_view_name(&proposed.name) {
                return Err(EditError::InvalidRouteName {
                    name: proposed.name.clone(),
                });
            }
            if !names.insert(proposed.name.clone()) {
                return Err(EditError::DuplicateProposedRoute {
                    name: proposed.name.clone(),
                });
            }
            if proposed.patterns.is_empty() {
                return Err(EditError::EmptyPath {
                    name: proposed.name.clone(),
                });
            }
            let mut path: Vec<String> = Vec::new();
            for (rule, pattern) in proposed.patterns.iter().enumerate() {
                if pattern.is_empty() {
                    return Err(EditError::EmptyPattern {
                        name: proposed.name.clone(),
                        rule,
                    });
                }
                match pattern::compile(pattern, Position::NONE) {
                    Ok(compiled) => path.push(compiled.fragment().to_owned()),
                    Err(_) => {
                        return Err(EditError::InvalidPattern {
                            name: proposed.name.clone(),
                            rule,
                        });
                    }
                }
            }
            for method in &proposed.methods {
                if !valid_method(method) {
                    return Err(EditError::InvalidMethod {
                        name: proposed.name.clone(),
                        method: method.clone(),
                    });
                }
            }
            let methods = normalised_methods(&proposed.methods);
            match view_routes(manifest, &proposed.name) {
                Some(existing) => {
                    // A view the document already declares with this exact
                    // shape is the same proposal twice: nothing to insert.
                    let same = existing.iter().all(|route| {
                        route.path() == path.as_slice() && route.methods() == methods.as_deref()
                    });
                    if !same {
                        return Err(EditError::RouteConflict {
                            name: proposed.name.clone(),
                        });
                    }
                }
                None => new_views.push(NewView {
                    name: proposed.name.clone(),
                    methods: proposed.methods.clone(),
                    patterns: proposed.patterns.clone(),
                    grants: Vec::new(),
                }),
            }
        }

        let mut patches: Vec<Patch> = Vec::new();
        for proposed in &edits.grants {
            let store_name = proposed.store.as_deref().unwrap_or(DEFAULT_STORE);
            if manifest.proxy(store_name).is_none() {
                return Err(EditError::UnknownProxy {
                    route: proposed.view.clone(),
                    proxy: store_name.to_owned(),
                });
            }
            let fresh = normalise_queries(&proposed.view, store_name, &proposed.queries)?;
            if fresh.is_empty() {
                continue;
            }
            if let Some(view) = new_views.iter_mut().find(|view| view.name == proposed.view) {
                merge_target(&mut view.grants, proposed.store.clone(), fresh);
                continue;
            }
            let Some(routes) = view_routes(manifest, &proposed.view) else {
                return Err(EditError::MissingRoute {
                    route: proposed.view.clone(),
                    proxy: store_name.to_owned(),
                });
            };
            let store = ProxyName::from(store_name);
            let existing = routes
                .first()
                .and_then(|route| route.grant(&store))
                .and_then(Grant::as_postgresql);
            if existing.is_some_and(PgGrant::is_unrestricted) {
                return Err(EditError::UnrestrictedGrant {
                    route: proposed.view.clone(),
                    proxy: store_name.to_owned(),
                });
            }
            let written: &[String] = existing.map(PgGrant::queries).unwrap_or(&[]);
            let fresh = fresh_queries(written, &fresh);
            if fresh.is_empty() {
                continue;
            }
            merge_patch(&mut patches, &proposed.view, proposed.store.clone(), fresh);
        }

        let insert = match catch_all_view(manifest) {
            Some(index) => Insertion::Above(index),
            None => Insertion::End,
        };
        Ok(Self {
            insert,
            new_views,
            patches,
        })
    }

    /// Produces the edited document, splicing only whole lines.
    fn render(&self, cst: &Cst, source: &str) -> Result<String, EditError> {
        let eol = line_ending(source);
        let mut splices: Vec<(usize, String)> = Vec::new();
        for patch in &self.patches {
            splices.push(patch_splice(cst, source, patch, eol)?);
        }
        if !self.new_views.is_empty() {
            splices.push(self.views_splice(cst, source, eol)?);
        }
        splices.sort_by_key(|splice| splice.0);

        let mut out = String::with_capacity(source.len().saturating_add(256));
        let mut cursor = 0;
        for (offset, text) in splices {
            let head = source.get(cursor..offset).ok_or(EditError::Verification {
                section: "the insertion points",
            })?;
            out.push_str(head);
            out.push_str(&text);
            cursor = offset;
        }
        out.push_str(source.get(cursor..).ok_or(EditError::Verification {
            section: "the insertion points",
        })?);
        Ok(out)
    }

    /// The splice that inserts every new view (§14.5's first two rows).
    fn views_splice(
        &self,
        cst: &Cst,
        source: &str,
        eol: &str,
    ) -> Result<(usize, String), EditError> {
        let mut block = String::new();
        for (index, view) in self.new_views.iter().enumerate() {
            if index > 0 {
                block.push_str(eol);
            }
            block.push_str(&render_view(view, eol));
        }

        match self.insert {
            Insertion::Above(index) => {
                let view = cst.views().nth(index).ok_or(EditError::Verification {
                    section: "the views",
                })?;
                let first = cst
                    .first_token(view)
                    .and_then(|token| cst.token_span(token))
                    .ok_or(EditError::Verification {
                        section: "the views",
                    })?;
                let at = comment_run_start(source, line_start(source, first.start as usize));
                let mut text = block;
                if at > 0 && !previous_line_blank(source, at) {
                    text.insert_str(0, eol);
                }
                if !blank_line_at(source, at) {
                    text.push_str(eol);
                }
                Ok((at, text))
            }
            Insertion::End => {
                let mut text = String::new();
                if !source.is_empty() {
                    if !source.ends_with('\n') {
                        text.push_str(eol);
                    }
                    if !last_line_blank(source) {
                        text.push_str(eol);
                    }
                }
                text.push_str(&block);
                Ok((source.len(), text))
            }
        }
    }
}

/// Adds a grant to a view the edit creates, merging a repeated proposal.
fn merge_target(grants: &mut Vec<GrantTarget>, store: Option<String>, fresh: Vec<String>) {
    match grants.iter_mut().find(|target| target.store == store) {
        Some(target) => {
            for query in fresh {
                if !target.queries.contains(&query) {
                    target.queries.push(query);
                }
            }
        }
        None => grants.push(GrantTarget {
            store,
            queries: fresh,
        }),
    }
}

/// Merges a repeated proposal for one view and store into one patch.
fn merge_patch(patches: &mut Vec<Patch>, view: &str, store: Option<String>, fresh: Vec<String>) {
    match patches
        .iter_mut()
        .find(|patch| patch.view == view && patch.store == store)
    {
        Some(patch) => {
            for query in fresh {
                if !patch.queries.contains(&query) {
                    patch.queries.push(query);
                }
            }
        }
        None => patches.push(Patch {
            view: view.to_owned(),
            store,
            queries: fresh,
        }),
    }
}

/// The splice that adds `patch`'s statements to one view's grant block.
fn patch_splice(
    cst: &Cst,
    source: &str,
    patch: &Patch,
    eol: &str,
) -> Result<(usize, String), EditError> {
    let view = view_node(cst, &patch.view).ok_or(EditError::Verification {
        section: "the views",
    })?;
    let statements: Vec<String> = patch
        .queries
        .iter()
        .map(|query| query_statement(query))
        .collect();

    let grant = cst
        .grants(view)
        .find(|grant| same_store(grant_store(cst, *grant), patch.store.as_deref()));
    match grant {
        Some(grant) => {
            let block = cst
                .first_of_kind(grant, NodeKind::GrantBlock)
                .ok_or_else(|| EditError::UnrestrictedGrant {
                    route: patch.view.clone(),
                    proxy: patch
                        .store
                        .clone()
                        .unwrap_or_else(|| DEFAULT_STORE.to_owned()),
                })?;
            let close = block_close(cst, block)?;
            Ok(insertion_at(cst, source, block, close, &statements, eol))
        }
        None => {
            let close = block_close(cst, view)?;
            let line = line_start(source, close);
            if source
                .get(line..close)
                .is_some_and(|prefix| prefix.trim().is_empty())
            {
                let item = format!("{}    ", indent_of(source, line));
                let mut text = String::new();
                text.push_str(&item);
                text.push_str(&store_head(patch.store.as_deref()));
                text.push_str(" {");
                text.push_str(eol);
                for statement in &statements {
                    text.push_str(&item);
                    text.push_str("    ");
                    text.push_str(statement);
                    text.push_str(eol);
                }
                text.push_str(&item);
                text.push('}');
                text.push_str(eol);
                Ok((line, text))
            } else {
                // A one-line view: its body is a `;`-separated list.
                let mut text = String::new();
                let block = format!(
                    "{} {{ {} }}",
                    store_head(patch.store.as_deref()),
                    statements.join("; ")
                );
                if block_is_empty(cst, view) {
                    text.push(' ');
                } else {
                    text.push_str("; ");
                }
                text.push_str(&block);
                let at = trim_spaces_before(source, close);
                Ok((at, text))
            }
        }
    }
}

/// The splice for one block whose closing `}` is at `close`.
///
/// A `}` that starts its line gets whole lines above it; a one-line block is
/// extended in place, with `;` between statements (and no `;` when the block is
/// empty, because a block cannot start with one).
fn insertion_at(
    cst: &Cst,
    source: &str,
    block: NodeId,
    close: usize,
    statements: &[String],
    eol: &str,
) -> (usize, String) {
    let line = line_start(source, close);
    if source
        .get(line..close)
        .is_some_and(|prefix| prefix.trim().is_empty())
    {
        let indent = format!("{}    ", indent_of(source, line));
        let mut text = String::new();
        for statement in statements {
            text.push_str(&indent);
            text.push_str(statement);
            text.push_str(eol);
        }
        (line, text)
    } else {
        let empty = block_is_empty(cst, block);
        let mut text = String::new();
        for (index, statement) in statements.iter().enumerate() {
            if index == 0 && empty {
                text.push(' ');
            } else {
                text.push_str("; ");
            }
            text.push_str(statement);
        }
        (trim_spaces_before(source, close), text)
    }
}

/// One new `view` block, laid out at the top level (§14.5's second row).
fn render_view(view: &NewView, eol: &str) -> String {
    let mut out = String::new();
    out.push_str("view ");
    out.push_str(&view.name);
    if !view.methods.is_empty() {
        out.push(' ');
        out.push_str(&view.methods.join(", "));
    }
    out.push(' ');
    let patterns: Vec<String> = view
        .patterns
        .iter()
        .map(|pattern| format!("\"{}\"", escape_string(pattern)))
        .collect();
    out.push_str(&patterns.join(", "));
    out.push_str(" {");
    out.push_str(eol);
    for target in &view.grants {
        out.push_str("    ");
        out.push_str(&store_head(target.store.as_deref()));
        out.push_str(" {");
        out.push_str(eol);
        for query in &target.queries {
            out.push_str("        ");
            out.push_str(&query_statement(query));
            out.push_str(eol);
        }
        out.push_str("    }");
        out.push_str(eol);
    }
    out.push('}');
    out.push_str(eol);
    out
}

/// The header of a grant: `postgres` for the unnamed block, else its store.
///
/// The unnamed block and a block naming `db` are the same store (§7.4), so the
/// editor writes the bare form for both: a view with no block gets
/// `postgres { … }`, which is what §14.5's table promises.
fn store_head(store: Option<&str>) -> String {
    match store {
        Some(name) if name != DEFAULT_STORE => format!("postgres {name}"),
        _ => String::from("postgres"),
    }
}

/// Whether two store spellings name the same store (§7.4).
///
/// `postgres { … }` and `postgres db { … }` are one store, so a proposal for
/// either has to find the block the author wrote, not add a second one.
fn same_store(found: Option<&str>, wanted: Option<&str>) -> bool {
    found.unwrap_or(DEFAULT_STORE) == wanted.unwrap_or(DEFAULT_STORE)
}

/// One `query` statement (§14.5's last two rows).
fn query_statement(sql: &str) -> String {
    if sql.contains('\'') {
        format!("query \"{}\"", escape_string(sql))
    } else {
        format!("query '{sql}'")
    }
}

/// The body of a `"…"` string: §5.1's four escapes, so the reader accepts it.
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// The `view` node with this name, if the document declares one.
fn view_node(cst: &Cst, name: &str) -> Option<NodeId> {
    cst.views()
        .find(|view| cst.words(*view).nth(1) == Some(name))
}

/// The store a `postgres` grant names, `None` for the unnamed form (§7.4).
fn grant_store(cst: &Cst, grant: NodeId) -> Option<&str> {
    cst.words(grant)
        .skip(1)
        .find(|word| *word != "unrestricted")
}

/// The byte offset of the `}` that closes `node`'s subtree.
fn block_close(cst: &Cst, node: NodeId) -> Result<usize, EditError> {
    cst.last_token(node)
        .and_then(|token| cst.token_span(token))
        .map(|span| span.start as usize)
        .ok_or(EditError::Verification {
            section: "the views",
        })
}

/// Whether `node`'s block holds no statement at all.
///
/// Emptiness is per shape, because the two nodes the editor extends do not hold
/// the same children. A `view` also holds the `view` keyword, its name and its
/// `ViewHeader` node, so only a statement node is content: counting the header
/// would put a `;` in front of the first statement of an empty one-line view. A
/// `grant_block` holds its clauses as bare tokens (`transactions`, `functions`,
/// `select …`) and its statements as `Query` nodes, so anything but a brace or
/// trivia is content: counting only nodes would splice a second statement onto
/// `transactions` with no separator.
fn block_is_empty(cst: &Cst, node: NodeId) -> bool {
    match cst.kind(node) {
        Some(NodeKind::View) => !cst.child_nodes(node).any(|child| {
            matches!(
                cst.kind(child),
                Some(
                    NodeKind::Grant
                        | NodeKind::Use
                        | NodeKind::Egress
                        | NodeKind::Prefork
                        | NodeKind::Body
                        | NodeKind::Endpoint
                )
            )
        }),
        _ => !cst.children(node).any(|element| match element {
            Element::Token(token) => !matches!(
                cst.token_kind(token),
                Some(TokenKind::LBrace | TokenKind::RBrace)
            ),
            Element::Trivia(_) => false,
            Element::Node { .. } => true,
        }),
    }
}

/// The line ending the document uses, so an edit does not mix two.
fn line_ending(source: &str) -> &'static str {
    match source.find('\n') {
        Some(at) if at > 0 && source.as_bytes().get(at - 1) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

/// The byte offset where the line holding `offset` starts.
fn line_start(source: &str, offset: usize) -> usize {
    source
        .get(..offset)
        .and_then(|head| head.rfind('\n'))
        .map_or(0, |at| at.saturating_add(1))
}

/// The leading spaces and tabs of the line holding `offset`.
fn indent_of(source: &str, offset: usize) -> &str {
    let start = line_start(source, offset);
    let rest = source.get(start..).unwrap_or("");
    let trimmed = rest.trim_start_matches([' ', '\t']);
    rest.get(..rest.len().saturating_sub(trimmed.len()))
        .unwrap_or("")
}

/// The byte offset of the first non-space before `offset`.
fn trim_spaces_before(source: &str, offset: usize) -> usize {
    source
        .get(..offset)
        .map(|head| head.trim_end_matches([' ', '\t']).len())
        .unwrap_or(offset)
}

/// The start of the run of comment-only lines that ends at `at`.
fn comment_run_start(source: &str, mut at: usize) -> usize {
    while at > 0 {
        let end = at.saturating_sub(1);
        let start = line_start(source, end);
        let Some(line) = source.get(start..end) else {
            break;
        };
        if !line.trim_start().starts_with('#') {
            break;
        }
        at = start;
    }
    at
}

/// Whether the line starting at `at` is blank.
fn blank_line_at(source: &str, at: usize) -> bool {
    source
        .get(at..)
        .and_then(|rest| rest.split('\n').next())
        .is_some_and(|line| line.trim().is_empty())
}

/// Whether the line above `at` is blank (true at the start of the file).
fn previous_line_blank(source: &str, at: usize) -> bool {
    if at == 0 {
        return true;
    }
    let end = at.saturating_sub(1);
    let start = line_start(source, end);
    source
        .get(start..end)
        .is_some_and(|line| line.trim().is_empty())
}

/// Whether the document's last line is blank.
fn last_line_blank(source: &str) -> bool {
    let trimmed = source.trim_end_matches(['\n', '\r']);
    match source.get(trimmed.len()..) {
        Some(tail) => tail.matches('\n').count() >= 2,
        None => false,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::{
        escape_string, last_line_blank, line_ending, query_statement, same_store, store_head,
    };

    /// §14.5's two query spellings, and the escape table the reader accepts.
    #[test]
    fn a_statement_is_written_in_the_form_its_bytes_allow() {
        assert_eq!(query_statement("SELECT 1"), "query 'SELECT 1'");
        assert_eq!(
            query_statement(r#"SELECT "t"."a" FROM "t" WHERE "t"."b" = $?"#),
            r#"query 'SELECT "t"."a" FROM "t" WHERE "t"."b" = $?'"#
        );
        assert_eq!(
            query_statement("SET TIME ZONE 'UTC'"),
            "query \"SET TIME ZONE 'UTC'\""
        );
        // A statement that holds both a quote and a double quote escapes the
        // double quote, so the reader decodes the statement it was given.
        assert_eq!(
            query_statement(r#"SELECT 'a' AS "b""#),
            "query \"SELECT 'a' AS \\\"b\\\"\""
        );
        assert_eq!(escape_string("a\\b\nc\td"), "a\\\\b\\nc\\td");
    }

    /// The unnamed store is written as the bare `postgres` form (§14.5).
    #[test]
    fn the_default_store_is_written_without_a_name() {
        assert_eq!(store_head(None), "postgres");
        assert_eq!(store_head(Some("db")), "postgres");
        assert_eq!(store_head(Some("analytics")), "postgres analytics");
        // `postgres { … }` and `postgres db { … }` are one store (§7.4).
        assert!(same_store(None, Some("db")));
        assert!(same_store(Some("db"), None));
        assert!(!same_store(Some("analytics"), None));
    }

    /// An edit keeps the document's own line ending.
    #[test]
    fn the_line_ending_is_the_documents() {
        assert_eq!(line_ending("view a \"/\" {\n}\n"), "\n");
        assert_eq!(line_ending("view a \"/\" {\r\n}\r\n"), "\r\n");
        assert_eq!(line_ending(""), "\n");
        assert_eq!(line_ending("\n"), "\n");
    }

    /// The append case separates itself from a document that ends without a
    /// blank line, and from one that already has one.
    #[test]
    fn the_last_line_is_blank_only_when_it_is() {
        assert!(last_line_blank("view a \"/\" {\n}\n\n"));
        assert!(!last_line_blank("view a \"/\" {\n}\n"));
        assert!(!last_line_blank(""));
    }
}
