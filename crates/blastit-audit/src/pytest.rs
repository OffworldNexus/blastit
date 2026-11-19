//! The pytest plugin's report document (D3b, ARCHITECTURE.md §6).
//!
//! The SQL rules such a report is compared with are [`crate::normalize`]'s — the
//! same function dbproxy and the manifest use, re-exported rather than rewritten
//! here.
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
//!     {"alias": "default", "proxy": "db", "sql": "SET TIME ZONE 'UTC'", "count": 2}
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
//!       "path": ["/notes/*"],
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
//!                            "functions": [], "blastfile": "# view notes postgres\n…",
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
//!       {"sql": "SET TIME ZONE 'UTC'", "proxy": "db", "count": 2,
//!        "wire_sql": "SET TIME ZONE 'UTC'",
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
//!
//! # This module
//!
//! The document above is the plugin's contract; [`audit`] is its entry point.
//!
//! [`audit`] takes a manifest and the observations a pytest session recorded and
//! returns one JSON report: for every route, the statements that were executed
//! but not granted, the grants nothing ever matched, and whether the route needs
//! `transaction_control` it was not given; plus the requests that would not have
//! been routed at all.
//!
//! The grant diff itself is [`crate::Ledger`]'s: this module only groups it into
//! the document the plugin, `fold` and the strict gate read.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Manifest, RoutingTable};
use blastit_sqlpolicy::{TableGrants, analyze};

use crate::assemble::{Run, assemble};
use crate::diff::resolve_proxy;
use crate::error::AuditError;
use crate::ledger::{Ledger, UnroutedTrace};
use crate::observe::parse_observations;
use crate::sections::GlobalState;

/// Diffs the observations of a test suite against a manifest (§6).
///
/// `manifest_text` is the `Blastfile` `fold` and the pytest plugin hand over, read
/// by [`Manifest::from_blastfile_str`] like every other reader (§15.2), then
/// validated so a document that would not boot is reported rather than diffed.
///
/// The observations carry the `PATH_INFO` the app saw, which is the string the
/// scheduler routed on — the view strips `BLASTIT_SCRIPT_NAME` before WSGI
/// (§2.5) — so no mount prefix is involved here. A caller that recorded mounted
/// paths has to strip its own prefix before calling: the scheduler's
/// `strip_script_name` is crate-private, and reimplementing it here would be a
/// second router to keep in step.
///
/// Returns the report as pretty-printed JSON; see the module documentation for
/// its shape.
///
/// # Errors
///
/// [`AuditError`] when the manifest is invalid, the observations do not have the
/// documented shape, or a statement cannot be attributed to a proxy.
pub fn audit(manifest_text: &str, observations_json: &str) -> Result<String, AuditError> {
    let manifest = Manifest::from_blastfile_str(manifest_text)
        .map_err(|errors| AuditError::manifest(&errors))?;
    let warnings = manifest
        .validate_default()
        .map_err(|errors| AuditError::manifest(&errors))?;
    let table = RoutingTable::new(&manifest).map_err(|errors| AuditError::manifest(&errors))?;
    let observations = parse_observations(observations_json)?;
    let requests = &observations.requests;
    // What the *global* groups grant (§6, D3c): a statement they grant belongs
    // to every route at once, so it is never a route's own `unused` line — the
    // report says it once, in `unused_global`.
    let mut global = GlobalState::of(&manifest);

    let mut ledger = Ledger::new(&manifest);
    let mut unrouted: BTreeMap<(String, String), UnroutedTrace> = BTreeMap::new();
    let mut routed_requests = 0usize;
    let mut routed_statements = 0usize;

    for request in requests {
        let Some(matched) = table.route(&request.method, &request.path_info) else {
            let entry = unrouted
                .entry((request.method.clone(), request.path_info.clone()))
                .or_insert_with(|| UnroutedTrace::new(&request.method, &request.path_info));
            entry.record(request.statements.len(), request.nodeid.as_deref());
            continue;
        };
        let route = matched.route;
        ledger.request(route, request.live);
        routed_requests += 1;

        for attempt in &request.egress {
            ledger.note_egress(route, attempt, request.nodeid.as_deref());
        }

        for statement in &request.statements {
            let proxy = resolve_proxy(&manifest, statement.proxy.as_deref())?;
            // The one grant diff: the pair's allowlist is asked about the text as
            // it went on the wire (a table grant has to *parse* it, D7), and the
            // normalised form is what a grant stores.
            let decision = ledger.record(
                route,
                &proxy,
                &statement.sql,
                request.nodeid.as_deref(),
                Some(request.path_info.as_str()),
            );
            routed_statements += 1;
            if request.transaction_control || decision.transaction_control {
                ledger.mark_transaction_control(route, &proxy);
            }

            // What a table grant covering this pair would have to say (D7).
            match analyze(&statement.sql) {
                Ok(analysis) => {
                    let covering = TableGrants::covering(std::iter::once(&analysis));
                    let locked: BTreeSet<String> = covering
                        .locks_without_a_column()
                        .map(str::to_string)
                        .collect();
                    let pair = ledger.pair_mut(route, &proxy);
                    if !locked.is_empty() {
                        pair.nameless_locks
                            .entry(decision.normalized.clone())
                            .or_default()
                            .extend(locked);
                    }
                    pair.table_grants.merge_from(&covering);
                    // Whether a global group's table grant was needed at all: a
                    // statement that only this group's tables and columns allow
                    // makes it a used one (D3c).
                    global.mark_used(&proxy, &analysis);
                }
                Err(error) => {
                    ledger
                        .pair_mut(route, &proxy)
                        .unanalysable
                        .entry(decision.normalized)
                        .or_insert_with(|| error.to_string());
                }
            }
        }
    }

    let report = assemble(
        &Run {
            manifest: &manifest,
            ledger: &ledger,
            global: &global,
        },
        &warnings,
        &observations,
        &unrouted,
        Summary {
            requests: requests.len(),
            routed: routed_requests,
            statements: routed_statements,
        },
    )?;
    serde_json::to_string_pretty(&report).map_err(|error| AuditError::Report(error.to_string()))
}

/// The totals the request loop accumulated, on their way into the report.
pub(crate) struct Summary {
    pub requests: usize,
    pub routed: usize,
    pub statements: usize,
}
