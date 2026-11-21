//! What a request can put into `$_SERVER`, and the INI lockdown it cannot touch.
//!
//! This module is where `php2shell` stops being a mitigation and becomes a
//! type error. The FastCGI/CGI escalation surface — `PHP_VALUE`,
//! `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `cgi.fix_pathinfo`, `-d` flags on an
//! argv, and `HTTP_PROXY` from a `Proxy:` header (`httpoxy`) — is not filtered
//! here: there is no API that could carry it. [`ServerVar::new`] refuses the
//! four names outright, [`ServerVars::push_header`] drops `Proxy:` before it is
//! ever mapped to `HTTP_PROXY`, and the INI surface is a fixed list rendered
//! once ([`IniPolicy`]) with no request-shaped constructor.
//!
//! The C shim re-checks the four names (`c/blastit_php_sapi.c`), so a bug on
//! this side cannot become an escalation there either. Both halves are tested:
//! the Rust side by `e2e/php2shell.py`'s adversarial request rows, and the shim
//! in the container by `make php-sapi-e2e`, which puts a `PHP_VALUE` entry in
//! the request and asserts `$_SERVER` never sees it.

use std::fmt;
use std::path::Path;

/// The functions `disable_functions` removes, **verbatim and in order** from
/// ADR-0010 §2: process spawning, `dl()`, and every `pcntl`/`posix` call with
/// which a script could signal, re-parent or escape the view worker's process
/// group. `posix_kill` and `posix_getpgid` are here because a denied
/// `posix_setsid` is worth nothing while a script can still `kill(-pgid)`.
pub const DISABLED_FUNCTIONS: &str = "exec,passthru,shell_exec,system,proc_open,popen,pcntl_exec,pcntl_fork,pcntl_rfork,pcntl_forkx,pcntl_signal,posix_kill,posix_getpgid,posix_setsid,posix_setpgid,dl";

/// The longest `$_SERVER` name accepted: a CGI name, never a buffer.
const MAX_NAME: usize = 127;
/// The longest `$_SERVER` value accepted (a header value, capped).
const MAX_VALUE: usize = 8192;

/// A `$_SERVER` name a request may never bring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservedVar {
    /// FastCGI's per-request INI override.
    PhpValue,
    /// FastCGI's per-request INI override that `ini_set` cannot undo.
    PhpAdminValue,
    /// The CGI flag that turned a query string into `php2shell`.
    RedirectStatus,
    /// `httpoxy`: a `Proxy:` header reaching PHP as `HTTP_PROXY`.
    HttpProxy,
}

impl ReservedVar {
    /// The name, as it would appear in `$_SERVER`.
    pub fn name(self) -> &'static str {
        match self {
            Self::PhpValue => "PHP_VALUE",
            Self::PhpAdminValue => "PHP_ADMIN_VALUE",
            Self::RedirectStatus => "REDIRECT_STATUS",
            Self::HttpProxy => "HTTP_PROXY",
        }
    }
}

impl fmt::Display for ReservedVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a `$_SERVER` entry was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerVarError {
    /// One of the four names a request may never carry.
    Reserved(ReservedVar),
    /// Not `[A-Z0-9_]`, or longer than 127 bytes.
    InvalidName,
    /// Contains a NUL, CR or LF, or is longer than 8192 bytes.
    InvalidValue,
    /// The list already holds this name.
    Duplicate,
    /// [`ServerVars::MAX`] entries are already present.
    TooMany,
}

impl From<ReservedVar> for ServerVarError {
    fn from(reserved: ReservedVar) -> Self {
        Self::Reserved(reserved)
    }
}

impl fmt::Display for ServerVarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reserved(var) => write!(f, "{var} may never come from a request"),
            Self::InvalidName => f.write_str("a $_SERVER name is [A-Z0-9_], at most 127 bytes"),
            Self::InvalidValue => {
                f.write_str("a $_SERVER value holds no NUL, CR or LF and at most 8192 bytes")
            }
            Self::Duplicate => f.write_str("that $_SERVER name is already present"),
            Self::TooMany => write!(f, "at most {} $_SERVER entries", ServerVars::MAX),
        }
    }
}

impl std::error::Error for ServerVarError {}

/// One validated `$_SERVER` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerVar {
    name: String,
    value: String,
}

impl ServerVar {
    /// Builds one entry, refusing the reserved names and any unmappable bytes.
    pub fn new(name: &str, value: &str) -> Result<Self, ServerVarError> {
        for reserved in [
            ReservedVar::PhpValue,
            ReservedVar::PhpAdminValue,
            ReservedVar::RedirectStatus,
            ReservedVar::HttpProxy,
        ] {
            if name.eq_ignore_ascii_case(reserved.name()) {
                return Err(ServerVarError::Reserved(reserved));
            }
        }
        if name.is_empty()
            || name.len() > MAX_NAME
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(ServerVarError::InvalidName);
        }
        if value.len() > MAX_VALUE || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
            return Err(ServerVarError::InvalidValue);
        }
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }

    /// The name, as `$_SERVER` sees it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value, byte for byte.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Why a request header was not turned into a `$_SERVER` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// A `Proxy:` header: `httpoxy`, refused rather than mapped to `HTTP_PROXY`.
    Httpoxy,
    /// Not an RFC 9110 field name.
    InvalidName,
    /// The derived name or the value failed [`ServerVar::new`].
    InvalidValue,
    /// The derived name is one of the four reserved names.
    Reserved,
    /// [`ServerVars::MAX`] is reached.
    TooMany,
}

impl fmt::Display for DropReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Httpoxy => "Proxy: a header never becomes HTTP_PROXY (httpoxy)",
            Self::InvalidName => "not an RFC 9110 field name",
            Self::InvalidValue => "the derived name or value is not representable",
            Self::Reserved => "the derived name is reserved",
            Self::TooMany => "the $_SERVER budget is exhausted",
        })
    }
}

/// What [`ServerVars::push_header`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderOutcome {
    /// Registered as `HTTP_<NAME>`.
    Registered,
    /// Not registered, for the stated reason.
    Dropped(DropReason),
}

impl HeaderOutcome {
    /// Whether the header became a `$_SERVER` entry.
    pub fn registered(self) -> bool {
        matches!(self, Self::Registered)
    }
}

/// The `$_SERVER` list a request builds: no reserved name is constructible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerVars {
    vars: Vec<ServerVar>,
}

impl ServerVars {
    /// The most entries one request may carry.
    pub const MAX: usize = 128;

    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one entry, refusing reserved names and duplicates.
    pub fn insert(&mut self, name: &str, value: &str) -> Result<(), ServerVarError> {
        let var = ServerVar::new(name, value)?;
        if self.vars.iter().any(|held| held.name == var.name) {
            return Err(ServerVarError::Duplicate);
        }
        if self.vars.len() >= Self::MAX {
            return Err(ServerVarError::TooMany);
        }
        self.vars.push(var);
        Ok(())
    }

    /// Maps a request header to `HTTP_<NAME>`, dropping `Proxy:` (httpoxy).
    ///
    /// The name is upper-cased with `-` to `_` exactly as CGI does, then goes
    /// through [`ServerVar::new`], so even a name that *derives* to
    /// `HTTP_PROXY` is refused twice.
    pub fn push_header(&mut self, name: &str, value: &str) -> HeaderOutcome {
        if !is_field_name(name) {
            return HeaderOutcome::Dropped(DropReason::InvalidName);
        }
        if name.eq_ignore_ascii_case("proxy") {
            return HeaderOutcome::Dropped(DropReason::Httpoxy);
        }
        let mut derived = String::with_capacity(5 + name.len());
        derived.push_str("HTTP_");
        for ch in name.chars() {
            derived.push(match ch {
                '-' => '_',
                ch => ch.to_ascii_uppercase(),
            });
        }
        match self.insert(&derived, value) {
            Ok(()) => HeaderOutcome::Registered,
            Err(ServerVarError::Reserved(_)) => HeaderOutcome::Dropped(DropReason::Reserved),
            Err(ServerVarError::TooMany) => HeaderOutcome::Dropped(DropReason::TooMany),
            Err(_) => HeaderOutcome::Dropped(DropReason::InvalidValue),
        }
    }

    /// The entries, in insertion order.
    pub fn as_slice(&self) -> &[ServerVar] {
        &self.vars
    }

    /// Iterates the entries.
    pub fn iter(&self) -> std::slice::Iter<'_, ServerVar> {
        self.vars.iter()
    }

    /// How many entries the list holds.
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
}

impl<'a> IntoIterator for &'a ServerVars {
    type Item = &'a ServerVar;
    type IntoIter = std::slice::Iter<'a, ServerVar>;

    fn into_iter(self) -> Self::IntoIter {
        self.vars.iter()
    }
}

/// RFC 9110 `field-name`: a token, which is what `push_header` accepts.
fn is_field_name(name: &str) -> bool {
    const PUNCTUATION: &[u8] = b"!#$%&'*+-.^_`|~";

    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || PUNCTUATION.contains(&byte))
}

/// The fixed `ZEND_INI_SYSTEM` entries: `(name, value)`, rendered once.
///
/// They arrive in `sapi_module_struct.ini_entries`, which `php_init_config`
/// parses at the startup stage *after* `php.ini` and every scanned `.ini`
/// (`main/php_ini.c`), so these values win over the host's configuration and a
/// script's `ini_set()` cannot relax a `PHP_INI_SYSTEM` directive afterwards.
/// There is no request-shaped path into this list.
pub const PINNED_INI: [(&str, &str); 18] = [
    ("allow_url_include", "0"),
    ("auto_prepend_file", ""),
    ("auto_append_file", ""),
    ("user_ini.filename", ""),
    ("expose_php", "0"),
    ("enable_dl", "0"),
    ("ffi.enable", "0"),
    ("ffi.preload", ""),
    ("display_errors", "0"),
    ("phar.readonly", "1"),
    ("phar.require_hash", "1"),
    ("include_path", "."),
    ("sendmail_path", ""),
    ("opcache.file_cache", ""),
    ("opcache.jit", "0"),
    ("opcache.jit_buffer_size", "0"),
    ("memory_limit", "256M"),
    ("disable_functions", DISABLED_FUNCTIONS),
];

/// Why an [`IniPolicy`] could not be built. Every case is refused, never escaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IniError {
    /// A directory the policy needs is not absolute.
    Relative {
        /// The directive whose value was refused.
        key: &'static str,
    },
    /// A value holds NUL, CR or LF and would inject a directive.
    ControlByte {
        /// The directive whose value was refused.
        key: &'static str,
    },
    /// A path holds `:`, the `open_basedir` entry separator.
    Separator {
        /// The directive whose value was refused.
        key: &'static str,
    },
    /// `max_execution_time=0` means "unlimited" in PHP; refuse it.
    Timeout,
}

impl fmt::Display for IniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative { key } => write!(f, "{key} needs an absolute path"),
            Self::ControlByte { key } => write!(f, "{key} holds NUL, CR or LF"),
            Self::Separator { key } => {
                write!(f, "{key} holds ':', which separates open_basedir entries")
            }
            Self::Timeout => f.write_str("max_execution_time must be at least 1 second"),
        }
    }
}

impl std::error::Error for IniError {}

/// Whether a route may reach the network, and therefore what the INI layer pins
/// `allow_url_fopen` to.
///
/// **This is defence in depth, never the wall.** `allow_url_fopen=0` stops PHP's
/// URL wrappers, and nothing else: the `curl` extension, `sockets`, and a PDO
/// driver each open their own `socket(AF_INET)`. The real boundary is the
/// worker's `SeccompRole::View { egress: false }` filter, which refuses
/// `socket(AF_INET)`/`socket(AF_INET6)` with `EPERM` (`crates/blastit-sys`); the
/// INI value is here so a missing filter is not the *only* thing standing in the
/// way, and so a route author who forgets to declare egress gets a PHP-level
/// error as well as a syscall refusal.
///
/// The variant is a required argument of [`IniPolicy::new`], not a defaulted
/// one: a caller that forgets to think about egress gets a compile error rather
/// than an egress-enabled worker. [`Egress::Allowed`] is the branch that has to
/// be justified at the call site (the route declared `egress = true`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Egress {
    /// `egress = false`: `allow_url_fopen=0`, and (the real wall) no
    /// `AF_INET`/`AF_INET6` socket.
    Denied,
    /// `egress = true`: `allow_url_fopen=1`, and the filter keeps `AF_INET`.
    Allowed,
}

impl Egress {
    /// Whether PHP's URL wrappers may reach the network.
    pub fn allows_url_fopen(self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// The value `allow_url_fopen` is pinned to, as PHP's `ini_get` reports it.
    pub fn allow_url_fopen(self) -> &'static str {
        if self.allows_url_fopen() { "1" } else { "0" }
    }
}

/// The `INI_SYSTEM` lockdown, rendered once per worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniPolicy {
    rendered: String,
    entries: usize,
}

impl IniPolicy {
    /// Renders the pinned entries plus the view-scoped ones.
    ///
    /// `manifest_dir` is the application root (`open_basedir` allows it and the
    /// view's own temp directory, nothing else); `view_tmp` is the worker's
    /// private scratch; `request_timeout_s` is the scheduler's wall clock, which
    /// PHP's own `max_execution_time` mirrors so a runaway script bails out as a
    /// contained fatal rather than a stuck worker; `egress` is the route's
    /// declared `egress`, which pins `allow_url_fopen` **both** ways so the
    /// effective value never depends on the host's `php.ini`.
    pub fn new(
        manifest_dir: &Path,
        view_tmp: &Path,
        request_timeout_s: u32,
        egress: Egress,
    ) -> Result<Self, IniError> {
        if request_timeout_s == 0 {
            return Err(IniError::Timeout);
        }
        let manifest = absolute_value("open_basedir", manifest_dir)?;
        let tmp = absolute_value("open_basedir", view_tmp)?;

        let mut rendered = String::new();
        for (key, value) in PINNED_INI {
            rendered.push_str(key);
            rendered.push('=');
            rendered.push_str(value);
            rendered.push('\n');
        }
        let dynamic = [
            ("allow_url_fopen", egress.allow_url_fopen().to_owned()),
            ("session.save_path", tmp.clone()),
            ("upload_tmp_dir", tmp.clone()),
            ("sys_temp_dir", tmp.clone()),
            ("open_basedir", format!("{manifest}:{tmp}")),
            ("max_execution_time", request_timeout_s.to_string()),
        ];
        for (key, value) in &dynamic {
            rendered.push_str(key);
            rendered.push('=');
            rendered.push_str(value);
            rendered.push('\n');
        }

        Ok(Self {
            rendered,
            entries: PINNED_INI.len() + dynamic.len(),
        })
    }

    /// The `key=value\n` blob handed to the shim as `cfg->ini`.
    pub fn as_str(&self) -> &str {
        &self.rendered
    }

    /// How many directives the policy pins.
    pub fn entry_count(&self) -> usize {
        self.entries
    }
}

/// One path value: absolute, free of the bytes that would inject a directive,
/// and free of `:`, which separates `open_basedir` entries.
fn absolute_value(key: &'static str, path: &Path) -> Result<String, IniError> {
    if !path.is_absolute() {
        return Err(IniError::Relative { key });
    }
    let value = path.to_string_lossy();
    if value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
        return Err(IniError::ControlByte { key });
    }
    // `open_basedir` is a `PATH_SEPARATOR`-delimited list. A directory whose
    // name contains `:` would render two entries — one of them often `""`, i.e.
    // "anywhere" — and every request would 500 on a warning that
    // `display_errors=0` drops. Refuse the boot instead of widening the box.
    if value.contains(':') {
        return Err(IniError::Separator { key });
    }
    Ok(value.into_owned())
}
