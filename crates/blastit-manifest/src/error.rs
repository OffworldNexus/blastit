//! What can be wrong with a manifest: parse errors, validation errors and the
//! non-fatal warnings (§6).

use std::fmt;
use std::fmt::Write as _;

use crate::model::ProxyName;

/// Longest value quoted in an error message before it is cut short.
pub(crate) const MAX_QUOTED_CHARS: usize = 120;

/// Escapes control characters, so a name or a path never breaks a line.
///
/// This is the CLI's renderer for the one thing the crate never quotes — the
/// path a caller passed in — so it is public rather than crate-private (§18).
pub fn escape_control(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", character as u32);
            }
            character => out.push(character),
        }
    }
    out
}

/// Renders a name, path or pattern for an error message: control characters
/// escaped and long values cut to 120 characters, without the surrounding
/// quotes the caller adds.
///
/// §18's one line is this function, so a caller that renders a path or a
/// message of its own calls it rather than keeping a second copy.
pub fn clipped(value: &str) -> String {
    let cut = value.char_indices().nth(MAX_QUOTED_CHARS).map(|(at, _)| at);
    let mut out = String::with_capacity(value.len() + 1);
    match cut {
        Some(at) => {
            out.push_str(&escape_control(value.get(..at).unwrap_or("")));
            out.push('…');
        }
        None => out.push_str(&escape_control(value)),
    }
    out
}

/// Renders a name, path or pattern for an error message: quoted, with control
/// characters escaped and long values cut to [`MAX_QUOTED_CHARS`] characters.
pub(crate) fn quoted(value: impl fmt::Display) -> String {
    format!("\"{}\"", clipped(&value.to_string()))
}

/// Where in a `Blastfile` a refusal points (§18): a 1-based `line` and a
/// 1-based `col` counted in `char`s, or [`Position::NONE`] for a whole-file
/// refusal, which renders without a position at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    line: u32,
    col: u32,
}

impl Position {
    /// No position: a whole-file refusal such as the size cap.
    pub const NONE: Self = Self { line: 0, col: 0 };

    /// A 1-based position; `col` counts `char`s, as an editor does.
    pub const fn at(line: u32, col: u32) -> Self {
        Self { line, col }
    }

    /// The 1-based line, or `0` when there is no position.
    pub const fn line(self) -> u32 {
        self.line
    }

    /// The 1-based column, or `0` when there is no position.
    pub const fn col(self) -> u32 {
        self.col
    }

    /// True for [`Position::NONE`].
    pub const fn is_none(self) -> bool {
        self.line == 0
    }
}

impl fmt::Display for Position {
    /// Renders `line:col`, or nothing at all for [`Position::NONE`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return Ok(());
        }
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// The `line:col: ` a positioned refusal starts with, or nothing at all for
/// [`Position::NONE`] (§18).
pub(crate) fn prefix(at: Position) -> String {
    if at.is_none() {
        String::new()
    } else {
        format!("{}:{}: ", at.line, at.col)
    }
}

/// A regex compile failure as a single line: the `regex` crate reports syntax
/// errors with a source excerpt, and the last line is the actual problem.
pub(crate) fn regex_message(error: &regex::Error) -> String {
    let text = error.to_string();
    let last = text
        .lines()
        .last()
        .map_or(text.as_str(), |line| line)
        .trim();
    escape_control(last)
}

/// Where in the manifest something sits, so an error can name it precisely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// The top-level `groups` list, which attaches a group to every route.
    Global,
    /// A group, which a `Blastfile` spells `trait <name> { … }` (§11).
    ///
    /// The model keeps the section's old name; it renders `trait "<name>"` —
    /// the noun a developer edits (§11.3).
    Group(String),
    /// A route, by index and by name.
    Route {
        /// Index of the route in the manifest.
        index: usize,
        /// The route's name, empty when the name itself failed to parse.
        name: String,
    },
    /// A top-level `static "<prefix>"` mount, by index and by prefix (§9.1).
    Mount {
        /// Index of the mount in the manifest.
        index: usize,
        /// The prefix as written.
        prefix: String,
    },
    /// A `Blastfile` trait, by name (§11.3).
    ///
    /// It renders `trait "<name>"` — the noun a developer edits (§11.3).
    Trait(String),
    /// A `view`, by sandbox index and by name (§7.3, §10.3).
    ///
    /// One `view` may lower to several routes, so the location names the view
    /// rather than a route index. The sandbox index is kept in the variant for
    /// the callers that key on it; it is not printed, because a view is named,
    /// not numbered, and `(index 0)` in a refusal would send a reader looking
    /// for a numbering the file does not have (§18).
    View {
        /// The view's sandbox, which is its index in file order.
        index: usize,
        /// The view's name.
        name: String,
    },
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => f.write_str("global groups"),
            Self::Group(group) => write!(f, "trait {}", quoted(group)),
            Self::Route { index, name } => {
                write!(f, "route {} (index {index})", quoted(name))
            }
            Self::Mount { index, prefix } => {
                write!(f, "mount {} (index {index})", quoted(prefix))
            }
            Self::Trait(name) => write!(f, "trait {}", quoted(name)),
            Self::View { name, .. } => write!(f, "view {}", quoted(name)),
        }
    }
}

/// One problem with a manifest.
///
/// Reading a `Blastfile` reports the structural ones
/// ([`ManifestError::Equals`], [`ManifestError::UnknownKeyword`],
/// [`ManifestError::Unexpected`]…); validation reports the ones that need the
/// whole document ([`ManifestError::UnknownGroup`],
/// [`ManifestError::InvalidRegex`]…).
///
/// [`Display`](fmt::Display) always renders one line, whatever the manifest
/// contains.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Top-level `app` is not a `module:attribute` WSGI callable (`R7`).
    ///
    /// The value is **never** printed (§18): a pasted `$TOKEN` or DSN can sit
    /// in that slot, so the sentence names the shape the value must have
    /// instead. `app` is kept in the variant because the model compares it, not
    /// because a message quotes it.
    #[error("app: must be `module:attribute`")]
    InvalidApp {
        /// The value as written; compared, never rendered.
        app: String,
    },
    /// A group is referenced where it was never declared (§6).
    #[error("{location}: unknown group {}", quoted(.group))]
    UnknownGroup {
        /// Where the group is referenced.
        location: Location,
        /// The undeclared group name.
        group: String,
    },
    /// A grant names a store that no `postgres <name>` declares (§5.1, §6).
    #[error("{location}: grant on unknown proxy {}", quoted(.proxy))]
    GrantOnUnknownProxy {
        /// Where the grant is written.
        location: Location,
        /// The undeclared proxy name.
        proxy: ProxyName,
    },
    /// Two routes share a name; routes are addressed by name in the logs and by
    /// index in the uid table, so a duplicate is ambiguous.
    #[error(
        "duplicate route name {} (routes {first} and {second})",
        quoted(.name)
    )]
    DuplicateRouteName {
        /// The duplicated name.
        name: String,
        /// Index of the first route with that name.
        first: usize,
        /// Index of the second one.
        second: usize,
    },
    /// Two mounts share a prefix, so one of them could never answer (§9.1).
    #[error(
        "duplicate static mount {} (mounts {first} and {second})",
        quoted(.prefix)
    )]
    DuplicateMount {
        /// The duplicated prefix.
        prefix: String,
        /// Index of the first mount with that prefix.
        first: usize,
        /// Index of the second one.
        second: usize,
    },
    /// A route name does not match `[a-z0-9][a-z0-9_-]*`.
    #[error(
        "route {index}: invalid name {}; must match [a-z0-9][a-z0-9_-]*",
        quoted(.name)
    )]
    InvalidRouteName {
        /// Index of the route in the manifest.
        index: usize,
        /// The offending name.
        name: String,
    },
    /// A route lists no path regexes at all, so nothing can ever reach it.
    #[error("route {}: path must list at least one regex", quoted(.route))]
    EmptyPath {
        /// The route's name.
        route: String,
    },
    /// A route lists an empty path regex, which would only match an empty path.
    #[error(
        "route {} rule {rule}: the path regex must not be empty",
        quoted(.route)
    )]
    EmptyPattern {
        /// The route's name.
        route: String,
        /// Index of the pattern in the route's `path` list.
        rule: usize,
    },
    /// A path regex does not compile (§6).
    #[error(
        "route {} rule {rule}: invalid regex {}: {message}",
        quoted(.route),
        quoted(.pattern)
    )]
    InvalidRegex {
        /// The route's name.
        route: String,
        /// Index of the pattern in the route's `path` list.
        rule: usize,
        /// The pattern as written.
        pattern: String,
        /// The regex engine's message, on one line.
        message: String,
    },
    /// A method is not an uppercase HTTP method token.
    #[error(
        "route {}: invalid method {}; expected an uppercase HTTP method token",
        quoted(.route),
        quoted(.method)
    )]
    InvalidMethod {
        /// The route's name.
        route: String,
        /// The offending method token.
        method: String,
    },
    /// A route lists methods but none of them, so it can never be reached.
    #[error(
        "route {}: methods must list at least one method, or be left out",
        quoted(.route)
    )]
    EmptyMethods {
        /// The route's name.
        route: String,
    },
    /// The route carrying the highest sandbox would leave the uid range blastit
    /// stays in (§2, §7.3).
    ///
    /// Routes are numbered by **sandbox**, not by position, so the route that
    /// overflows is the one with the highest sandbox id — which is not
    /// necessarily the last route in the table.
    #[error(
        "too many routes: the route carrying the highest sandbox (index {index}) would use \
         uid {uid}, but uids must stay below {max} (base {base})"
    )]
    TooManyRoutes {
        /// Index of the route carrying the highest sandbox.
        index: usize,
        /// The uid that route would take.
        uid: u32,
        /// The uid base in use.
        base: u32,
        /// Exclusive upper bound of the uid range.
        max: u32,
    },
    /// The uid base itself leaves no room for blastit's own processes.
    #[error("uid base {base}: dbproxy would use uid {uid}, but uids must stay below {max}")]
    UidBaseTooHigh {
        /// The uid base in use.
        base: u32,
        /// The uid dbproxy would take.
        uid: u32,
        /// Exclusive upper bound of the uid range.
        max: u32,
    },
    /// A trait carries `unrestricted` (§6, §11.3). It is only accepted inline on
    /// a view, so that it can never be attached to views by accident.
    #[error(
        "{location}: unrestricted is only allowed inline on a view (grant on store {})",
        quoted(.proxy)
    )]
    UnrestrictedGroup {
        /// Where the grant is written.
        location: Location,
        /// The store the trait's grant names.
        proxy: ProxyName,
    },
    /// A grant carries both `unrestricted = true` and a list of queries (§6).
    ///
    /// The list would read as a restriction and is not one: the route may run
    /// anything on that proxy either way, so rather than leave a manifest that
    /// claims more than it does, the boot is refused.
    #[error(
        "{location}: unrestricted grants take no queries (grant on proxy {})",
        quoted(.proxy)
    )]
    UnrestrictedWithQueries {
        /// Where the grant is written.
        location: Location,
        /// The proxy the grant names.
        proxy: ProxyName,
    },
    /// A grant carries both `unrestricted = true` and table grants or a function
    /// list (§6, D7).
    ///
    /// Same rule as `queries`: the grant would read as a restriction that is not
    /// there, because the route may run anything on that proxy either way.
    #[error(
        "{location}: unrestricted grants take no {what} (grant on proxy {})",
        quoted(.proxy)
    )]
    UnrestrictedWithContent {
        /// Where the grant is written.
        location: Location,
        /// The proxy the grant names.
        proxy: ProxyName,
        /// What the grant should not carry: `tables` or `functions`.
        what: &'static str,
    },
    /// A table grant names one of PostgreSQL's system catalogues (§6, D7).
    ///
    /// `pg_catalog` and `information_schema` describe everything else in the
    /// database, and the analysis never reads them through a table grant, so an
    /// entry naming one could only ever be dead weight that reads like a
    /// capability.
    #[error(
        "{location}: {table} is a system catalogue, which a table grant can never allow (grant on proxy {})",
        quoted(.proxy)
    )]
    UnusableTable {
        /// Where the grant is written.
        location: Location,
        /// The proxy the grant names.
        proxy: ProxyName,
        /// The table as written.
        table: String,
    },
    /// A `queries` entry still holds the `…` that redaction writes (`R24`,
    /// `ADR-0003`).
    ///
    /// `blastit check --learn` refuses such a statement instead of writing it,
    /// and a copied `sql.denied` line is redacted by construction: granting it
    /// would allow a shape, not a statement. The entry itself is never printed
    /// — it may still hold a value next to the placeholder — so the message
    /// names its position instead.
    #[error(
        "{location}: queries[{rule}] holds a redacted `…` placeholder (grant on proxy {}); \
         grant the exact statement, with `$?` where the values bind",
        quoted(.proxy)
    )]
    RedactedQueryPlaceholder {
        /// Where the grant is written.
        location: Location,
        /// The proxy the grant names.
        proxy: ProxyName,
        /// Index of the entry in the grant's `queries` list.
        rule: usize,
    },
    /// A `queries` entry inlines a quoted literal outside
    /// [`blastit_sqlpolicy::KNOWN_STATIC_SETUP_SQL`] (`R24`, `ADR-0003`).
    ///
    /// An exact grant is trusted and run as written, so a statement that
    /// carries a value is a value committed to the repository — and the grant
    /// would only ever match that one value. The entry is never printed, since
    /// printing it is the very thing the rule forbids.
    #[error(
        "{location}: queries[{rule}] inlines a quoted literal (grant on proxy {}); \
         bind the value with `$?` instead of writing it in the statement",
        quoted(.proxy)
    )]
    LiteralQuery {
        /// Where the grant is written.
        location: Location,
        /// The proxy the grant names.
        proxy: ProxyName,
        /// Index of the entry in the grant's `queries` list.
        rule: usize,
    },
    /// A `static "<prefix>"` mount's prefix is not a URL prefix the router can
    /// match (§9.1).
    ///
    /// `rule` is one of the fixed phrases the design pins — `must start with /`,
    /// `must not end with /`, `must not contain ..`, `must not contain an empty
    /// segment`, `must not contain \`, `must not contain a control byte`,
    /// `must not contain ?`, `must not contain #`, `must not contain %` — so the
    /// message reads the same on every host. The location carries the prefix, so
    /// it is not printed twice.
    #[error("{location}: {rule}")]
    StaticMountPrefixInvalid {
        /// Where the `static` block is written.
        location: Location,
        /// The fixed rule the prefix breaks.
        rule: &'static str,
    },
    /// A mount's `fallback` is not a file name the responder can answer a miss
    /// with (§9.1, §9.3).
    ///
    /// The rules are the `index`'s — a relative path of one or more segments,
    /// no `..`, no `\`, no control byte — plus the one that matters here: no
    /// dotfile segment, because the fallback answers a request the request-path
    /// check never sees.
    #[error("{location}: static fallback {}: {rule}", quoted(.fallback))]
    StaticFallbackInvalid {
        /// Where the `static` block is written.
        location: Location,
        /// The fallback as written.
        fallback: String,
        /// The fixed rule the fallback breaks.
        rule: &'static str,
    },
    /// A `[route.static]` `root` is not a path the responder can serve (`R2`).
    ///
    /// `rule` is one of the fixed phrases the design pins — `must be a relative
    /// path`, `must not contain ..`, `must not contain \`, `must not contain a
    /// control byte` — so the message reads the same on every host.
    #[error("{location}: static root {}: {rule}", quoted(.root))]
    StaticRootInvalid {
        /// Where the `[route.static]` block is written.
        location: Location,
        /// The root as written.
        root: String,
        /// The fixed rule the root breaks.
        rule: &'static str,
    },
    /// A `[route.static]` `root` passes the syntax but the filesystem refuses
    /// it: missing, a file, or unlistable (`R2`, `R13`).
    ///
    /// Checked by [`crate::Manifest::check_static_roots`], never by validation:
    /// a manifest must validate before the build that creates the directory has
    /// run. `reason` is a fixed phrase (`no such file or directory`, `not a
    /// directory`, `permission denied`), never `strerror`.
    #[error("{location}: static root {}: {reason}", quoted(.root))]
    StaticRootUnusable {
        /// Where the `[route.static]` block is written.
        location: Location,
        /// The root as written.
        root: String,
        /// The fixed reason the filesystem gave.
        reason: &'static str,
    },
    /// A `[route.static]` `index` is not a file path beneath `root` (`R3`).
    #[error("{location}: static index {}: {rule}", quoted(.index))]
    StaticIndexInvalid {
        /// Where the `[route.static]` block is written.
        location: Location,
        /// The index as written.
        index: String,
        /// The fixed rule the index breaks.
        rule: &'static str,
    },
    /// An `immutable` pattern cannot be matched against an answering path
    /// (`R5`).
    ///
    /// For the count rule the value is the first pattern past the limit, the
    /// one an operator can delete.
    #[error("{location}: static immutable {}: {rule}", quoted(.pattern))]
    StaticImmutablePattern {
        /// Where the `[route.static]` block is written.
        location: Location,
        /// The pattern as written.
        pattern: String,
        /// The fixed rule the pattern breaks.
        rule: &'static str,
    },
    /// A static route that can never reach the app still declares something
    /// only the app could use (`R4`).
    ///
    /// `why` is `fallback "index.html" never reaches the app` or `the manifest
    /// declares no app`; `key` is the declaration, as a `Blastfile` writes it:
    /// `body json`, `prefork`, `use`, or the grant's `postgres [<name>] { … }`
    /// block. Each is dead weight that reads like a capability, so the boot
    /// refuses it rather than let it look like it runs.
    #[error("{location}: {why}; {} can never run", escape_control(.key))]
    StaticDeadDeclaration {
        /// Where the static route is declared.
        location: Location,
        /// The dead declaration, as written.
        key: String,
        /// Why the route can never reach the app.
        why: String,
    },
    /// A `[route.php]` `entry` is not a relative `.php` path beneath the
    /// manifest's directory (`ADR-0010` §2; design §4.1, unchanged from the
    /// retired key).
    ///
    /// `rule` is one of the fixed phrases the design pins — `must be a relative
    /// path`, `must not contain ..`, `must not contain \`, `must not contain a
    /// control byte`, `must end in .php` — and `must be under static root
    /// "<root>"` when the route also declares `[route.static]`. It is a
    /// `String` because that last phrase interpolates a value, and because the
    /// entry probe adds `must exist`, `must be a regular file`, `must be
    /// readable by uid <uid>` and `must be stat-able` to the same variant.
    ///
    /// The entry itself is **never** printed (§18): a pasted DSN or token can
    /// sit in that slot, so the sentence names the position (`view "app"`) and
    /// the rule; `entry` is kept for the model's equality, not for a message.
    #[error("{location}: php entry {rule}")]
    PhpEntryInvalid {
        /// Where the `[route.php]` block is written.
        location: Location,
        /// The entry as written; compared, never rendered.
        entry: String,
        /// The fixed rule the entry breaks.
        rule: String,
    },
    /// An explicit `[route.php]` `workers` cap is outside `1..=64`
    /// (`ADR-0010` §2).
    ///
    /// An absent `workers` is not an error: blastit derives the route's
    /// concurrency from its per-view budget, and `workers` may only lower it.
    /// `why` is `must be at least 1` or `at most 64`.
    #[error("{location}: php workers {workers}: {why}")]
    PhpWorkersInvalid {
        /// Where the `[route.php]` block is written.
        location: Location,
        /// The cap as written.
        workers: u32,
        /// Why the cap is refused.
        why: &'static str,
    },
    /// A `[route.php]` route carries a declaration that never reaches the front
    /// controller (`ADR-0010` §2).
    ///
    /// `key` names the declaration as a `Blastfile` writes it (`fallback
    /// "<file>"`, `CONNECT`, `TRACE`) and `why` is the sentence the design pins;
    /// the message is `route "app" (index 0): <why>`, exactly as the design's
    /// table renders it.
    #[error("{location}: {why}")]
    PhpDeadDeclaration {
        /// Where the `[route.php]` block is written.
        location: Location,
        /// The declaration that can never run.
        key: String,
        /// Why it can never reach the front controller.
        why: String,
    },
    /// `=` is not Blastfile syntax (§5.3).
    ///
    /// The lexer emits [`crate::blastfile`]'s `Equals` token only so the parser
    /// can refuse it here: `body = "json"` must name the fix, which is to write
    /// the value after the keyword.
    #[error("{}`=` is not Blastfile syntax; write the value after the keyword: `body json`", prefix(*at))]
    Equals {
        /// Where the `=` is.
        at: Position,
    },
    /// A quoted string where a store reads `$IDENT` (§5.3).
    ///
    /// The literal is **never** echoed: a pasted DSN or token can sit in that
    /// slot, and an error is written to logs and CI output (§17.1, R26).
    #[error(
        "{}a quoted string is not a variable name; a store reads `$IDENT`: `postgres $DATABASE_URL`",
        prefix(*at)
    )]
    QuotedStore {
        /// Where the string is.
        at: Position,
    },
    /// An environment-variable name that does not start `[A-Z_]` (§5.3).
    ///
    /// The name is a name, not a value, so it is safe to quote; it is escaped
    /// and cut like every other value (§18).
    #[error(
        "{}`${}` is not an environment variable name: names are upper-case, `[A-Z_][A-Z0-9_]*`, like `$DATABASE_URL`",
        prefix(*at),
        clipped(name)
    )]
    EnvName {
        /// Where the `$` is.
        at: Position,
        /// The name as written, without the `$`.
        name: String,
    },
    /// A `$` that starts neither a name nor a bind placeholder (§5.3).
    #[error(
        "{}`$` must start an environment variable name, `[A-Z_][A-Z0-9_]*`; `$?` is a bind placeholder and belongs inside a `query '…'` string",
        prefix(*at)
    )]
    EnvMarker {
        /// Where the `$` is.
        at: Position,
    },
    /// A `"…"` string with no closing quote (§5.3).
    #[error("{}unterminated string: the opening `\"` is on line {opened}", prefix(*at))]
    UnterminatedString {
        /// Where the opening quote is.
        at: Position,
        /// The line the opening quote is on.
        opened: u32,
    },
    /// A `'…'` string with no closing quote (§5.3).
    #[error("{}unterminated raw string: the opening `'` is on line {opened}", prefix(*at))]
    UnterminatedRawString {
        /// Where the opening quote is.
        at: Position,
        /// The line the opening quote is on.
        opened: u32,
    },
    /// A backslash that starts no escape the grammar allows (§5.3).
    ///
    /// The escape is one `char`, escaped so it cannot break the one line (§18).
    #[error(
        "{}`\\{}` is not an escape; the escapes are `\\\"`, `\\\\`, `\\n`, `\\t`",
        prefix(*at),
        clipped(&escape.to_string())
    )]
    InvalidEscape {
        /// Where the backslash is.
        at: Position,
        /// The character after the backslash.
        escape: char,
    },
    /// A `\r` that no `\n` follows (§5.3).
    #[error("{}carriage return without a newline is not Blastfile syntax", prefix(*at))]
    CarriageReturn {
        /// Where the `\r` is.
        at: Position,
    },
    /// A byte outside the token set that is not ASCII (§5.3).
    #[error(
        "{}byte 0x{byte:x} is not Blastfile syntax; quote the text, or start a name with a letter or a digit",
        prefix(*at)
    )]
    NonAsciiByte {
        /// Where the byte is.
        at: Position,
        /// The byte's first UTF-8 byte, as written in the file.
        byte: u8,
    },
    /// A `-` that follows no letter or digit (§5.3).
    #[error(
        "{}`-` is part of a name or a keyword only after a letter or a digit",
        prefix(*at)
    )]
    BareHyphen {
        /// Where the `-` is.
        at: Position,
    },
    /// A `Blastfile` over the 16 MiB cap (§4.5).
    ///
    /// Position-less: the caller prefixes the path exactly once, giving
    /// `<path>: the file is <n> bytes; …` (§18).
    #[error(
        "the file is {bytes} bytes; Blastfile is limited to {} bytes — split the grants into a `trait`, or delete a generated block",
        crate::blastfile::MAX_SOURCE_BYTES
    )]
    TooLarge {
        /// The size that was refused: the metadata length, or the bytes read.
        bytes: u64,
    },
    /// A path that is not a regular file: a FIFO, a device or a directory
    /// (§4.5).
    ///
    /// Position-less, like [`ManifestError::TooLarge`]. `read` refuses it from
    /// its metadata, before it opens anything, so a FIFO with no writer can
    /// never block the boot.
    #[error("the file is not a regular file; Blastfile must be a plain file")]
    NotRegularFile,
    /// A `Blastfile` that could not be read or is not UTF-8 (§4.5).
    ///
    /// Position-less, like [`ManifestError::TooLarge`]; `message` is the
    /// operating system's own sentence, escaped so it stays on one line.
    #[error("the file could not be read: {message}")]
    Unreadable {
        /// The error as the operating system reported it, escaped.
        message: String,
    },
    /// A token stream over §4.5's token cap.
    ///
    /// Position-less: the caller prefixes the path exactly once (§18). The
    /// advice is something the author can actually do to this one file: a
    /// `Blastfile` has no `include`, so "put the rest elsewhere" was never a
    /// remedy.
    #[error(
        "{} tokens; Blastfile is limited to {} — the file is too large: replace repeated `query '…'` lines with table or column grants (`select`, `insert`, `update`, `delete`), or fold the shared grants into a `trait` that many views `use`",
        tokens,
        crate::blastfile::MAX_TOKENS
    )]
    TooManyTokens {
        /// The count that was refused: one past the cap.
        tokens: usize,
    },
    /// A trivia stream over §4.5's trivia cap.
    ///
    /// Position-less like [`ManifestError::TooManyTokens`], with the same
    /// remedy: the file is too large, and its comments and blank runs are what
    /// this budget counts.
    #[error(
        "{} trivia runs; Blastfile is limited to {} — the file is too large: delete the generated comments and blank lines, or fold the shared grants into a `trait` that many views `use`",
        runs,
        crate::blastfile::MAX_TRIVIA
    )]
    TooManyTrivia {
        /// The count that was refused: one past the cap.
        runs: usize,
    },
    /// A word at the top level that is not a section keyword (§6.7).
    ///
    /// The word is a `Word` — `[A-Za-z0-9_-]` — so it is a name, never a
    /// value, and it is safe to quote (§18).
    #[error(
        "{}`{}` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`",
        prefix(*at),
        clipped(word)
    )]
    UnknownKeyword {
        /// Where the word is.
        at: Position,
        /// The word as written.
        word: String,
    },
    /// A second `app` (§6.7).
    #[error("{}a file declares one `app`; the first is on line {first}", prefix(*at))]
    DuplicateApp {
        /// Where the second `app` is.
        at: Position,
        /// The line of the first `app`.
        first: u32,
    },
    /// A directive inside a block that does not take it (§6.7).
    ///
    /// `block` names the block (`a view`, `a store block`, …) and `takes` lists
    /// what it does take, so the sentence names the fix as well as the mistake.
    #[error(
        "{}`{}` is not a directive here; {block} takes {takes}",
        prefix(*at),
        clipped(word)
    )]
    NotADirectiveHere {
        /// Where the word is.
        at: Position,
        /// The word as written.
        word: String,
        /// The block the word was written in.
        block: &'static str,
        /// What that block takes instead.
        takes: &'static str,
    },
    /// A `Method` word where a view name belongs (§6.7).
    #[error(
        "{}a view name cannot be `{}`; names start with a lower-case letter or a digit",
        prefix(*at),
        clipped(name)
    )]
    MethodAsName {
        /// Where the name is.
        at: Position,
        /// The upper-case word as written.
        name: String,
    },
    /// A lower-case word where a method belongs (§6.7, §6.3).
    ///
    /// `upper` is the same word upper-cased, which is the fix.
    #[error(
        "{}`{}` is not a method: methods are upper-case, `{}`",
        prefix(*at),
        clipped(word),
        clipped(upper)
    )]
    NotAMethod {
        /// Where the word is.
        at: Position,
        /// The word as written.
        word: String,
        /// The word upper-cased.
        upper: String,
    },
    /// A `view` header that lists methods but no pattern (§6.7).
    #[error(
        "{}`view \"{}\"` lists methods but no pattern; write the path it answers",
        prefix(*at),
        clipped(name)
    )]
    MethodsWithoutPattern {
        /// Where the missing pattern would start.
        at: Position,
        /// The view's name.
        name: String,
    },
    /// A `view` block that declares no endpoint (§6.7).
    #[error(
        "{}view \"{}\" has no endpoint: declare at least `GET \"/products\"`",
        prefix(*at),
        clipped(name)
    )]
    ViewWithoutEndpoint {
        /// Where the block opens.
        at: Position,
        /// The view's name.
        name: String,
    },
    /// An endpoint inside a view that declares its methods in the header
    /// (§6.7).
    ///
    /// `method` and `pattern` are what the endpoint wrote; a pattern is a URL
    /// path or a regex, never a value that could be a credential.
    #[error(
        "{}view \"{}\" declares its methods in the header; `{} {}` belongs in a view without one",
        prefix(*at),
        clipped(name),
        clipped(method),
        clipped(pattern)
    )]
    EndpointInHeaderView {
        /// Where the endpoint's first method is.
        at: Position,
        /// The view's name.
        name: String,
        /// The endpoint's method.
        method: String,
        /// The endpoint's pattern, as written.
        pattern: String,
    },
    /// A store written as both a URL and a block (§6.7).
    #[error(
        "{}`postgres $DATABASE_URL` and a store block are two different forms; write one",
        prefix(*at)
    )]
    StoreTwoForms {
        /// Where the second form starts.
        at: Position,
    },
    /// A store block with no slot (§6.7).
    #[error(
        "{}a store block declares at least one slot: `postgres {{ host $DB_HOST; database $DB_DATABASE }}`",
        prefix(*at)
    )]
    EmptyStoreBlock {
        /// Where the block opens.
        at: Position,
    },
    /// `url` inside a store block (§6.7).
    #[error(
        "{}`url` is the one-line form; write `postgres $DATABASE_URL` (or a named `postgres db $DATABASE_URL`)",
        prefix(*at)
    )]
    UrlSlot {
        /// Where the `url` word is.
        at: Position,
    },
    /// A slot written twice in one store block (§6.7).
    #[error(
        "{}`{slot}` is written twice; a store block names each slot once",
        prefix(*at)
    )]
    DuplicateSlot {
        /// Where the second slot is.
        at: Position,
        /// The slot's keyword.
        slot: &'static str,
    },
    /// A directive inside a `trait` that is not a `postgres` grant (§6.7,
    /// §11.3).
    #[error(
        "{}a trait holds `postgres` blocks only; {what} belongs {place}",
        prefix(*at)
    )]
    TraitHoldsGrantsOnly {
        /// Where the directive is.
        at: Position,
        /// The directive, in backticks.
        what: String,
        /// Where it belongs instead.
        place: &'static str,
    },
    /// A `postgres` grant inside an endpoint block (§6.7).
    #[error(
        "{}a grant is per view; write the `postgres` block in the view body, where it applies to every endpoint",
        prefix(*at)
    )]
    GrantIsPerView {
        /// Where the `postgres` word is.
        at: Position,
    },
    /// `unvalidated` on a codec that already validates (§6.7).
    #[error(
        "{}`unvalidated` acknowledges `raw` only; `body {}` already validates the body",
        prefix(*at),
        clipped(codec)
    )]
    UnvalidatedNeedsRaw {
        /// Where the `unvalidated` word is.
        at: Position,
        /// The codec the `body` directive declared.
        codec: String,
    },
    /// `body` with no codec (§6.7).
    #[error("{}`body` needs a codec: `json`, `form`, `none` or `raw`", prefix(*at))]
    BodyNeedsCodec {
        /// Where the missing codec would start.
        at: Position,
    },
    /// A word after `body` that is not a codec (§6.7).
    #[error(
        "{}`{}` is not a codec; the codecs are `json`, `form`, `none`, `raw`",
        prefix(*at),
        clipped(codec)
    )]
    UnknownCodec {
        /// Where the word is.
        at: Position,
        /// The word as written.
        codec: String,
    },
    /// `workers`, removed in 0.2.0 (§6.7, §12.3).
    #[error(
        "{}`workers` was removed in 0.2.0: the scheduler owns parallelism, so delete the line and add nothing",
        prefix(*at)
    )]
    WorkersRemoved {
        /// Where the `workers` word is.
        at: Position,
    },
    /// A directive other than `preload` inside `entrypoint python`'s block
    /// (§6.7).
    #[error(
        "{}the `entrypoint python` block holds `preload` only",
        prefix(*at)
    )]
    EntrypointBlockHoldsPreload {
        /// Where the directive is.
        at: Position,
    },
    /// A second `entrypoint` (§6.7).
    #[error(
        "{}a file declares one `entrypoint`; the first is on line {first}",
        prefix(*at)
    )]
    DuplicateEntrypoint {
        /// Where the second `entrypoint` is.
        at: Position,
        /// The line of the first `entrypoint`.
        first: u32,
    },
    /// A `}` with no open block (§6.7).
    #[error(
        "{}`}}` closes nothing; the last open block is line {opened}",
        prefix(*at)
    )]
    StrayCloseBrace {
        /// Where the `}` is.
        at: Position,
        /// The line of the most recently opened block.
        opened: u32,
    },
    /// A block that is not closed before the end of the file (§6.7).
    #[error(
        "{}the block opened on line {opened} is not closed before the end of the file",
        prefix(*at)
    )]
    UnclosedBlock {
        /// Where the file ends.
        at: Position,
        /// The line of the `{` that opened the block.
        opened: u32,
    },
    /// A list whose first item is a `,` (§6.7).
    #[error("{}a list cannot start with `,`", prefix(*at))]
    ListStartsWithComma {
        /// Where the `,` is.
        at: Position,
    },
    /// A list with a trailing `,` (§6.7).
    #[error("{}a list has no trailing comma; delete the `,`", prefix(*at))]
    TrailingComma {
        /// Where the `,` is.
        at: Position,
    },
    /// A third nested block (§4.5, §6.7).
    #[error(
        "{}blocks nest two deep at most: a view's `postgres` block is the deepest",
        prefix(*at)
    )]
    BlockTooDeep {
        /// Where the third `{` is.
        at: Position,
    },
    /// A name that is not `[a-z0-9][a-z0-9_-]*` (§6.7).
    #[error(
        "{}`{}` is not a name; a view, trait or store name is `[a-z0-9][a-z0-9_-]*`",
        prefix(*at),
        clipped(name)
    )]
    InvalidName {
        /// Where the name is.
        at: Position,
        /// The word as written.
        name: String,
    },
    /// A view, trait or store declared twice (§6.7).
    ///
    /// `kind` is `view`, `trait` or `store`; `name` is the duplicated name.
    #[error(
        "{}`{kind} \"{}\"` is declared twice; the first is on line {first}",
        prefix(*at),
        clipped(name)
    )]
    DuplicateName {
        /// Where the second declaration is.
        at: Position,
        /// Which kind of name was duplicated.
        kind: &'static str,
        /// The duplicated name.
        name: String,
        /// The line of the first declaration.
        first: u32,
    },
    /// `body` twice in one view (§6.7).
    #[error(
        "{}`body` is declared once per view; the one for a single method goes inside that endpoint",
        prefix(*at)
    )]
    DuplicateBody {
        /// Where the second `body` is.
        at: Position,
    },
    /// Two `postgres` blocks for one store in one view (§6.7).
    #[error(
        "{}`postgres` is declared once per store in a view; put both grants in one block",
        prefix(*at)
    )]
    DuplicateGrant {
        /// Where the second grant is.
        at: Position,
    },
    /// `body` twice in one endpoint (§6.7).
    #[error("{}`body` is declared once per endpoint", prefix(*at))]
    DuplicateEndpointBody {
        /// Where the second `body` is.
        at: Position,
    },
    /// A `.` outside a `functions` name (§6.7).
    #[error(
        "{}`.` is only part of a qualified function name: `functions pg_catalog.st_distance`",
        prefix(*at)
    )]
    DotOutsideFunction {
        /// Where the `.` is.
        at: Position,
    },
    /// A byte the lexer carried as `Unknown` (§5.1, §5.3).
    ///
    /// §5.1 gives any other byte to `Unknown` "so the parser can name it in
    /// hex"; this is that sentence, at the byte's own position.
    #[error("{}byte 0x{byte:x} is not Blastfile syntax", prefix(*at))]
    UnknownByte {
        /// Where the byte is.
        at: Position,
        /// The byte as written.
        byte: u8,
    },
    /// A list over §4.5's item cap.
    #[error(
        "{}a list holds at most {} items",
        prefix(*at),
        crate::blastfile::MAX_LIST_ITEMS
    )]
    TooManyListItems {
        /// Where the first item past the cap is.
        at: Position,
    },
    /// An `app "<name>"` that is empty, over §4.5's cap, or holds a control
    /// byte (§4.3).
    ///
    /// The name is a label, never a path or a callable, so the sentence is the
    /// whole rule: the cap is interpolated from `MAX_APP_NAME_CHARS`, so it
    /// cannot drift from the check, and nothing about the value is quoted.
    #[error(
        "{}an `app` name is 1 to {} characters with no control byte",
        prefix(*at),
        crate::blastfile::MAX_APP_NAME_CHARS
    )]
    AppNameInvalid {
        /// Where the name's string is.
        at: Position,
    },
    /// `unrestricted` where a store name belongs (§6.3).
    #[error(
        "{}`unrestricted` is a grant keyword, not a store name; write `postgres unrestricted` for the default store",
        prefix(*at)
    )]
    StoreNameReserved {
        /// Where the `unrestricted` word is.
        at: Position,
    },
    /// `regex` in a pattern list with no raw string after it (§6.3).
    ///
    /// The reserved-word row: `regex` carries a directive meaning wherever a
    /// pattern could appear, so it must be followed by a raw string. §8.3's
    /// [`ManifestError::RegexNeedsRawString`] is the narrower row for the
    /// escaped-string spelling.
    #[error(
        "{}`regex` must be followed by a raw string: `regex '/v[0-9]+/items'`",
        prefix(*at)
    )]
    RegexWithoutRawString {
        /// Where the token after `regex` is.
        at: Position,
    },
    /// `regex` followed by an escaped string (§6.1, §8.3).
    #[error(
        "{}`regex` takes a raw string: write `regex '/v[0-9]+/items'`",
        prefix(*at)
    )]
    RegexNeedsRawString {
        /// Where the `"…"` string is.
        at: Position,
    },
    /// A parameter a pattern never closes (§8.3).
    ///
    /// `parameter` is the text from the `{` to the end of the pattern, escaped
    /// and cut like every other value (§18); a pattern is a URL path, never a
    /// value that could be a credential (§17.1).
    #[error(
        "{}the parameter `{}` is not closed; write `{{id:int}}`",
        prefix(*at),
        clipped(parameter)
    )]
    PatternParameterUnclosed {
        /// Where the pattern is.
        at: Position,
        /// The text from the `{` to the end of the pattern.
        parameter: String,
    },
    /// `{}` in a pattern, which names nothing (§8.3).
    #[error(
        "{}`{{}}` has no name; write `{{id}}`, `{{id:int}}` or `{{id:uuid}}`",
        prefix(*at)
    )]
    PatternParameterUnnamed {
        /// Where the pattern is.
        at: Position,
    },
    /// A parameter name outside `[a-z][a-z0-9_]*` (§8.3).
    ///
    /// `parameter` is the whole `{…}` as written, so the fix is visible.
    #[error(
        "{}`{{{}}}` is not a parameter name; names start with a lower-case letter and are `[a-z][a-z0-9_]*`",
        prefix(*at),
        clipped(parameter)
    )]
    PatternParameterName {
        /// Where the pattern is.
        at: Position,
        /// The parameter as written, without its braces.
        parameter: String,
    },
    /// A parameter type that is not `int` or `uuid` (§8.3).
    #[error(
        "{}`{{{}}}` is not a pattern type; the types are `int` and `uuid`",
        prefix(*at),
        clipped(parameter)
    )]
    PatternParameterType {
        /// Where the pattern is.
        at: Position,
        /// The parameter as written, without its braces.
        parameter: String,
    },
    /// A `*` that is not the last byte of a pattern after a `/` (§8.2, §8.3).
    #[error(
        "{}`*` is only valid at the end of a pattern; write `/x/*`",
        prefix(*at)
    )]
    PatternStarPosition {
        /// Where the pattern is.
        at: Position,
    },
    /// One parameter name used twice in one pattern (§8.3).
    #[error(
        "{}the parameter `{{{}}}` appears twice; a pattern names each parameter once",
        prefix(*at),
        clipped(name)
    )]
    PatternParameterDuplicate {
        /// Where the pattern is.
        at: Position,
        /// The parameter's name, without its braces.
        name: String,
    },
    /// A pattern that does not start with `/` (§8.3).
    #[error("{}a pattern starts with `/`: write `/x`", prefix(*at))]
    PatternNotAnchored {
        /// Where the pattern is.
        at: Position,
    },
    /// A pattern over §4.5's `MAX_PATTERN_BYTES` cap, in either spelling.
    ///
    /// The cap is interpolated from `pattern::MAX_PATTERN_BYTES`, so the
    /// sentence cannot drift from the check. It is a cap on the pattern's
    /// source text: the compiled regex is still bounded by `REGEX_SIZE_LIMIT`
    /// (§8.1, §17.4).
    #[error(
        "{}a pattern holds at most {} bytes",
        prefix(*at),
        crate::blastfile::MAX_PATTERN_BYTES
    )]
    PatternTooLong {
        /// Where the pattern is.
        at: Position,
    },
    /// A `regex '…'` pattern the `regex` crate refuses (§8.3, §17.4).
    ///
    /// `message` is the crate's own sentence, escaped and on one line, so the
    /// developer reads the compiler's answer rather than a paraphrase. §8.3
    /// pins "escaped and cut", so it goes through the same `clipped` rule as
    /// every other value: a crate message that quotes a long excerpt cannot
    /// break the one-line, 120-`char` rule (§18).
    #[error(
        "{}regex `{}` does not compile: {}",
        prefix(*at),
        clipped(pattern),
        clipped(message)
    )]
    PatternRegex {
        /// Where the pattern is.
        at: Position,
        /// The pattern as written.
        pattern: String,
        /// The `regex` crate's message, on one line.
        message: String,
    },
    /// A method a view can never answer (§8.4).
    ///
    /// `CONNECT` and `TRACE` cannot reach a view: blastit answers `GET`/`HEAD`
    /// static files and forwards only the methods a view declares.
    #[error(
        "{}`{}` cannot reach a view; Blastit answers `GET`/`HEAD` static files and forwards the methods you declare",
        prefix(*at),
        clipped(method)
    )]
    MethodCannotReachView {
        /// Where the method is.
        at: Position,
        /// The method as written.
        method: String,
    },
    /// A token the grammar cannot use where it stands.
    ///
    /// §6.7 pins a sentence for every *word* a developer is likely to write in
    /// the wrong place, and §5.3/§5.1 pin the `=`, the `.` and an unknown byte;
    /// this is the one refusal left for a token that is not a word at all — a
    /// string where a keyword belongs, a `{` where a value belongs. Both slots
    /// are fixed descriptions (a token class, a punctuation spelling, or a
    /// `Word`/`$IDENT` as written), so no value that could be a credential is
    /// ever echoed (§18).
    #[error("{}expected {expected}, found {found}", prefix(*at))]
    Unexpected {
        /// Where the token is.
        at: Position,
        /// What the grammar allows there.
        expected: &'static str,
        /// What is there instead.
        found: String,
    },
    /// Two `postgres` sections that lower to one store (§7.3).
    ///
    /// The unnamed form takes the name `db`, so `postgres $DATABASE_URL` and
    /// `postgres db $OTHER_URL` are one store written twice.
    #[error(
        "{}store {} is declared twice; the unnamed `postgres $DATABASE_URL` takes the name `db`, so name one of the two",
        prefix(*at),
        quoted(name)
    )]
    DuplicateProxyName {
        /// Where the second declaration is.
        at: Position,
        /// The store's name.
        name: String,
    },
    /// A `use` that names no trait (§11.2).
    ///
    /// A trait may be written after the view that uses it, so this is resolved
    /// once the whole file has been read — never by the parser (§11.2, §7.3).
    #[error(
        "{}view {}: `use {name}` names no trait; a trait is declared at the top level: `trait {name} {{ … }}`",
        prefix(*at),
        quoted(view)
    )]
    UnknownTrait {
        /// Where the `use` name is.
        at: Position,
        /// The view that uses it.
        view: String,
        /// The trait name that resolves to nothing.
        name: String,
    },
    /// An `entrypoint php` whose path breaks one of `check_php_entry`'s rules
    /// (§12.2).
    ///
    /// The path is never printed: a pasted token can end up in that slot
    /// (§18), so the sentence names the position and the rule instead.
    #[error("{}entrypoint php: {rule}", prefix(*at))]
    PhpEntrypointInvalid {
        /// Where the `entrypoint` is.
        at: Position,
        /// The rule the path breaks, in `check_php_entry`'s words.
        rule: &'static str,
    },
    /// A `static` block with no `root` (§9.1).
    ///
    /// `root` is required: a mount with no directory answers nothing, and
    /// defaulting it would make the omission invisible.
    #[error("{}a `static` block needs `root`: write `root \"public\"`", prefix(*at))]
    StaticRootMissing {
        /// Where the `static` keyword is.
        at: Position,
    },
    /// One `root`, `index`, `fallback` or `immutable` written twice in one
    /// `static` block (§9.1).
    ///
    /// The retired format refused a duplicate key; a second directive would
    /// otherwise silently replace the first.
    #[error(
        "{}`{directive}` is written twice in one `static` block; the first is on line {first}",
        prefix(*at)
    )]
    DuplicateStaticDirective {
        /// Where the second directive is.
        at: Position,
        /// The directive's keyword.
        directive: String,
        /// The line of the first one.
        first: u32,
    },
}

impl ManifestError {
    /// Where in the file this refusal points ([`Position::NONE`] for a
    /// whole-file one), so a caller that prefixes the manifest path reads the
    /// position instead of parsing the rendered line (§18).
    ///
    /// Every variant that renders a position carries it as `at`; the rest are
    /// whole-file refusals, whose sentence has no position to prefix.
    pub fn position(&self) -> Position {
        match self {
            Self::AppNameInvalid { at, .. }
            | Self::BareHyphen { at, .. }
            | Self::BlockTooDeep { at, .. }
            | Self::BodyNeedsCodec { at, .. }
            | Self::CarriageReturn { at, .. }
            | Self::DotOutsideFunction { at, .. }
            | Self::DuplicateApp { at, .. }
            | Self::DuplicateBody { at, .. }
            | Self::DuplicateEndpointBody { at, .. }
            | Self::DuplicateEntrypoint { at, .. }
            | Self::DuplicateGrant { at, .. }
            | Self::DuplicateName { at, .. }
            | Self::DuplicateProxyName { at, .. }
            | Self::DuplicateSlot { at, .. }
            | Self::DuplicateStaticDirective { at, .. }
            | Self::EmptyStoreBlock { at, .. }
            | Self::EndpointInHeaderView { at, .. }
            | Self::EntrypointBlockHoldsPreload { at, .. }
            | Self::EnvMarker { at, .. }
            | Self::EnvName { at, .. }
            | Self::Equals { at, .. }
            | Self::GrantIsPerView { at, .. }
            | Self::InvalidEscape { at, .. }
            | Self::InvalidName { at, .. }
            | Self::ListStartsWithComma { at, .. }
            | Self::MethodAsName { at, .. }
            | Self::MethodCannotReachView { at, .. }
            | Self::MethodsWithoutPattern { at, .. }
            | Self::NonAsciiByte { at, .. }
            | Self::NotADirectiveHere { at, .. }
            | Self::NotAMethod { at, .. }
            | Self::PatternNotAnchored { at, .. }
            | Self::PatternParameterDuplicate { at, .. }
            | Self::PatternParameterName { at, .. }
            | Self::PatternParameterType { at, .. }
            | Self::PatternParameterUnclosed { at, .. }
            | Self::PatternParameterUnnamed { at, .. }
            | Self::PatternRegex { at, .. }
            | Self::PatternStarPosition { at, .. }
            | Self::PatternTooLong { at, .. }
            | Self::PhpEntrypointInvalid { at, .. }
            | Self::QuotedStore { at, .. }
            | Self::RegexNeedsRawString { at, .. }
            | Self::RegexWithoutRawString { at, .. }
            | Self::StaticRootMissing { at, .. }
            | Self::StoreNameReserved { at, .. }
            | Self::StoreTwoForms { at, .. }
            | Self::StrayCloseBrace { at, .. }
            | Self::TooManyListItems { at, .. }
            | Self::TrailingComma { at, .. }
            | Self::TraitHoldsGrantsOnly { at, .. }
            | Self::UnclosedBlock { at, .. }
            | Self::Unexpected { at, .. }
            | Self::UnknownByte { at, .. }
            | Self::UnknownCodec { at, .. }
            | Self::UnknownKeyword { at, .. }
            | Self::UnknownTrait { at, .. }
            | Self::UnterminatedRawString { at, .. }
            | Self::UnterminatedString { at, .. }
            | Self::UnvalidatedNeedsRaw { at, .. }
            | Self::UrlSlot { at, .. }
            | Self::ViewWithoutEndpoint { at, .. }
            | Self::WorkersRemoved { at, .. } => *at,
            _ => Position::NONE,
        }
    }
}

/// Something suspicious that does not stop the server from starting (§6).
///
/// Every sentence names the `Blastfile` node a developer edits — a `trait`, a
/// `view` or a `store` (§13.2) — never the retired file's `group`/`route`/
/// `proxy`, and `error.rs` (not `validate.rs`) is where they live because each
/// one is this enum's `thiserror` attribute. The triggers and the emission order
/// are `validate::warnings`' and did not change.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestWarning {
    /// A `trait` that no view `use`s and no global `groups` entry references:
    /// it grants nothing (§11.3).
    #[error(
        "trait {} is attached to no view and no global traits: it grants nothing",
        quoted(.group)
    )]
    UnattachedGroup {
        /// The trait's name (the model still calls the section a group).
        group: String,
    },
    /// A store that no view and no trait grants: it is declared, but nothing
    /// may use it, so its variables never reach the app (§13.2).
    #[error(
        "store {} has no grant in any view or trait: it is unusable",
        quoted(.proxy)
    )]
    UngrantedProxy {
        /// The store's name (the model still calls it a proxy).
        proxy: ProxyName,
    },
    /// A view that may run any statement on one store (§13.1,
    /// `postgres [name] unrestricted`). Reported once per (view, store), at
    /// boot, so the grant is never invisible in the logs.
    #[error(
        "view {} may run any statement on store {} (unrestricted)",
        quoted(.route),
        quoted(.proxy)
    )]
    UnrestrictedRoute {
        /// The view's name (the model still calls it a route).
        route: String,
        /// The store the view is unrestricted on.
        proxy: ProxyName,
    },
    /// A view that declares `body raw`: the request body reaches the app
    /// exactly as the client sent it (`R11`, §10.1).
    ///
    /// `raw` is the loud opt-out, so a boot says so once per view: every other
    /// codec decodes, validates and re-encodes the body before the app sees it.
    #[error("view {}: body raw passes the request body through unvalidated", quoted(.route))]
    RawIngress {
        /// The view's name.
        route: String,
    },
    /// A view that declares `egress any`: it may open any outbound address and
    /// port, cloud metadata included (`R11`, §12.4).
    #[error("view {}: egress any opens any outbound address and port", quoted(.route))]
    EgressRoute {
        /// The view's name.
        route: String,
    },
    /// An `entrypoint php` that no view forwards to (§12.2, `R30`).
    ///
    /// A warning, never an error: a static mount and a front controller can
    /// ship together before the catch-all view is written, and this is the
    /// reminder. The entry is a relative `.php` path the boot already accepted.
    #[error(
        "entrypoint php {} is reached by no view: declare a `view` that forwards to it",
        quoted(.entry)
    )]
    EntrypointNotReached {
        /// The front controller the file names.
        entry: String,
    },
}

/// Every problem found in one manifest, so a boot can report them all at once.
///
/// [`Display`](fmt::Display) lists them, one per line, with no header and no
/// per-line prefix (§18).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestErrors(Vec<ManifestError>);

impl ManifestErrors {
    /// Collects problems, in the order they were found.
    pub fn new(errors: Vec<ManifestError>) -> Self {
        Self(errors)
    }

    /// The problems themselves.
    pub fn errors(&self) -> &[ManifestError] {
        &self.0
    }

    /// How many problems were found.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there is nothing to report. A value handed back as an error is
    /// never empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the list and returns the problems.
    pub fn into_errors(self) -> Vec<ManifestError> {
        self.0
    }
}

impl fmt::Display for ManifestErrors {
    /// One line per problem and nothing else (§18): no `manifest has N error(s)`
    /// header, no `  - ` prefix and no trailing newline. A caller that prints
    /// them prefixes the manifest path exactly once, and the list is exactly
    /// `len()` lines — the boot's report and the `fmt` refusal are the same N
    /// lines, one per problem.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ManifestErrors {}

impl From<ManifestError> for ManifestErrors {
    fn from(error: ManifestError) -> Self {
        Self(vec![error])
    }
}

impl From<Vec<ManifestError>> for ManifestErrors {
    fn from(errors: Vec<ManifestError>) -> Self {
        Self::new(errors)
    }
}

impl IntoIterator for ManifestErrors {
    type Item = ManifestError;
    type IntoIter = std::vec::IntoIter<ManifestError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a ManifestErrors {
    type Item = &'a ManifestError;
    type IntoIter = std::slice::Iter<'a, ManifestError>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
