//! The parsed manifest: proxies, groups, routes, grants and the effective
//! grants a route ends up with (§5.1, §6).

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use blastit_ingress::IngressPolicy;
use blastit_sqlpolicy::{Allowlist, TableGrants};

use crate::error::{Location, ManifestError, ManifestErrors, ManifestWarning};
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
    ///
    /// `#[doc(hidden)]`: `lower` is the only production builder, and the
    /// crate's integration tests need it to build the expected model of the
    /// §20 R8 parity oracle field by field.
    #[doc(hidden)]
    pub fn new(
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
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_unrestricted(mut self, unrestricted: bool) -> Self {
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

/// A `[route.static]` block: what the responder serves for one route (`R1`–`R5`).
///
/// The values are kept exactly as written. Their syntax is checked by
/// [`Manifest::validate`], and whether the directory exists by
/// [`Manifest::check_static_roots`] — never by validation, because a manifest
/// has to parse on a machine that has not run the build yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticPolicy {
    /// The directory served, relative to the directory holding the `Blastfile`.
    pub root: String,
    /// The file a directory answers with, relative to `root`. `index.html`
    /// unless the manifest says otherwise.
    pub index: String,
    /// The file that answers a request no file and no view answered, relative
    /// to `root` (§9.3), or `None` when such a request is a `404`.
    ///
    /// It is a different file from [`StaticPolicy::index`]: a deep link may be
    /// answered by a shell that is not the directory's index. The retired
    /// reader lowers the per-route flag that answered every miss with the index
    /// to `fallback = Some(index)`, so the two spellings carry one fact.
    pub fallback: Option<String>,
    /// `/`-anchored whole-match globs against the answering file's path; a
    /// match gets the year-long cache (`R5`).
    pub immutable: Vec<String>,
}

/// A top-level `static "<prefix>"` mount: one URL prefix answered from one
/// directory (§9.1).
///
/// A mount is not a route: it owns no uid, starts no process and declares no
/// methods. The scheduler answers a matching request from
/// [`StaticMount::policy`] and only then consults the view table (§9.2), so a
/// mount can never shadow a view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticMount {
    /// The URL prefix, exactly as written: `"/"`, or a `/`-anchored path with
    /// no trailing `/` (§9.1).
    ///
    /// It is a URL prefix, not a filesystem path: the directory served is
    /// [`StaticPolicy::root`], relative to the manifest's own directory.
    pub prefix: String,
    /// What the mount serves: the directory, its index and its cache globs.
    pub policy: StaticPolicy,
}

/// A `[route.php]` block: the front controller a route's embedded PHP workers
/// run (`ADR-0010` §2).
///
/// PHP runs embedded in blastit's own unprivileged `view` workers: the host's
/// NTS `libphp.so` is `dlopen`ed inside the worker, so there is no pool, no
/// `listen` and no socket a manifest could name (`ADR-0010` §2 pins `entry` and
/// the optional `workers` cap).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpPolicy {
    /// The front controller, relative to the manifest's own directory (beneath
    /// a `static` mount's `root` when both are declared).
    pub entry: String,
    /// An optional **cap** on the route's derived concurrency, or `None` to let
    /// blastit use the route's per-view budget unchanged.
    ///
    /// Downward only: an explicit value can lower the ceiling the deployment
    /// allows, never raise the deployment's admission ceiling. The derivation is
    /// the scheduler's, not the manifest's — an absent value is "use the
    /// budget", not a default this crate invents.
    pub workers: Option<u32>,
}

/// The process a route's requests are handed to (design §4.2.1).
///
/// A closed set of executor kinds rather than a `()`, so the static responder,
/// the WSGI view and the embedded PHP worker join the same table and a backend
/// registry is keyed by [`Executor::kind`]. `[route.fastcgi]` is retired and
/// produces no arm (`ADR-0010` §1: embedded PHP only, no fallback).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Executor {
    /// The route's WSGI view process; [`Executor::kind`] is `"wsgi"`.
    View,
    /// The route's embedded PHP workers; [`Executor::kind`] is `"php"`.
    ///
    /// The policy comes with the plan, so the scheduler never re-reads the
    /// route to find out what it is running.
    Php(PhpPolicy),
}

impl Executor {
    /// Every executor kind this build has: the single source of the backend
    /// kinds a registry accepts at boot.
    pub const ALL_KINDS: &'static [&'static str] = &["wsgi", "php"];

    /// The backend kind, spelled as `BackendBuilder::kind` spells it: `"wsgi"`
    /// for [`Executor::View`], `"php"` for [`Executor::Php`].
    pub fn kind(&self) -> &'static str {
        match self {
            Self::View => "wsgi",
            Self::Php(_) => "php",
        }
    }
}

/// Who declared a route's `body` (§10.3, §13.3).
///
/// A view-level `body` applies to every endpoint and has no endpoint to name in
/// a locator; an endpoint's `body` belongs to that method. Two documents can
/// lower to the same [`IngressPolicy`] and differ here, which is why this is
/// carried rather than derived from [`Route::methods`] — a view-level `body` on
/// a view with one `POST` endpoint is indistinguishable from that endpoint's own
/// `body` by the methods alone, and `check`'s `fix.to` names the node a
/// developer edits (§13.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BodyOwner {
    /// The view's own `body`: `view upload { body raw }`.
    #[default]
    View,
    /// The endpoint's `body`: `view upload { POST "/upload" { body raw } }`.
    Endpoint,
}

/// One route of a `view`: one sandbox, one process, one uid (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    name: String,
    sandbox: u32,
    path: Vec<String>,
    methods: Option<Vec<String>>,
    groups: Vec<String>,
    egress: bool,
    prefork: bool,
    grants: BTreeMap<ProxyName, Grant>,
    ingress: Option<IngressPolicy>,
    body_owner: BodyOwner,
    static_policy: Option<StaticPolicy>,
    php: Option<PhpPolicy>,
}

impl Route {
    /// Builds a route. Parsing is the only intended constructor.
    ///
    /// The route starts in sandbox `0`; a caller that knows which view it
    /// belongs to chains [`Route::with_sandbox`]. A rebuild of an existing
    /// route must chain it, or the rebuilt route claims a different sandbox
    /// than the one it was copied from.
    ///
    /// `#[doc(hidden)]`: `lower` is the only production builder, and the
    /// crate's integration tests need it to build the expected model of the
    /// §20 R8 parity oracle field by field.
    #[doc(hidden)]
    pub fn new(
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
            sandbox: 0,
            path,
            methods,
            groups,
            egress,
            prefork,
            grants,
            ingress: None,
            body_owner: BodyOwner::View,
            static_policy: None,
            php: None,
        }
    }

    /// The same route with the owner of its `body` (§10.3, §13.3).
    ///
    /// `#[doc(hidden)]`: `lower` sets it, and the crate's integration tests chain
    /// it when their expected model carries an endpoint's `body`.
    #[doc(hidden)]
    pub fn with_body_owner(mut self, owner: BodyOwner) -> Self {
        self.body_owner = owner;
        self
    }

    /// Who declared this route's `body`: the view, or the endpoint (§10.3).
    ///
    /// It is what `check`'s `raw_ingress` finding needs to name a node the file
    /// actually has — `view upload body` for a view-level `body`, `view upload
    /// POST body` for that endpoint's (§13.3).
    pub fn body_owner(&self) -> BodyOwner {
        self.body_owner
    }

    /// The same route in the given sandbox (§7.3, §10.3).
    ///
    /// One `view` with several endpoints lowers to several routes that all
    /// carry that view's sandbox, so they share one uid, one socket directory
    /// and one worker. The retired TOML reader numbers one sandbox per route,
    /// which is what today's per-route uid table does.
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_sandbox(mut self, sandbox: u32) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// The same route with its retired `[route.static]`-shaped block (`R1`).
    ///
    /// No `Blastfile` can set it: a static site is a top-level `static` mount.
    /// It stays in the model, and in `check_static`'s coverage, until the
    /// per-route pair is removed with `MB.21.5.1`.
    pub(crate) fn with_static(mut self, static_policy: Option<StaticPolicy>) -> Self {
        self.static_policy = static_policy;
        self
    }

    /// The same route with its retired `[route.php]`-shaped block (`ADR-0010`
    /// §2).
    ///
    /// No `Blastfile` can set it: PHP is one `entrypoint php` for every view.
    /// It stays in the model until the per-route pair is removed with
    /// `MB.21.5.1`.
    pub(crate) fn with_php(mut self, php: Option<PhpPolicy>) -> Self {
        self.php = php;
        self
    }

    /// The same route with its `[route.ingress]` policy, if it declared one
    /// (§4.2 A, `ADR-0006`).
    ///
    /// `None` is the fail-closed default: R12 turns any non-empty body into a
    /// `413` for a route that declares nothing.
    pub fn with_ingress(mut self, ingress: Option<IngressPolicy>) -> Self {
        self.ingress = ingress;
        self
    }

    /// The route's name, as it appears in `environ["blastit.route"]` and in the
    /// event stream (§8.1).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The view this route belongs to (§7.3, §10.3).
    ///
    /// Several routes of one `view` — its several endpoints — carry the same
    /// sandbox and therefore the same uid, one private socket directory and one
    /// worker; the next view takes the next sandbox. The retired TOML reader
    /// numbers one sandbox per route, so its routes number exactly as they did
    /// before [`Route::sandbox`] existed.
    pub fn sandbox(&self) -> u32 {
        self.sandbox
    }

    /// The path regexes, in matching order, exactly as written (§6).
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// The methods this route answers, if it restricts them.
    ///
    /// `None` means any method the front end lets through — not any byte
    /// string (§8.4).
    ///
    /// A `Blastfile` view that declares `GET` answers `HEAD` too. Lowering
    /// normalises the list before the route exists, so `HEAD` is already in
    /// this list and request matching itself compares exact strings (§8.4).
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

    /// The `[route.ingress]` policy, or `None` when the route declared none
    /// (`R11`).
    ///
    /// `None` behaves exactly like [`blastit_ingress::BodyCodec::None`]: the
    /// route may receive an empty body and nothing else.
    pub fn ingress(&self) -> Option<IngressPolicy> {
        self.ingress
    }

    /// The route's `[route.static]` block, or `None` when the route declares
    /// none and the responder answers nothing for it (`R1`).
    pub fn static_policy(&self) -> Option<&StaticPolicy> {
        self.static_policy.as_ref()
    }

    /// The route's `[route.php]` block, or `None` when the route is not a PHP
    /// route (`ADR-0010` §2).
    ///
    /// `Some` makes the route its own executor ([`Executor::Php`]): it needs no
    /// `app`, and every request the responder does not answer from
    /// `[route.static]` reaches `entry` inside an embedded `libphp` worker.
    pub fn php(&self) -> Option<&PhpPolicy> {
        self.php.as_ref()
    }

    /// Whether this is the method-unrestricted catch-all `--learn` inserts new
    /// routes above (`R28`, `ADR-0003`).
    ///
    /// Every spelling that matches every request counts. The bare string
    /// `path = ".*"` parses to `[".*"]`, and `path = ['.*']` is the same list;
    /// an anchored `^.*$` matches the same requests. The lowered form of a
    /// `Blastfile` `view app "/*"` is `/.*` (`MB.21.3.5`, §4.3), so it counts
    /// too: `--learn` inserts new views above the catch-all it finds in either
    /// document. A catch-all restricted to some methods is not unrestricted: a
    /// request with another method still falls through to the route below it.
    pub fn is_unrestricted_catch_all(&self) -> bool {
        self.methods.is_none()
            && self
                .path
                .iter()
                .any(|pattern| matches!(pattern.as_str(), ".*" | "^.*$" | "/.*"))
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
    ///
    /// `#[doc(hidden)]`: `lower` is the only production builder, and the
    /// crate's integration tests need it to build the expected model of the
    /// §20 R8 parity oracle field by field.
    #[doc(hidden)]
    pub fn new(name: String, grants: BTreeMap<ProxyName, Grant>) -> Self {
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
    app: Option<String>,
    name: Option<String>,
    preload: Vec<String>,
    global_groups: Vec<String>,
    proxies: BTreeMap<ProxyName, Proxy>,
    groups: BTreeMap<String, Group>,
    routes: Vec<Route>,
    mounts: Vec<StaticMount>,
    php: Option<PhpPolicy>,
    route_lines: Vec<u32>,
    unresolved_grants: Vec<UnresolvedGrant>,
}

impl Manifest {
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

    /// Whether two manifests describe the same document, ignoring where each
    /// route was written (§4.4).
    ///
    /// [`Manifest::route_lines`] is the only field left out: it holds source
    /// positions, and a formatter or an editor may move a route to another line
    /// without changing what the manifest means (`R7`, `P4`). Every other
    /// field, `unresolved_grants` included, has to match.
    pub fn equivalent(&self, other: &Self) -> bool {
        self.version == other.version
            && self.app == other.app
            && self.name == other.name
            && self.preload == other.preload
            && self.global_groups == other.global_groups
            && self.proxies == other.proxies
            && self.groups == other.groups
            && self.routes == other.routes
            && self.mounts == other.mounts
            && self.php == other.php
            && self.unresolved_grants == other.unresolved_grants
    }

    /// The application's name, from the Blastfile's `app "<name>"` (§4.3).
    ///
    /// Carried, not interpreted: nothing in 0.2.0 reads it. The retired TOML
    /// format has no spelling for it, so a manifest read by that reader always
    /// answers `None`.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The top-level `app`: the WSGI callable, `module:attribute`, exactly as
    /// written (`R7`).
    ///
    /// The manifest only carries it; `BLASTIT_APP` overrides it and the
    /// supervisor is what turns it into a Python object. `None` means the file
    /// names no callable, and then `BLASTIT_APP` has to.
    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    /// The top-level `preload` modules, in the order written, deduplicated
    /// (`R7`).
    ///
    /// `preload = "mod"` and `preload = ["mod"]` are the same list. An empty
    /// list means the manifest preloads nothing; `BLASTIT_PRELOAD` overrides
    /// it.
    pub fn preload(&self) -> &[String] {
        &self.preload
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

    /// The routes the `view` entries lowered to, in manifest order — which is
    /// matching order (§6).
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

    /// The top-level static mounts, in file order (§9.1).
    ///
    /// Mounts are tried before views, longest prefix first, so a static mount
    /// never shadows a view (§9.2). A mount owns no uid and starts no process.
    pub fn mounts(&self) -> &[StaticMount] {
        &self.mounts
    }

    /// The one `entrypoint php` front controller for the whole file (§12.2).
    ///
    /// `Some` makes the manifest its own executor: it needs no `app`, and every
    /// request no mount answers reaches `entry` inside an embedded `libphp`
    /// worker. The retired TOML reader fills this from the first route that
    /// declares `[route.php]`, and keeps each route's own block as well.
    pub fn php(&self) -> Option<&PhpPolicy> {
        self.php.as_ref()
    }

    /// True when the manifest declares no `app` and answers every request from
    /// the filesystem: no callable, no front controller, at least one mount
    /// (§9.5).
    ///
    /// A `Blastfile`'s mounts are the file's, so a static-only file is mounts and
    /// **no views**, and the `routes.is_empty()` arm below is the whole answer
    /// for it. The `route.static_policy()` arm is the retired per-route block's
    /// compatibility path — no `Blastfile` can reach it, and it goes with the
    /// per-route pair in `MB.21.5.1`. It is **not** a second definition:
    /// a `Blastfile` never reaches it. `BLASTIT_APP` may still supply an app, so
    /// this is the manifest-level answer and the boot decides what to do with it.
    pub fn is_all_static(&self) -> bool {
        if self.app.is_some() || self.php.is_some() {
            return false;
        }
        if self.routes.is_empty() {
            return !self.mounts.is_empty();
        }
        self.routes
            .iter()
            .all(|route| route.static_policy().is_some())
    }

    /// True when the manifest needs no `BLASTIT_APP`: the file's one front
    /// controller answers every view (`entrypoint php`, §12.2), or the file is
    /// static mounts and no views (§9.5).
    ///
    /// This is the **one** answer to "does this file need a callable"; the boot's
    /// `runs_own_executors` and `is_static_only` are this predicate under the
    /// names the boot reads it by. An `entrypoint php` no view reaches is still
    /// `true` here: §12.2 makes that the `EntrypointNotReached` warning, never a
    /// missing app, so a static mount and a front controller can ship together
    /// before the catch-all view exists.
    ///
    /// The per-route arms are the retired per-route blocks' compatibility path
    /// (they are unreachable from a `Blastfile` and go with the pair in
    /// `MB.21.5.1`). A `Blastfile` never sets `route.php()` or
    /// `route.static_policy()`, so for it this reduces to §9.5's two shapes. An
    /// empty manifest is not bootable without an app.
    pub fn can_boot_without_app(&self) -> bool {
        let any_route_declares = self
            .routes
            .iter()
            .any(|route| route.php().is_some() || route.static_policy().is_some());
        if self.php.is_some() && !any_route_declares {
            // §12.2: one entrypoint, every view.
            return true;
        }
        if self.routes.is_empty() {
            return self.php.is_some() || !self.mounts.is_empty();
        }
        self.routes
            .iter()
            .all(|route| route.static_policy().is_some() || route.php().is_some())
    }

    /// The one-based line of every `view` header, in routing order.
    ///
    /// A boot error names the Blastfile line it came from, and the model no
    /// longer remembers where a `view` was written, so the reader keeps the
    /// header lines beside the routes.
    pub fn route_lines(&self) -> Vec<u32> {
        self.route_lines.clone()
    }

    /// The declared roots, mount index and absolute path, in mount order: the
    /// input of [`Manifest::check_static_roots`] and of the responder-uid
    /// visibility probe (`R13`, §9.6).
    ///
    /// A root belongs to a top-level `static "<prefix>"` mount, not to a view: a
    /// static-only file has mounts and no routes, and its roots still have to be
    /// audited (§9.5).
    pub fn static_roots(&self, manifest_dir: &Path) -> Vec<(usize, PathBuf)> {
        self.mounts
            .iter()
            .enumerate()
            .map(|(index, mount)| (index, manifest_dir.join(&mount.policy.root)))
            .collect()
    }

    /// Checks that every declared static root exists, is a directory and can be
    /// listed, relative to `manifest_dir` (`R2`, `R13`, §9.6).
    ///
    /// Deliberately separate from [`Manifest::validate`]: a manifest has to
    /// validate before its build output exists (the documentation fences are
    /// parsed from the repository root, where no `public/` does), while a boot
    /// or `check` must refuse a root it could never serve. Symlinks are
    /// followed, so a `current/public` deployment works.
    pub fn check_static_roots(&self, manifest_dir: &Path) -> Result<(), ManifestErrors> {
        let mut errors = Vec::new();
        for (index, mount) in self.mounts.iter().enumerate() {
            let policy = &mount.policy;
            let root = manifest_dir.join(&policy.root);
            let reason = match std::fs::metadata(&root) {
                Err(error) => Some(root_reason(&error)),
                Ok(metadata) if !metadata.is_dir() => Some(NOT_A_DIRECTORY),
                Ok(_) => match std::fs::read_dir(&root) {
                    Err(error) => Some(root_reason(&error)),
                    // A directory that answers its first entry with an error is
                    // not listable, and a root the responder cannot list is one
                    // it cannot serve.
                    Ok(mut entries) => match entries.next() {
                        Some(Err(error)) => Some(root_reason(&error)),
                        Some(Ok(_)) | None => None,
                    },
                },
            };
            if let Some(reason) = reason {
                errors.push(ManifestError::StaticRootUnusable {
                    location: Location::Mount {
                        index,
                        prefix: mount.prefix.clone(),
                    },
                    root: policy.root.clone(),
                    reason,
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ManifestErrors::new(errors))
        }
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

    /// The same manifest with its top-level `app`; parsing is the only caller.
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_app(mut self, app: Option<String>) -> Self {
        self.app = app;
        self
    }

    /// The same manifest with its application name; parsing is the only caller
    /// (§4.3).
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// The same manifest with its static mounts; parsing is the only caller
    /// (§9.1).
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_mounts(mut self, mounts: Vec<StaticMount>) -> Self {
        self.mounts = mounts;
        self
    }

    /// The same manifest with its one front controller; parsing is the only
    /// caller (§12.2).
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_php(mut self, php: Option<PhpPolicy>) -> Self {
        self.php = php;
        self
    }

    /// The same manifest with its top-level `preload`; parsing is the only
    /// caller.
    ///
    /// `#[doc(hidden)]`: the crate's integration tests build the expected model
    /// of the §20 R8 parity oracle with it.
    #[doc(hidden)]
    pub fn with_preload(mut self, preload: Vec<String>) -> Self {
        self.preload = preload;
        self
    }

    /// The same manifest with the lines its `view` headers were written on;
    /// the reader is the only caller.
    pub(crate) fn with_route_lines(mut self, route_lines: Vec<u32>) -> Self {
        self.route_lines = route_lines;
        self
    }

    /// Whether a statement is already allowed by the route's own table grants
    /// or by the grants of a group it takes (`R34`).
    ///
    /// `blastit check` uses this to tell a `queries` entry that is really
    /// unused from one the route may run anyway through
    /// `[route.<db>.tables]` or an attached group: only the former is a
    /// `dead_grant`.
    ///
    /// The route's own `queries` are deliberately not consulted — that entry is
    /// the one being judged, so it cannot be its own coverage — but its table
    /// grants are, and so are the queries, tables and `unrestricted` flag of
    /// the global groups and the route's groups. The statement is matched
    /// exactly after normalisation, or by analysis against those table grants.
    pub fn is_query_covered_by_tables_or_groups(
        &self,
        route_index: usize,
        proxy: &ProxyName,
        normalized_sql: &str,
    ) -> bool {
        let Some(route) = self.routes.get(route_index) else {
            return false;
        };

        let mut tables = TableGrants::default();
        let mut queries: Vec<String> = Vec::new();
        let mut transaction_control = false;
        let mut unrestricted = false;

        // The route's own table grants cover a statement; its `queries` do not.
        if let Some(grant) = route.grant(proxy).and_then(Grant::as_postgresql) {
            tables.merge_from(grant.tables());
            transaction_control |= grant.transaction_control();
            unrestricted |= grant.is_unrestricted();
        }

        // Every group the route takes, globally or by name.
        for grant in self
            .global_groups
            .iter()
            .chain(route.groups().iter())
            .filter_map(|name| self.groups.get(name))
            .filter_map(|group| group.grant(proxy))
            .filter_map(Grant::as_postgresql)
        {
            for query in grant.queries() {
                if !queries.contains(query) {
                    queries.push(query.clone());
                }
            }
            tables.merge_from(grant.tables());
            transaction_control |= grant.transaction_control();
            unrestricted |= grant.is_unrestricted();
        }

        if unrestricted {
            return true;
        }
        Allowlist::new(queries, transaction_control)
            .with_tables(tables)
            .check(&parameterised(normalized_sql))
            .is_allowed()
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
            app: None,
            name: None,
            preload: Vec::new(),
            global_groups,
            proxies,
            groups,
            routes,
            mounts: Vec::new(),
            php: None,
            route_lines: Vec::new(),
            unresolved_grants,
        }
    }

    /// Builds a manifest from the parts the §20 R8 parity oracle compares.
    ///
    /// `#[doc(hidden)]`: `lower` is the only production builder (§4.3), and an
    /// integration test is a separate crate that cannot reach `assemble`. The
    /// oracle builds the **expected** model field by field with this, then
    /// compares it with [`Manifest::equivalent`], so both sides of every row are
    /// readable in one file and neither comes from a document.
    ///
    /// It takes no unresolved grants: a grant on an undeclared store is a
    /// refusal, so no fixture that lowers cleanly has one.
    #[doc(hidden)]
    pub fn from_parts(
        version: u32,
        global_groups: Vec<String>,
        proxies: BTreeMap<ProxyName, Proxy>,
        groups: BTreeMap<String, Group>,
        routes: Vec<Route>,
    ) -> Self {
        Self::assemble(version, global_groups, proxies, groups, routes, Vec::new())
    }
}

/// Rewrites the manifest's `$?` placeholders into the numbered form
/// PostgreSQL's parser accepts.
///
/// [`blastit_sqlpolicy::normalize`] writes every bind placeholder as `$?`, which
/// no SQL parser accepts, and the table-grant analysis parses the text it is
/// given. A statement taken from a manifest — or from an already normalised
/// observation — therefore has to get its numbers back before it can be
/// analysed. Every placeholder becomes `$1`: the analysis only asks which
/// tables, columns and functions a statement touches, and a parameter's number
/// says nothing about that.
///
/// Quoted regions are copied verbatim, so a column named `"a$?"` stays the
/// column the grant names instead of becoming `"a$1"`. This is deliberately not
/// a SQL lexer: it exists for one check-reported finding, and a statement it
/// rewrites imperfectly can only make the analysis deny, never allow.
fn parameterised(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut characters = sql.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\'' | '"' => {
                out.push(character);
                while let Some(inner) = characters.next() {
                    out.push(inner);
                    if inner != character {
                        continue;
                    }
                    // A doubled quote is an escaped one, not the end.
                    if characters.peek() == Some(&character) {
                        if let Some(second) = characters.next() {
                            out.push(second);
                        }
                        continue;
                    }
                    break;
                }
            }
            '$' if characters.peek() == Some(&'?') => {
                characters.next();
                out.push_str("$1");
            }
            other => out.push(other),
        }
    }
    out
}

/// The fixed reason for a root that is a file: the design pins the phrases so a
/// test never depends on `strerror` (`R2`).
const NOT_A_DIRECTORY: &str = "not a directory";

/// The fixed reason for a root the filesystem refuses (`R2`).
///
/// The three documented kinds are named exactly; anything else (a symlink loop,
/// a broken mount) is still reported without `strerror`, because the message is
/// part of the boot contract and must read the same on every host.
fn root_reason(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory",
        io::ErrorKind::NotADirectory => NOT_A_DIRECTORY,
        io::ErrorKind::PermissionDenied => "permission denied",
        _ => "cannot be read",
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// `assemble` is the parser's constructor, so the accessor has to return
    /// what it was handed. `parse` only ever passes `1` (validation refuses any
    /// other version), which is why these build the value directly: a
    /// `version()` that always answered `1` would hide a wrong version instead
    /// of carrying it.
    #[test]
    fn assemble_keeps_the_version_it_was_given() {
        fn assembled(version: u32) -> Manifest {
            Manifest::assemble(
                version,
                Vec::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
            )
        }
        assert_eq!(assembled(1).version(), 1);
        assert_eq!(assembled(2).version(), 2);
        assert_eq!(assembled(0).version(), 0);
    }

    /// The executor kind table is the one place a backend registry learns the
    /// spellings it accepts; the retired `fastcgi` key is no kind at all
    /// (`ADR-0010` §1) and the embedded PHP worker is `"php"` (`MB.3.5.4`).
    #[test]
    fn executor_kinds_are_the_one_table() {
        assert_eq!(Executor::ALL_KINDS, ["wsgi", "php"].as_slice());
        assert_eq!(Executor::View.kind(), "wsgi");
        assert!(!Executor::ALL_KINDS.contains(&"fastcgi"));

        let php = Executor::Php(PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: Some(4),
        });
        assert_eq!(php.kind(), "php");
        assert_eq!(php.clone(), php);
        assert_ne!(php, Executor::View);
    }

    /// `can_boot_without_app` accepts only a manifest whose every route has its
    /// own executor, and `is_all_static` the subset that declares no `app`.
    ///
    /// The per-route executors are built with the crate-private builders: no
    /// `Blastfile` can set either one, and both leave the model with the
    /// per-route pair in `MB.21.5.1` — but these predicates still read them.
    #[test]
    fn the_boot_predicates_read_every_routes_executor() {
        fn manifest(routes: Vec<Route>) -> Manifest {
            Manifest::assemble(
                1,
                Vec::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                routes,
                Vec::new(),
            )
        }
        fn route(name: &str) -> Route {
            Route::new(
                name.to_string(),
                vec![".*".to_string()],
                None,
                Vec::new(),
                false,
                false,
                BTreeMap::new(),
            )
        }
        let static_policy = StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            fallback: None,
            immutable: Vec::new(),
        };

        let static_site = manifest(vec![route("site").with_static(Some(static_policy.clone()))]);
        assert!(static_site.can_boot_without_app());
        assert!(static_site.is_all_static());

        // A plain forwarding route still needs the app, and an empty manifest
        // needs everything.
        let mixed = manifest(vec![
            route("site").with_static(Some(static_policy)),
            route("app"),
        ]);
        assert!(!mixed.can_boot_without_app());
        assert!(!mixed.is_all_static());

        // A PHP route is its own executor, so it needs no app — and it is still
        // not a static site (`MB.3.5.4`).
        let php = PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: None,
        };
        let php_site = manifest(vec![route("app").with_php(Some(php))]);
        assert!(php_site.can_boot_without_app());
        assert!(!php_site.is_all_static());

        let empty = manifest(Vec::new());
        assert!(!empty.can_boot_without_app());
        assert!(!empty.is_all_static());
    }

    /// `route_lines` returns each `view` header's line in routing order, and
    /// nothing for a document that declares no view.
    #[test]
    fn route_lines_are_the_header_lines_in_routing_order() {
        let manifest = Manifest::from_blastfile_str("view a \"/*\" {}\n\nview b \"/b\" {}\n")
            .expect("the manifest parses");
        assert_eq!(manifest.route_lines(), vec![1, 3]);

        let without = Manifest::from_blastfile_str("").expect("the manifest parses");
        assert_eq!(without.route_lines(), Vec::<u32>::new());
    }

    /// The document `equivalent` is tested against: every field a `Blastfile`
    /// can set is non-default, so a comparison that skipped one would still
    /// pass. The per-route `static`/`php` pair and `global_groups` have no file
    /// spelling; `equivalent_honours_every_other_field` pins them through the
    /// model builders.
    const EQUIVALENT: &str = r#"app "shop"

entrypoint python "shop.wsgi:application" {
    preload "shop.boot"
}

postgres $DATABASE_URL

trait shared {
    postgres { select cms_page(id) }
}

view app "/*" {
    use shared
    postgres { query 'SELECT "cms_page"."body" FROM "cms_page"' }
}
"#;

    fn equivalent_fixture() -> Manifest {
        Manifest::from_blastfile_str(EQUIVALENT).expect("the fixture lowers")
    }

    /// [`Manifest::equivalent`] leaves out the source positions and nothing
    /// else: a document moved down one line is the same manifest, and every
    /// other field is compared.
    #[test]
    fn equivalent_ignores_only_the_route_lines() {
        let base = equivalent_fixture();
        let moved = Manifest::from_blastfile_str(&format!("\n{EQUIVALENT}")).expect("lowers");
        assert_ne!(
            base.route_lines(),
            moved.route_lines(),
            "the fixture is not testing what it claims"
        );
        assert!(base.equivalent(&moved));
        assert!(moved.equivalent(&base));

        // A manifest is equivalent to its own clone, and to itself.
        assert!(base.equivalent(&base.clone()));
        assert!(base.equivalent(&base));
    }

    /// Every field but `route_lines` is honoured: one field changed at a time
    /// makes the two manifests unequal, in both directions.
    #[test]
    fn equivalent_honours_every_other_field() {
        let base = equivalent_fixture();

        /// The parts `assemble` takes, so a pair can differ in exactly one.
        fn parts(
            version: u32,
            global_groups: Vec<String>,
            proxies: BTreeMap<ProxyName, Proxy>,
            groups: BTreeMap<String, Group>,
            routes: Vec<Route>,
            unresolved_grants: Vec<UnresolvedGrant>,
        ) -> Manifest {
            Manifest::assemble(
                version,
                global_groups,
                proxies,
                groups,
                routes,
                unresolved_grants,
            )
        }

        let same = || {
            parts(
                1,
                base.global_groups().to_vec(),
                base.proxies().clone(),
                base.groups().clone(),
                base.routes().to_vec(),
                base.unresolved_grants().to_vec(),
            )
        };
        let with_version = |version| {
            parts(
                version,
                base.global_groups().to_vec(),
                base.proxies().clone(),
                base.groups().clone(),
                base.routes().to_vec(),
                base.unresolved_grants().to_vec(),
            )
        };
        let with_groups = |global_groups: Vec<String>| {
            parts(
                1,
                global_groups,
                base.proxies().clone(),
                base.groups().clone(),
                base.routes().to_vec(),
                base.unresolved_grants().to_vec(),
            )
        };
        let with_proxies = |proxies: BTreeMap<ProxyName, Proxy>| {
            parts(
                1,
                base.global_groups().to_vec(),
                proxies,
                base.groups().clone(),
                base.routes().to_vec(),
                base.unresolved_grants().to_vec(),
            )
        };
        let with_grant_groups = |groups: BTreeMap<String, Group>| {
            parts(
                1,
                base.global_groups().to_vec(),
                base.proxies().clone(),
                groups,
                base.routes().to_vec(),
                base.unresolved_grants().to_vec(),
            )
        };
        let with_routes = |routes: Vec<Route>| {
            parts(
                1,
                base.global_groups().to_vec(),
                base.proxies().clone(),
                base.groups().clone(),
                routes,
                base.unresolved_grants().to_vec(),
            )
        };
        let with_unresolved = |unresolved_grants: Vec<UnresolvedGrant>| {
            parts(
                1,
                base.global_groups().to_vec(),
                base.proxies().clone(),
                base.groups().clone(),
                base.routes().to_vec(),
                unresolved_grants,
            )
        };

        let php = PhpPolicy {
            entry: "public/index.php".to_string(),
            workers: Some(2),
        };
        let mount = StaticMount {
            prefix: "/assets".to_string(),
            policy: StaticPolicy {
                root: "public/assets".to_string(),
                index: "index.html".to_string(),
                fallback: None,
                immutable: vec!["/assets/*".to_string()],
            },
        };
        let unresolved = UnresolvedGrant::new(
            ProxyName::from("db"),
            Location::Route {
                index: 0,
                name: "app".to_string(),
            },
        );

        let pairs: [(&str, Manifest, Manifest); 11] = [
            ("version", with_version(1), with_version(2)),
            (
                "app",
                base.clone()
                    .with_app(Some("shop.wsgi:application".to_string())),
                base.clone()
                    .with_app(Some("other.wsgi:application".to_string())),
            ),
            (
                "name",
                base.clone().with_name(Some("shop".to_string())),
                base.clone().with_name(None),
            ),
            (
                "preload",
                base.clone().with_preload(vec!["shop.boot".to_string()]),
                base.clone().with_preload(Vec::new()),
            ),
            (
                "global_groups",
                with_groups(vec!["shared".to_string()]),
                with_groups(Vec::new()),
            ),
            (
                "proxies",
                with_proxies(base.proxies().clone()),
                with_proxies(BTreeMap::new()),
            ),
            (
                "groups",
                with_grant_groups(base.groups().clone()),
                with_grant_groups(BTreeMap::new()),
            ),
            (
                "routes",
                with_routes(base.routes().to_vec()),
                with_routes(Vec::new()),
            ),
            (
                "mounts",
                base.clone().with_mounts(vec![mount]),
                base.clone().with_mounts(Vec::new()),
            ),
            (
                "php",
                base.clone().with_php(Some(php)),
                base.clone().with_php(None),
            ),
            (
                "unresolved_grants",
                with_unresolved(vec![unresolved]),
                with_unresolved(Vec::new()),
            ),
        ];

        assert!(same().equivalent(&same()), "the parts are stable");
        for (field, left, right) in pairs {
            assert!(!left.equivalent(&right), "{field} is not compared");
            assert!(
                !right.equivalent(&left),
                "{field} is not compared, reversed"
            );
        }
    }

    /// `normalize` writes every bind placeholder as `$?`, which no SQL parser
    /// accepts. Only that spelling is rewritten.
    #[test]
    fn only_the_question_mark_placeholder_is_rewritten() {
        assert_eq!(
            parameterised(r#"SELECT "t"."a" FROM "t" WHERE "t"."id" = $?"#),
            r#"SELECT "t"."a" FROM "t" WHERE "t"."id" = $1"#
        );
        assert_eq!(parameterised("SELECT $?, $?"), "SELECT $1, $1");

        // A numbered placeholder is already analysable, and a `$` that opens
        // nothing (a dollar in an identifier, a trailing one) is not a
        // placeholder either: inventing a `$1` there would change the table
        // name the analysis resolves.
        assert_eq!(parameterised("SELECT $2 FROM t"), "SELECT $2 FROM t");
        assert_eq!(parameterised("SELECT a$ FROM t"), "SELECT a$ FROM t");
        assert_eq!(parameterised("SELECT a$"), "SELECT a$");

        // Quoted regions are copied verbatim, doubled quotes included, so a
        // column named `"a$?"` stays the column the grant names, and a value
        // spelled `'it''s $?'` stays one literal.
        assert_eq!(
            parameterised(r#"SELECT "a$?"."b" FROM "a$?""#),
            r#"SELECT "a$?"."b" FROM "a$?""#
        );
        assert_eq!(
            parameterised("SELECT 1 WHERE x = 'it''s' AND y = $?"),
            "SELECT 1 WHERE x = 'it''s' AND y = $1"
        );
        assert_eq!(
            parameterised("SELECT 1 WHERE x = 'a$?'"),
            "SELECT 1 WHERE x = 'a$?'"
        );
        assert_eq!(
            parameterised("SELECT 1 WHERE x = 'a"),
            "SELECT 1 WHERE x = 'a"
        );
        assert_eq!(parameterised(""), "");
    }

    /// `--learn` inserts a new view above the catch-all. A `Blastfile`
    /// `view app "/*"` lowers to `/.*` (§4.3), so the predicate accepts that
    /// spelling as well as the bare `.*`, and still refuses a prefix pattern or
    /// a method-restricted catch-all.
    #[test]
    fn the_lowered_catch_all_is_unrestricted_too() {
        fn route(path: &[&str], methods: Option<&[&str]>) -> Route {
            Route::new(
                "app".to_string(),
                path.iter().map(|pattern| (*pattern).to_string()).collect(),
                methods.map(|list| list.iter().map(|method| (*method).to_string()).collect()),
                Vec::new(),
                false,
                false,
                BTreeMap::new(),
            )
        }

        let unrestricted: [&[&str]; 3] = [&[".*"], &["/.*"], &["^.*$"]];
        for path in unrestricted {
            assert!(
                route(path, None).is_unrestricted_catch_all(),
                "for path {path:?}"
            );
        }

        let restricted: [&[&str]; 3] = [&["/products/.*"], &["/"], &[]];
        for path in restricted {
            assert!(
                !route(path, None).is_unrestricted_catch_all(),
                "for path {path:?}"
            );
        }

        // A catch-all restricted to some methods still falls through for
        // another method, whatever spelling it uses.
        assert!(!route(&["/.*"], Some(&["GET", "HEAD"])).is_unrestricted_catch_all());
        assert!(!route(&[".*"], Some(&["GET"])).is_unrestricted_catch_all());
    }
}
