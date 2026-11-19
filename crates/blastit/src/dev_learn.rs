//! `blastit dev`: the ephemeral runtime root, the startup banner, the
//! `BLASTIT_DEV_EXPOSE` guard and the `--learn` collector
//! (`docs/design/ma-lean-dx.md` §1.2, `R20`, `R22`, `R29`; `ADR-0001`,
//! `ADR-0003`).
//!
//! Everything here is *state*, not a process: stage 0 creates the root and
//! prints the banner, the supervisor in stage 1 drains `obs_fd` and removes the
//! root when it exits. Nothing in this module ever sees a credential: the
//! observation stream carries [`WireSqlObs`], whose `sql` is already redacted by
//! dbproxy (`R24`), and the manifest it writes holds statements, not values.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::fd::{AsFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use blastit_dbproxy::{ObsTarget, WireSqlObs};
use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::write_atomic;
use blastit_manifest::{Grant, Manifest, ProxyName};

/// The `privilege` value every `blastit dev` boot reports (`R22`).
pub const PRIVILEGE: &str = "unprivileged-dev";

/// Reserved route identity for `dev --learn`'s unrouted-request fallback view
/// (`R26`, `R27`).
///
/// Valid manifest route names match `[a-z0-9][a-z0-9_-]*` (never a leading `_`),
/// and inferred route names trim leading non-`[a-z0-9]` characters, so this
/// sentinel can never collide with any declared or proposed `[[route]]`.
pub const LEARN_FALLBACK_ROUTE: &str = "_learn_fallback";

/// The variable that acknowledges exposing a non-loopback `--host` (`R22`).
pub const EXPOSE_VARIABLE: &str = "BLASTIT_DEV_EXPOSE";

/// The effective uid every `dev` process runs as (`R19`).
pub fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// Whether `host` is one the loopback binds.
pub fn is_loopback(host: &std::net::IpAddr) -> bool {
    host.is_loopback()
}

/// The sentence a non-loopback `--host` without the acknowledgement fails with.
///
/// It names the variable to set and the fact that matters: the simulation has no
/// authentication (`S3`).
pub fn exposure_error(host: &std::net::IpAddr) -> String {
    format!(
        "`--host {host}` exposes `blastit dev` beyond loopback. The simulation has no \
         authentication: anyone who reaches it drives every route as uid {}. Set \
         {EXPOSE_VARIABLE}=1 if that is what you want",
        euid()
    )
}

/// Whether the operator acknowledged the exposure (`R22`).
pub fn expose_requested() -> bool {
    std::env::var_os(EXPOSE_VARIABLE).is_some_and(|value| value == "1")
}

/// The startup banner (`docs/design/ma-lean-dx.md` §1.2).
///
/// It is informational, printed once, before the server binds: never an event.
/// Line 1 names the address as something a terminal can click
/// (`http://127.0.0.1:8080`, `http://[::1]:8080` for IPv6 — a bare IPv6 address
/// in a URL has to be bracketed), with `[LEARN MODE]` when `--learn` is on. An
/// exposed non-loopback bind keeps the bare `host:port` form, because the point
/// there is which address the whole network reaches, not a link.
///
/// Then the honesty line, the components that run, and one aligned row per
/// served thing: `view  NAME  METHODS patterns` per declared route, plus a
/// `static  PREFIX  GET, HEAD  ROOT` row per mount (§9.5). `routes` and the
/// marks count are spelled in the singular when there is one (`1 route`,
/// `1 directory`).
pub fn banner(
    host: &std::net::IpAddr,
    port: u16,
    manifest: &Manifest,
    exposed: bool,
    learn: bool,
) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("  blastit dev · single-uid simulation · ");
    out.push_str(&address(host, port, exposed));
    if exposed {
        out.push_str(&format!(" · uid {} (EXPOSED, no auth)", euid()));
    }
    if learn {
        out.push_str("  [LEARN MODE]");
    }
    out.push('\n');
    out.push_str("  Not a sandbox. Production `blastit` refuses to run like this.\n");
    let routes = manifest.routes().len();
    out.push_str(&format!(
        "  http · scheduler · dbproxy · {routes} {}\n",
        crate::audit_v1::plural(routes, "route")
    ));
    out.push_str(&serving_table(manifest));
    out
}

/// The banner's one row per served thing: a `view` per declared route and a
/// `static` row per mount, in three aligned columns.
///
/// A route with no method list answers every method, and the table says `*` the
/// same way the front end does. A `static` mount is not a route (§9.5) — it owns
/// no uid and starts no process — but it answers requests, so a banner that
/// listed only views named a tree it was not serving. The columns are as wide as
/// their widest cell, so one route reads `  view  home  GET, HEAD  /` and a file
/// with mounts lines up under it.
fn serving_table(manifest: &Manifest) -> String {
    // (kind, name, methods, target)
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for route in manifest.routes() {
        let methods = match route.methods() {
            Some(methods) => methods.join(", "),
            // A view with no method list answers every method the front end
            // lets through (§8.4), and the banner says so the same way.
            None => "*".to_string(),
        };
        rows.push((
            "view".to_string(),
            route.name().to_string(),
            methods,
            route.path().join(", "),
        ));
    }
    for mount in manifest.mounts() {
        // A mount answers `GET`/`HEAD` and nothing else (design §4.4), so the
        // row states the same two methods a `405` would.
        rows.push((
            "static".to_string(),
            mount.prefix.clone(),
            "GET, HEAD".to_string(),
            mount.policy.root.clone(),
        ));
    }
    // Column widths are counted in `char`s, not bytes: `format!` pads a `str` by
    // its characters, so a non-ASCII mount prefix (or a view name) would shift
    // every column after it if the width were `len()`.
    let kind_width = rows
        .iter()
        .map(|row| row.0.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    let name_width = rows
        .iter()
        .map(|row| row.1.chars().count())
        .max()
        .unwrap_or(0);
    let method_width = rows
        .iter()
        .map(|row| row.2.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (kind, name, methods, target) in rows {
        out.push_str(&format!(
            "  {kind:<kind_width$}  {name:<name_width$}  {methods:<method_width$}  {target}\n"
        ));
    }
    out
}

/// The address line 1 names: a clickable loopback URL, or the bare bind.
fn address(host: &std::net::IpAddr, port: u16, exposed: bool) -> String {
    match (exposed, host.is_ipv6()) {
        (false, false) => format!("http://{host}:{port}"),
        (false, true) => format!("http://[{host}]:{port}"),
        (true, true) => format!("[{host}]:{port}"),
        (true, false) => format!("{host}:{port}"),
    }
}

/// The ephemeral `0700` runtime root of one `blastit dev` run (`R20`).
///
/// It is `$XDG_RUNTIME_DIR/blastit-dev-<pid>-<nonce>` when that directory is a
/// real directory owned by the caller, and `/tmp/blastit-dev-<uid>-<pid>-<nonce>`
/// otherwise. The leaf is created with [`rustix::fs::mkdir`] and then reopened
/// with `O_NOFOLLOW | O_DIRECTORY`: a symlink planted at the name, or a
/// directory another uid owns, is a failed boot rather than a root something
/// else can write.
#[derive(Debug)]
pub struct DevRoot {
    path: PathBuf,
}

impl DevRoot {
    /// Creates this run's root.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when the base cannot be used, the nonce cannot be
    /// read, or the leaf is not a `0700` directory owned by the caller.
    pub fn create() -> io::Result<Self> {
        let uid = euid();
        let nonce = nonce()?;
        let name = format!("blastit-dev-{uid}-{}-{nonce}", std::process::id());
        let base = runtime_base(uid);
        let path = base.join(&name);
        // `mkdir` is masked by the umask; the mode is set again on the open
        // descriptor below, which is the only check that matters.
        rustix::fs::mkdir(&path, rustix::fs::Mode::from_bits_truncate(0o700))?;
        verify_private_dir(&path, uid)?;
        Ok(Self { path })
    }

    /// The root itself.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The private directory of the route at `index` (`R20`).
    ///
    /// `<root>/views/<index>` — the route's number, never its uid: in `dev` every
    /// view has the same uid, so the index is the only name that separates them.
    pub fn view_dir(&self, index: usize) -> PathBuf {
        self.path.join("views").join(index.to_string())
    }

    /// Creates every route's private directory, `0700` (`R20`).
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when a directory cannot be created or verified.
    pub fn create_view_dirs(&self, routes: usize) -> io::Result<()> {
        let views = self.path.join("views");
        rustix::fs::mkdir(&views, rustix::fs::Mode::from_bits_truncate(0o700))?;
        verify_private_dir(&views, euid())?;
        for index in 0..routes {
            let dir = self.view_dir(index);
            rustix::fs::mkdir(&dir, rustix::fs::Mode::from_bits_truncate(0o700))?;
            verify_private_dir(&dir, euid())?;
        }
        Ok(())
    }

    /// Removes the root, best effort.
    ///
    /// It is only removed when it is still the private directory this run
    /// created: a path that became a symlink, or a directory another uid owns, is
    /// left alone rather than followed.
    pub fn cleanup(&self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if !metadata.file_type().is_dir() || metadata.uid() != euid() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The `blastit dev` runtime stage 1 inherited from stage 0 (`R20`, `R22`).
#[derive(Debug)]
pub struct Dev {
    root: PathBuf,
    host: std::net::IpAddr,
}

impl Dev {
    /// Adopts the root stage 0 created and the address http binds.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when the path is not a `0700` directory owned by the
    /// caller: the flag is internal, and a root somebody else owns is never
    /// used.
    pub fn adopt(root: PathBuf, host: std::net::IpAddr) -> io::Result<Self> {
        verify_private_dir(&root, euid())?;
        Ok(Self { root, host })
    }

    /// The route's private directory, `<root>/views/<index>`.
    ///
    /// Read by the view fork (`Supervisor::spawn_now`), which only a build with
    /// an interpreter has.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub fn view_dir(&self, index: usize) -> PathBuf {
        self.root.join("views").join(index.to_string())
    }

    /// The address the http role binds.
    pub fn host(&self) -> std::net::IpAddr {
        self.host
    }

    /// Removes the ephemeral root (`R20`).
    pub fn cleanup(&self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.root) else {
            return;
        };
        if !metadata.file_type().is_dir() || metadata.uid() != euid() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One `dev --learn` observation channel (`R29`).
///
/// Per-route sockets in `<root>/views/<index>/` already identify the route on
/// dbproxy's side (`ListenerSpec.route`), so stage 1 only drains `obs_fd` (and
/// holds `_ctl` if an optional control descriptor was inherited).
#[derive(Debug)]
pub struct Learn {
    /// The read end of dbproxy's `obs_fd`.
    pub obs: OwnedFd,
    /// Optional inherited control socket end (`--dev-ctl-fd`), kept owned until
    /// shutdown when passed.
    _ctl: Option<OwnedFd>,
    /// The observations collected so far.
    pub run: LearnRun,
    /// Whether the observation pipe still has a writer.
    pub open: bool,
}

impl Learn {
    /// Adopts the inherited descriptor(s) and starts an observation run.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when a descriptor cannot be switched to non-blocking.
    pub fn adopt(obs_fd: RawFd, ctl_fd: Option<RawFd>) -> io::Result<Self> {
        // SAFETY: stage 0 cleared `FD_CLOEXEC` on these descriptors and handed
        // them to this process; ownership is transferred here once.
        let obs = unsafe { adopt_fd(obs_fd)? };
        let ctl = match ctl_fd {
            Some(fd) => Some(unsafe { adopt_fd(fd)? }),
            None => None,
        };
        Ok(Self {
            obs,
            _ctl: ctl,
            run: LearnRun::new(),
            open: true,
        })
    }

    /// Reads everything the observation pipe holds, remembering its end.
    pub fn poll(&mut self) {
        if !self.open {
            return;
        }
        self.open = drain_until_closed(&self.obs, &mut self.run);
    }
}

/// The directory the root is created in.
///
/// `$XDG_RUNTIME_DIR` is used only when it is absolute, a directory (not a
/// symlink) and owned by the caller — the same test [`DevRoot::create`] applies
/// to the leaf, so the root is never created inside somebody else's directory.
fn runtime_base(uid: u32) -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        && dir.is_absolute()
        && let Ok(metadata) = std::fs::symlink_metadata(&dir)
        && metadata.file_type().is_dir()
        && metadata.uid() == uid
        && metadata.permissions().mode() & 0o077 == 0
    {
        return dir;
    }
    std::env::temp_dir()
}

/// Opens `path` with `O_NOFOLLOW | O_DIRECTORY` and checks it is a `0700`
/// directory owned by `uid`.
fn verify_private_dir(path: &Path, uid: u32) -> io::Result<()> {
    let dir = blastit_sys::open_dir_nofollow(path)?;
    let stat = rustix::fs::fstat(&dir)?;
    if stat.st_uid != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is owned by uid {}", path.display(), stat.st_uid),
        ));
    }
    if stat.st_mode & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is mode {:04o}, not 0700",
                path.display(),
                stat.st_mode & 0o777
            ),
        ));
    }
    Ok(())
}

/// A random nonce for the root's name.
///
/// `getrandom(2)` through `/dev/urandom`; a host without it falls back to the
/// clock and the pid, because the nonce only has to make two concurrent `dev`
/// runs of the same uid distinguishable.
fn nonce() -> io::Result<String> {
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let mut bytes = [0u8; 8];
        use std::io::Read;
        if file.read_exact(&mut bytes).is_ok() {
            return Ok(hex(&bytes));
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    Ok(format!("{nanos:x}"))
}

/// `bytes` as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// One `dev --learn` run's collected state (`R29`).
///
/// The supervisor owns it: observations arrive on `obs_fd` as one JSON
/// [`WireSqlObs`] per line, and the scheduler's own `request` events name the
/// requests it could not route. [`LearnRun::finish`] turns both into
/// [`BlastfileEdits`] and one atomic write.
#[derive(Debug, Default)]
pub struct LearnRun {
    /// Every statement dbproxy observed, in order.
    observations: Vec<WireSqlObs>,
    /// The unrouted requests the scheduler served, `(method, path)`.
    unrouted: BTreeSet<(String, String)>,
    /// Bytes of the observation line being read.
    partial: Vec<u8>,
    /// The refusals to print, redacted (`R25`).
    refusals: BTreeSet<String>,
}

impl LearnRun {
    /// An empty `--learn` collector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the bytes read from `obs_fd`, parsing every complete line.
    ///
    /// A line that is not a [`WireSqlObs`] is ignored rather than failing the
    /// run: dbproxy is the writer, and the worst a malformed line can do is cost
    /// one observation.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.partial.extend_from_slice(bytes);
        while let Some(at) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=at).collect();
            let text = String::from_utf8_lossy(&line);
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                continue;
            }
            if let Ok(obs) = serde_json::from_str::<WireSqlObs>(text) {
                if let blastit_sqlpolicy::LearnVerdict::Refused { redacted, reason } = &obs.verdict
                {
                    self.refusals.insert(format!("{redacted}: {reason}"));
                }
                self.observations.push(obs);
            }
        }
        // A writer that never newline-terminates must not grow without bound.
        if self.partial.len() > 1024 * 1024 {
            self.partial.clear();
        }
    }

    /// Records one scheduler event, keeping the requests it could not route.
    ///
    /// `event` is the JSON line the scheduler wrote; only `request` events
    /// without a `route` are interesting — those are the `--learn` fallback
    /// requests, and each proposes a `[[route]]` (`R26`, `R28`).
    pub fn note_event(&mut self, event: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(event) else {
            return;
        };
        if value.get("event").and_then(serde_json::Value::as_str) != Some("request") {
            return;
        }
        // §8.1: `route` is `null` on a request that matched nothing, so "is
        // there a string?" is the question, not "is the key there?".
        if value
            .get("route")
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return;
        }
        let method = value
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let path = value
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !method.is_empty() && !path.is_empty() {
            self.unrouted.insert((method.to_string(), path.to_string()));
        }
    }

    /// Whether anything at all was observed.
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty() && self.unrouted.is_empty()
    }

    /// Writes the proposals next to the manifest and returns the exit code.
    ///
    /// `0` when every observation was grantable and the write succeeded; `1` when
    /// a statement was refused or the manifest could not be written, which is the
    /// documented `dev --learn` failure (`R25`, §1.1).
    pub fn finish(self, manifest: &Path, uid_base: u32, pretty: bool) -> i32 {
        for refusal in &self.refusals {
            eprintln!("learn: refused {refusal}");
        }
        let refused = self.refusals.len();
        if self.is_empty() && refused == 0 {
            // A run that saw nothing has nothing to read or write; it is still a
            // clean, honest `--learn`.
            print_summary(manifest, pretty, 0, 0);
            return 0;
        }

        // The boot read the file, but `--learn` runs for as long as the suite
        // does and the path can have been swapped since: `config::read_manifest`
        // is `blastfile::read`, which refuses a FIFO or a device from its
        // metadata before opening it, so an observation can never block the
        // write path. Its sentence escapes the path, so a newline-bearing
        // `BLASTIT_MANIFEST` still prints one line.
        let source = match crate::config::read_manifest(manifest) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("learn: {}", err.sentence(manifest));
                return 1;
            }
        };
        let parsed = match Manifest::parse_blastfile(&source, uid_base) {
            Ok((manifest, _warnings)) => manifest,
            Err(errors) => {
                eprintln!("learn: {errors}");
                return 1;
            }
        };
        let (edits, note) = self.edits(&parsed);
        if let Some(note) = note {
            eprintln!("learn: {note}");
        }
        let routes_added = edits.views.len();
        let queries_added: usize = edits.grants.iter().map(|grant| grant.queries.len()).sum();
        let edited = match apply_edits(&source, &edits) {
            Ok(edited) => edited,
            Err(err) => {
                eprintln!("learn: {err}");
                return 1;
            }
        };
        if edited != source
            && let Err(err) = write_atomic(manifest, &edited)
        {
            eprintln!("learn: {}: {err}", crate::config::display_path(manifest));
            return 1;
        }

        print_summary(manifest, pretty, routes_added, queries_added);
        if refused > 0 { 1 } else { 0 }
    }

    /// The edits this run proposes, filtered against the grants already there
    /// (`R27`: `--learn` writes only views and exact `queries`).
    fn edits(&self, manifest: &Manifest) -> (BlastfileEdits, Option<String>) {
        let mut edits = BlastfileEdits::default();
        let names: BTreeSet<String> = manifest
            .routes()
            .iter()
            .map(|route| route.name().to_string())
            .collect();

        // Views: one per name, the methods and patterns collected from every
        // unrouted request. A path a literal pattern cannot carry (a brace the
        // grammar reads as a placeholder, a quote, a control byte) is not
        // proposed: the writer would refuse the whole file for it. Neither is a
        // scanner probe or a path a `static` mount already answers: `--learn`
        // never invents a view for an attacker's path or for files the
        // responder serves (`R14`, `R28`).
        let mounts = blastit_manifest::MountTable::new(manifest);
        let mut proposed: BTreeMap<String, (BTreeSet<String>, Vec<String>)> = BTreeMap::new();
        let mut used = names.clone();
        for (method, path) in &self.unrouted {
            if !crate::infer_routes::is_learnable_path(path) || mounts.lookup(path).is_some() {
                continue;
            }
            let (name, pattern) = name_and_pattern(path);
            if !crate::audit_v1::pattern_is_writable(path) {
                continue;
            }
            let mut candidate = name.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{name}-{suffix}");
                suffix += 1;
            }
            let entry = proposed.entry(candidate).or_default();
            entry.0.insert(method.to_ascii_uppercase());
            if !entry.1.contains(&pattern) {
                entry.1.push(pattern);
            }
        }
        for (name, (methods, patterns)) in proposed {
            used.insert(name.clone());
            edits.views.push(ProposedView {
                name,
                methods: methods.into_iter().collect(),
                patterns,
            });
        }

        // Statements that ran on the reserved fallback view belong to an
        // unrouted request, and a per-route socket cannot say which one. When
        // this run proposes exactly one view, every unrouted request maps to it,
        // so the attribution is exact; with more than one the statement cannot be
        // placed, and guessing would hand one new view another's grants. This is
        // what makes one `dev --learn` pass write the queries of the view it
        // discovered (R26, R27, `docs/local.md`).
        //
        // The same reasoning forbids attributing them when *any* unrouted request
        // was one `--learn` refuses to turn into a view: a probe's `/.env` or
        // `/../etc/passwd` request has no path on its statement, so its SQL would
        // land in the one legitimate view's `queries` (`MB.40.2`).
        let unattributable = self.unrouted.iter().any(|(_, path)| {
            !crate::infer_routes::is_learnable_path(path) || mounts.lookup(path).is_some()
        });
        let fallback_owner = match edits.views.as_slice() {
            [only] if !unattributable => Some(only.name.clone()),
            _ => None,
        };
        let note = unattributable.then(|| {
            "an unrouted request was a path --learn never turns into a view (a traversal, a \
             dotfile, or a `static` mount), so this run's split-socket statements are not \
             attributed to any view; grant what your application really ran by hand"
                .to_string()
        });

        // Grants: only grantable statements, only routes the manifest declares
        // or this run proposes. An existing route's own grants are filtered out;
        // a proposed route has none yet.
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut grants: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for obs in &self.observations {
            let ObsTarget::Routed { route } = &obs.target else {
                continue;
            };
            let route = if route == LEARN_FALLBACK_ROUTE {
                let Some(owner) = &fallback_owner else {
                    continue;
                };
                owner.clone()
            } else {
                route.clone()
            };
            let Some(normalized) = obs.verdict.normalized() else {
                continue;
            };
            let proxy = ProxyName::new(obs.proxy.clone());
            if let Some(index) = manifest
                .routes()
                .iter()
                .position(|candidate| candidate.name() == route)
            {
                let already_granted = manifest
                    .effective_grants(index)
                    .get(&proxy)
                    .and_then(Grant::as_postgresql)
                    .is_some_and(|grant| grant.queries().iter().any(|query| query == normalized));
                if already_granted
                    || manifest.is_query_covered_by_tables_or_groups(index, &proxy, normalized)
                {
                    continue;
                }
            }
            if !seen.insert((route.clone(), obs.proxy.clone(), normalized.to_string())) {
                continue;
            }
            grants
                .entry((route.clone(), obs.proxy.clone()))
                .or_default()
                .push(normalized.to_string());
        }
        for ((view, proxy), queries) in grants {
            edits.grants.push(ProposedGrant {
                view,
                // The unnamed `postgres { … }` block is the store `db` (§7.4);
                // a named store is written `postgres <name> { … }`.
                store: (proxy != blastit_manifest::blastfile::DEFAULT_STORE).then_some(proxy),
                queries,
                tables: std::collections::BTreeMap::new(),
                functions: std::collections::BTreeSet::new(),
            });
        }
        (edits, note)
    }
}

/// Builds the `learn:` summary line (`§1.3`), with the absolute manifest path.
///
/// Both counts are written in their own number — `1 route, 1 query added`,
/// `2 routes, 38 queries added` — because this is a sentence a developer reads,
/// and it is the one `docs/local.md` shows. `plural` is the same helper
/// `check.rs`'s `learn_line` uses, so the two entry points cannot drift.
fn summary_line(manifest: &Path, routes: usize, queries: usize) -> String {
    let path = crate::config::display_path(
        &std::path::absolute(manifest).unwrap_or_else(|_| manifest.to_path_buf()),
    );
    let route_noun = crate::audit_v1::plural(routes, "route");
    let query_noun = crate::audit_v1::plural(queries, "query");
    format!("learn: {routes} {route_noun}, {queries} {query_noun} added → {path}")
}

/// Prints the `learn:` summary line (`§1.3`).
///
/// It goes to stdout in `pretty` and to stderr under `BLASTIT_LOG_FORMAT=json`,
/// where stdout is the event stream alone (`§1.1`).
fn print_summary(manifest: &Path, pretty: bool, routes: usize, queries: usize) {
    let summary = summary_line(manifest, routes, queries);
    if pretty {
        println!("{summary}");
    } else {
        eprintln!("{summary}");
    }
}

/// A `(name, path pattern)` for one unrouted request (`R28`).
///
/// The name is the first non-empty segment, lowercased, with runs outside
/// `[a-z0-9_-]` collapsed to `-` and every leading or trailing character
/// outside `[a-z0-9]` trimmed; `/` is `root`. Numeric and UUID segments become
/// the pattern placeholders `{id:int}` and `{id:uuid}`; everything else is
/// literal, because a quoted pattern in a `Blastfile` matches its bytes and
/// escapes nothing (`.` is a dot, not a wildcard).
fn name_and_pattern(path: &str) -> (String, String) {
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if segments.is_empty() {
        return ("root".to_string(), "/".to_string());
    }
    let first = segments.first().copied().unwrap_or("");
    let mut name = String::new();
    for character in first.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            name.push(character.to_ascii_lowercase());
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_matches(|character: char| !character.is_ascii_alphanumeric());
    let name = if name.is_empty() { "route" } else { name };
    let mut pattern = String::from("/");
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            pattern.push('/');
        }
        pattern.push_str(&segment_pattern(segment));
    }
    (name.to_string(), pattern)
}

/// One path segment as a `Blastfile` pattern piece.
fn segment_pattern(segment: &str) -> String {
    if segment.chars().all(|character| character.is_ascii_digit()) {
        return "{id:int}".to_string();
    }
    if is_uuid(segment) {
        return "{id:uuid}".to_string();
    }
    segment.to_string()
}

/// Whether `segment` is shaped like a UUID.
fn is_uuid(segment: &str) -> bool {
    segment.len() == 36
        && segment.char_indices().all(|(at, character)| match at {
            8 | 13 | 18 | 23 => character == '-',
            _ => character.is_ascii_hexdigit(),
        })
}

/// Adopts the write end of an observation pipe (`R29`).
///
/// # Safety
///
/// `fd` is a descriptor stage 0 cleared `FD_CLOEXEC` on and handed to stage 1;
/// the caller transfers ownership exactly once.
///
/// # Errors
///
/// [`std::io::Error`] when the descriptor cannot be switched to non-blocking.
pub unsafe fn adopt_fd(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: the caller's contract above is this function's.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    rustix::fs::fcntl_setfl(&owned, rustix::fs::OFlags::NONBLOCK)?;
    Ok(owned)
}

/// Reads whatever an adopted observation descriptor holds, without blocking.
///
/// Returns `false` once the writer is gone, so the supervisor can stop polling
/// a descriptor that will never say anything again.
pub fn drain_until_closed(fd: &OwnedFd, run: &mut LearnRun) -> bool {
    let borrowed = fd.as_fd();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match rustix::io::read(borrowed, &mut buffer) {
            Ok(0) => return false,
            Ok(read) => run.push_bytes(&buffer[..read]),
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return true,
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_names_a_clickable_address_every_view_and_the_learn_badge() {
        let (manifest, _) = Manifest::parse_blastfile(
            "view home GET \"/\" {}\n\nview cart GET \"/cart\" {}\n",
            20_000,
        )
        .unwrap();
        let host: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let rendered = banner(&host, 8080, &manifest, false, false);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(
            lines[0],
            "  blastit dev · single-uid simulation · http://127.0.0.1:8080"
        );
        assert_eq!(
            lines[1],
            "  Not a sandbox. Production `blastit` refuses to run like this."
        );
        assert_eq!(lines[2], "  http · scheduler · dbproxy · 2 routes");
        assert_eq!(lines[3], "  view  home  GET, HEAD  /");
        assert_eq!(lines[4], "  view  cart  GET, HEAD  /cart");
        assert_eq!(lines.len(), 5, "one line per declared view:\n{rendered}");

        // IPv6 has to be bracketed, or the URL is not one a terminal links.
        let v6: std::net::IpAddr = "::1".parse().unwrap();
        assert!(
            banner(&v6, 9000, &manifest, false, false)
                .starts_with("  blastit dev · single-uid simulation · http://[::1]:9000"),
            "an IPv6 address is bracketed in the URL"
        );

        // `--learn` says so on the first line, once.
        let learn = banner(&host, 8080, &manifest, false, true);
        assert_eq!(
            learn.lines().next().unwrap(),
            "  blastit dev · single-uid simulation · http://127.0.0.1:8080  [LEARN MODE]"
        );

        // An exposed bind names the address the network reaches and the uid,
        // exactly as before: it is not a link, it is a warning.
        let exposed = banner(&host, 8080, &manifest, true, false);
        assert_eq!(
            exposed.lines().next().unwrap(),
            format!(
                "  blastit dev · single-uid simulation · 127.0.0.1:8080 · uid {} (EXPOSED, no auth)",
                euid()
            )
        );
    }

    #[test]
    fn the_banner_names_static_mounts_in_the_same_columns() {
        let (manifest, _) = Manifest::parse_blastfile(
            "view home GET \"/\" {}\n\nstatic \"/assets\" {\n    root \"public/assets\"\n}\n",
            20_000,
        )
        .unwrap();
        let host: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let rendered = banner(&host, 8080, &manifest, false, false);
        let lines: Vec<&str> = rendered.lines().collect();
        // A mount is not a route (§9.5), so it adds no route count — but it
        // answers requests, so the table names it and the columns line up with
        // the view row above it.
        assert_eq!(lines[2], "  http · scheduler · dbproxy · 1 route");
        assert_eq!(lines[3], "  view    home     GET, HEAD  /");
        assert_eq!(lines[4], "  static  /assets  GET, HEAD  public/assets");
        assert_eq!(lines.len(), 5, "one row per view and mount:\n{rendered}");
    }

    #[test]
    fn a_non_ascii_mount_prefix_keeps_the_columns_aligned() {
        // `format!` pads a `str` by characters, so the widths must be counted in
        // characters too: with `len()` the methods column of the second row
        // would start two bytes later than the first row's.
        let (manifest, _) = Manifest::parse_blastfile(
            "view home GET \"/\" {}\n\nstatic \"/café\" {\n    root \"public/café\"\n}\n",
            20_000,
        )
        .unwrap();
        let host: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let rendered = banner(&host, 8080, &manifest, false, false);
        let rows: Vec<&str> = rendered
            .lines()
            .filter(|line| line.starts_with("  view") || line.starts_with("  static"))
            .collect();
        assert_eq!(
            rows,
            vec![
                "  view    home   GET, HEAD  /",
                "  static  /café  GET, HEAD  public/café",
            ],
            "a non-ASCII prefix must not shift the columns:\n{rendered}"
        );
    }

    #[test]
    fn the_banner_counts_one_route_and_pluralises() {
        let (one, _) = Manifest::parse_blastfile("view home GET \"/\" {}\n", 20_000).unwrap();
        let (two, _) = Manifest::parse_blastfile(
            "view home GET \"/\" {}\n\nview cart GET \"/cart\" {}\n",
            20_000,
        )
        .unwrap();
        let host: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        assert!(
            banner(&host, 8080, &one, false, false).contains("dbproxy · 1 route\n"),
            "one route is singular"
        );
        assert!(
            banner(&host, 8080, &two, false, false).contains("dbproxy · 2 routes\n"),
            "two routes are plural"
        );
        assert_eq!(
            crate::audit_v1::plural(0, "route"),
            "routes",
            "zero routes are plural"
        );
    }

    #[test]
    fn the_learn_summary_pluralises_both_counts() {
        let manifest = Path::new("/srv/shop/Blastfile");
        assert_eq!(
            summary_line(manifest, 1, 1),
            "learn: 1 route, 1 query added → /srv/shop/Blastfile",
            "one route and one query are both singular"
        );
        assert_eq!(
            summary_line(manifest, 2, 38),
            "learn: 2 routes, 38 queries added → /srv/shop/Blastfile",
            "many routes and many queries are both plural"
        );
        assert_eq!(
            summary_line(manifest, 1, 0),
            "learn: 1 route, 0 queries added → /srv/shop/Blastfile",
            "zero counts are plural"
        );
    }

    #[test]
    fn a_non_loopback_host_is_named_with_the_variable_to_set() {
        let host = "0.0.0.0".parse().unwrap();
        assert!(!is_loopback(&host));
        let message = exposure_error(&host);
        assert!(message.contains("BLASTIT_DEV_EXPOSE"), "{message}");
        assert!(message.contains("0.0.0.0"), "{message}");
        assert!(message.contains(&euid().to_string()), "{message}");
    }

    #[test]
    fn the_root_is_private_and_cleaned_up() {
        // `/tmp` is what the test uses: the suite cannot rely on a runtime
        // directory existing, and the fallback is the documented one.
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        // SAFETY: this unit test names its own unique variable state, and no
        // other test in this binary reads it at the same time (`cargo test`
        // runs modules' tests in parallel, so this is deliberately a *read* of
        // the fallback only).
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        let root = DevRoot::create().unwrap();
        let metadata = std::fs::symlink_metadata(root.path()).unwrap();
        assert!(metadata.file_type().is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(metadata.uid(), euid());
        root.create_view_dirs(2).unwrap();
        assert!(root.view_dir(0).is_dir());
        assert!(root.view_dir(1).is_dir());
        let path = root.path().to_path_buf();
        root.cleanup();
        assert!(!path.exists(), "the root is removed");
        if let Some(previous) = previous {
            // SAFETY: see above; the test puts back what it found.
            unsafe { std::env::set_var("XDG_RUNTIME_DIR", previous) };
        }
    }

    #[test]
    fn a_symlinked_root_is_refused() {
        let dir = std::env::temp_dir().join(format!("blastit-dev-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target");
        std::fs::create_dir(&target).unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = verify_private_dir(&link, euid()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotADirectory);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn view_names_and_patterns_follow_the_documented_rules() {
        assert_eq!(name_and_pattern("/"), ("root".to_string(), "/".to_string()));
        assert_eq!(
            name_and_pattern("/products/42/"),
            ("products".to_string(), "/products/{id:int}".to_string())
        );
        assert_eq!(
            name_and_pattern("/orders/550e8400-e29b-41d4-a716-446655440000"),
            ("orders".to_string(), "/orders/{id:uuid}".to_string())
        );
        // A quoted pattern is literal: `.` and `+` need no escaping, which is
        // what makes `--learn`'s output readable and its match exact.
        assert_eq!(
            name_and_pattern("/robots.txt"),
            ("robots-txt".to_string(), "/robots.txt".to_string())
        );
        assert_eq!(
            name_and_pattern("/api.v2/items+list/"),
            ("api-v2".to_string(), "/api.v2/items+list".to_string())
        );
    }

    #[test]
    fn observations_are_parsed_one_per_line() {
        let mut run = LearnRun::new();
        let line = serde_json::json!({
            "target": {"scope": "routed", "route": "home"},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 1"}
        });
        let text = format!("{line}\n{line}\n");
        run.push_bytes(text.as_bytes());
        assert_eq!(run.observations.len(), 2);
        // A half line waits for the rest.
        run.push_bytes(b"{\"target\"");
        assert_eq!(run.observations.len(), 2);
    }

    #[test]
    fn an_unrouted_request_proposes_a_route() {
        let mut run = LearnRun::new();
        run.note_event(r#"{"event":"request","method":"GET","path":"/orders/42/"}"#);
        assert!(
            run.unrouted
                .contains(&("GET".to_string(), "/orders/42/".to_string()))
        );
        run.note_event(r#"{"event":"request","method":"GET","path":"/x","route":"home"}"#);
        assert_eq!(run.unrouted.len(), 1, "a routed request proposes nothing");
    }

    #[test]
    fn first_declared_view_and_matching_proxy_name_keep_grants_when_unrouted_exists() {
        let source = "\
postgres home $DATABASE_URL

view home GET \"/\" {}

view items GET \"/items\" {}
";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let mut run = LearnRun::new();
        run.note_event(r#"{"event":"request","method":"GET","path":"/extra/1"}"#);
        let home_obs = serde_json::json!({
            "target": {"scope": "routed", "route": "home"},
            "proxy": "home",
            "kind": "query",
            "sql": "SELECT 111",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 111"}
        });
        let items_obs = serde_json::json!({
            "target": {"scope": "routed", "route": "items"},
            "proxy": "home",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 1"}
        });
        let fallback_obs = serde_json::json!({
            "target": {"scope": "routed", "route": LEARN_FALLBACK_ROUTE},
            "proxy": "home",
            "kind": "query",
            "sql": "SELECT 2",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 2"}
        });
        let lines = format!("{home_obs}\n{items_obs}\n{fallback_obs}\n");
        run.push_bytes(lines.as_bytes());

        let (edits, note) = run.edits(&manifest);
        assert!(note.is_none(), "{note:?}");
        assert_eq!(edits.views.len(), 1);
        assert_eq!(edits.views[0].name, "extra");
        assert_eq!(
            edits.grants,
            vec![
                ProposedGrant {
                    view: "extra".to_string(),
                    store: Some("home".to_string()),
                    queries: vec!["SELECT 2".to_string()],
                    tables: Default::default(),
                    functions: Default::default(),
                },
                ProposedGrant {
                    view: "home".to_string(),
                    store: Some("home".to_string()),
                    queries: vec!["SELECT 111".to_string()],
                    tables: Default::default(),
                    functions: Default::default(),
                },
                ProposedGrant {
                    view: "items".to_string(),
                    store: Some("home".to_string()),
                    queries: vec!["SELECT 1".to_string()],
                    tables: Default::default(),
                    functions: Default::default(),
                },
            ],
            "the one proposed route receives the statement its unrouted request ran, while \
             each declared route keeps only its own"
        );
    }

    #[test]
    fn two_proposed_views_do_not_borrow_each_others_fallback_statements() {
        let source = "\
postgres $DATABASE_URL

view home GET \"/\" {
    postgres {
        query 'SELECT 1'
    }
}
";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let mut run = LearnRun::new();
        // Two unrouted requests propose two different views, so a statement
        // that ran on the single fallback view cannot be placed: granting it to
        // either would hand one new view the other's access.
        run.note_event(r#"{"event":"request","method":"GET","path":"/extra/1"}"#);
        run.note_event(r#"{"event":"request","method":"GET","path":"/orders/2"}"#);
        let fallback_obs = serde_json::json!({
            "target": {"scope": "routed", "route": LEARN_FALLBACK_ROUTE},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 2",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 2"}
        });
        run.push_bytes(format!("{fallback_obs}\n").as_bytes());

        let (edits, note) = run.edits(&manifest);
        assert!(note.is_none(), "{note:?}");
        let names: Vec<&str> = edits.views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(names, vec!["extra", "orders"]);
        assert!(
            edits.grants.is_empty(),
            "an ambiguous fallback statement is not granted to a guess: {:?}",
            edits.grants
        );
    }

    /// A scanner probe never donates its SQL to the one view a legitimate request
    /// discovered (`MB.40.2`).
    ///
    /// dbproxy tags every split-socket statement with the fallback route and no
    /// path, so `--learn` can only attribute them when *every* unrouted request
    /// is one it would turn into a view. One probe among them makes the whole
    /// batch unattributable, and the run says so.
    #[test]
    fn a_scanner_probe_never_donates_its_sql_to_the_one_proposed_view() {
        let source = "\
postgres $DATABASE_URL

view home GET \"/\" {}
";
        let (manifest, _) = Manifest::parse_blastfile(source, 20_000).unwrap();
        let mut run = LearnRun::new();
        run.note_event(r#"{"event":"request","method":"GET","path":"/notes/"}"#);
        run.note_event(r#"{"event":"request","method":"GET","path":"/.env"}"#);
        let legit = serde_json::json!({
            "target": {"scope": "routed", "route": LEARN_FALLBACK_ROUTE},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 1"}
        });
        let probe = serde_json::json!({
            "target": {"scope": "routed", "route": LEARN_FALLBACK_ROUTE},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 2",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 2"}
        });
        run.push_bytes(format!("{legit}\n{probe}\n").as_bytes());

        let (edits, note) = run.edits(&manifest);
        let names: Vec<&str> = edits.views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["notes"],
            "the probe proposes no view of its own"
        );
        assert!(
            edits.grants.is_empty(),
            "no fallback statement is attributed while a probe was in the batch: {:?}",
            edits.grants
        );
        let note = note.expect("the refusal is said out loud");
        assert!(note.contains("--learn"), "{note}");
        assert!(
            !note.contains(".env"),
            "the note never echoes a probe: {note}"
        );
    }

    /// The `dev --learn` write path (`docs/frameworks/php.md`: "records the
    /// views and exact queries you click through") against the documented
    /// PHP-shaped file: the learned query must land, and the mount and the
    /// front controller must be the bytes they were.
    ///
    /// This is [`LearnRun::finish`], the one function the supervisor calls on
    /// `SIGTERM`; no PHP binary takes part in writing a file.
    #[test]
    fn a_learned_query_keeps_the_files_executor_blocks_on_disk() {
        const PHP_STATIC: &str = "\
# The shop: Symfony serves it from one front controller.
app \"shop\"

# One database, one credential.
postgres $DATABASE_URL

entrypoint php \"public/index.php\"

# Only fingerprinted output is cached for a year.
static \"/\" {
    root \"public\"
    index \"index.html\"
    immutable \"/assets/*\", \"/build/*\"
}

view app regex '.*' {
    postgres {
        query 'SELECT \"shop_product\".\"id\" FROM \"shop_product\"'
    }
}
";
        const LEARNED: &str =
            r#"SELECT "shop_product"."sku" FROM "shop_product" WHERE "shop_product"."id" = $?"#;

        let dir = std::env::temp_dir().join(format!(
            "blastit-mb321-learn-{}-{}",
            std::process::id(),
            LEARNED.len()
        ));
        std::fs::create_dir_all(&dir).expect("a fresh fixture directory");
        let manifest = dir.join("Blastfile");
        std::fs::write(&manifest, PHP_STATIC).expect("write the fixture");

        let mut run = LearnRun::new();
        let obs = serde_json::json!({
            "target": {"scope": "routed", "route": "app"},
            "proxy": "db",
            "kind": "query",
            "sql": LEARNED,
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": LEARNED}
        });
        run.push_bytes(format!("{obs}\n").as_bytes());

        let status = run.finish(&manifest, 20_000, false);
        let written = std::fs::read_to_string(&manifest).expect("read the manifest back");
        let without_queries = |document: &str| -> String {
            document
                .lines()
                .filter(|line| !line.trim_start().starts_with("query"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        assert_eq!(status, 0, "the learn write succeeds:\n{written}");
        assert!(written.contains(LEARNED), "the query landed:\n{written}");
        assert_eq!(
            without_queries(&written),
            without_queries(PHP_STATIC),
            "a mount, the entrypoint or another author line changed:\n{written}"
        );
        assert_eq!(written.matches("static \"/\"").count(), 1, "{written}");
        assert_eq!(written.matches("entrypoint php").count(), 1, "{written}");
        let (parsed, _warnings) =
            Manifest::parse_blastfile(&written, 20_000).expect("the written file boots");
        assert_eq!(
            parsed.php().map(|policy| policy.entry.as_str()),
            Some("public/index.php")
        );
        assert_eq!(
            parsed
                .mounts()
                .first()
                .map(|mount| mount.policy.root.as_str()),
            Some("public")
        );

        // The denied twin: a statement the policy refused is never written, and
        // the file (mount and entrypoint included) stays exactly as it was.
        let refused_dir = dir.join("refused");
        std::fs::create_dir_all(&refused_dir).expect("a second fixture directory");
        let refused_manifest = refused_dir.join("Blastfile");
        std::fs::write(&refused_manifest, PHP_STATIC).expect("write the second fixture");
        let mut refused_run = LearnRun::new();
        let refused = serde_json::json!({
            "target": {"scope": "routed", "route": "app"},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1 WHERE … = …",
            "is_tx": false,
            "allowed": false,
            "verdict": {
                "kind": "refused",
                "redacted": "SELECT 1 WHERE … = …",
                "reason": "the literal was inlined"
            }
        });
        refused_run.push_bytes(format!("{refused}\n").as_bytes());
        assert_eq!(
            refused_run.finish(&refused_manifest, 20_000, false),
            1,
            "a refused statement is an exit-1 learn, not a silent write"
        );
        assert_eq!(
            std::fs::read_to_string(&refused_manifest).expect("read the second fixture back"),
            PHP_STATIC,
            "a refusal writes nothing"
        );

        // A malformed executor block is refused by the parse, so `finish`
        // returns `1` with no writer reachable and the file untouched.
        let broken_dir = dir.join("broken");
        std::fs::create_dir_all(&broken_dir).expect("a third fixture directory");
        let broken_manifest = broken_dir.join("Blastfile");
        let broken = PHP_STATIC.replace("entrypoint php \"public/index.php\"", "entrypoint php 4");
        std::fs::write(&broken_manifest, &broken).expect("write the third fixture");
        let mut broken_run = LearnRun::new();
        broken_run.push_bytes(format!("{obs}\n").as_bytes());
        assert_eq!(
            broken_run.finish(&broken_manifest, 20_000, false),
            1,
            "a file that does not parse is an exit-1 learn"
        );
        assert_eq!(
            std::fs::read_to_string(&broken_manifest).expect("read the third fixture back"),
            broken,
            "a parse refusal writes nothing"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `dev --learn` reads the manifest again when it writes, and the path can
    /// have been swapped for a FIFO since the boot read it: the one reader
    /// refuses a non-regular file from its metadata, so the write path exits `1`
    /// promptly instead of blocking for ever on `open`.
    ///
    /// The bound is the assertion: `fs::read_to_string` on a FIFO with no writer
    /// never returns, so this test would hang instead of failing without the fix.
    #[test]
    fn a_manifest_swapped_for_a_fifo_cannot_block_the_learn_write() {
        let dir =
            std::env::temp_dir().join(format!("blastit-mb2142-learn-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a fresh fixture directory");
        let manifest = dir.join("Blastfile");
        std::fs::write(
            &manifest,
            "postgres $DATABASE_URL\n\nview home GET \"/\" {}\n",
        )
        .expect("write the fixture");

        let mut run = LearnRun::new();
        let obs = serde_json::json!({
            "target": {"scope": "routed", "route": "home"},
            "proxy": "db",
            "kind": "query",
            "sql": "SELECT 1",
            "is_tx": false,
            "allowed": false,
            "verdict": {"kind": "query", "normalized": "SELECT 1"}
        });
        run.push_bytes(format!("{obs}\n").as_bytes());

        // The swap: a FIFO where the file was, so any reader that opens it
        // without a writer blocks.
        std::fs::remove_file(&manifest).expect("remove the fixture");
        let status = std::process::Command::new("mkfifo")
            .arg(&manifest)
            .status()
            .expect("mkfifo runs");
        assert!(status.success(), "mkfifo failed: {status}");

        let (sender, receiver) = std::sync::mpsc::channel();
        let path = manifest.clone();
        std::thread::spawn(move || {
            let code = run.finish(&path, 20_000, false);
            let _ = sender.send(code);
        });
        let code = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("`learn` refused the FIFO instead of blocking on it");
        assert_eq!(code, 1, "an unreadable manifest is an exit-1 learn");

        let _ = std::fs::remove_file(&manifest);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
