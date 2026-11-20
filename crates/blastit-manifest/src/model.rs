//! The parsed manifest: proxies, groups, routes, grants and the effective
//! grants a route ends up with (§5.1, §6).

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use blastit_ingress::IngressPolicy;
use blastit_sqlpolicy::{Allowlist, TableGrants, redact_literals, restore_parameters};
use regex::Regex;

use crate::blastfile::{compile_pattern_text, compile_regex_text};
use crate::error::{Location, ManifestError, ManifestErrors, ManifestWarning, Position};
use crate::routing::compile_pattern as compile_anchored;
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

/// Where one `queries` entry was written (§13.4.5, `R34`).
///
/// [`PgGrant::query_sites`] has exactly one of these per
/// [`PgGrant::queries`] entry, in the same order — always, including for a
/// grant built by [`PgGrant::new`], whose single site has
/// [`Position::NONE`]. `NONE` means "this entry has no site of its own", never
/// "the grant came from no document": a programmatic grant still has one entry
/// per query, so the two vectors are zipped, never searched.
///
/// The site holds no copy of the source: `at` is the position of the first
/// character **inside** the opening quote (so offset `0` is the statement's
/// first character), `interior` is that character's byte range up to the
/// closing quote, `escapes` lists the decoded character indices whose source
/// spelling is a two-character escape, and `raw` is the decoded statement
/// before [`blastit_sqlpolicy::normalize`], which is what the template check
/// and every offset run on.
///
/// **Bound.** One site is one `Position`, one `String` and one `u32` per
/// escape. An escape costs two source characters, so a grant holds at most
/// twice its own `query` text; the document is capped at
/// [`crate::blastfile::MAX_SOURCE_BYTES`] and no second cap is added.
#[derive(Clone, PartialEq, Eq)]
pub struct QuerySite {
    /// The position of the first character inside the opening quote.
    ///
    /// For `query ''` it is the closing quote — the empty statement's only
    /// honest site — and [`Position::NONE`] for a programmatic grant.
    pub at: Position,
    /// The interior's source **byte** range: its start is `at`'s character and
    /// its end is the closing quote, so `'`/`"` being one byte each makes this
    /// `token.start + 1 .. token.end - 1`.
    pub interior: Range<u32>,
    /// The decoded→source map: the decoded character indices whose source
    /// spelling is a two-character escape (`\"`, `\\`, `\n`, `\t`). Strictly
    /// increasing, and empty for the `'…'` spelling, which has no escapes.
    pub escapes: Vec<u32>,
    /// The raw decoded statement, before [`blastit_sqlpolicy::normalize`].
    pub raw: String,
}

impl QuerySite {
    /// A site for a grant no document wrote: one entry, no position (§13.4.5).
    fn unsited(query: String) -> Self {
        Self {
            at: Position::NONE,
            interior: 0..0,
            escapes: Vec::new(),
            raw: query,
        }
    }

    /// The position of a decoded character offset into [`QuerySite::raw`].
    ///
    /// The walk starts at [`QuerySite::at`] and counts **characters**, as
    /// [`Position`] does: an escape costs two source columns, a raw newline
    /// moves to the next line, and a `\n` *escape* costs two columns on the
    /// same line because the source has no line break there. `escapes` is read
    /// with one cursor, so the walk is `O(offset)`.
    ///
    /// [`Position::NONE`] stays `NONE` for every offset: a site with no
    /// position of its own has no position for its characters either. An
    /// offset past the end of [`QuerySite::raw`] is a bug in the caller, not a
    /// site: it falls back to [`QuerySite::at`], the string's own origin,
    /// rather than inventing a column (§13.4.5).
    pub fn position_at(&self, offset: u32) -> Position {
        if self.at.is_none() {
            return Position::NONE;
        }
        let (mut line, mut column) = (self.at.line(), self.at.col());
        let mut escapes = self.escapes.iter().copied().peekable();
        let mut walked: u32 = 0;
        for (index, character) in self.raw.chars().enumerate().take(offset as usize) {
            if escapes.peek() == Some(&(index as u32)) {
                escapes.next();
                // The source spelled this character as `\x`: two columns.
                column += 2;
            } else if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            walked = walked.saturating_add(1);
        }
        if walked < offset {
            return self.at;
        }
        Position::at(line, column)
    }
}

/// [`QuerySite`]'s `Debug` redacts the statement it carries (§13.4.5, §17.1).
///
/// [`PgGrant`] derives `Debug`, so a derived site would print a pasted value
/// from any `{:?}` — the same rule [`blastit_sqlpolicy::redact_literals`]
/// enforces on every other surface.
impl fmt::Debug for QuerySite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuerySite")
            .field("at", &self.at)
            .field("interior", &self.interior)
            .field("escapes", &self.escapes)
            .field("raw", &redact_literals(&self.raw))
            .finish()
    }
}

/// The contents of a grant on a `postgresql` proxy (§5.2): the statements the
/// route may run, whether transaction control is allowed, whether every
/// statement is allowed instead (§6, `unrestricted = true`), and which tables,
/// columns and functions its statements may touch (§6, D7).
///
/// [`PartialEq`] compares the grant's **semantics** and not its
/// [`QuerySite`]s: a site is source metadata, exactly as `route_lines` is, so
/// [`Manifest::equivalent`] and the parity oracle keep passing on grants the
/// builder produces. `Debug` derives, so `QuerySite`'s own `Debug` redacts.
#[derive(Clone, Debug, Default)]
pub struct PgGrant {
    queries: Vec<String>,
    query_sites: Vec<QuerySite>,
    transaction_control: bool,
    unrestricted: bool,
    tables: TableGrants,
}

impl PartialEq for PgGrant {
    fn eq(&self, other: &Self) -> bool {
        self.queries == other.queries
            && self.transaction_control == other.transaction_control
            && self.unrestricted == other.unrestricted
            && self.tables == other.tables
    }
}

impl Eq for PgGrant {}

impl PgGrant {
    /// Builds a grant from its manifest fields.
    pub fn new(queries: Vec<String>, transaction_control: bool) -> Self {
        let query_sites = queries.iter().cloned().map(QuerySite::unsited).collect();
        Self {
            queries,
            query_sites,
            transaction_control,
            unrestricted: false,
            tables: TableGrants::default(),
        }
    }

    /// The same grant with the sites its queries were written at (§13.4.5).
    ///
    /// `#[doc(hidden)]`: only the parser records sites, and it passes one per
    /// `queries` entry. A length mismatch is not an error the model can report
    /// — [`PgGrant::query_sites`] and `check_queries` read the site for an
    /// index and fall back to [`Position::NONE`], so the worst case is a
    /// refusal without a position, never a wrong one.
    #[doc(hidden)]
    pub fn with_query_sites(mut self, query_sites: Vec<QuerySite>) -> Self {
        self.query_sites = query_sites;
        self
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
            query_sites: Vec::new(),
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

    /// Where each [`PgGrant::queries`] entry was written (§13.4.5, `R34`).
    ///
    /// Exactly one entry per query, in the same order, whatever built the
    /// grant: a grant from a document carries the `line:col` of each string's
    /// first character inside the opening quote, and one built by
    /// [`PgGrant::new`] carries [`Position::NONE`] for every entry. The two
    /// vectors are zipped, so an entry never has two sites and a duplicate
    /// spelling that lowered to one entry keeps the first one.
    pub fn query_sites(&self) -> &[QuerySite] {
        &self.query_sites
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
    ///
    /// The sites follow their query, so the two vectors stay parallel: a query
    /// `self` already has keeps **its** site, and a query only `other` has
    /// brings `other`'s site along.
    pub fn merge_from(&mut self, other: &Self) {
        self.transaction_control |= other.transaction_control;
        self.unrestricted |= other.unrestricted;
        for (index, query) in other.queries.iter().enumerate() {
            if !self.queries.contains(query) {
                self.queries.push(query.clone());
                self.query_sites.push(
                    other
                        .query_sites
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| QuerySite::unsited(query.clone())),
                );
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

/// What one `static` block's `immutable` directive names (`R5`, §9.4).
///
/// Three shapes, and the grammar of each is the one a `view` pattern already
/// has (§8.1): bare `immutable` is [`Self::All`], `immutable <pattern>[,
/// <pattern>…]` is [`Self::Patterns`], and a mount with no `immutable` is
/// [`Self::None`]. There is no fourth spelling and no second matcher: every
/// pattern was compiled by `blastfile::pattern` and anchored by
/// `routing::compile_pattern` when the mount was lowered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StaticImmutable {
    /// No `immutable` directive: every answer revalidates.
    #[default]
    None,
    /// Bare `immutable`: every **exact file** the mount serves is cached for a
    /// year (§9.4).
    ///
    /// A directory `index` and a `fallback` answer are still `no-cache`
    /// (§4.5, `S3`): the responder asks [`StaticImmutable::matches`] about an
    /// exact-file answer only. It is a value of its own rather than a `"/*"`
    /// pattern so the responder can tell the author's promise — "every name in
    /// this tree changes with its bytes" — from a written catch-all.
    All,
    /// `immutable <pattern>[, …]`: the compiled patterns, in written order.
    Patterns(Vec<StaticPattern>),
}

impl StaticImmutable {
    /// True for bare `immutable`.
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// The compiled patterns, in written order: empty for `None` and `All`.
    pub fn patterns(&self) -> &[StaticPattern] {
        match self {
            Self::Patterns(patterns) => patterns,
            Self::None | Self::All => &[],
        }
    }

    /// The pattern texts, in written order — what `blastit fmt` prints, and
    /// what a `--json` payload would carry.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.patterns().iter().map(StaticPattern::text)
    }

    /// Whether an **exact-file** answer whose path is `url_path` is cached for
    /// a year (`R5`, §9.4).
    ///
    /// `url_path` is the path the client asked for, mount prefix included
    /// (`StaticFile::url_path`), never the root-relative remainder. The match is
    /// one anchored comparison: the program was compiled when the mount was
    /// lowered, so a request never compiles a pattern.
    pub fn matches(&self, url_path: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Patterns(patterns) => patterns.iter().any(|pattern| pattern.matches(url_path)),
        }
    }
}

/// Which spelling a pattern was written in (§8.1).
///
/// It is recorded rather than re-derived from the text because the two
/// spellings share a compiled form but not a text meaning: `regex '/assets/*'`
/// is `/assets` followed by zero or more `/`, which is not what the quoted
/// grammar's trailing `/*` means. Matching is the compiled program either way;
/// equality ([`StaticPattern`]) and a surface that shows a pattern read this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spelling {
    /// `"/assets/*"`: the compiled grammar (escaped literals, `{…}`, trailing `/*`).
    Quoted,
    /// `regex '…'`: the expression verbatim, the escape hatch.
    Regex,
}

/// One `immutable` pattern: the text as written, the spelling it was written
/// in, and the anchored matcher the one compiler built (§8.1).
///
/// Equality is `(text, spelling)`: a quoted `"/a/*"` and `regex '/a/*'` share a
/// compiled form but not a meaning — `*` is a tail crossing `/` in one and a
/// repeat of the previous atom in the other — so they cache different paths and
/// must not compare equal, or [`Manifest::equivalent`] would accept a spelling
/// flip that changes behaviour. A compiled program has no meaningful structural
/// equality, so it is never compared.
#[derive(Clone, Debug)]
pub struct StaticPattern {
    /// The pattern's value, decoded for `"…"` and verbatim for `'…'`.
    text: String,
    /// The spelling the author wrote (§8.1).
    spelling: Spelling,
    /// `^(?:…)$` over the fragment `pattern.rs` emitted, built once.
    matcher: Regex,
}

impl StaticPattern {
    /// Compiles one quoted pattern — the same scan, caps and refusals a `view`
    /// pattern gets (`/`-anchored, `*` only at the end after a `/`, `{name}`,
    /// `{name:int}`, `{name:uuid}`, at most 4096 bytes).
    ///
    /// `None` for a pattern the compiler refuses. A `Blastfile` never reaches
    /// this answer: the parser compiles the same text first and refuses it with
    /// the sentence and `line:col`; this is the constructor for a policy built
    /// by hand (tests, the retired reader).
    pub fn quoted(pattern: &str) -> Option<Self> {
        let compiled = compile_pattern_text(pattern, Position::NONE).ok()?;
        Self::from_fragment(
            compiled.text().to_owned(),
            compiled.fragment(),
            Spelling::Quoted,
        )
    }

    /// Compiles one `regex '…'` pattern: the expression kept verbatim and
    /// compiled as written (§8.1).
    ///
    /// `None` when the `regex` crate refuses it, or it is over the cap.
    pub fn regex(expression: &str) -> Option<Self> {
        let compiled = compile_regex_text(expression, Position::NONE).ok()?;
        Self::from_fragment(
            compiled.text().to_owned(),
            compiled.fragment(),
            Spelling::Regex,
        )
    }

    /// A pattern from its text, its spelling and the anchored matcher the
    /// caller compiled.
    pub(crate) fn new(text: String, spelling: Spelling, matcher: Regex) -> Self {
        Self {
            text,
            spelling,
            matcher,
        }
    }

    /// A pattern from the text and the fragment the one compiler emitted.
    ///
    /// `quoted`/`regex` build a policy by hand; `lower` compiles the fragment it
    /// already has, so the anchor is compiled once and a request never pays for
    /// it.
    pub(crate) fn from_fragment(text: String, fragment: &str, spelling: Spelling) -> Option<Self> {
        let matcher = compile_anchored(fragment).ok()?;
        Some(Self::new(text, spelling, matcher))
    }

    /// The pattern as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// True for `regex '…'`, false for a quoted pattern (§8.1).
    ///
    /// It is recorded because the two spellings share a compiled form but not a
    /// text one: a consumer that shows, edits or equality-compares a pattern
    /// must not read `regex '/assets/*'` with the quoted grammar's meaning.
    /// Matching never consults it — [`StaticPattern::matches`] is the compiled
    /// program, so both spellings are matched by the one matcher.
    pub fn is_regex(&self) -> bool {
        self.spelling == Spelling::Regex
    }

    /// Whether the pattern matches the whole of `url_path` (§8.1).
    pub fn matches(&self, url_path: &str) -> bool {
        self.matcher.is_match(url_path)
    }
}

impl PartialEq for StaticPattern {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.spelling == other.spelling
    }
}

impl Eq for StaticPattern {}

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
    /// The files that never change: bare `immutable` caches every exact file
    /// the mount serves, a pattern list caches the exact files it matches, and
    /// no directive caches none (`R5`, §9.4).
    pub immutable: StaticImmutable,
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
    /// What the mount serves: the directory, its index and its cache patterns.
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
            .check(&restore_parameters(normalized_sql))
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
