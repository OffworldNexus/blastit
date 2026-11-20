//! The grant diff itself: what each view asked for, and what it was granted.
//!
//! Both presenters feed their observations into one [`Ledger`], per route:
//!
//! * a request is counted ([`Ledger::request`]) so a route nothing reached is
//!   reported as unexercised rather than as a wall of dead grants;
//! * every statement goes through [`Ledger::record`], which normalises it,
//!   asks the pair's [`GrantCheck`] and files it under `matched` or `missing`.
//!   That pair of sets *is* the answer to "executed but not granted / dead
//!   grant", and it is computed once, here, for `audit` and `check` alike;
//! * egress and the table grant a pair's statements would need are recorded
//!   beside it, because they are per `(route, proxy)` facts too — `check` simply
//!   does not read them.
//!
//! A dead grant is then read off the same record: a granted statement that is
//! not in `matched` was never used. How the two reports *group* that set is
//! their own business (the legacy report keeps a named group's grant under each
//! route, `check` reports it once); what is dead is decided here.

use std::collections::{BTreeMap, BTreeSet};

use blastit_manifest::{Manifest, ProxyName};
use blastit_sqlpolicy::normalize;

use super::diff::{GrantCheck, grant_of};
use super::{EXAMPLES, NO_TEST};

/// What one `(route, proxy)` pair did.
#[derive(Default)]
pub struct ProxyLedger {
    /// Statements observed on the pair, granted or not.
    pub executions: usize,
    /// Statements that were executed and not granted, by normalised text.
    pub missing: BTreeMap<String, Occurrence>,
    /// Normalised statements the pair's grant allowed.
    ///
    /// "Allowed", not "listed": an `unrestricted` grant lists nothing, and a
    /// statement a table grant covers is allowed without appearing in
    /// `queries`. Both are statements the route ran and was permitted to run.
    pub matched: BTreeSet<String>,
}

/// What one route did in the observed requests.
#[derive(Default)]
pub struct RouteLedger {
    pub requests: usize,
    proxies: BTreeMap<ProxyName, ProxyLedger>,
}

/// The grant diff of one run, indexed by route.
///
/// Holds the manifest it diffs against, so a statement's pair is checked
/// against the same allowlist dbproxy enforces without the caller passing it in
/// again.
pub struct Ledger<'a> {
    manifest: &'a Manifest,
    routes: BTreeMap<usize, RouteLedger>,
}

/// What one statement was, once the pair's grant had seen it.
pub struct Decision {
    /// Whether the pair's grant allows it: `false` is "executed but not
    /// granted".
    pub granted: bool,
}

impl<'a> Ledger<'a> {
    pub fn new(manifest: &'a Manifest) -> Self {
        Self {
            manifest,
            routes: BTreeMap::new(),
        }
    }

    /// Counts one routed request.
    pub fn request(&mut self, route: usize) -> &mut RouteLedger {
        let entry = self.routes.entry(route).or_default();
        entry.requests += 1;
        entry
    }

    /// Files one observed statement against the pair's grant.
    pub fn record(
        &mut self,
        route: usize,
        proxy: &ProxyName,
        wire_sql: &str,
        nodeid: Option<&str>,
        path: Option<&str>,
    ) -> Decision {
        let check = GrantCheck::of(grant_of(self.manifest, route, proxy.as_str()).as_ref());
        let normalized = normalize(wire_sql);
        let granted = check.allows(wire_sql);
        let entry = self
            .routes
            .entry(route)
            .or_default()
            .proxies
            .entry(proxy.clone())
            .or_default();
        entry.executions += 1;
        if granted {
            entry.matched.insert(normalized.clone());
        } else {
            entry
                .missing
                .entry(normalized.clone())
                .or_default()
                .record(wire_sql, nodeid, path);
        }
        Decision { granted }
    }

    /// The record of one route, if a request reached it.
    pub fn route(&self, route: usize) -> Option<&RouteLedger> {
        self.routes.get(&route)
    }

    /// How many requests reached `route`; `0` for a route nothing reached.
    pub fn requests(&self, route: usize) -> usize {
        self.route(route).map_or(0, |route| route.requests)
    }

    /// Every route that was reached, in index order.
    pub fn routes(&self) -> impl Iterator<Item = (usize, &RouteLedger)> {
        self.routes.iter().map(|(index, route)| (*index, route))
    }

    /// Whether any proxy of `route` was allowed one normalised statement.
    ///
    /// This is `check`'s dead-grant question, and the only form the answer is
    /// needed in: a grant of that route that no observed statement matched was
    /// never used, whichever store the observation named (`R34`). It walks the
    /// per-proxy sets instead of flattening them, so a route with many grants
    /// costs no allocation per query.
    pub fn matched_contains(&self, route: usize, sql: &str) -> bool {
        self.routes.get(&route).is_some_and(|route| {
            route
                .proxies
                .values()
                .any(|proxy| proxy.matched.contains(sql))
        })
    }
}

/// One statement executed but not granted, and the first examples of who ran it.
#[derive(Default)]
pub struct Occurrence {
    pub count: usize,
    pub wire_sql: String,
    pub tests: Vec<String>,
    pub paths: Vec<String>,
    seen_tests: BTreeSet<String>,
    seen_paths: BTreeSet<String>,
}

impl Occurrence {
    /// Counts one execution and keeps the first examples of who ran it.
    pub fn record(&mut self, wire_sql: &str, nodeid: Option<&str>, path: Option<&str>) {
        self.count += 1;
        if self.wire_sql.is_empty() {
            self.wire_sql = wire_sql.to_string();
        }
        let test = nodeid.unwrap_or("").trim();
        let test = if test.is_empty() { NO_TEST } else { test };
        if self.tests.len() < EXAMPLES && self.seen_tests.insert(test.to_string()) {
            self.tests.push(test.to_string());
        }
        if let Some(path) = path
            && self.paths.len() < EXAMPLES
            && self.seen_paths.insert(path.to_string())
        {
            self.paths.push(path.to_string());
        }
    }
}
