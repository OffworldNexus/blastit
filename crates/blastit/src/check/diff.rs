//! The grant a `(view, proxy)` pair is checked against, and the `Blastfile`
//! node a finding edits.
//!
//! [`GrantCheck`] is the allowlist dbproxy would enforce, built from the pair's
//! effective grant: the one place `blastit check` asks "may this view run this
//! statement?". Beside it sits the effective grant of a pair ([`grant_of`]), and
//! below it the locators a `fix` names: `view notes postgres`,
//! `trait connection`, `view notes POST body` (`§13.3`).

use blastit_manifest::blastfile::DEFAULT_STORE;
use blastit_manifest::{BodyOwner, Manifest, PgGrant, ProxyName, Route};
use blastit_sqlpolicy::Allowlist;

/// The effective `postgresql` grant of one `(route, proxy)` pair, if any (§6).
pub fn grant_of(manifest: &Manifest, route: usize, proxy: &str) -> Option<PgGrant> {
    manifest
        .effective_grants(route)
        .get(&ProxyName::new(proxy.to_string()))
        .and_then(|grant| grant.as_postgresql())
        .cloned()
}

/// The allowlist of one `(route, proxy)` pair: the one place `blastit check`
/// asks "may this view run this statement?" (§6).
pub struct GrantCheck {
    /// What dbproxy would check a statement against.
    allow: Allowlist,
}

impl GrantCheck {
    /// Builds the check of an effective grant (absent: a route with no grant for
    /// that proxy gets no listener, so nothing is allowed — §2.2).
    ///
    /// `unrestricted = true` (§6, D3a) is the other extreme: its allowlist
    /// allows everything, so nothing is ever missing there.
    pub fn of(grant: Option<&PgGrant>) -> Self {
        let Some(grant) = grant else {
            return Self {
                allow: Allowlist::default(),
            };
        };
        if grant.is_unrestricted() {
            return Self {
                allow: Allowlist::unrestricted(),
            };
        }
        Self {
            allow: Allowlist::new(grant.queries(), grant.transaction_control())
                .with_tables(grant.tables().clone()),
        }
    }

    /// Whether the policy allows one statement as it went on the wire.
    ///
    /// The wire text, not the normalised one: `Allowlist::check` normalises it
    /// itself and a table grant has to *parse* it (D7).
    pub fn allows(&self, wire_sql: &str) -> bool {
        self.allow.check(wire_sql).is_allowed()
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
