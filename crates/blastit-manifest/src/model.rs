//! The parsed manifest: proxies, groups, routes, grants and the effective
//! grants a route ends up with (§5.1, §6).

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;

use blastit_sqlpolicy::TableGrants;

use crate::error::{ManifestErrors, ManifestWarning};
use crate::parse;
use crate::validate;

/// Name of a proxy declared as `[proxy.<name>]`.
///
/// A grant only refers to a proxy by name; the name is the link between a route
/// and the `[proxy.<name>]` section that says what the proxy does (§5.1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProxyName(String);

impl ProxyName {
    /// Wraps a proxy name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProxyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for ProxyName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProxyName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProxyName {
    fn from(name: &str) -> Self {
        Self(name.to_string())
    }
}

impl From<String> for ProxyName {
    fn from(name: String) -> Self {
        Self(name)
    }
}

/// The wire protocol a proxy brokers (§5.1).
///
/// A proxy declaration and the grants that refer to it are both interpreted
/// through its protocol: `postgresql` today, more later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Protocol {
    /// `postgresql`, served by dbproxy (§5.2).
    Postgresql,
}

impl Protocol {
    /// Every protocol this build knows.
    pub const ALL: &'static [Protocol] = &[Protocol::Postgresql];

    /// The manifest spelling of the protocol.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
        }
    }

    /// Parses the manifest spelling; `None` for a protocol this build lacks.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.as_str() == name)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `[proxy.<name>]` declaration: a named broker for one external service
/// (§5.1).
///
/// `input` and `output` are kept exactly as written. Interpreting a slot name,
/// checking that the input variable exists in the environment and rendering a
/// template are the job of the environment builder (§5.3), not of the manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proxy {
    name: ProxyName,
    protocol: Protocol,
    input: BTreeMap<String, String>,
    output: BTreeMap<String, String>,
}

impl Proxy {
    /// Builds a proxy declaration.
    pub(crate) fn new(
        name: ProxyName,
        protocol: Protocol,
        input: BTreeMap<String, String>,
        output: BTreeMap<String, String>,
    ) -> Self {
        Self {
            name,
            protocol,
            input,
            output,
        }
    }

    /// The proxy's name, as used by grants.
    pub fn name(&self) -> &ProxyName {
        &self.name
    }

    /// The protocol the proxy speaks.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Input slots mapped to the environment variables that hold them, raw
    /// (§5.1).
    pub fn input(&self) -> &BTreeMap<String, String> {
        &self.input
    }

    /// Environment variable names mapped to output templates, raw (§5.1).
    ///
    /// Empty when the protocol's default outputs apply.
    pub fn output(&self) -> &BTreeMap<String, String> {
        &self.output
    }
}

/// The contents of a grant on a `postgresql` proxy (§5.2): the statements the
/// route may run, whether transaction control is allowed, whether every
/// statement is allowed instead (§6, `unrestricted = true`), and which tables,
/// columns and functions its statements may touch (§6, D7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PgGrant {
    queries: Vec<String>,
    transaction_control: bool,
    unrestricted: bool,
    tables: TableGrants,
}

impl PgGrant {
    /// Builds a grant from its manifest fields.
    pub fn new(queries: Vec<String>, transaction_control: bool) -> Self {
        Self {
            queries,
            transaction_control,
            unrestricted: false,
            tables: TableGrants::default(),
        }
    }

    /// The same grant with table and column grants (§6, D7).
    ///
    /// A statement that no exact query lists is then checked against these by
    /// parsing it ([`blastit_sqlpolicy::analyze`]), so a route can grant a
    /// database shape instead of every statement text.
    pub fn with_tables(mut self, tables: TableGrants) -> Self {
        self.tables = tables;
        self
    }

    /// A grant that allows every statement on its proxy (§6).
    ///
    /// The manifest spelling is `unrestricted = true`, and it is only accepted
    /// inline on a route ([`crate::ManifestError::UnrestrictedGroup`]): a group
    /// could attach it to every route by accident.
    pub fn unrestricted() -> Self {
        Self {
            queries: Vec::new(),
            transaction_control: false,
            unrestricted: true,
            tables: TableGrants::default(),
        }
    }

    /// The same grant, with the `unrestricted` flag of a manifest entry.
    ///
    /// Only the parser builds grants from a document; this is how it carries
    /// the flag into a value [`PgGrant::new`] cannot produce.
    pub(crate) fn with_unrestricted(mut self, unrestricted: bool) -> Self {
        self.unrestricted = unrestricted;
        self
    }

    /// The statements the grant allows, in manifest order.
    ///
    /// Empty for an unrestricted grant, which allows by not listing.
    pub fn queries(&self) -> &[String] {
        &self.queries
    }

    /// The tables, columns and functions the grant allows beyond its queries
    /// (§6, D7).
    pub fn tables(&self) -> &TableGrants {
        &self.tables
    }

    /// Whether `BEGIN`/`COMMIT`/`ROLLBACK` and savepoints are allowed.
    ///
    /// Always false for an unrestricted grant, which needs no such flag: it
    /// allows transaction control along with everything else.
    pub fn transaction_control(&self) -> bool {
        self.transaction_control
    }

    /// Whether the grant allows every statement on its proxy, whatever it is.
    pub fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// True when the grant allows nothing at all.
    ///
    /// An empty effective grant means the route gets no dbproxy listener for
    /// that proxy at all (§2.2, §6).
    pub fn is_empty(&self) -> bool {
        !self.unrestricted
            && self.queries.is_empty()
            && !self.transaction_control
            && self.tables.is_empty()
    }

    /// Unions `other` into `self`: queries are deduplicated in insertion order,
    /// transaction control and `unrestricted` are OR-ed, and table and column
    /// grants are unioned per table and column (§6).
    pub fn merge_from(&mut self, other: &Self) {
        self.transaction_control |= other.transaction_control;
        self.unrestricted |= other.unrestricted;
        for query in &other.queries {
            if !self.queries.contains(query) {
                self.queries.push(query.clone());
            }
        }
        self.tables.merge_from(&other.tables);
    }
}

/// A grant, resolved through the protocol of the proxy it names (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    /// A grant on a `postgresql` proxy (§5.2).
    Postgresql(PgGrant),
}

impl Grant {
    /// True when the grant allows nothing at all.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Postgresql(grant) => grant.is_empty(),
        }
    }

    /// The `postgresql` contents, when this is a grant on such a proxy.
    pub fn as_postgresql(&self) -> Option<&PgGrant> {
        match self {
            Self::Postgresql(grant) => Some(grant),
        }
    }

    /// The `postgresql` contents, consumed.
    pub fn into_postgresql(self) -> Option<PgGrant> {
        match self {
            Self::Postgresql(grant) => Some(grant),
        }
    }

    /// Unions another grant on the same protocol into this one.
    ///
    /// Grants always agree on the protocol, because both were resolved through
    /// the same proxy declaration.
    pub(crate) fn merge_from(&mut self, other: &Self) {
        match (self, other) {
            (Self::Postgresql(this), Self::Postgresql(other)) => this.merge_from(other),
        }
    }
}

/// A `[[route]]` entry: one sandbox, one process, one uid (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    name: String,
    path: Vec<String>,
    methods: Option<Vec<String>>,
    groups: Vec<String>,
    egress: bool,
    prefork: bool,
    grants: BTreeMap<ProxyName, Grant>,
}

impl Route {
    /// Builds a route. Parsing is the only intended constructor.
    pub(crate) fn new(
        name: String,
        path: Vec<String>,
        methods: Option<Vec<String>>,
        groups: Vec<String>,
        egress: bool,
        prefork: bool,
        grants: BTreeMap<ProxyName, Grant>,
    ) -> Self {
        Self {
            name,
            path,
            methods,
            groups,
            egress,
            prefork,
            grants,
        }
    }

    /// The route's name, as it appears in `environ["blastit.route"]` and in the
    /// event stream (§8.1).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The path regexes, in matching order, exactly as written (§6).
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// The methods this route answers, if it restricts them.
    ///
    /// `None` means any method. `HEAD` is never implied by `GET`.
    pub fn methods(&self) -> Option<&[String]> {
        self.methods.as_deref()
    }

    /// The groups this route takes grants from.
    pub fn groups(&self) -> &[String] {
        &self.groups
    }

    /// Whether the route may open outbound connections (§7.3).
    pub fn egress(&self) -> bool {
        self.egress
    }

    /// Whether the view process is forked at startup instead of on first use
    /// (§8 sizing).
    pub fn prefork(&self) -> bool {
        self.prefork
    }

    /// The route's inline grants, by proxy.
    pub fn grants(&self) -> &BTreeMap<ProxyName, Grant> {
        &self.grants
    }

    /// The route's inline grant on one proxy.
    pub fn grant(&self, proxy: &ProxyName) -> Option<&Grant> {
        self.grants.get(proxy)
    }
}

/// A `[group.<name>]` section: grants several routes share (§6).
///
/// A group grants nothing until it is attached, to routes or globally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    name: String,
    grants: BTreeMap<ProxyName, Grant>,
}

impl Group {
    /// Builds a group. Parsing is the only intended constructor.
    pub(crate) fn new(name: String, grants: BTreeMap<ProxyName, Grant>) -> Self {
        Self { name, grants }
    }

    /// The group's name, as routes refer to it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The group's grants, by proxy.
    pub fn grants(&self) -> &BTreeMap<ProxyName, Grant> {
        &self.grants
    }

    /// The group's grant on one proxy.
    pub fn grant(&self, proxy: &ProxyName) -> Option<&Grant> {
        self.grants.get(proxy)
    }
}

/// A grant that names a proxy the manifest never declares.
///
/// Kept so that validation can report it with the place it was written in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UnresolvedGrant {
    proxy: ProxyName,
    location: crate::error::Location,
}

impl UnresolvedGrant {
    /// Builds an unresolved grant reference.
    pub(crate) fn new(proxy: ProxyName, location: crate::error::Location) -> Self {
        Self { proxy, location }
    }

    /// The undeclared proxy name.
    pub(crate) fn proxy(&self) -> &ProxyName {
        &self.proxy
    }

    /// Where the grant was written.
    pub(crate) fn location(&self) -> &crate::error::Location {
        &self.location
    }
}

/// A parsed manifest: the routing table and every capability it hands out
/// (§5.1, §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    version: u32,
    global_groups: Vec<String>,
    proxies: BTreeMap<ProxyName, Proxy>,
    groups: BTreeMap<String, Group>,
    routes: Vec<Route>,
    unresolved_grants: Vec<UnresolvedGrant>,
}

impl Manifest {
    /// Parses a manifest document strictly: an unknown key is an error, not a
    /// comment (§6).
    ///
    /// Only structural problems are reported here. The checks that need the
    /// whole document — unknown groups, grants on unknown proxies, duplicate
    /// route names, path regexes, method tokens and the uid range — live in
    /// [`Manifest::validate`], and [`Manifest::parse_and_validate`] runs both.
    pub fn from_toml_str(source: &str) -> Result<Self, ManifestErrors> {
        parse::parse(source)
    }

    /// Parses and validates in one step, on the given uid base (§2).
    ///
    /// Returns the manifest together with its warnings. Only the parse errors
    /// can be reported when parsing fails, because validation needs a manifest.
    pub fn parse_and_validate(
        source: &str,
        base: u32,
    ) -> Result<(Self, Vec<ManifestWarning>), ManifestErrors> {
        let manifest = Self::from_toml_str(source)?;
        let warnings = manifest.validate(base)?;
        Ok((manifest, warnings))
    }

    /// Checks everything that needs the whole document, on the given uid base
    /// (§6), and returns the non-fatal warnings.
    pub fn validate(&self, base: u32) -> Result<Vec<ManifestWarning>, ManifestErrors> {
        validate::validate(self, base)
    }

    /// [`Manifest::validate`] on [`crate::UidLayout::DEFAULT_BASE`].
    pub fn validate_default(&self) -> Result<Vec<ManifestWarning>, ManifestErrors> {
        self.validate(crate::UidLayout::DEFAULT_BASE)
    }

    /// The manifest version; always `1` for a manifest that parsed.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The top-level `groups` list: groups attached to every route (§6).
    pub fn global_groups(&self) -> &[String] {
        &self.global_groups
    }

    /// Every `[proxy.<name>]` declaration, by name.
    pub fn proxies(&self) -> &BTreeMap<ProxyName, Proxy> {
        &self.proxies
    }

    /// One proxy declaration, by name.
    pub fn proxy(&self, name: &str) -> Option<&Proxy> {
        self.proxies.get(name)
    }

    /// Every `[group.<name>]` section, by name.
    pub fn groups(&self) -> &BTreeMap<String, Group> {
        &self.groups
    }

    /// One group, by name.
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.get(name)
    }

    /// The `[[route]]` entries, in manifest order — which is matching order
    /// (§6).
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// How many routes the manifest declares.
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// One route by index.
    pub fn route(&self, index: usize) -> Option<&Route> {
        self.routes.get(index)
    }

    /// The union of the grants a route ends up with, per proxy (§6).
    ///
    /// The union is taken in a stable order — the global groups in order, then
    /// the route's groups, then the route's inline grants — and is deduplicated
    /// by statement. `transaction_control` is on when any contributor turns it
    /// on. A proxy whose union allows nothing is absent, because a route with
    /// no grant gets no listener for it at all (§2.2).
    ///
    /// Grants are returned as written, resolved through their proxy's protocol;
    /// a caller that only knows `postgresql` narrows them with
    /// [`Grant::as_postgresql`].
    ///
    /// An out-of-range index has no grants.
    pub fn effective_grants(&self, route_index: usize) -> BTreeMap<ProxyName, Grant> {
        let Some(route) = self.routes.get(route_index) else {
            return BTreeMap::new();
        };

        let mut merged: BTreeMap<ProxyName, Grant> = BTreeMap::new();
        for group in self
            .global_groups
            .iter()
            .chain(route.groups.iter())
            .filter_map(|name| self.groups.get(name))
        {
            merge_grants(&mut merged, &group.grants);
        }
        merge_grants(&mut merged, &route.grants);

        merged
            .into_iter()
            .filter(|(_, grant)| !grant.is_empty())
            .collect()
    }

    /// Grants written for a proxy the manifest never declares, in the order
    /// they were found.
    pub(crate) fn unresolved_grants(&self) -> &[UnresolvedGrant] {
        &self.unresolved_grants
    }

    /// Builds a manifest from its parts; parsing is the only intended caller.
    pub(crate) fn assemble(
        version: u32,
        global_groups: Vec<String>,
        proxies: BTreeMap<ProxyName, Proxy>,
        groups: BTreeMap<String, Group>,
        routes: Vec<Route>,
        unresolved_grants: Vec<UnresolvedGrant>,
    ) -> Self {
        Self {
            version,
            global_groups,
            proxies,
            groups,
            routes,
            unresolved_grants,
        }
    }
}

/// Unions `source` into `target`, proxy by proxy and grant by grant.
fn merge_grants(target: &mut BTreeMap<ProxyName, Grant>, source: &BTreeMap<ProxyName, Grant>) {
    for (proxy, grant) in source {
        match target.get_mut(proxy) {
            Some(existing) => existing.merge_from(grant),
            None => {
                target.insert(proxy.clone(), grant.clone());
            }
        }
    }
}
