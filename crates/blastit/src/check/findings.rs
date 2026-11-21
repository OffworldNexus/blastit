//! The `blastit check` document: the closed nine-kind finding enum, the `fix`
//! object that answers each kind, and the counts the report carries
//! (`R38`, `ADR-0004`).
//!
//! `fix.action` is one of the closed set of `R38` — `add_query`, its
//! trait-scoped twin `add_group`, `remove_query`, `add_route`, `remove_route`,
//! `add_test`, `declare_ingress`, `fix_ingress` and `observe` — and `fix.to`
//! names the **Blastfile node** the action edits, never a directive to paste
//! (§13.3). `codecs`/`allow_raw` describe an ingress declaration, and `unsafe`
//! marks the destructive `remove_query`/`remove_route` actions.

use std::collections::BTreeMap;

use blastit_ingress::BodyCodec;
use blastit_manifest::Manifest;
use serde::{Deserialize, Serialize};

use super::error::ObsError;
use super::obs::SCHEMA_VERSION;
/// How a finding fails the run (`R38`: every kind is `error` in `0.1.1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fails the run and sets `ok` to `false`.
    Error,
    /// Printed, but leaves `ok` and the exit code alone.
    Warning,
}

/// The machine-readable `fix` object of a finding.
///
/// `action` is one of the closed set of `R38` — `add_query`, its trait-scoped
/// twin `add_group`, `remove_query`, `add_route`, `remove_route`, `add_test`,
/// `declare_ingress`, `fix_ingress` and `observe`; `to` names the **Blastfile
/// node** the action edits — `view <view> postgres [<store>]` for a statement
/// grant, `trait <name>` for a trait's, and `view <view> [<METHOD>] body` for
/// `raw_ingress` — never a directive to paste (§13.3). `codecs`/`allow_raw`
/// describe an ingress declaration, and `unsafe` marks the destructive
/// `remove_query`/`remove_route` actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// The closed action name.
    pub action: &'static str,
    /// The Blastfile node the action edits, when it edits one (§13.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The codecs a `declare_ingress`/`fix_ingress` action proposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codecs: Option<Vec<BodyCodec>>,
    /// Whether `body raw` also needs `unvalidated` (§10.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_raw: Option<bool>,
    /// Whether the action is destructive in production too.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "unsafe")]
    pub unsafe_: Option<bool>,
}

impl Fix {
    pub(crate) fn new(action: &'static str) -> Self {
        Self {
            action,
            to: None,
            codecs: None,
            allow_raw: None,
            unsafe_: None,
        }
    }

    pub(crate) fn observe() -> Self {
        Self::new("observe")
    }

    pub(crate) fn add_route() -> Self {
        Self::new("add_route")
    }

    pub(crate) fn add_test() -> Self {
        Self::new("add_test")
    }

    pub(crate) fn add_query(to: String) -> Self {
        Self {
            to: Some(to),
            ..Self::new("add_query")
        }
    }

    /// The connection-setup hint: a group every route attaches, because the
    /// statement runs on whichever route opens the connection first.
    pub(crate) fn add_group(to: String) -> Self {
        Self {
            to: Some(to),
            ..Self::new("add_group")
        }
    }

    pub(crate) fn remove_query(to: String) -> Self {
        Self {
            to: Some(to),
            unsafe_: Some(true),
            ..Self::new("remove_query")
        }
    }

    pub(crate) fn declare_ingress(codecs: Vec<BodyCodec>, allow_raw: bool) -> Self {
        Self {
            codecs: Some(codecs),
            allow_raw: allow_raw.then_some(true),
            ..Self::new("declare_ingress")
        }
    }

    pub(crate) fn fix_ingress(codecs: Vec<BodyCodec>, allow_raw: bool) -> Self {
        Self {
            codecs: Some(codecs),
            allow_raw: allow_raw.then_some(true),
            ..Self::new("fix_ingress")
        }
    }

    pub(crate) fn fix_raw_ingress(to: String) -> Self {
        Self {
            to: Some(to),
            allow_raw: Some(true),
            ..Self::new("fix_ingress")
        }
    }
}
/// One completeness finding, over the closed nine-kind enum (`§4.2 E.3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// A statement executed on a route but not granted (`R33`).
    MissingGrant {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route that ran it.
        route: String,
        /// The proxy it ran on.
        proxy: String,
        /// The normalised statement, or the transaction-control label.
        sql: String,
        /// `add_query`, `add_group` for connection setup, or `observe`.
        fix: Fix,
    },
    /// A grant nothing used (`R34`).
    DeadGrant {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route whose grant is dead (an attaching route, for a group).
        route: String,
        /// The group the grant belongs to, when it is a group's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        /// The proxy the grant is on.
        proxy: String,
        /// The normalised statement.
        sql: String,
        /// `remove_query`, flagged unsafe.
        fix: Fix,
    },
    /// A declared route no request reached (`R35`).
    UnexercisedRoute {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// `add_test`.
        fix: Fix,
    },
    /// A body was sent to a route with no `[route.ingress]` (`R36`).
    MissingIngress {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// The distinct MIME essences observed.
        observed: Vec<String>,
        /// `declare_ingress`.
        fix: Fix,
    },
    /// The declared codec is not what the suite sent (`R36`).
    IngressMismatch {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// The declared codec.
        declared: BodyCodec,
        /// The distinct MIME essences observed.
        observed: Vec<String>,
        /// `fix_ingress`.
        fix: Fix,
    },
    /// A view declares `body raw` without `unvalidated` (`R36`).
    RawIngress {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The route name.
        route: String,
        /// `fix_ingress` naming the view's `body` node (§13.3).
        fix: Fix,
    },
    /// A request no route matched (`R32`).
    UnroutedRequest {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The request method.
        method: String,
        /// The request path.
        path: String,
        /// `add_route`.
        fix: Fix,
    },
    /// The suite reached no route or no proxy statement (`R37`).
    NoObservations {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// `observe`.
        fix: Fix,
    },
    /// A declared proxy saw no statement (`R37`).
    ProxyUnused {
        /// Always `error` in `0.1.1`.
        severity: Severity,
        /// The proxy name.
        proxy: String,
        /// `observe`.
        fix: Fix,
    },
}

impl Finding {
    /// The severity of this finding.
    pub fn severity(&self) -> Severity {
        match self {
            Self::MissingGrant { severity, .. }
            | Self::DeadGrant { severity, .. }
            | Self::UnexercisedRoute { severity, .. }
            | Self::MissingIngress { severity, .. }
            | Self::IngressMismatch { severity, .. }
            | Self::RawIngress { severity, .. }
            | Self::UnroutedRequest { severity, .. }
            | Self::NoObservations { severity, .. }
            | Self::ProxyUnused { severity, .. } => *severity,
        }
    }
}

/// How many routes the manifest declares, and how many the suite reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteCounts {
    /// `[[route]]` blocks in the manifest.
    pub total: u64,
    /// Routes with at least one matched request.
    pub exercised: u64,
}

/// What one declared proxy saw.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyCounts {
    /// Statements observed on that proxy, routed or not.
    pub statements: u64,
    /// How many of them no grant allowed.
    pub denied: u64,
}

/// The `blastit check --json` document (`R38`, `ADR-0004`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Always [`SCHEMA_VERSION`].
    pub schema_version: u64,
    /// True exactly when no finding has [`Severity::Error`].
    pub ok: bool,
    /// Observed HTTP requests.
    pub requests: u64,
    /// Sum of `proxies[*].statements`.
    pub statements: u64,
    /// Route totals.
    pub routes: RouteCounts,
    /// Per declared proxy counters, in name order.
    pub proxies: BTreeMap<String, ProxyCounts>,
    /// The findings, in `§1.3` order.
    pub findings: Vec<Finding>,
}

impl Report {
    /// The `check` document for a manifest every request of which a `static`
    /// mount answers (`R14`).
    ///
    /// There is no suite to run, and no request any in-process suite could make
    /// against a mount: the responder answers before anything reaches a view. So
    /// every count is zero but the one the manifest answers — `routes.total` —
    /// and `findings` is empty: a mount the run never reached is a warning on
    /// stderr ([`super::static_roots::unrequested_static_roots`]), never a finding and never a reason
    /// to fail. The shape is the same document a run with an app prints, so
    /// `--json` and the Python contract stay one schema.
    pub fn all_static(manifest: &Manifest) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ok: true,
            requests: 0,
            statements: 0,
            routes: RouteCounts {
                total: manifest.route_count() as u64,
                exercised: 0,
            },
            proxies: BTreeMap::new(),
            findings: Vec::new(),
        }
    }

    /// Renders the report as pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// [`ObsError::Report`] only when the value cannot be rendered, which
    /// cannot happen in practice.
    pub fn to_json(&self) -> Result<String, ObsError> {
        serde_json::to_string_pretty(self).map_err(|error| ObsError::Report(error.to_string()))
    }
}
