//! The grant a `(view, proxy)` pair is checked against, and the `Blastfile`
//! node a finding edits.
//!
//! [`GrantCheck`] is the allowlist dbproxy would enforce, built from the pair's
//! effective grant: the one place both presenters ask "may this view run this
//! statement?". Above it sit the two pieces every caller needs — which proxy a
//! statement belongs to ([`resolve_proxy`]) and the effective grant of a pair
//! ([`grant_of`]) — and below it the locators a `fix` names: `view notes
//! postgres`, `trait connection`, `view notes POST body` (`§13.3`).

use std::collections::BTreeSet;

use blastit_manifest::blastfile::DEFAULT_STORE;
use blastit_manifest::{BodyOwner, Manifest, PgGrant, Protocol, ProxyName, Route};
use blastit_sqlpolicy::Allowlist;

use crate::error::AuditError;
use crate::ledger::RouteLedger;

/// The `postgresql` proxies a route should be reported on: the ones it has
/// grants on, and the ones it was observed on.
pub(crate) fn proxy_names(
    manifest: &Manifest,
    route: usize,
    ledger: Option<&RouteLedger>,
) -> BTreeSet<ProxyName> {
    let mut names: BTreeSet<ProxyName> = ledger
        .map(|ledger| ledger.proxies().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    names.extend(manifest.effective_grants(route).into_keys());
    names
}

/// The effective `postgresql` grant of one `(route, proxy)` pair, if any (§6).
pub fn grant_of(manifest: &Manifest, route: usize, proxy: &str) -> Option<PgGrant> {
    manifest
        .effective_grants(route)
        .get(&ProxyName::new(proxy.to_string()))
        .and_then(|grant| grant.as_postgresql())
        .cloned()
}

/// The proxy a statement belongs to, from the observations or the manifest.
pub fn resolve_proxy(manifest: &Manifest, named: Option<&str>) -> Result<ProxyName, AuditError> {
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
pub struct GrantCheck {
    /// What dbproxy would check a statement against.
    allow: Allowlist,
    /// The granted statements, normalised, in manifest order and without
    /// duplicates: the candidates for a dead grant.
    granted: Vec<String>,
    /// Whether the grant allows transaction control. An `unrestricted = true`
    /// grant does, by allowing everything; `PgGrant::transaction_control()` is
    /// false for it, so the flag alone would say a route needs the flag it
    /// already has.
    transaction_control: bool,
}

impl GrantCheck {
    /// Builds the check of an effective grant (absent: a route with no grant for
    /// that proxy gets no listener, so nothing is allowed — §2.2).
    ///
    /// `unrestricted = true` (§6, D3a) is the other extreme: its allowlist
    /// allows everything, so nothing can be missing there and its (empty)
    /// granted set has nothing to call dead.
    pub fn of(grant: Option<&PgGrant>) -> Self {
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
            let query = crate::normalize(query);
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

    /// Whether the policy allows one statement as it went on the wire.
    ///
    /// The wire text, not the normalised one: `Allowlist::check` normalises it
    /// itself and a table grant has to *parse* it (D7).
    pub fn allows(&self, wire_sql: &str) -> bool {
        self.allow.check(wire_sql).is_allowed()
    }

    /// The granted statements, normalised and deduplicated, in manifest order.
    pub fn granted(&self) -> &[String] {
        &self.granted
    }

    /// Whether the grant allows transaction control.
    pub fn transaction_control(&self) -> bool {
        self.transaction_control
    }
}

/// Recognises the transaction-control statements of §5.2 through the public
/// policy API: an empty allowlist without `transaction_control` denies exactly
/// them, and the same allowlist with it allows them and nothing else.
pub struct TcProbe {
    without: Allowlist,
    with: Allowlist,
}

impl TcProbe {
    pub fn new() -> Self {
        Self {
            without: Allowlist::default(),
            with: Allowlist::new(std::iter::empty::<&str>(), true),
        }
    }

    /// Whether `sql` is a single transaction-control statement.
    pub fn is_transaction_control(&self, sql: &str) -> bool {
        !self.without.check(sql).is_allowed() && self.with.check(sql).is_allowed()
    }
}

impl Default for TcProbe {
    fn default() -> Self {
        Self::new()
    }
}

/// The `Blastfile` node a pair's table grant belongs in: `view <name> postgres`
/// for the unnamed store, `view <name> postgres <store>` otherwise.
///
/// The header is what makes the rendered value a fragment a developer can paste
/// as it stands, instead of a list of directives with no place to go.
pub fn grant_locator(view: &str, store: &str) -> String {
    if store == DEFAULT_STORE {
        format!("view {view} postgres")
    } else {
        format!("view {view} postgres {store}")
    }
}

/// The node a trait's grant lives on: `trait <name>` (§13.3).
pub fn trait_locator(trait_name: &str) -> String {
    format!("trait {trait_name}")
}

/// The `body` node of a view: `view <view> body`, or `view <view> <METHOD[s]>
/// body` when an endpoint declares it (§13.3, §10.3).
///
/// The owner comes out of lowering ([`Route::body_owner`]), never out of
/// [`Route::methods`]: a view-level `body` on a view with one `POST` endpoint has
/// the same method list as that endpoint's own `body`, and the locator has to
/// name the node the file actually has.
pub(crate) fn body_locator(route: &Route) -> String {
    if route.body_owner() == BodyOwner::Endpoint
        && let Some(methods) = endpoint_methods(route)
    {
        return format!("view {} {methods} body", route.name());
    }
    format!("view {} body", route.name())
}

/// The methods an endpoint names, as the locator writes them.
///
/// `HEAD` is implied by `GET` (§8.4) and is never written, so a `GET` endpoint
/// answers `GET`; a route with no methods answers every method and has no
/// endpoint to name.
fn endpoint_methods(route: &Route) -> Option<String> {
    let methods: Vec<&str> = route
        .methods()?
        .iter()
        .map(String::as_str)
        .filter(|method| *method != "HEAD")
        .collect();
    (!methods.is_empty()).then(|| methods.join(", "))
}
