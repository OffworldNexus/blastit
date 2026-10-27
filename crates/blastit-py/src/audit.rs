//! The two functions behind the pytest plugin (D3b, ARCHITECTURE.md §6).
//!
//! [`normalize_sql`] is [`blastit_sqlpolicy::normalize`]: the same rules the
//! manifest and dbproxy use, so a Python caller can see exactly how a statement
//! will be compared.
//!
//! [`audit`] takes a manifest and the observations a pytest session recorded
//! and returns one JSON report: for every route, the statements that were
//! executed but not granted, the grants nothing ever matched, and whether the
//! route needs `transaction_control` it was not given; plus the requests that
//! would not have been routed at all.
//!
//! Routing, grants and matching are blastit's own — [`RoutingTable`] on
//! `(method, PATH_INFO)`, [`Manifest::effective_grants`] per `(route, proxy)`,
//! and [`Allowlist::check`] on the normalised SQL — so the report is the diff a
//! human or an agent needs to fill the manifest in, with no second
//! implementation of blastit's rules to drift from.
//!
//! # The observations document
//!
//! The plugin writes it, and [`audit`] reads it:
//!
//! ```json
//! {
//!   "version": 1,
//!   "requests": [
//!     {
//!       "method": "GET",
//!       "path_info": "/notes/",
//!       "nodeid": "tests/test_notes.py::test_list",
//!       "transaction_control": false,
//!       "statements": [
//!         {"alias": "default", "proxy": "db", "sql": "SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1"}
//!       ],
//!       "egress": [{"family": "AF_INET", "host": "redis", "port": 6379}]
//!     }
//!   ],
//!   "connection_setup": [
//!     {"alias": "default", "proxy": "db", "sql": "SET application_name = 'app'", "count": 2}
//!   ],
//!   "unrecognised": [
//!     {"alias": "default", "query": "Template('SELECT ...')", "count": 1}
//!   ],
//!   "warnings": ["the database alias 'default' does not use server-side binding"]
//! }
//! ```
//!
//! * `method` and `path_info` are what the app saw, i.e. the same
//!   `(method, PATH_INFO)` the scheduler routed on;
//! * `nodeid` is the pytest node id that ran the request, absent outside a test;
//! * `transaction_control` is true when the request ran on a database alias with
//!   `ATOMIC_REQUESTS` on, i.e. a transaction (or a savepoint, which is what a
//!   nested `atomic` looks like inside a test) surrounds it;
//! * `statements` are the wire SQL texts in execution order, `$n` placeholders
//!   included, with the database alias they came from in `alias` (informational)
//!   and the manifest proxy they belong to in `proxy` (absent: the manifest has
//!   to declare exactly one `postgresql` proxy);
//! * `egress` is every outbound connection the request tried to open, as the
//!   socket named it — one entry per attempt, so a repeated destination is a
//!   count (§7.3);
//! * `sql` is what counts: it is normalised with [`blastit_sqlpolicy::normalize`]
//!   before it is compared to a grant, exactly like the text on the wire;
//! * `connection_setup` is what psycopg ran on its own while a Django connection
//!   was being established — its `SET`s, the type lookups a backend does at
//!   connect, anything a `connection_created` receiver sends. They belong to no
//!   request, so they can appear with no request at all, and `count` says how
//!   often they ran;
//! * `unrecognised` is what the plugin could not read: a psycopg t-string
//!   `Template` has no text before it is rendered with its values, and inventing
//!   one would be worse than saying so. The report counts them instead of
//!   dropping them, because they do go on the wire;
//! * `warnings` is what the plugin itself could not do — a connection that would
//!   inline its parameters, so the recorded text is not the text dbproxy checks.
//!   The report carries them, and they are part of `clean`.
//!
//! # The report
//!
//! ```json
//! {
//!   "version": 1,
//!   "manifest": {"routes": 1, "proxies": {"db": "postgresql"}, "warnings": []},
//!   "routes": [
//!     {
//!       "name": "notes",
//!       "path": ["/notes/.*"],
//!       "requests": 2,
//!       "needs_transaction_control": false,
//!       "egress": {
//!         "observed": [{"target": "redis:6379", "host": "redis", "port": 6379,
//!                       "family": "AF_INET", "count": 2,
//!                       "tests": ["tests/test_notes.py::test_cache"]}],
//!         "needed": true
//!       },
//!       "proxies": [
//!         {
//!           "name": "db",
//!           "observed": 3,
//!           "missing": [
//!             {
//!               "sql": "DELETE FROM \"notes\"",
//!               "count": 1,
//!               "wire_sql": "DELETE FROM \"notes\" WHERE \"id\" = $1",
//!               "tests": ["tests/test_notes.py::test_delete"],
//!               "paths": ["/notes/3/delete/"]
//!             }
//!           ],
//!           "unused": ["SELECT ..."],
//!           "transaction_control": {"observed": false, "granted": false, "needed": false},
//!           "table_grants": {"tables": {"notes": {"select": ["id"]}},
//!                            "functions": [], "toml": "[route.db.tables]\n…",
//!                            "unanalysable": []}
//!         }
//!       ]
//!     }
//!   ],
//!   "unrouted": [
//!     {"method": "GET", "path_info": "/oops/", "count": 1, "statements": 0,
//!      "tests": ["tests/test_oops.py::test_oops"]}
//!   ],
//!   "unexercised_routes": [],
//!   "unused_global": {
//!     "statements": [{"proxy": "db", "sql": "SELECT 1",
//!                     "groups": ["connection"]}],
//!     "tables": [{"proxy": "db", "group": "catalog",
//!                 "tables": {"shop_product": {"select": ["id"]}},
//!                 "functions": []}],
//!     "count": 2
//!   },
//!   "connection_setup": {
//!     "statements": [
//!       {"sql": "SET application_name = 'app'", "proxy": "db", "count": 2,
//!        "wire_sql": "SET application_name = 'app'",
//!        "ungranted_in": ["checkout"], "granted_in": 4}
//!     ],
//!     "ungranted": 1
//!   },
//!   "unrecognised": {"statements": [{"query": "Template('SELECT ...')",
//!                                    "alias": "default", "count": 1}],
//!                    "count": 1},
//!   "plugin_warnings": [],
//!   "totals": {
//!     "requests": 2, "routed": 2, "unrouted": 0, "routes": 1,
//!     "routes_exercised": 1, "statements": 3, "missing": 1,
//!     "missing_distinct": 1, "unused": 1, "unused_global": 0,
//!     "needs_transaction_control": [],
//!     "connection_setup": 1, "connection_setup_ungranted": 1, "egress": 1,
//!     "unrecognised": 1, "plugin_warnings": 0, "clean": false
//!   }
//! }
//! ```
//!
//! * `missing[].sql` is the normalised text — paste it into a grant, it is what
//!   the manifest comparison reads; `wire_sql` is one raw occurrence, for a human
//!   who wants to see the parameters as sent;
//! * `missing[].tests` and `.paths` are up to five distinct examples, in the
//!   order they were observed;
//! * `unused` lists the grants of that `(route, proxy)` pair that nothing
//!   matched, in manifest order. A route that no request reached reports no
//!   proxy at all: `unexercised_routes` names it, so listing grants as unused
//!   would be noise. A statement a route is granted only through a *global*
//!   group (§6) is never listed here: that group applies to every route, so the
//!   statement would be "unused" once per route. It is reported once, in
//!   `unused_global`, and only when no route matched it (D3c);
//! * `unused_global` is the same idea for the global groups: `statements` lists
//!   the statements a global group grants that nothing used, with the groups
//!   that grant them, and `tables` lists the table grants a global group hands
//!   out that no observed statement needed. `unused` never holds either, so a
//!   statement of a global group is reported once and not once per route. A
//!   statement `connection_setup` accounts for is *used*: it only runs while a
//!   connection is established, so no request can match it, and a group that
//!   grants it (`groups = ["connection"]`) is doing its job (D3c);
//! * `egress.observed` is what the route reached for, most attempted first, and
//!   `egress.needed` is true when it did and the route has no `egress = true`
//!   (§7.3): a route that may not open connections would have them refused;
//! * `connection_setup` is the other half of the same job: those statements are
//!   not a route's, so what matters is not which route ran them but which routes
//!   do not allow them. `ungranted_in` names those (a route with no grant on the
//!   proxy gets no listener and never runs them, so it is not counted either
//!   way), and `granted_in` counts the routes that already do — the fix is a
//!   group attached to every route, `groups = ["connection"]` (§6);
//! * `totals.clean` is true when nothing is missing, nothing is unrouted, no
//!   route needs transaction control it was not granted, no
//!   connection-setup statement is ungranted in a route, no route opens
//!   connections it may not, nothing was unreadable and the plugin itself
//!   reported nothing — what `--blastit-strict` fails a session over.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use blastit_manifest::{Grant, Manifest, PgGrant, Protocol, ProxyName, RoutingTable};
use blastit_sqlpolicy::{Allowlist, Analysis, TableGrants, analyze, normalize};
use serde_json::{Map, Value, json};

/// How many distinct test node ids and paths one `missing` entry keeps.
const EXAMPLES: usize = 5;

/// Version of the report and of the observations document this build reads.
const VERSION: u64 = 1;

/// The label a request made outside any test is reported under.
const NO_TEST: &str = "(no test)";

/// Why an audit could not run.
///
/// Every variant is a problem in what the caller handed over — the manifest or
/// the observations — never something blastit could not do.
#[derive(Debug)]
pub enum AuditError {
    /// The manifest is not a valid blastit manifest.
    Manifest(String),
    /// The observations are not the document this module reads.
    Observations(String),
    /// A statement names a proxy the manifest never declares.
    UnknownProxy(String),
    /// A statement names a proxy that is not a `postgresql` one.
    NotPostgresql(String),
    /// The manifest declares no `postgresql` proxy at all, so nothing can be
    /// granted and no statement can be attributed.
    NoProxy,
    /// A statement names no proxy and the manifest declares several, so there is
    /// no way to tell which grant applies.
    AmbiguousProxy(Vec<String>),
    /// The report could not be rendered as JSON. It always can in practice.
    Report(String),
}

impl AuditError {
    /// Wraps a rendered [`blastit_manifest::ManifestErrors`].
    fn manifest(errors: &impl fmt::Display) -> Self {
        Self::Manifest(errors.to_string())
    }
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(message) => write!(f, "the manifest is invalid: {message}"),
            Self::Observations(message) => write!(f, "the observations are invalid: {message}"),
            Self::UnknownProxy(proxy) => write!(
                f,
                "the observations name the proxy {proxy:?}, which the manifest does not declare"
            ),
            Self::NotPostgresql(proxy) => write!(
                f,
                "the observations name the proxy {proxy:?}, which is not a postgresql proxy"
            ),
            Self::NoProxy => f.write_str(
                "the manifest declares no postgresql proxy, so no observed statement can be checked",
            ),
            Self::AmbiguousProxy(proxies) => write!(
                f,
                "the manifest declares {} postgresql proxies ({}), and the observations do not say which one a statement used",
                proxies.len(),
                proxies.join(", ")
            ),
            Self::Report(message) => write!(f, "the report could not be rendered: {message}"),
        }
    }
}

impl std::error::Error for AuditError {}

/// Normalises one SQL text for exact comparison (ARCHITECTURE.md §6).
///
/// This is [`blastit_sqlpolicy::normalize`], re-exported so the plugin can show
/// what the manifest comparison will read.
pub(crate) fn normalize_sql(sql: &str) -> String {
    normalize(sql)
}

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
pub(crate) fn analyze_statement(sql: &str) -> Result<String, String> {
    let analysis = analyze(sql).map_err(|error| error.to_string())?;
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
pub(crate) const LOCK_NEEDS_A_COLUMN: &str = "its rows are locked (`FOR UPDATE`) without reading \
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
///   "toml": "[route.db.tables]\nshop_product = { select = [\"id\", \"name\"] }\n",
///   "unanalysable": [{"sql": "SET TIME ZONE 'UTC'",
///                     "reason": "only SELECT, INSERT, UPDATE and DELETE…"}]
/// }
/// ```
///
/// `unanalysable` lists the statements a table grant cannot describe at all —
/// they have to stay exact `queries`, and the reason is the same one a
/// `sql.denied` event carries.
///
/// # Errors
///
/// Only a JSON rendering failure, which cannot happen in practice: a statement
/// this build cannot analyse is data, not an error.
pub(crate) fn covering(statements: &[String], prefix: &str) -> Result<String, String> {
    let mut analyses = Vec::with_capacity(statements.len());
    let mut unanalysable = Vec::new();
    for sql in statements {
        match analyze(sql) {
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
        map.insert("toml".to_string(), json!(tables.to_toml(prefix)));
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
pub(crate) fn classify(sql: &str) -> Result<String, String> {
    let classified = blastit_sqlpolicy::classify(sql).map_err(|error| error.to_string())?;
    let document = json!({
        "class": classified.class().as_str(),
        "label": classified.label(),
        "plain": classified.is_plain(),
        "statements": classified.statements(),
    });
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}

/// Diffs the observations of a test suite against a manifest (§6).
///
/// The observations carry the `PATH_INFO` the app saw, which is the string the
/// scheduler routed on — the view strips `BLASTIT_SCRIPT_NAME` before WSGI
/// (§2.5) — so no mount prefix is involved here. A caller that recorded mounted
/// paths has to strip its own prefix before calling: the scheduler's
/// `strip_script_name` is crate-private, and reimplementing it here would be a
/// second router to keep in step (`blastit-py` cannot see the private module
/// the mount normalisation lives in).
///
/// Returns the report as pretty-printed JSON; see the module documentation for
/// its shape.
///
/// # Errors
///
/// [`AuditError`] when the manifest is invalid, the observations do not have the
/// documented shape, or a statement cannot be attributed to a proxy.
pub(crate) fn audit(manifest_toml: &str, observations_json: &str) -> Result<String, AuditError> {
    let manifest =
        Manifest::from_toml_str(manifest_toml).map_err(|errors| AuditError::manifest(&errors))?;
    let warnings = manifest
        .validate_default()
        .map_err(|errors| AuditError::manifest(&errors))?;
    let table = RoutingTable::new(&manifest).map_err(|errors| AuditError::manifest(&errors))?;
    let observations = parse_observations(observations_json)?;
    let requests = &observations.requests;
    let probe = TcProbe::new();
    // What the *global* groups grant (§6, D3c): a statement they grant belongs
    // to every route at once, so it is never a route's own `unused` line — the
    // report says it once, in `unused_global`.
    let mut global = GlobalState::of(&manifest);

    let mut traces: BTreeMap<usize, RouteTrace> = BTreeMap::new();
    let mut unrouted: BTreeMap<(String, String), UnroutedTrace> = BTreeMap::new();
    let mut routed_requests = 0usize;
    let mut routed_statements = 0usize;

    for request in requests {
        let Some(matched) = table.route(&request.method, &request.path_info) else {
            let entry = unrouted
                .entry((request.method.clone(), request.path_info.clone()))
                .or_insert_with(|| UnroutedTrace::new(&request.method, &request.path_info));
            entry.record(&request.statements, request.nodeid.as_deref());
            continue;
        };

        let trace = traces.entry(matched.route).or_default();
        trace.requests += 1;
        if !request.live {
            trace.suite_requests += 1;
        }
        routed_requests += 1;

        for attempt in &request.egress {
            trace
                .egress
                .entry(attempt.key())
                .or_default()
                .record(attempt, request.nodeid.as_deref());
        }

        for statement in &request.statements {
            let proxy = resolve_proxy(&manifest, statement.proxy.as_deref())?;
            let grant = grant_of(&manifest, matched.route, &proxy);
            let policy = Policy::of(grant.as_ref());
            let normalised = normalize(&statement.sql);
            let trace = trace.proxies.entry(proxy.as_str().to_string()).or_default();
            trace.executions += 1;
            routed_statements += 1;

            // The policy is asked about the text as it went on the wire: it
            // normalises it itself, and a table grant has to *parse* it (D7), so
            // the normalised form (`$?`) is not what it can be handed.
            if policy.allow.check(&statement.sql).is_allowed() {
                if policy.granted.contains(&normalised) {
                    trace.matched.insert(normalised.clone());
                }
            } else {
                trace.missing.entry(normalised.clone()).or_default().record(
                    &statement.sql,
                    request.nodeid.as_deref(),
                    &request.path_info,
                );
            }

            // What a table grant covering this pair would have to say (D7).
            // `analyze` is cached by the same normalised text the policy just
            // checked, so a statement that was analysed there is not parsed
            // again.
            match analyze(&statement.sql) {
                Ok(analysis) => {
                    let covering = TableGrants::covering(std::iter::once(&analysis));
                    let locked: BTreeSet<String> = covering
                        .locks_without_a_column()
                        .map(str::to_string)
                        .collect();
                    if !locked.is_empty() {
                        trace
                            .nameless_locks
                            .entry(normalised.clone())
                            .or_default()
                            .extend(locked);
                    }
                    trace.table_grants.merge_from(&covering);
                    // Whether a global group's table grant was needed at all: a
                    // statement that only this group's tables and columns allow
                    // makes it a used one (D3c).
                    global.mark_used(&proxy, &analysis);
                }
                Err(error) => {
                    trace
                        .unanalysable
                        .entry(normalised.clone())
                        .or_insert_with(|| error.to_string());
                }
            }

            if request.transaction_control || probe.is_transaction_control(&normalised) {
                trace.transaction_control = true;
            }
        }
    }

    let report = assemble(
        &manifest,
        &warnings,
        &observations,
        &traces,
        &unrouted,
        &global,
        Summary {
            requests: requests.len(),
            routed: routed_requests,
            statements: routed_statements,
        },
    )?;
    serde_json::to_string_pretty(&report).map_err(|error| AuditError::Report(error.to_string()))
}

/// The totals the request loop accumulated, on their way into the report.
struct Summary {
    requests: usize,
    routed: usize,
    statements: usize,
}

/// What the *global* groups of a manifest grant, and whether it was needed
/// (§6, D3c).
///
/// A global group is attached to every route at once (`groups = […]` at the top
/// level), so a statement it grants is in every route's effective grant and no
/// route can claim it. Reporting it as unused per route would repeat one dead
/// grant once per route, which is what a manifest with twenty routes and one
/// global group would drown in; reporting it once — here — says the same thing
/// honestly. A statement is unused only when *no* route matched it.
struct GlobalState {
    /// Per proxy, every statement a global group grants, with the groups that
    /// grant it, in manifest order.
    queries: BTreeMap<ProxyName, BTreeMap<String, Vec<String>>>,
    /// Every table grant a global group hands out, and whether any observed
    /// statement needed it.
    tables: Vec<GlobalTables>,
}

/// One global group's table grant on one proxy, and whether it was used.
struct GlobalTables {
    group: String,
    proxy: ProxyName,
    tables: TableGrants,
    used: bool,
}

impl GlobalState {
    /// Collects the global groups' grants, per proxy and per statement.
    fn of(manifest: &Manifest) -> Self {
        let mut queries: BTreeMap<ProxyName, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        let mut tables = Vec::new();
        for name in manifest.global_groups() {
            let Some(group) = manifest.group(name) else {
                continue;
            };
            for (proxy, grant) in group.grants() {
                let Some(pg) = grant.as_postgresql() else {
                    continue;
                };
                for query in pg.queries() {
                    let normalised = normalize(query);
                    if normalised.is_empty() {
                        continue;
                    }
                    queries
                        .entry(proxy.clone())
                        .or_default()
                        .entry(normalised)
                        .or_default()
                        .push(name.clone());
                }
                if !pg.tables().is_empty() {
                    tables.push(GlobalTables {
                        group: name.clone(),
                        proxy: proxy.clone(),
                        tables: pg.tables().clone(),
                        used: false,
                    });
                }
            }
        }
        Self { queries, tables }
    }

    /// Notes that a global group's table grant permitted one observed statement.
    ///
    /// `TableGrants::check` is also happy with a statement that touches nothing
    /// (a `SELECT 1`), so an analysis that needs no grant at all is not evidence
    /// that anything was used.
    fn mark_used(&mut self, proxy: &ProxyName, analysis: &Analysis) {
        let needs_grant =
            !analysis.access_set().is_empty() || analysis.extra_functions().next().is_some();
        if !needs_grant {
            return;
        }
        for tables in &mut self.tables {
            if !tables.used && &tables.proxy == proxy && tables.tables.check(analysis).is_ok() {
                tables.used = true;
            }
        }
    }
}

/// Builds the report document out of the traces of the request loop.
fn assemble(
    manifest: &Manifest,
    warnings: &[blastit_manifest::ManifestWarning],
    observations: &Observations,
    traces: &BTreeMap<usize, RouteTrace>,
    unrouted: &BTreeMap<(String, String), UnroutedTrace>,
    global: &GlobalState,
    summary: Summary,
) -> Result<Value, AuditError> {
    let mut routes = Vec::with_capacity(manifest.route_count());
    let mut unexercised = Vec::new();
    let mut needs_transaction_control = Vec::new();
    let mut needs_egress = Vec::new();
    let mut exercised = 0usize;
    let mut missing = 0usize;
    let mut missing_distinct = 0usize;
    let mut unused = 0usize;
    // The `(proxy, statement)` pairs some route matched: a global group's
    // statement is used as soon as *one* route ran it.
    let mut global_matched: BTreeSet<(ProxyName, String)> = BTreeSet::new();

    for index in 0..manifest.route_count() {
        let (Some(route), trace) = (manifest.route(index), traces.get(&index)) else {
            continue;
        };
        let requests = trace.map_or(0, |trace| trace.requests);
        if requests == 0 {
            // Nothing ran here: naming the route is the signal, and listing its
            // grants as unused would say the same thing five times over.
            unexercised.push(json!(route.name()));
            routes.push(json!({
                "name": route.name(),
                "path": route.path(),
                "requests": 0,
                "needs_transaction_control": false,
                "egress": {"observed": [], "needed": false},
                "proxies": [],
            }));
            continue;
        }
        exercised += 1;

        // A route that only a live server's log saw: the requests are real, but
        // the statements that *ran* are not in it — only the denied ones are.
        // So nothing can be said about the route's grants having matched (D3c).
        let live_only = trace.is_some_and(|trace| trace.suite_requests == 0);

        let mut egress: Vec<Value> = Vec::new();
        let mut attempts: Vec<(&(String, u16), &EgressTrace)> = trace
            .map(|trace| trace.egress.iter().collect())
            .unwrap_or_default();
        attempts.sort_by(|(left, left_trace), (right, right_trace)| {
            right_trace
                .count
                .cmp(&left_trace.count)
                .then_with(|| left.cmp(right))
        });
        for ((host, port), attempt) in attempts {
            egress.push(json!({
                "target": format!("{host}:{port}"),
                "host": host,
                "port": port,
                "family": attempt.family,
                "count": attempt.count,
                "tests": attempt.tests,
            }));
        }
        let egress_needed = !egress.is_empty() && !route.egress();
        if egress_needed {
            needs_egress.push(json!(route.name()));
        }

        let mut proxies = Vec::new();
        let mut route_needs_transaction_control = false;
        for proxy in proxy_names(manifest, index, trace) {
            let grant = grant_of(manifest, index, &proxy);
            let policy = Policy::of(grant.as_ref());
            let observed = trace.and_then(|trace| trace.proxies.get(proxy.as_str()));
            let matched = observed.map_or(&EMPTY_MATCHED, |observed| &observed.matched);
            for sql in matched {
                global_matched.insert((proxy.clone(), sql.clone()));
            }

            let mut entries: Vec<(&String, &MissingTrace)> = observed
                .map(|observed| observed.missing.iter().collect())
                .unwrap_or_default();
            entries.sort_by(|(left, left_trace), (right, right_trace)| {
                right_trace
                    .count
                    .cmp(&left_trace.count)
                    .then_with(|| left.cmp(right))
            });
            let entries: Vec<Value> = entries
                .into_iter()
                .map(|(sql, trace)| {
                    missing += trace.count;
                    missing_distinct += 1;
                    json!({
                        "sql": sql,
                        "count": trace.count,
                        "wire_sql": trace.wire_sql,
                        "tests": trace.tests,
                        "paths": trace.paths,
                    })
                })
                .collect();

            // A statement this route only gets through a *global* group is not
            // this route's unused line: that group is attached to every route,
            // so the same grant would be reported dead once per route. What a
            // route *does* own — its inline grant and the groups it names — is
            // reported here; the global groups' statements are reported once,
            // below, when no route matched them (D3c).
            let own = own_queries(manifest, index, &proxy);
            let unused_here: Vec<&String> = if live_only {
                // Nothing observed this route but its own log: no grant of it
                // can be called dead.
                Vec::new()
            } else {
                policy
                    .granted
                    .iter()
                    .filter(|query| {
                        !matched.contains(query.as_str()) && own.contains(query.as_str())
                    })
                    .collect()
            };
            unused += unused_here.len();

            let observed_control = observed.is_some_and(|observed| observed.transaction_control);
            let granted_control = policy.transaction_control;
            let needed = observed_control && !granted_control;
            route_needs_transaction_control |= needed;

            let (table_grants, unanalysable) = observed.map_or_else(
                || (TableGrants::default(), Vec::new()),
                |observed| {
                    let mut tables = observed.table_grants.clone();
                    // A lock one statement could not name a column for may yet be
                    // named from what another statement of the pair reads, which
                    // is what makes the merged grant better than any one of them
                    // (§6, D3c).
                    tables.pick_locking_columns();
                    let nameless: BTreeSet<&str> = tables.locks_without_a_column().collect();
                    let mut unanalysable: Vec<Value> = observed
                        .unanalysable
                        .iter()
                        .map(|(sql, reason)| json!({"sql": sql, "reason": reason}))
                        .collect();
                    for (sql, locked) in &observed.nameless_locks {
                        if locked.iter().any(|table| nameless.contains(table.as_str())) {
                            unanalysable.push(json!({"sql": sql, "reason": LOCK_NEEDS_A_COLUMN}));
                        }
                    }
                    (tables, unanalysable)
                },
            );

            proxies.push(json!({
                "name": proxy.as_str(),
                "observed": observed.map_or(0, |observed| observed.executions),
                "missing": entries,
                "unused": unused_here,
                "transaction_control": {
                    "observed": observed_control,
                    "granted": granted_control,
                    "needed": needed,
                },
                "table_grants": table_grants_json(&table_grants, proxy.as_str(), unanalysable),
            }));
        }
        if route_needs_transaction_control {
            needs_transaction_control.push(json!(route.name()));
        }

        routes.push(json!({
            "name": route.name(),
            "path": route.path(),
            "requests": requests,
            "needs_transaction_control": route_needs_transaction_control,
            "egress": {"observed": egress, "needed": egress_needed},
            "proxies": proxies,
        }));
    }

    let mut unrouted_entries: Vec<&UnroutedTrace> = unrouted.values().collect();
    unrouted_entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.method.cmp(&right.method))
            .then_with(|| left.path.cmp(&right.path))
    });
    let unrouted_count: usize = unrouted_entries.iter().map(|entry| entry.count).sum();
    let unrouted_entries: Vec<Value> = unrouted_entries
        .into_iter()
        .map(|entry| {
            json!({
                "method": entry.method,
                "path_info": entry.path,
                "count": entry.count,
                "statements": entry.statements,
                "tests": entry.tests,
            })
        })
        .collect();

    let proxies: Map<String, Value> = manifest
        .proxies()
        .iter()
        .map(|(name, proxy)| (name.as_str().to_string(), json!(proxy.protocol().as_str())))
        .collect();
    let warnings: Vec<Value> = warnings
        .iter()
        .map(|warning| json!(warning.to_string()))
        .collect();
    let setup = setup_section(manifest, &observations.connection_setup)?;
    let unrecognised_total: u64 = observations
        .unrecognised
        .iter()
        .map(|entry| entry.count)
        .sum();
    let unrecognised: Vec<Value> = observations
        .unrecognised
        .iter()
        .map(|entry| {
            json!({
                "query": entry.query,
                "alias": entry.alias,
                "count": entry.count,
            })
        })
        .collect();
    let unused_global = unused_global_section(
        global,
        &used_statements(&global_matched, &setup),
        &live_only_proxies(manifest, traces),
    );
    let clean = missing == 0
        && unrouted_count == 0
        && needs_transaction_control.is_empty()
        && setup.ungranted == 0
        && needs_egress.is_empty()
        && unrecognised_total == 0
        && observations.warnings.is_empty();

    Ok(json!({
        "version": VERSION,
        "manifest": {
            "routes": manifest.route_count(),
            "proxies": proxies,
            "warnings": warnings,
        },
        "routes": routes,
        "unrouted": unrouted_entries,
        "unexercised_routes": unexercised,
        "unused_global": unused_global,
        "connection_setup": setup.value,
        "unrecognised": {
            "statements": unrecognised,
            "count": unrecognised_total,
        },
        "plugin_warnings": observations.warnings,
        "totals": {
            "requests": summary.requests,
            "routed": summary.routed,
            "unrouted": unrouted_count,
            "routes": manifest.route_count(),
            "routes_exercised": exercised,
            "statements": summary.statements,
            "missing": missing,
            "missing_distinct": missing_distinct,
            "unused": unused,
            "unused_global": unused_global["count"],
            "needs_transaction_control": needs_transaction_control,
            "connection_setup": setup.statements,
            "connection_setup_ungranted": setup.ungranted,
            "egress": needs_egress.len(),
            "unrecognised": unrecognised_total,
            "plugin_warnings": observations.warnings.len(),
            "clean": clean,
        },
    }))
}

/// The proxies on which some route was seen *only* in a live server's log
/// (D3c).
///
/// A global group's grant is only called unused when no route matched it — and
/// on such a proxy no route can be *asked*: the log records denials, not the
/// statements that ran, so "nothing matched it" would be a claim about traffic
/// nobody observed.
fn live_only_proxies(
    manifest: &Manifest,
    traces: &BTreeMap<usize, RouteTrace>,
) -> BTreeSet<ProxyName> {
    traces
        .iter()
        .filter(|(_, trace)| trace.requests > 0 && trace.suite_requests == 0)
        .flat_map(|(index, _)| manifest.effective_grants(*index).into_keys())
        .collect()
}

/// Every `(proxy, statement)` pair the suite has used (§6, D3c).
///
/// Two things make a grant used: a route that matched its statement, and
/// [`setup_section`] — the statements that run while a connection is being
/// established. The second is what makes a global group's
/// `SELECT set_config('TimeZone', …)` used: it only runs when a connection
/// opens, so no *request* can match it, and the connection-setup section is
/// precisely where the report already says the grant covers it. Reading the two
/// together is what keeps a correct manifest from being reported as one grant
/// too many.
fn used_statements(
    matched: &BTreeSet<(ProxyName, String)>,
    setup: &SetupSection,
) -> BTreeSet<(ProxyName, String)> {
    let mut used = matched.clone();
    used.extend(setup.accounted.iter().cloned());
    used
}

/// The `unused_global` section: what the global groups grant that nothing used
/// (§6, D3c).
///
/// A global group is attached to every route, so its statements are in every
/// route's effective grant. Reporting each one as unused once per route would
/// be noise a reader cannot act on; reporting them once, here, is the same
/// information: *no* request in the suite touched it, so the grant — or the
/// requests — can go.
///
/// `matched` is every `(proxy, statement)` the suite used, per
/// [`used_statements`]: a statement a route ran, **or** one
/// [`setup_section`] accounts for. The second is what a connection-setup group
/// needs: `SELECT set_config('TimeZone', …)` never runs inside a request, so no
/// route can match it, and a report that called the group that grants it dead
/// would be telling an operator to delete a grant the same report shows
/// `connection_setup` using.
///
/// * `statements` is every statement a global group grants that nothing used,
///   with the groups that grant it;
/// * `tables` is every `tables`/`functions` grant a global group hands out that
///   no observed statement needed: the group's table grant allowed nothing the
///   suite ran. A statement that only *half* used a group's tables does not
///   make it used in this sense — the whole grant is reported, because that is
///   the unit a manifest author edits.
///
/// A proxy on which a route was only seen in a *live server's* log is skipped
/// whole (D3c): that log records what was denied, never what ran, so no global
/// grant of it can be shown to be dead.
///
/// A statement a route also grants itself stays that route's business: it is
/// reported in the route's own `unused`, and here as well while no route
/// matched it, because the global group is what put it in every route's grant.
fn unused_global_section(
    global: &GlobalState,
    matched: &BTreeSet<(ProxyName, String)>,
    unverifiable: &BTreeSet<ProxyName>,
) -> Value {
    let mut statements: Vec<Value> = Vec::new();
    for (proxy, granted) in &global.queries {
        if unverifiable.contains(proxy) {
            continue;
        }
        for (sql, groups) in granted {
            if matched.contains(&(proxy.clone(), sql.clone())) {
                continue;
            }
            statements.push(json!({
                "proxy": proxy.as_str(),
                "sql": sql,
                "groups": groups,
            }));
        }
    }
    let mut tables: Vec<Value> = Vec::new();
    for grant in &global.tables {
        if grant.used || unverifiable.contains(&grant.proxy) {
            continue;
        }
        tables.push(json!({
            "proxy": grant.proxy.as_str(),
            "group": grant.group,
            "tables": grant.tables.tables(),
            "functions": grant.tables.functions().collect::<Vec<_>>(),
        }));
    }
    let count = statements.len() + tables.len();
    json!({
        "statements": statements,
        "tables": tables,
        "count": count,
    })
}

/// The statements a route is granted *without* the global groups: its own
/// inline grant and the grants of the groups it names.
///
/// This is the set whose unmatched entries are the route's own `unused` lines
/// (§6, D3c): a statement only a global group grants is not this route's to
/// call dead, because that group grants it to every route at once.
fn own_queries(manifest: &Manifest, index: usize, proxy: &ProxyName) -> BTreeSet<String> {
    let mut queries = BTreeSet::new();
    let Some(route) = manifest.route(index) else {
        return queries;
    };
    collect_queries(route.grant(proxy), &mut queries);
    for name in route.groups() {
        collect_queries(
            manifest.group(name).and_then(|group| group.grant(proxy)),
            &mut queries,
        );
    }
    queries
}

/// Adds a grant's queries, normalised, to a set.
fn collect_queries(grant: Option<&Grant>, into: &mut BTreeSet<String>) {
    let Some(pg) = grant.and_then(Grant::as_postgresql) else {
        return;
    };
    for query in pg.queries() {
        let normalised = normalize(query);
        if !normalised.is_empty() {
            into.insert(normalised);
        }
    }
}

/// The `connection_setup` section of the report: the statements psycopg runs as
/// a connection is established, and which routes do not grant them (§6).
///
/// Every request opens its own connection (§6, "What the app must do"), so in
/// production these statements run on every route that has a listener for that
/// proxy, once per request — while in tests the connection is often reused and
/// they run outside any request at all. They belong in a group attached to
/// every route (`groups = ["connection"]`), so the report names the routes that
/// are missing them rather than diffing them against one route.
///
/// A route with no grant on that proxy gets no listener and never runs them, so
/// it is not counted either way.
struct SetupSection {
    /// The section itself.
    value: Value,
    /// How many distinct statements it holds.
    statements: usize,
    /// How many of them at least one route does not grant.
    ungranted: usize,
    /// The `(proxy, statement)` pairs it accounts for, normalised.
    ///
    /// A statement that only runs while a connection opens is *used*: it is not
    /// a request's statement, so no route matches it, and a global group that
    /// grants it is doing exactly its job (`groups = ["connection"]`). The
    /// report says so by keeping it out of `unused_global` (D3c, §6).
    accounted: BTreeSet<(ProxyName, String)>,
}

/// Builds the `connection_setup` section.
fn setup_section(
    manifest: &Manifest,
    observed: &[ObservedSetup],
) -> Result<SetupSection, AuditError> {
    let mut traces: BTreeMap<(ProxyName, String), SetupTrace> = BTreeMap::new();
    for statement in observed {
        let proxy = resolve_proxy(manifest, statement.proxy.as_deref())?;
        let normalised = normalize(&statement.sql);
        if normalised.is_empty() {
            continue;
        }
        let trace = traces
            .entry((proxy, normalised))
            .or_insert_with(|| SetupTrace {
                wire_sql: statement.sql.clone(),
                ..SetupTrace::default()
            });
        trace.count += statement.count;
    }

    let mut ungranted_total = 0usize;
    let mut statements = Vec::with_capacity(traces.len());
    let mut accounted: BTreeSet<(ProxyName, String)> = BTreeSet::new();
    for ((proxy, sql), mut trace) in traces {
        accounted.insert((proxy.clone(), sql.clone()));
        for index in 0..manifest.route_count() {
            let (Some(route), true) = (
                manifest.route(index),
                manifest.effective_grants(index).contains_key(&proxy),
            ) else {
                continue;
            };
            let grant = grant_of(manifest, index, &proxy);
            // The wire text, not the normalised one: a table grant has to parse
            // it (D7).
            if Policy::of(grant.as_ref())
                .allow
                .check(&trace.wire_sql)
                .is_allowed()
            {
                trace.granted_in += 1;
            } else {
                trace.ungranted_in.push(route.name().to_string());
            }
        }
        if !trace.ungranted_in.is_empty() {
            ungranted_total += 1;
        }
        statements.push(json!({
            "sql": sql,
            "proxy": proxy.as_str(),
            "count": trace.count,
            "wire_sql": trace.wire_sql,
            "ungranted_in": trace.ungranted_in,
            "granted_in": trace.granted_in,
        }));
    }
    statements.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .cmp(&left["count"].as_u64())
            .then_with(|| left["sql"].as_str().cmp(&right["sql"].as_str()))
    });

    Ok(SetupSection {
        statements: statements.len(),
        value: json!({
            "statements": statements,
            "ungranted": ungranted_total,
        }),
        ungranted: ungranted_total,
        accounted,
    })
}

/// What one connection-setup statement did, and what the manifest says about it.
#[derive(Default)]
struct SetupTrace {
    /// How many times it ran over the session.
    count: u64,
    /// One raw occurrence, for a human who wants to see the placeholders.
    wire_sql: String,
    /// The routes that have a listener for that proxy and do not grant it.
    ungranted_in: Vec<String>,
    /// How many routes that have a listener for that proxy do grant it.
    granted_in: usize,
}

/// The `postgresql` proxies a route should be reported on: the ones it has
/// grants on, and the ones it was observed on.
fn proxy_names(
    manifest: &Manifest,
    route: usize,
    trace: Option<&RouteTrace>,
) -> BTreeSet<ProxyName> {
    let mut names: BTreeSet<ProxyName> = trace
        .map(|trace| {
            trace
                .proxies
                .keys()
                .map(|name| ProxyName::new(name.clone()))
                .collect()
        })
        .unwrap_or_default();
    names.extend(manifest.effective_grants(route).into_keys());
    names
}

/// The effective `postgresql` grant of one `(route, proxy)` pair, if any (§6).
fn grant_of(manifest: &Manifest, route: usize, proxy: &ProxyName) -> Option<PgGrant> {
    manifest
        .effective_grants(route)
        .get(proxy)
        .and_then(|grant| grant.as_postgresql())
        .cloned()
}

/// The proxy a statement belongs to, from the observations or the manifest.
fn resolve_proxy(manifest: &Manifest, named: Option<&str>) -> Result<ProxyName, AuditError> {
    match named {
        Some(name) => {
            let declared = manifest
                .proxy(name)
                .ok_or_else(|| AuditError::UnknownProxy(name.to_string()))?;
            if declared.protocol() != Protocol::Postgresql {
                return Err(AuditError::NotPostgresql(name.to_string()));
            }
            Ok(ProxyName::new(name))
        }
        None => {
            let postgres: Vec<&ProxyName> = manifest
                .proxies()
                .iter()
                .filter(|(_, proxy)| proxy.protocol() == Protocol::Postgresql)
                .map(|(name, _)| name)
                .collect();
            match postgres.as_slice() {
                [only] => Ok((*only).clone()),
                [] => Err(AuditError::NoProxy),
                many => Err(AuditError::AmbiguousProxy(
                    many.iter().map(|name| name.as_str().to_string()).collect(),
                )),
            }
        }
    }
}

/// The allowlist of one `(route, proxy)` pair, and the grants it holds.
struct Policy {
    /// What dbproxy would check a statement against.
    allow: Allowlist,
    /// The granted statements, normalised, in manifest order and without
    /// duplicates: the candidates for `unused`.
    granted: Vec<String>,
    /// Whether the grant allows transaction control. An `unrestricted = true`
    /// grant does, by allowing everything; `PgGrant::transaction_control()` is
    /// false for it, so the flag alone would say a route needs the flag it
    /// already has.
    transaction_control: bool,
}

impl Policy {
    /// Builds the policy of an effective grant (absent: a route with no grant
    /// for that proxy gets no listener, so nothing is allowed — §2.2).
    ///
    /// `unrestricted = true` (§6, D3a) is the other extreme: its allowlist
    /// allows everything, so nothing can be missing there and its (empty)
    /// granted set has nothing to call unused.
    fn of(grant: Option<&PgGrant>) -> Self {
        let Some(grant) = grant else {
            return Self {
                allow: Allowlist::default(),
                granted: Vec::new(),
                transaction_control: false,
            };
        };
        if grant.is_unrestricted() {
            return Self {
                allow: Allowlist::unrestricted(),
                granted: Vec::new(),
                transaction_control: true,
            };
        }
        let mut granted: Vec<String> = Vec::with_capacity(grant.queries().len());
        for query in grant.queries() {
            let query = normalize(query);
            if !query.is_empty() && !granted.contains(&query) {
                granted.push(query);
            }
        }
        Self {
            allow: Allowlist::new(grant.queries(), grant.transaction_control())
                .with_tables(grant.tables().clone()),
            granted,
            transaction_control: grant.transaction_control(),
        }
    }
}

/// Recognises the transaction-control statements of §5.2 through the public
/// policy API: an empty allowlist without `transaction_control` denies exactly
/// them, and the same allowlist with it allows them and nothing else.
struct TcProbe {
    without: Allowlist,
    with: Allowlist,
}

impl TcProbe {
    fn new() -> Self {
        Self {
            without: Allowlist::default(),
            with: Allowlist::new(std::iter::empty::<&str>(), true),
        }
    }

    /// Whether `sql` is a single transaction-control statement.
    fn is_transaction_control(&self, sql: &str) -> bool {
        !self.without.check(sql).is_allowed() && self.with.check(sql).is_allowed()
    }
}

/// What one route did in the observed requests.
#[derive(Default)]
struct RouteTrace {
    requests: usize,
    /// How many of those requests were recorded by this process — the suite
    /// itself — rather than read out of a live server's log (D3c).
    suite_requests: usize,
    proxies: BTreeMap<String, ProxyTrace>,
    egress: BTreeMap<(String, u16), EgressTrace>,
}

/// What one `(route, proxy)` pair did.
#[derive(Default)]
struct ProxyTrace {
    executions: usize,
    /// Statements that were executed and not granted, by normalised text.
    missing: BTreeMap<String, MissingTrace>,
    /// Normalised statements that matched a grant.
    matched: BTreeSet<String>,
    /// Whether a transaction-control statement, or a whole request that needs
    /// one, was observed.
    transaction_control: bool,
    /// The smallest table grant that covers every statement this pair ran
    /// (§6, D7): what the route would have to write to drop its exact queries.
    table_grants: TableGrants,
    /// The statements a table grant cannot describe at all, by normalised text,
    /// with the reason.
    unanalysable: BTreeMap<String, String>,
    /// The tables whose row lock this pair's statements could not name a column
    /// for, by the statement that locked them (D3c). A statement here is only
    /// reported when the *merged* grant cannot name one either — another
    /// statement of the route may read a column the lock can use.
    nameless_locks: BTreeMap<String, BTreeSet<String>>,
}

/// What one route did on the network (§7.3).
#[derive(Default)]
struct EgressTrace {
    /// `AF_INET`, `AF_INET6`… of the first attempt seen.
    family: String,
    /// How many times the route tried to reach it.
    count: usize,
    /// Up to [`EXAMPLES`] tests that made the attempt.
    tests: Vec<String>,
    seen_tests: BTreeSet<String>,
}

impl EgressTrace {
    /// Counts one attempt and keeps an example of who made it.
    fn record(&mut self, attempt: &ObservedEgress, nodeid: Option<&str>) {
        self.count += 1;
        if self.family.is_empty() {
            self.family = attempt.family.clone();
        }
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen_tests.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
    }
}

/// A set that is always empty, to borrow instead of allocating for an
/// observation that never happened.
static EMPTY_MATCHED: BTreeSet<String> = BTreeSet::new();

/// The `table_grants` section of one `(route, proxy)` pair (§6, D7).
///
/// It is the smallest table grant that covers every statement the pair was
/// observed to run, plus the TOML a manifest author pastes into
/// `[route.<proxy>.tables]` — the rewrite of a route's exact queries into table
/// grants. It is per pair, not per route, because a grant is: a route with two
/// `postgresql` proxies has two grants, and the plugin reports each where it
/// belongs.
///
/// `unanalysable` lists the statements a table grant cannot describe at all
/// (a `SET`, a `COPY`, a statement this build does not understand). They are
/// reported, never dropped: the covering grant is then not a complete
/// replacement for the route's exact queries.
fn table_grants_json(tables: &TableGrants, proxy: &str, unanalysable: Vec<Value>) -> Value {
    let mut section = serde_json::to_value(tables).unwrap_or_else(|_| json!({}));
    if let Some(map) = section.as_object_mut() {
        map.insert(
            "toml".to_string(),
            json!(tables.to_toml(&format!("route.{proxy}"))),
        );
        map.insert("unanalysable".to_string(), json!(unanalysable));
    }
    section
}

/// One statement executed but not granted.
#[derive(Default)]
struct MissingTrace {
    count: usize,
    wire_sql: String,
    tests: Vec<String>,
    paths: Vec<String>,
    seen_tests: BTreeSet<String>,
    seen_paths: BTreeSet<String>,
}

impl MissingTrace {
    /// Counts one execution and keeps the first examples of who ran it.
    fn record(&mut self, wire_sql: &str, nodeid: Option<&str>, path: &str) {
        self.count += 1;
        if self.wire_sql.is_empty() {
            self.wire_sql = wire_sql.to_string();
        }
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen_tests.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
        if self.paths.len() < EXAMPLES && self.seen_paths.insert(path.to_string()) {
            self.paths.push(path.to_string());
        }
    }
}

/// One `(method, path)` blastit would not have routed.
#[derive(Default)]
struct UnroutedTrace {
    method: String,
    path: String,
    count: usize,
    statements: usize,
    tests: Vec<String>,
    seen: BTreeSet<String>,
}

impl UnroutedTrace {
    fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            ..Self::default()
        }
    }

    /// Counts one request, its statements and the test that made it.
    fn record(&mut self, statements: &[ObservedStatement], nodeid: Option<&str>) {
        self.count += 1;
        self.statements += statements.len();
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
    }
}

/// One request as the plugin recorded it.
struct ObservedRequest {
    method: String,
    path_info: String,
    nodeid: Option<String>,
    transaction_control: bool,
    statements: Vec<ObservedStatement>,
    /// The outbound connections the request tried to open (§7.3).
    egress: Vec<ObservedEgress>,
    /// True when the request is known only from a *live server's* log (D3c):
    /// `"source": "live"` in the observations.
    ///
    /// A log records the requests that were served and the statements that were
    /// **denied** — never the statements that ran. So a route only such a log
    /// saw can be told what it is missing, and nothing about which of its grants
    /// matched, which is what the report refuses to guess at.
    live: bool,
}

/// One outbound connection attempt.
struct ObservedEgress {
    /// `AF_INET`, `AF_INET6`… as the socket named it.
    family: String,
    host: String,
    port: u16,
}

/// One statement, as it went on the wire.
struct ObservedStatement {
    /// The proxy it belongs to, when the plugin could tell.
    proxy: Option<String>,
    /// The wire SQL: `$n` placeholders, as psycopg sends it with server-side
    /// binding.
    sql: String,
}

impl ObservedEgress {
    /// What the attempt is keyed by: one destination, one line.
    fn key(&self) -> (String, u16) {
        (self.host.clone(), self.port)
    }
}

/// A statement psycopg itself ran: what goes on the wire as a connection is
/// established, before (or without) any Django cursor of a request.
struct ObservedSetup {
    /// The proxy it belongs to, when the plugin could tell.
    proxy: Option<String>,
    /// The wire SQL.
    sql: String,
    /// How many times it ran. Defaults to one.
    count: u64,
}

/// A statement the plugin could not read — a t-string `Template`, which has no
/// text before it is rendered with its values — counted rather than dropped.
struct ObservedUnrecognised {
    /// The alias it came from, when the plugin could tell.
    alias: Option<String>,
    /// What the plugin saw, as a Python repr, for a human to recognise.
    query: String,
    /// How many times it ran. Defaults to one.
    count: u64,
}

/// Everything one pytest session recorded: the requests, the statements psycopg
/// ran on its own to set a connection up, and what could not be read.
struct Observations {
    requests: Vec<ObservedRequest>,
    connection_setup: Vec<ObservedSetup>,
    unrecognised: Vec<ObservedUnrecognised>,
    /// What the plugin itself could not do — a connection that would inline its
    /// parameters, a worker report it did not understand. The report carries them
    /// so a reader of the JSON sees them too.
    warnings: Vec<String>,
}

/// Reads the observations document the plugin writes.
///
/// Unknown keys are ignored, so a plugin and an extension from different
/// versions still work together; the keys that carry meaning must be there and
/// must have the documented type.
fn parse_observations(source: &str) -> Result<Observations, AuditError> {
    let value: Value = serde_json::from_str(source)
        .map_err(|error| AuditError::Observations(format!("not valid JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| bad("the observations", "expected a JSON object"))?;
    if let Some(version) = object.get("version")
        && version.as_u64() != Some(VERSION)
    {
        return Err(bad(
            "version",
            &format!("expected {VERSION}, found {}", compact(version)),
        ));
    }
    let requests = object
        .get("requests")
        .ok_or_else(|| bad("the observations", "missing `requests`"))?
        .as_array()
        .ok_or_else(|| bad("requests", "expected an array"))?;

    let mut out = Vec::with_capacity(requests.len());
    for (index, request) in requests.iter().enumerate() {
        out.push(parse_request(request, index)?);
    }

    let setup = match object.get("connection_setup") {
        None | Some(Value::Null) => Vec::new(),
        Some(setup) => {
            let list = setup
                .as_array()
                .ok_or_else(|| bad("connection_setup", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, statement) in list.iter().enumerate() {
                out.push(parse_setup(statement, index)?);
            }
            out
        }
    };

    let unrecognised = match object.get("unrecognised") {
        None | Some(Value::Null) => Vec::new(),
        Some(entries) => {
            let list = entries
                .as_array()
                .ok_or_else(|| bad("unrecognised", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, entry) in list.iter().enumerate() {
                out.push(parse_unrecognised(entry, index)?);
            }
            out
        }
    };

    let warnings = match object.get("warnings") {
        None | Some(Value::Null) => Vec::new(),
        Some(entries) => {
            let list = entries
                .as_array()
                .ok_or_else(|| bad("warnings", "expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, entry) in list.iter().enumerate() {
                out.push(
                    entry
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad(&format!("warnings[{index}]"), "expected a string"))?,
                );
            }
            out
        }
    };

    Ok(Observations {
        requests: out,
        connection_setup: setup,
        unrecognised,
        warnings,
    })
}

/// Reads one `unrecognised[]` entry.
fn parse_unrecognised(value: &Value, index: usize) -> Result<ObservedUnrecognised, AuditError> {
    let at = format!("unrecognised[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let count = match object.get("count") {
        None | Some(Value::Null) => 1,
        Some(count) => count
            .as_u64()
            .ok_or_else(|| bad(&at, "count: expected a non-negative integer"))?,
    };
    Ok(ObservedUnrecognised {
        alias: optional_string(object, "alias", &at)?,
        query: required_string(object, "query", &at)?,
        count,
    })
}

/// Reads one `connection_setup[]` entry.
fn parse_setup(value: &Value, index: usize) -> Result<ObservedSetup, AuditError> {
    let at = format!("connection_setup[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let count = match object.get("count") {
        None | Some(Value::Null) => 1,
        Some(count) => count
            .as_u64()
            .ok_or_else(|| bad(&at, "count: expected a non-negative integer"))?,
    };
    Ok(ObservedSetup {
        proxy: optional_string(object, "proxy", &at)?,
        sql: required_string(object, "sql", &at)?,
        count,
    })
}

/// Reads one `requests[]` entry.
fn parse_request(value: &Value, index: usize) -> Result<ObservedRequest, AuditError> {
    let at = format!("requests[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let method = required_string(object, "method", &at)?;
    let path_info = required_string(object, "path_info", &at)?;
    let nodeid = optional_string(object, "nodeid", &at)?;
    let transaction_control = optional_bool(object, "transaction_control", &at)?;
    // Where the request is known from: a live server's log (`"live"`), or this
    // process's own Django (anything else, absence included).
    let live = optional_string(object, "source", &at)?.as_deref() == Some("live");
    let statements = match object.get("statements") {
        None => Vec::new(),
        Some(statements) => {
            let list = statements
                .as_array()
                .ok_or_else(|| bad(&at, "statements: expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, statement) in list.iter().enumerate() {
                let at = format!("{at}.statements[{index}]");
                let object = statement
                    .as_object()
                    .ok_or_else(|| bad(&at, "expected an object"))?;
                out.push(ObservedStatement {
                    proxy: optional_string(object, "proxy", &at)?,
                    sql: required_string(object, "sql", &at)?,
                });
            }
            out
        }
    };
    let egress = match object.get("egress") {
        None | Some(Value::Null) => Vec::new(),
        Some(attempts) => {
            let list = attempts
                .as_array()
                .ok_or_else(|| bad(&at, "egress: expected an array"))?;
            let mut out = Vec::with_capacity(list.len());
            for (index, attempt) in list.iter().enumerate() {
                out.push(parse_egress(attempt, index, &at)?);
            }
            out
        }
    };
    Ok(ObservedRequest {
        method,
        path_info,
        nodeid,
        transaction_control,
        statements,
        egress,
        live,
    })
}

/// Reads one `egress[]` entry of a request.
fn parse_egress(value: &Value, index: usize, at: &str) -> Result<ObservedEgress, AuditError> {
    let at = format!("{at}.egress[{index}]");
    let object = value
        .as_object()
        .ok_or_else(|| bad(&at, "expected an object"))?;
    let host = required_string(object, "host", &at)?;
    let port = match object.get("port") {
        None => return Err(bad(&at, "missing `port`")),
        Some(port) => port
            .as_u64()
            .filter(|port| *port <= u64::from(u16::MAX))
            .ok_or_else(|| bad(&at, "port: expected an integer between 0 and 65535"))?,
    };
    Ok(ObservedEgress {
        family: optional_string(object, "family", &at)?.unwrap_or_else(|| "AF_INET".to_string()),
        host,
        port: port as u16,
    })
}

/// A required string field.
fn required_string(object: &Map<String, Value>, key: &str, at: &str) -> Result<String, AuditError> {
    match object.get(key) {
        None => Err(bad(at, &format!("missing `{key}`"))),
        Some(value) => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| bad(at, &format!("{key}: expected a string"))),
    }
}

/// An optional string field; `null` and absence are both `None`.
fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    at: &str,
) -> Result<Option<String>, AuditError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| bad(at, &format!("{key}: expected a string or null"))),
    }
}

/// An optional boolean field, `false` when absent.
fn optional_bool(object: &Map<String, Value>, key: &str, at: &str) -> Result<bool, AuditError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| bad(at, &format!("{key}: expected a boolean"))),
    }
}

/// One observations problem, naming where it is.
fn bad(at: &str, what: &str) -> AuditError {
    AuditError::Observations(format!("{at}: {what}"))
}

/// Renders a JSON value on one line for an error message.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A manifest with a session group, a notes route that takes it, an
    /// unexercised route and a second proxy.
    const MANIFEST: &str = r#"
        version = 1
        groups = ["session"]

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [group.session.db]
        transaction_control = true
        queries = [
          'SELECT "django_session"."session_key" FROM "django_session" WHERE "django_session"."session_key" = $1',
        ]

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        methods = ["GET", "POST"]
        [route.db]
        queries = [
          'SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $1 LIMIT 21',
        ]

        [[route]]
        name = "checkout"
        path = ['/checkout/']
        [route.db]
        queries = ['INSERT INTO "orders" ("id") VALUES ($1)']
    "#;

    /// A manifest whose only proxy is `db`, without the session group.
    const SIMPLE: &str = r#"
        version = 1

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        [route.db]
        queries = ['SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $1 LIMIT 21']
    "#;

    /// Runs the audit on a report and returns the parsed JSON.
    fn run(manifest: &str, observations: &str) -> Value {
        let json = audit(manifest, observations).expect("the audit runs");
        serde_json::from_str(&json).expect("the report is JSON")
    }

    /// The observations document for one request.
    fn document(requests: &str) -> String {
        format!(r#"{{"version": 1, "requests": [{requests}]}}"#)
    }

    /// The observations document for a request that opened connections.
    fn document_with_egress(requests: &str, egress: &str) -> String {
        format!(
            r#"{{"version": 1, "requests": [{{"method": "GET", "path_info": "{requests}",
                "nodeid": "tests/test_app.py::test_it", "statements": [], "egress": [{egress}]}}]}}"#
        )
    }

    /// One observed request.
    fn request(method: &str, path: &str, statements: &str) -> String {
        format!(
            r#"{{"method": "{method}", "path_info": "{path}",
                "nodeid": "tests/test_app.py::test_it", "transaction_control": false,
                "statements": [{statements}]}}"#
        )
    }

    /// One observed statement.
    fn statement(sql: &str) -> String {
        format!(r#"{{"alias": "default", "proxy": "db", "sql": "{sql}"}}"#)
    }

    /// The route named `name` in a report.
    fn route<'a>(report: &'a Value, name: &str) -> &'a Value {
        report["routes"]
            .as_array()
            .expect("routes is a list")
            .iter()
            .find(|route| route["name"] == name)
            .expect("the route is in the report")
    }

    /// The `proxy` block of one route.
    fn proxy<'a>(report: &'a Value, name: &str) -> &'a Value {
        &route(report, name)["proxies"][0]
    }

    /// The SQL of every `missing` entry, in report order.
    fn missing_sql(report: &Value, name: &str) -> Vec<String> {
        proxy(report, name)["missing"]
            .as_array()
            .expect("missing is a list")
            .iter()
            .map(|entry| entry["sql"].as_str().expect("a string").to_string())
            .collect()
    }

    /// A row lock a table grant can name a column for, and one it cannot (D3c).
    #[test]
    fn a_locking_statement_gets_a_column_named_never_a_star() {
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
            [route.db]
            queries = ['SELECT 1']
        "#;
        let locked = run(
            manifest,
            &document(&request(
                "GET",
                "/notes/",
                &statement(
                    r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"id\" = $1 FOR UPDATE"#,
                ),
            )),
        );
        let grants = &proxy(&locked, "notes")["table_grants"];
        assert_eq!(grants["tables"]["notes"]["update"], json!(["id"]));
        assert!(
            !grants["toml"]
                .as_str()
                .expect("a string")
                .contains("update = \"*\""),
            "a lock never renders a star: {}",
            grants["toml"]
        );
        assert_eq!(grants["unanalysable"], json!([]));

        // `SELECT *` names no column to give the lock: the covering grant cannot
        // allow the statement, and the report says so instead of writing `"*"`.
        let star = run(
            manifest,
            &document(&request(
                "GET",
                "/notes/",
                &statement(r#"SELECT * FROM \"notes\" FOR UPDATE"#),
            )),
        );
        let grants = &proxy(&star, "notes")["table_grants"];
        assert_eq!(
            grants["tables"]["notes"]["update"],
            json!([]),
            "the lock is named by nothing: the entry grants no column"
        );
        assert!(
            !grants["toml"]
                .as_str()
                .expect("a string")
                .contains("update"),
            "and the rendered grant leaves it out: {}",
            grants["toml"]
        );
        assert_eq!(grants["unanalysable"].as_array().expect("a list").len(), 1);
        assert_eq!(
            grants["unanalysable"][0]["sql"],
            r#"SELECT * FROM "notes" FOR UPDATE"#
        );
        assert!(
            grants["unanalysable"][0]["reason"]
                .as_str()
                .expect("a string")
                .contains("primary key"),
            "{}",
            grants["unanalysable"][0]["reason"]
        );
        assert!(
            !grants["toml"]
                .as_str()
                .expect("a string")
                .contains("update = \"*\""),
            "{}",
            grants["toml"]
        );
    }

    #[test]
    fn a_lock_another_statement_reads_a_column_for_is_named_from_it() {
        // The pair's grant is merged statement by statement; the lock of one is
        // named from what the other reads, because PostgreSQL asks for the UPDATE
        // privilege on any single column of a locked table (D3c).
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
            [route.db]
            queries = ['SELECT 1']
        "#;
        let report = run(
            manifest,
            &document(&request(
                "GET",
                "/notes/",
                &format!(
                    "{}, {}",
                    statement(r#"SELECT 1 FROM \"notes\" FOR UPDATE"#),
                    statement(
                        r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"id\" = $1"#,
                    ),
                ),
            )),
        );
        let grants = &proxy(&report, "notes")["table_grants"];
        assert_eq!(grants["tables"]["notes"]["update"], json!(["id"]));
        assert_eq!(
            grants["unanalysable"],
            json!([]),
            "the lock is named after all"
        );
    }

    #[test]
    fn a_granted_statement_is_neither_missing_nor_unused() {
        let report = run(
            SIMPLE,
            &document(&request(
                "GET",
                "/notes/",
                &statement(
                    r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21"#,
                ),
            )),
        );
        assert_eq!(report["totals"]["missing"], 0);
        assert_eq!(report["totals"]["unused"], 0);
        assert_eq!(report["totals"]["clean"], true);
        assert_eq!(proxy(&report, "notes")["observed"], 1);
        assert_eq!(route(&report, "notes")["requests"], 1);
        assert_eq!(missing_sql(&report, "notes").len(), 0);
    }

    #[test]
    fn reformatting_a_granted_statement_does_not_make_it_missing() {
        // Same statement, laid out differently and with a trailing separator:
        // normalisation is what the comparison reads (§6).
        let report = run(
            SIMPLE,
            &document(&request(
                "GET",
                "/notes/",
                &statement(
                    "SELECT   \\\"notes\\\".\\\"id\\\"\\n  FROM \\\"notes\\\" WHERE \\\"notes\\\".\\\"owner_id\\\" = $1 LIMIT 21;",
                ),
            )),
        );
        assert_eq!(report["totals"]["missing"], 0);
        assert_eq!(report["totals"]["unused"], 0);
    }

    #[test]
    fn a_statement_executed_but_not_granted_is_reported_with_its_context() {
        let report = run(
            SIMPLE,
            &document(&request(
                "POST",
                "/notes/3/delete/",
                &format!(
                    "{}, {}",
                    statement(
                        r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21"#,
                    ),
                    statement(r#"DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1"#),
                ),
            )),
        );
        assert_eq!(report["totals"]["missing"], 1);
        assert_eq!(report["totals"]["missing_distinct"], 1);
        assert_eq!(report["totals"]["clean"], false);
        let missing = &proxy(&report, "notes")["missing"][0];
        assert_eq!(
            missing["sql"],
            r#"DELETE FROM "notes" WHERE "notes"."id" = $?"#
        );
        assert_eq!(missing["count"], 1);
        assert_eq!(
            missing["wire_sql"],
            r#"DELETE FROM \"notes\" WHERE \"notes\".\"id\" = $1"#.replace("\\\"", "\"")
        );
        assert_eq!(
            missing["tests"][0], "tests/test_app.py::test_it",
            "the test that ran it is named"
        );
        assert_eq!(missing["paths"][0], "/notes/3/delete/");
        // The granted statement was used, so nothing is unused.
        assert_eq!(
            proxy(&report, "notes")["unused"].as_array().unwrap().len(),
            0
        );
    }

    #[test]
    fn the_normalised_sql_is_what_a_grant_wants() {
        // The report hands over the normalised text, which is what the manifest
        // comparison reads: pasting it into a grant must make it match.
        let report = run(
            SIMPLE,
            &document(&request(
                "GET",
                "/notes/",
                &statement("DELETE   FROM \\\"notes\\\"  WHERE  id = $1 ;"),
            )),
        );
        assert_eq!(
            missing_sql(&report, "notes"),
            ["DELETE FROM \"notes\" WHERE id = $?"]
        );
        let normalised = missing_sql(&report, "notes")[0].clone();
        let repaired = SIMPLE.replace(
            "'SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21'",
            &format!("'{normalised}'"),
        );
        let after = run(
            &repaired,
            &document(&request(
                "GET",
                "/notes/",
                &statement("DELETE   FROM \\\"notes\\\"  WHERE  id = $1 ;"),
            )),
        );
        assert_eq!(
            after["totals"]["missing"], 0,
            "the normalised text pasted into a grant matches the statement"
        );
    }

    #[test]
    fn every_execution_of_a_missing_statement_counts() {
        let statements = [
            statement(r#"DELETE FROM \"notes\" WHERE id = $1"#),
            statement("SELECT 1"),
        ];
        let report = run(
            SIMPLE,
            &document(&format!(
                "{}, {}, {}",
                request("GET", "/notes/", &statements.join(", ")),
                request("GET", "/notes/", &statements[0]),
                request("GET", "/notes/", &statements[0]),
            )),
        );
        assert_eq!(report["totals"]["requests"], 3);
        assert_eq!(
            report["totals"]["missing"], 4,
            "two here, then one, then one"
        );
        assert_eq!(report["totals"]["missing_distinct"], 2);
        // Sorted by how often they ran, so the biggest hole comes first.
        assert_eq!(
            missing_sql(&report, "notes"),
            [r#"DELETE FROM "notes" WHERE id = $?"#, "SELECT 1"]
        );
        let counts: Vec<u64> = proxy(&report, "notes")["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["count"].as_u64().unwrap())
            .collect();
        assert_eq!(counts, [3, 1]);
    }

    #[test]
    fn grants_that_nothing_matched_are_unused() {
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
            [route.db]
            queries = [
              'SELECT 1',
              'SELECT 2 ;',
              'SELECT 2',
            ]
        "#;
        let report = run(
            manifest,
            &document(&request("GET", "/notes/", &statement("SELECT 1;"))),
        );
        // `SELECT 2` and `SELECT 2 ;` are the same grant: it is listed once.
        assert_eq!(proxy(&report, "notes")["unused"], json!(["SELECT 2"]));
        assert_eq!(report["totals"]["unused"], 1);
        assert_eq!(
            report["totals"]["clean"], true,
            "unused grants do not fail strict"
        );
    }

    #[test]
    fn a_route_that_ran_but_never_touched_a_proxy_lists_all_its_grants_as_unused() {
        let report = run(SIMPLE, &document(&request("GET", "/notes/", "")));
        assert_eq!(route(&report, "notes")["requests"], 1);
        assert_eq!(proxy(&report, "notes")["observed"], 0);
        assert_eq!(
            proxy(&report, "notes")["unused"],
            json!([r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21"#])
        );
    }

    #[test]
    fn a_route_with_no_grant_denies_everything() {
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
            [route.db]
            queries = ["SELECT 1"]
            [[route]]
            name = "about"
            path = ['/about/.*']
        "#;
        let report = run(
            manifest,
            &document(&request("GET", "/about/x", &statement("SELECT 1"))),
        );
        assert_eq!(missing_sql(&report, "about"), ["SELECT 1"]);
        assert_eq!(route(&report, "about")["proxies"][0]["name"], "db");
        assert_eq!(
            report["unexercised_routes"],
            json!(["notes"]),
            "the route with a grant was never requested"
        );
        assert_eq!(report["totals"]["unused"], 0);
    }

    #[test]
    fn an_unrouted_request_is_reported_with_its_test() {
        let report = run(
            SIMPLE,
            &document(&format!(
                "{}, {}",
                request("GET", "/oops/", &statement("SELECT 1")),
                request("GET", "/oops/", ""),
            )),
        );
        let unrouted = &report["unrouted"][0];
        assert_eq!(unrouted["method"], "GET");
        assert_eq!(unrouted["path_info"], "/oops/");
        assert_eq!(unrouted["count"], 2);
        assert_eq!(unrouted["statements"], 1);
        assert_eq!(unrouted["tests"][0], "tests/test_app.py::test_it");
        assert_eq!(report["totals"]["unrouted"], 2);
        assert_eq!(report["totals"]["routed"], 0);
        assert_eq!(report["totals"]["clean"], false);
    }

    #[test]
    fn a_savepoint_needs_transaction_control() {
        // A nested `atomic` inside a test is a savepoint, and a nested atomic is
        // what an `ATOMIC_REQUESTS` request looks like from inside pytest-django.
        let denying = run(
            SIMPLE,
            &document(&request("POST", "/notes/", &statement("SAVEPOINT s1"))),
        );
        assert_eq!(missing_sql(&denying, "notes"), ["SAVEPOINT s1"]);
        assert_eq!(route(&denying, "notes")["needs_transaction_control"], true);
        assert_eq!(
            route(&denying, "notes")["proxies"][0]["transaction_control"]["needed"],
            true
        );
        assert_eq!(
            denying["totals"]["needs_transaction_control"],
            json!(["notes"])
        );

        let granted = run(
            MANIFEST,
            &document(&request("POST", "/notes/", &statement("SAVEPOINT s1"))),
        );
        // The session group grants transaction control to every route.
        assert_eq!(missing_sql(&granted, "notes").len(), 0);
        assert_eq!(route(&granted, "notes")["needs_transaction_control"], false);
        assert_eq!(
            route(&granted, "notes")["proxies"][0]["transaction_control"]["granted"],
            true
        );
        assert_eq!(granted["totals"]["clean"], true);
    }

    #[test]
    fn a_request_that_needs_a_transaction_says_so_without_a_statement() {
        // `ATOMIC_REQUESTS` on the alias: the transaction is there even when the
        // test's own atomic swallows the `BEGIN`.
        let observations = format!(
            r#"{{"version": 1, "requests": [{{"method": "POST", "path_info": "/notes/",
                "nodeid": "tests/test_app.py::test_it", "transaction_control": true,
                "statements": [{}]}}]}}"#,
            statement("SELECT 1")
        );
        let report = run(SIMPLE, &observations);
        assert_eq!(route(&report, "notes")["needs_transaction_control"], true);
        assert_eq!(
            report["totals"]["needs_transaction_control"],
            json!(["notes"])
        );
        assert_eq!(report["totals"]["clean"], false);
    }

    #[test]
    fn a_route_no_request_reached_is_named_and_reports_no_proxy() {
        let report = run(MANIFEST, &document(&request("GET", "/notes/", "")));
        assert_eq!(report["unexercised_routes"], json!(["checkout"]));
        assert_eq!(route(&report, "checkout")["requests"], 0);
        assert_eq!(route(&report, "checkout")["proxies"], json!([]));
        assert_eq!(report["totals"]["routes"], 2);
        assert_eq!(report["totals"]["routes_exercised"], 1);
        // `notes` ran without touching the database, so its own grant and the
        // group it names are unused; the global group's statement is not a
        // route's unused line (D3c), and `checkout` did not run at all, so it
        // claims nothing.
        assert_eq!(report["totals"]["unused"], 1);
        assert_eq!(report["totals"]["unused_global"], 1);
    }

    #[test]
    fn a_method_a_route_does_not_answer_is_unrouted() {
        let report = run(MANIFEST, &document(&request("GET", "/checkout/", "")));
        // `/checkout/` answers no method but POST? It lists none, so GET is
        // routed there; the `notes` route only answers GET and POST.
        assert_eq!(report["unrouted"].as_array().unwrap().len(), 0);
        let report = run(MANIFEST, &document(&request("DELETE", "/notes/", "")));
        assert_eq!(report["unrouted"][0]["method"], "DELETE");
        assert_eq!(report["unrouted"][0]["path_info"], "/notes/");
    }

    #[test]
    fn catches_all_routing_is_the_manifests_own() {
        // First match wins, in manifest order.
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "fallback"
            path = ['.*']
            [[route]]
            name = "notes"
            path = ['/notes/.*']
        "#;
        let report = run(
            manifest,
            &document(&request("GET", "/notes/", &statement("SELECT 1"))),
        );
        assert_eq!(report["unrouted"].as_array().unwrap().len(), 0);
        assert_eq!(report["unexercised_routes"], json!(["notes"]));
        // The catch-all has no grant, so the statement is missing there.
        assert_eq!(
            route(&report, "fallback")["proxies"][0]["missing"][0]["sql"],
            "SELECT 1"
        );
    }

    #[test]
    fn the_only_postgresql_proxy_is_inferred() {
        let observations = document(&request(
            "GET",
            "/notes/",
            r#"{"alias": "replica", "sql": "SELECT 1"}"#,
        ));
        let report = run(SIMPLE, &observations);
        assert_eq!(route(&report, "notes")["proxies"][0]["name"], "db");
        assert_eq!(missing_sql(&report, "notes"), ["SELECT 1"]);
    }

    #[test]
    fn an_undeclared_proxy_is_an_error() {
        let observations = document(&request(
            "GET",
            "/notes/",
            r#"{"proxy": "nope", "sql": "SELECT 1"}"#,
        ));
        let error = audit(SIMPLE, &observations).expect_err("unknown proxy");
        assert!(matches!(error, AuditError::UnknownProxy(ref name) if name == "nope"));
        assert!(error.to_string().contains("nope"), "{error}");
    }

    #[test]
    fn several_proxies_without_a_statement_proxy_is_an_error() {
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [proxy.warehouse]
            protocol = "postgresql"
            input = { url = "WAREHOUSE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
            [route.db]
            queries = ["SELECT 1"]
        "#;
        let observations = document(&request(
            "GET",
            "/notes/",
            r#"{"alias": "default", "sql": "SELECT 1"}"#,
        ));
        let error = audit(manifest, &observations).expect_err("ambiguous");
        assert!(
            matches!(error, AuditError::AmbiguousProxy(ref names) if names == &["db", "warehouse"])
        );
        // Naming the proxy makes it work, on either one.
        let observations = document(&request(
            "GET",
            "/notes/",
            r#"{"alias": "default", "proxy": "warehouse", "sql": "SELECT 1"}"#,
        ));
        let report = run(manifest, &observations);
        let proxies = route(&report, "notes")["proxies"].as_array().unwrap();
        assert_eq!(
            proxies.len(),
            2,
            "the declared grant and the observed proxy"
        );
        assert_eq!(proxies[0]["name"], "db");
        assert_eq!(proxies[0]["observed"], 0);
        assert_eq!(proxies[0]["unused"], json!(["SELECT 1"]));
        assert_eq!(proxies[1]["name"], "warehouse");
        assert_eq!(proxies[1]["missing"][0]["sql"], "SELECT 1");
    }

    #[test]
    fn a_manifest_without_a_postgresql_proxy_is_an_error() {
        let manifest = r#"
            version = 1
            [proxy.db]
            protocol = "postgresql"
            input = { url = "DATABASE_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
        "#;
        let observation = document(&request(
            "GET",
            "/notes/",
            r#"{"alias": "default", "sql": "SELECT 1"}"#,
        ));
        // ... a route with no grant still knows the proxy: the manifest has one.
        assert_eq!(run(manifest, &observation)["totals"]["missing"], 1);

        let no_proxy = r#"
            version = 1
            [[route]]
            name = "notes"
            path = ['/notes/.*']
        "#;
        let error = audit(no_proxy, &observation).expect_err("no proxy");
        assert!(matches!(error, AuditError::NoProxy));
    }

    #[test]
    fn a_non_postgresql_proxy_cannot_be_used() {
        // Nothing but `postgresql` exists today, so the manifest itself rejects
        // the declaration; the audit reports it as a manifest error.
        let manifest = r#"
            version = 1
            [proxy.mq]
            protocol = "amqp"
            input = { url = "AMQP_URL" }
            [[route]]
            name = "notes"
            path = ['/notes/.*']
        "#;
        let error = audit(manifest, &document("")).expect_err("unknown protocol");
        assert!(matches!(error, AuditError::Manifest(_)), "{error}");
    }

    #[test]
    fn a_broken_manifest_is_an_error() {
        for manifest in [
            "version = 2\n[[route]]\nname = \"a\"\npath = ['/']\n",
            "version = 1\n[[route]]\nname = \"a\"\npath = ['(']\n",
            "version = 1\n[[route]]\nname = \"a\"\npath = ['/']\n[route.db]\nqueries = ['SELECT 1']\n",
        ] {
            let error = audit(manifest, &document("")).expect_err("invalid manifest");
            assert!(matches!(error, AuditError::Manifest(_)), "{error}");
        }
    }

    #[test]
    fn broken_observations_are_errors_that_name_where() {
        let cases = [
            ("", "not valid JSON"),
            ("[]", "expected a JSON object"),
            (r#"{"version": 2, "requests": []}"#, "expected 1"),
            (r#"{"version": 1}"#, "missing `requests`"),
            (r#"{"version": 1, "requests": {}}"#, "expected an array"),
            (
                r#"{"version": 1, "requests": [1]}"#,
                "requests[0]: expected an object",
            ),
            (
                r#"{"version": 1, "requests": [{"path_info": "/"}]}"#,
                "requests[0]: missing `method`",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET"}]}"#,
                "requests[0]: missing `path_info`",
            ),
            (
                r#"{"version": 1, "requests": [{"method": 1, "path_info": "/"}]}"#,
                "requests[0]: method: expected a string",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "nodeid": 3}]}"#,
                "nodeid: expected a string or null",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "transaction_control": "yes"}]}"#,
                "transaction_control: expected a boolean",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": {}}]}"#,
                "statements: expected an array",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": [{"sql": 1}]}]}"#,
                "requests[0].statements[0]: sql: expected a string",
            ),
            (
                r#"{"version": 1, "requests": [{"method": "GET", "path_info": "/", "statements": [{}]}]}"#,
                "statements[0]: missing `sql`",
            ),
            (
                r#"{"version": 1, "requests": [], "connection_setup": {}}"#,
                "connection_setup: expected an array",
            ),
            (
                r#"{"version": 1, "requests": [], "connection_setup": [1]}"#,
                "connection_setup[0]: expected an object",
            ),
            (
                r#"{"version": 1, "requests": [], "connection_setup": [{}]}"#,
                "connection_setup[0]: missing `sql`",
            ),
            (
                r#"{"version": 1, "requests": [], "connection_setup": [{"sql": "SELECT 1", "count": "3"}]}"#,
                "connection_setup[0]: count: expected a non-negative integer",
            ),
            (
                r#"{"version": 1, "requests": [], "connection_setup": [{"sql": 1}]}"#,
                "connection_setup[0]: sql: expected a string",
            ),
        ];
        for (source, expected) in cases {
            let error = audit(SIMPLE, source).expect_err("invalid observations");
            let message = error.to_string();
            assert!(
                message.contains(expected),
                "{source}: expected {expected:?} in {message:?}"
            );
        }
    }

    #[test]
    fn unknown_keys_are_ignored_so_versions_can_differ() {
        let source = r#"{"version": 1, "plugin": "0.2", "requests": [
            {"method": "GET", "path_info": "/notes/", "extra": true, "statements": [
                {"alias": "default", "proxy": "db", "sql": "SELECT 1", "how_long": 3}
            ]}
        ]}"#;
        assert_eq!(run(SIMPLE, source)["totals"]["missing"], 1);
    }

    #[test]
    fn examples_are_deduplicated_and_capped_at_five() {
        let mut requests = Vec::new();
        for index in 0..9 {
            requests.push(format!(
                r#"{{"method": "GET", "path_info": "/notes/{index}/",
                    "nodeid": "tests/test_app.py::test_{index}",
                    "statements": [{}]}}"#,
                statement("SELECT 1")
            ));
        }
        // The same test and path twice, to check they are not listed twice.
        requests.push(requests[0].clone());
        let report = run(SIMPLE, &document(&requests.join(",")));
        let missing = &proxy(&report, "notes")["missing"][0];
        assert_eq!(missing["count"], 10);
        let tests: Vec<&str> = missing["tests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|test| test.as_str().unwrap())
            .collect();
        assert_eq!(tests.len(), 5);
        assert_eq!(tests[0], "tests/test_app.py::test_0");
        assert_eq!(tests[4], "tests/test_app.py::test_4");
        let paths: Vec<&str> = missing["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "/notes/0/",
                "/notes/1/",
                "/notes/2/",
                "/notes/3/",
                "/notes/4/"
            ]
        );
    }

    #[test]
    fn a_request_outside_a_test_is_labelled() {
        let report = run(
            SIMPLE,
            &document(
                r#"{"method": "GET", "path_info": "/notes/", "statements": [{"sql": "SELECT 1"}]}"#,
            ),
        );
        assert_eq!(proxy(&report, "notes")["missing"][0]["tests"][0], NO_TEST);
    }

    #[test]
    fn an_empty_session_reports_every_route_as_unexercised() {
        let report = run(MANIFEST, &document(""));
        assert_eq!(report["unexercised_routes"], json!(["notes", "checkout"]));
        assert_eq!(report["totals"]["requests"], 0);
        assert_eq!(report["totals"]["routes_exercised"], 0);
        assert_eq!(report["totals"]["missing"], 0);
        assert_eq!(report["totals"]["unused"], 0);
        assert_eq!(report["totals"]["clean"], true);
        assert_eq!(report["unrouted"], json!([]));
    }

    #[test]
    fn the_report_names_the_manifests_proxies_and_warnings() {
        let manifest = format!("{MANIFEST}\n[group.orphan.db]\nqueries = ['SELECT 1']\n");
        let report = run(&manifest, &document(""));
        assert_eq!(report["manifest"]["proxies"], json!({"db": "postgresql"}));
        assert_eq!(report["manifest"]["routes"], 2);
        assert_eq!(report["version"], 1);
        let warnings = report["manifest"]["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1, "one unattached group: {warnings:?}");
        assert!(
            warnings[0].as_str().unwrap().contains("orphan"),
            "{warnings:?}"
        );
    }

    #[test]
    fn two_aliases_on_one_proxy_are_merged() {
        // Two database aliases mapped to the same proxy are one allowlist: the
        // report is per (route, proxy), not per alias.
        let observations = document(&request(
            "GET",
            "/notes/",
            &format!(
                "{}, {}",
                r#"{"alias": "default", "proxy": "db", "sql": "SELECT 1"}"#,
                r#"{"alias": "replica", "proxy": "db", "sql": "SELECT 1"}"#
            ),
        ));
        let report = run(SIMPLE, &observations);
        assert_eq!(route(&report, "notes")["proxies"][0]["observed"], 2);
        assert_eq!(proxy(&report, "notes")["missing"][0]["count"], 2);
        assert_eq!(route(&report, "notes")["requests"], 1);
    }

    #[test]
    fn the_report_is_stable() {
        let observations = document(&format!(
            "{}, {}",
            request("GET", "/notes/", &statement("SELECT 1")),
            request("GET", "/oops/", "")
        ));
        let first = audit(MANIFEST, &observations).expect("runs");
        let second = audit(MANIFEST, &observations).expect("runs");
        assert_eq!(first, second, "the same observations give the same report");
        assert!(first.ends_with('}'));
    }

    #[test]
    fn normalisation_is_the_policys_own() {
        assert_eq!(normalize_sql("  SELECT  1 ;  "), "SELECT 1");
        assert_eq!(normalize_sql("SELECT 'a  b'"), "SELECT 'a  b'");
        assert_eq!(normalize_sql("SELECT 1 -- c\nFROM t"), "SELECT 1 FROM t");
        for sql in ["SELECT 1 ;", "SELECT 'a  b'", "", ";", "SELECT $1"] {
            let once = normalize_sql(sql);
            assert_eq!(normalize_sql(&once), once, "idempotent for {sql:?}");
        }
    }

    #[test]
    fn odd_inputs_never_panic() {
        let observations = document(&format!(
            "{}, {}",
            request("GET", "/notes/", &statement("")),
            r#"{"method": "", "path_info": "", "nodeid": "", "statements": [{"sql": "🦀"}]}"#
        ));
        let report = run(SIMPLE, &observations);
        assert_eq!(report["totals"]["routed"], 1);
        let sqls = missing_sql(&report, "notes");
        assert!(
            sqls.contains(&String::new()),
            "an empty statement is denied, never matched: {sqls:?}"
        );
    }

    #[test]
    fn transaction_control_is_recognised_by_the_policy() {
        let probe = TcProbe::new();
        for sql in [
            "BEGIN",
            "commit ;",
            "ROLLBACK;",
            "SAVEPOINT s1",
            "SAVEPOINT \"s1_x\"",
            "RELEASE SAVEPOINT s1",
            "ROLLBACK TO SAVEPOINT s1",
            "BEGIN; -- a comment",
        ] {
            assert!(
                probe.is_transaction_control(&normalize(sql)),
                "{sql:?} is transaction control"
            );
        }
        for sql in [
            "SELECT 1",
            "BEGIN; DROP TABLE t",
            "SAVEPOINT",
            "SAVEPOINT s1; SELECT 1",
            "",
            "SET TIME ZONE 'UTC'",
        ] {
            assert!(
                !probe.is_transaction_control(&normalize(sql)),
                "{sql:?} is not transaction control"
            );
        }
    }

    #[test]
    fn a_query_followed_by_a_second_statement_never_matches() {
        // The whole normalised text is compared, so a listed query with a second
        // statement appended is a different text (§6).
        let report = run(
            SIMPLE,
            &document(&request(
                "GET",
                "/notes/",
                &statement(
                    r#"SELECT \"notes\".\"id\" FROM \"notes\" WHERE \"notes\".\"owner_id\" = $1 LIMIT 21; DROP TABLE \"notes\""#,
                ),
            )),
        );
        assert_eq!(
            missing_sql(&report, "notes"),
            [
                r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21; DROP TABLE "notes""#
            ]
        );
    }

    /// A one-route manifest whose grant holds exactly `queries`, as TOML basic
    /// strings (the escaping is JSON's).
    fn manifest_with_queries(queries: &[String]) -> String {
        let mut out = String::from(
            "version = 1\n\
             [proxy.db]\n\
             protocol = \"postgresql\"\n\
             input = { url = \"DATABASE_URL\" }\n\
             [[route]]\n\
             name = \"notes\"\n\
             path = ['/notes/.*']\n\
             methods = [\"GET\", \"POST\"]\n\
             [route.db]\n\
             queries = [\n",
        );
        for query in queries {
            out.push_str("  ");
            out.push_str(&serde_json::to_string(query).expect("a string"));
            out.push_str(",\n");
        }
        out.push_str("]\n");
        out
    }

    /// One observed statement whose SQL is escaped for the JSON document, for a
    /// test that would rather not escape it by hand.
    fn json_statement(sql: &str) -> String {
        format!(
            r#"{{"alias": "default", "proxy": "db", "sql": {}}}"#,
            serde_json::to_string(sql).expect("a string")
        )
    }

    #[test]
    fn a_list_of_placeholders_after_in_is_one_grant_of_any_length() {
        // D3a: `IN ($1)`, `IN ($1, $2)`, `IN ($1, $2, $3)`… are one shape, so a
        // prefix or a `prefetch_related` of ten objects needs no grant of its
        // own — which is exactly the statement an `__in` lookup produces.
        let one = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" IN ($1)"#;
        let three = r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" IN ($1, $2, $3)"#;
        let report = run(
            &manifest_with_queries(&[one.to_string()]),
            &document(&format!(
                "{}, {}",
                request("GET", "/notes/", &json_statement(one)),
                request("GET", "/notes/", &json_statement(three)),
            )),
        );
        assert_eq!(
            report["totals"]["missing"], 0,
            "one grant covers both list lengths"
        );
        assert_eq!(report["totals"]["unused"], 0, "and the grant was used");
        assert_eq!(report["totals"]["statements"], 2);
    }

    #[test]
    fn a_run_of_values_rows_is_one_grant_and_a_single_row_is_another() {
        // D3a again, for `bulk_create`: the run of identical rows collapses, so
        // one grant fits three rows, thirty rows, or none of them twice. A
        // single row is left alone on purpose, and is therefore a shape of its
        // own — one grant never covers both.
        let three = r#"INSERT INTO "notes" ("owner_id", "body") VALUES ($1, $2), ($3, $4), ($5, $6) RETURNING "notes"."id""#;
        let one =
            r#"INSERT INTO "notes" ("owner_id", "body") VALUES ($1, $2) RETURNING "notes"."id""#;

        let bare = run(
            &manifest_with_queries(&[]),
            &document(&request("POST", "/notes/", &json_statement(three))),
        );
        let shape = missing_sql(&bare, "notes")[0].clone();
        assert!(
            shape.contains(", ..."),
            "the run of rows collapses: {shape}"
        );

        let manifest = manifest_with_queries(std::slice::from_ref(&shape));
        let many = run(
            &manifest,
            &document(&request("POST", "/notes/", &json_statement(three))),
        );
        assert_eq!(
            many["totals"]["missing"], 0,
            "one grant, any number of rows"
        );

        let single = run(
            &manifest,
            &document(&request("POST", "/notes/", &json_statement(one))),
        );
        assert_eq!(missing_sql(&single, "notes"), [normalize_sql(one)]);
        assert_ne!(normalize_sql(one), shape, "the two shapes stay distinct");

        // ... and granting both shapes makes both requests clean: the report is
        // what tells a human there are two of them.
        let both = run(
            &manifest_with_queries(&[shape, normalize_sql(one)]),
            &document(&format!(
                "{}, {}",
                request("POST", "/notes/", &json_statement(one)),
                request("POST", "/notes/", &json_statement(three)),
            )),
        );
        assert_eq!(both["totals"]["missing"], 0);
        assert_eq!(both["totals"]["unused"], 0, "each grant matched its shape");
    }

    /// A manifest with a group attached to one route, one route that grants
    /// everything, one that grants something else, and one with no listener.
    const SETUP: &str = r#"
        version = 1

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [group.connection.db]
        queries = ["SET application_name = 'app'"]

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        groups = ["connection"]

        [[route]]
        name = "checkout"
        path = ['/checkout/']
        [route.db]
        unrestricted = true

        [[route]]
        name = "about"
        path = ['/about/']
        [route.db]
        queries = ['SELECT "cms_page"."body" FROM "cms_page" LIMIT 21']

        [[route]]
        name = "static"
        path = ['/static/.*']
    "#;

    /// One `connection_setup` entry.
    fn setup(sql: &str, count: u64) -> String {
        format!(
            r#"{{"alias": "default", "proxy": "db", "sql": {}, "count": {count}}}"#,
            serde_json::to_string(sql).expect("a string")
        )
    }

    /// A document with only connection-setup statements.
    fn setup_document(statements: &str) -> String {
        format!(r#"{{"version": 1, "requests": [], "connection_setup": [{statements}]}}"#)
    }

    #[test]
    fn connection_setup_names_the_routes_that_do_not_grant_it() {
        // No request at all: these statements are not a route's, they run when a
        // connection is established — which, under blastit, every request does
        // afresh (§6, "What the app must do").
        let report = run(
            SETUP,
            &setup_document(&setup("SET application_name = 'app'", 2)),
        );
        let entry = &report["connection_setup"]["statements"][0];
        assert_eq!(entry["sql"], "SET application_name = 'app'");
        assert_eq!(entry["wire_sql"], "SET application_name = 'app'");
        assert_eq!(entry["proxy"], "db");
        assert_eq!(entry["count"], 2);
        assert_eq!(
            entry["ungranted_in"],
            json!(["about"]),
            "the global group covers notes, `unrestricted` covers checkout, \
             and the route with no grant has no listener to run it on"
        );
        assert_eq!(entry["granted_in"], 2);
        assert_eq!(report["connection_setup"]["ungranted"], 1);
        assert_eq!(report["totals"]["connection_setup"], 1);
        assert_eq!(report["totals"]["connection_setup_ungranted"], 1);
        assert_eq!(report["totals"]["clean"], false);
    }

    #[test]
    fn connection_setup_granted_everywhere_is_clean() {
        let report = run(
            &manifest_with_queries(&["SET application_name = 'app'".to_string()]),
            &setup_document(&setup("SET application_name = 'app'", 1)),
        );
        assert_eq!(
            report["connection_setup"],
            json!({"statements": [{
                "sql": "SET application_name = 'app'",
                "proxy": "db",
                "count": 1,
                "wire_sql": "SET application_name = 'app'",
                "ungranted_in": [],
                "granted_in": 1,
            }], "ungranted": 0})
        );
        assert_eq!(report["totals"]["clean"], true);
        assert_eq!(report["totals"]["connection_setup_ungranted"], 0);
    }

    #[test]
    fn a_route_with_no_listener_is_not_counted_in_connection_setup() {
        // `static` has no `[route.db]` grant, so it gets no listener at all: it
        // can never run one of these, and naming it would be noise.
        let report = run(SETUP, &setup_document(&setup("SELECT 1", 1)));
        let entry = &report["connection_setup"]["statements"][0];
        assert_eq!(entry["ungranted_in"], json!(["notes", "about"]));
        assert_eq!(
            entry["granted_in"], 1,
            "only the unrestricted route allows it"
        );
    }

    #[test]
    fn connection_setup_is_aggregated_normalised_and_proxied() {
        // Two aliases, the same statement twice, written differently: one entry,
        // because that is what a grant holds. No proxy named, one declared.
        let report = run(
            SETUP,
            &setup_document(&format!(
                "{}, {}, {}",
                setup("SET application_name = 'app'", 2),
                r#"{"alias": "replica", "sql": "SET   application_name  =  'app' ;", "count": 3}"#,
                setup("SELECT 1", 1),
            )),
        );
        let statements = report["connection_setup"]["statements"]
            .as_array()
            .expect("a list");
        assert_eq!(statements.len(), 2);
        let app = statements
            .iter()
            .find(|entry| entry["sql"] == "SET application_name = 'app'")
            .expect("the SET is there");
        assert_eq!(app["count"], 5, "2 plus a reformatted 3");
        assert_eq!(app["proxy"], "db", "inferred from the only proxy");
        assert_eq!(app["wire_sql"], "SET application_name = 'app'");
    }

    /// A manifest whose *global* group is Django's connection housekeeping (§6).
    ///
    /// `SELECT set_config('TimeZone', $1, false)` only runs while a connection
    /// opens, so it is granted to every route and no request ever runs it; the
    /// second statement is the control — a grant nothing used at all.
    const CONNECTION: &str = r#"
        version = 1
        groups = ["connection"]

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [group.connection.db]
        queries = [
          "SELECT set_config('TimeZone', $1, false)",
          "SELECT 'never runs'",
        ]

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        [route.db]
        queries = ["SELECT 1"]
    "#;

    #[test]
    fn a_statement_connection_setup_accounts_for_is_used_rather_than_unused() {
        // The report used to say both things at once: `connection_setup` showed
        // the group's `set_config` granted in every route, and `unused_global`
        // called the same grant one that "nothing matched" — because it only
        // runs while a connection opens, which is not a request. A statement
        // the connection-setup section accounts for is *used*.
        let observations = format!(
            r#"{{"version": 1, "requests": [{}], "connection_setup": [{}]}}"#,
            request("GET", "/notes/", &statement("SELECT 1")),
            setup("SELECT set_config('TimeZone', $1, false)", 2),
        );
        let report = run(CONNECTION, &observations);

        let entry = &report["connection_setup"]["statements"][0];
        assert_eq!(entry["sql"], "SELECT set_config('TimeZone', $?, false)");
        assert_eq!(entry["granted_in"], 1, "the group grants it everywhere");
        assert_eq!(entry["ungranted_in"], json!([]));
        assert_eq!(report["totals"]["clean"], true);

        // The grant it accounts for is not in `unused_global`, and the summary
        // cannot say "nothing matched" about it. The control statement, which
        // nothing ran at all, still is — the fix is about the statements that
        // run while a connection opens, not about every statement.
        assert_eq!(report["unused_global"]["count"], 1, "{report}");
        assert_eq!(
            report["unused_global"]["statements"],
            json!([{
                "proxy": "db",
                "sql": "SELECT 'never runs'",
                "groups": ["connection"],
            }])
        );
        assert_eq!(report["totals"]["unused_global"], 1);
        assert_eq!(
            report["totals"]["unused"], 0,
            "the route's own grant matched"
        );
    }

    #[test]
    fn connection_setup_can_be_absent_and_is_never_present_twice() {
        let report = run(SETUP, &document(""));
        assert_eq!(
            report["connection_setup"],
            json!({"statements": [], "ungranted": 0})
        );
        assert_eq!(report["totals"]["connection_setup"], 0);
        assert_eq!(report["totals"]["clean"], true);
    }

    #[test]
    fn an_unrestricted_route_has_nothing_to_fill_in() {
        // `unrestricted = true` (§6, D3a) allows every statement, transaction
        // control included, so nothing can be missing there — and the report
        // must not ask for a `transaction_control` flag the route already has.
        let report = run(
            SETUP,
            &document(&request(
                "POST",
                "/checkout/",
                &format!(
                    "{}, {}",
                    json_statement(r#"DELETE FROM "orders" WHERE "orders"."id" = $1"#),
                    json_statement("SAVEPOINT s1"),
                ),
            )),
        );
        assert_eq!(missing_sql(&report, "checkout"), Vec::<String>::new());
        assert_eq!(proxy(&report, "checkout")["unused"], json!([]));
        assert_eq!(
            proxy(&report, "checkout")["transaction_control"],
            json!({"observed": true, "granted": true, "needed": false})
        );
        assert_eq!(
            route(&report, "checkout")["needs_transaction_control"],
            false
        );
        assert_eq!(report["totals"]["missing"], 0);
    }

    /// One `egress` entry.
    fn attempt(host: &str, port: u16) -> String {
        format!(r#"{{"family": "AF_INET", "host": "{host}", "port": {port}}}"#)
    }

    #[test]
    fn a_route_that_opens_connections_without_egress_is_reported() {
        // A route without `egress = true` may not open outbound connections
        // (§7.3): the seccomp filter, or the egress grant, is what allows it.
        let report = run(
            SETUP,
            &document_with_egress(
                "/notes/",
                &format!(
                    "{}, {}, {}",
                    attempt("redis", 6379),
                    attempt("redis", 6379),
                    attempt("example.com", 443)
                ),
            ),
        );
        let egress = &route(&report, "notes")["egress"];
        assert_eq!(egress["needed"], true);
        let observed = egress["observed"].as_array().expect("a list");
        assert_eq!(observed.len(), 2, "one line per destination");
        assert_eq!(
            observed[0]["target"], "redis:6379",
            "the most attempted first"
        );
        assert_eq!(observed[0]["host"], "redis");
        assert_eq!(observed[0]["port"], 6379);
        assert_eq!(observed[0]["family"], "AF_INET");
        assert_eq!(observed[0]["count"], 2);
        assert_eq!(observed[0]["tests"], json!(["tests/test_app.py::test_it"]));
        assert_eq!(observed[1]["target"], "example.com:443");
        assert_eq!(observed[1]["count"], 1);
        assert_eq!(report["totals"]["egress"], 1);
        assert_eq!(report["totals"]["clean"], false);
    }

    #[test]
    fn a_route_that_may_reach_the_network_is_not_asking_for_anything() {
        let manifest = format!("{SETUP}\n# the route that runs it may open connections\n");
        let report = run(
            &manifest.replace(
                "name = \"notes\"\n        path = ['/notes/.*']",
                "name = \"notes\"\n        path = ['/notes/.*']\n        egress = true",
            ),
            &document_with_egress("/notes/", &attempt("redis", 6379)),
        );
        let egress = &route(&report, "notes")["egress"];
        assert_eq!(egress["observed"].as_array().unwrap().len(), 1);
        assert_eq!(egress["needed"], false, "the route grants egress already");
        assert_eq!(report["totals"]["egress"], 0);
    }

    #[test]
    fn a_route_that_opens_nothing_reports_no_egress() {
        let report = run(SETUP, &document(&request("GET", "/notes/", "")));
        assert_eq!(
            route(&report, "notes")["egress"],
            json!({"observed": [], "needed": false})
        );
        assert_eq!(route(&report, "static")["egress"]["observed"], json!([]));
        assert_eq!(report["totals"]["egress"], 0);
    }

    #[test]
    fn statements_the_plugin_could_not_read_are_counted_not_dropped() {
        let observations = r#"{"version": 1, "requests": [], "unrecognised": [
            {"alias": "default", "query": "Template('SELECT ...')", "count": 3},
            {"query": "Template('BEGIN')"}
        ]}"#;
        let report = run(SETUP, observations);
        assert_eq!(report["unrecognised"]["count"], 4);
        let statements = report["unrecognised"]["statements"]
            .as_array()
            .expect("a list");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0]["query"], "Template('SELECT ...')");
        assert_eq!(statements[0]["alias"], "default");
        assert_eq!(statements[0]["count"], 3);
        assert_eq!(statements[1]["count"], 1, "a missing count means one");
        assert_eq!(statements[1]["alias"], Value::Null);
        assert_eq!(report["totals"]["unrecognised"], 4);
        assert_eq!(report["totals"]["clean"], false, "the report is incomplete");
    }

    #[test]
    fn a_plugin_warning_is_carried_into_the_report() {
        let observations = r#"{"version": 1, "requests": [], "warnings": [
            "the database alias 'default' does not use server-side binding"
        ]}"#;
        let report = run(SETUP, observations);
        assert_eq!(
            report["plugin_warnings"],
            json!(["the database alias 'default' does not use server-side binding"])
        );
        assert_eq!(report["totals"]["plugin_warnings"], 1);
        assert_eq!(report["totals"]["clean"], false, "a warning is not clean");
    }

    #[test]
    fn an_empty_session_is_clean_with_the_new_sections() {
        let report = run(SETUP, &document(""));
        assert_eq!(
            report["unrecognised"],
            json!({"statements": [], "count": 0})
        );
        assert_eq!(report["plugin_warnings"], json!([]));
        assert_eq!(
            route(&report, "notes")["egress"],
            json!({"observed": [], "needed": false})
        );
        assert_eq!(report["totals"]["clean"], true);
        assert!(
            report.get("script_name").is_none(),
            "no mount prefix to report"
        );
    }

    //
    // Global groups are nobody's route (D3c).
    //

    /// Two routes, one group of each kind: a global one applied to every route
    /// and one the route names itself.
    const GLOBAL: &str = r#"
        version = 1
        groups = ["session"]

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [group.session.db]
        queries = ['SELECT 0']

        [group.own.db]
        queries = ['SELECT 9']

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        groups = ["own"]
        [route.db]
        queries = ['SELECT 1']

        [[route]]
        name = "orders"
        path = ['/orders/']
        [route.db]
        queries = ['SELECT 2']
    "#;

    #[test]
    fn a_global_group_statement_is_not_unused_once_per_route() {
        let report = run(
            GLOBAL,
            &document(&request("GET", "/notes/", &statement("SELECT 1"))),
        );
        // The route's own group's statement is the route's to call dead; the
        // global group's is not, once per route.
        assert_eq!(proxy(&report, "notes")["unused"], json!(["SELECT 9"]));
        assert_eq!(
            report["unused_global"]["statements"],
            json!([{"proxy": "db", "sql": "SELECT 0", "groups": ["session"]}])
        );
        assert_eq!(report["unused_global"]["count"], 1);
        assert_eq!(report["totals"]["unused"], 1, "the route's own group only");
        assert_eq!(report["totals"]["unused_global"], 1);
        assert_eq!(
            report["totals"]["clean"], true,
            "unused grants do not fail strict"
        );
    }

    #[test]
    fn a_global_group_statement_one_route_used_is_unused_nowhere() {
        let report = run(
            GLOBAL,
            &document(&request("GET", "/notes/", &statement("SELECT 0"))),
        );
        assert_eq!(report["unused_global"]["statements"], json!([]));
        assert_eq!(report["unused_global"]["count"], 0);
        assert_eq!(report["totals"]["unused_global"], 0);
        assert_eq!(
            proxy(&report, "notes")["unused"],
            json!(["SELECT 9", "SELECT 1"]),
            "one route ran the global group's statement, so all of them are live"
        );
    }

    /// A global group whose only grant is a table grant (D7).
    const GLOBAL_TABLES: &str = r#"
        version = 1
        groups = ["catalog"]

        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }

        [group.catalog.db.tables]
        shop_product = { select = ["id"] }

        [[route]]
        name = "notes"
        path = ['/notes/.*']
        [route.db]
        queries = ['SELECT 1']
    "#;

    #[test]
    fn a_global_group_table_grant_nothing_needed_is_reported_once() {
        let report = run(
            GLOBAL_TABLES,
            &document(&request("GET", "/notes/", &statement("SELECT 1"))),
        );
        assert_eq!(report["unused_global"]["statements"], json!([]));
        let tables = report["unused_global"]["tables"]
            .as_array()
            .expect("a list");
        assert_eq!(tables.len(), 1, "{tables:?}");
        assert_eq!(tables[0]["proxy"], "db");
        assert_eq!(tables[0]["group"], "catalog");
        assert_eq!(tables[0]["tables"]["shop_product"]["select"], json!(["id"]));
        assert_eq!(tables[0]["functions"], json!([]));
        assert_eq!(report["totals"]["unused_global"], 1);
        assert_eq!(
            proxy(&report, "notes")["unused"],
            json!([]),
            "a table grant is never a route's unused line"
        );
    }

    #[test]
    fn a_global_group_table_grant_a_statement_needed_is_used() {
        let report = run(
            GLOBAL_TABLES,
            &document(&request(
                "GET",
                "/notes/",
                &statement(r#"SELECT \"shop_product\".\"id\" FROM \"shop_product\""#),
            )),
        );
        assert_eq!(
            proxy(&report, "notes")["missing"],
            json!([]),
            "the global group's table grant allowed it"
        );
        assert_eq!(report["unused_global"]["tables"], json!([]));
        assert_eq!(report["unused_global"]["count"], 0);
    }

    #[test]
    fn the_covering_grant_of_a_set_of_statements_is_taken_at_once() {
        // A row lock asks for `update` on *one* column, and the covering grant
        // picks one the route already reads when it is given every statement at
        // once — which is why the fold tool calls this function and not the
        // per-statement analysis.
        let statements = vec![
            r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1 FOR UPDATE"#.to_string(),
            r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $1"#.to_string(),
        ];
        let document: Value =
            serde_json::from_str(&covering(&statements, "route.db").expect("renders"))
                .expect("JSON");
        assert_eq!(
            document["tables"]["notes"]["select"],
            json!(["id", "owner_id"])
        );
        assert_eq!(document["tables"]["notes"]["update"], json!(["id"]));
        assert_eq!(document["functions"], json!([]));
        assert_eq!(document["unanalysable"], json!([]));
        assert_eq!(
            document["toml"].as_str().expect("a string"),
            "[route.db.tables]\n\
             # notes: `update = [\"id\"]` only locks its rows (`FOR UPDATE`) — any one \
             column is enough, as PostgreSQL requires.\n\
             notes = { select = [\"id\", \"owner_id\"], update = [\"id\"] }\n"
        );
    }

    /// A route whose only evidence is a live server's log (D3c).
    #[test]
    fn a_live_logs_request_claims_nothing_about_a_routes_grants() {
        let observations = format!(
            r#"{{"version": 1, "requests": [{{"method": "GET", "path_info": "/notes/",
                "source": "live", "statements": [{}]}}]}}"#,
            statement("SELECT 'denied'")
        );
        let report = run(GLOBAL, &observations);
        assert_eq!(
            missing_sql(&report, "notes"),
            ["SELECT 'denied'"],
            "a denial in the log is a missing grant, whether it came from a log or not"
        );
        assert_eq!(
            proxy(&report, "notes")["unused"],
            json!([]),
            "the log says what was denied, never what ran: no grant is called dead"
        );
        assert_eq!(report["unused_global"]["statements"], json!([]));
        assert_eq!(report["unused_global"]["count"], 0);
        assert_eq!(report["totals"]["unused"], 0);
        assert_eq!(report["totals"]["unused_global"], 0);
        assert_eq!(report["totals"]["clean"], false, "a grant is missing");
    }

    #[test]
    fn a_route_the_suite_reached_still_reports_its_own_grants_as_unused() {
        let observations = format!(
            r#"{{"version": 1, "requests": [
                {{"method": "GET", "path_info": "/notes/", "statements": []}},
                {{"method": "GET", "path_info": "/orders/", "source": "live",
                  "statements": [{}]}}
            ]}}"#,
            statement("SELECT 'denied'")
        );
        let report = run(GLOBAL, &observations);
        assert_eq!(
            proxy(&report, "notes")["unused"],
            json!(["SELECT 9", "SELECT 1"]),
            "the suite's own request is evidence about the route it reached"
        );
        assert_eq!(
            proxy(&report, "orders")["unused"],
            json!([]),
            "and a route only a log saw still claims nothing"
        );
        assert_eq!(
            report["unused_global"]["statements"],
            json!([]),
            "a global grant on a proxy a live route used cannot be called dead"
        );
    }

    #[test]
    fn a_statement_no_table_grant_can_describe_comes_back_separately() {
        let statements = vec!["SET TIME ZONE 'UTC'".to_string()];
        let document: Value =
            serde_json::from_str(&covering(&statements, "route.db").expect("renders"))
                .expect("JSON");
        assert_eq!(document["tables"], json!({}));
        assert_eq!(document["functions"], json!([]));
        let unanalysable = document["unanalysable"].as_array().expect("a list");
        assert_eq!(unanalysable.len(), 1);
        assert_eq!(unanalysable[0]["sql"], "SET TIME ZONE 'UTC'");
        assert!(
            unanalysable[0]["reason"]
                .as_str()
                .expect("a string")
                .contains("only SELECT"),
            "{:?}",
            unanalysable[0]["reason"]
        );
    }
}
