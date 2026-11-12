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
use blastit_manifest::edit::{
    ManifestEdits, ProposedGrant, ProposedRoute, apply_edits, write_atomic,
};
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

/// The exact 3-line banner (§1.2).
///
/// `routes` is the manifest's route count; `exposed` is whether a non-loopback
/// host was accepted, which is the only case that names the uid.
pub fn banner(host: &std::net::IpAddr, port: u16, routes: usize, exposed: bool) -> String {
    let first = if exposed {
        format!(
            "  blastit dev · single-uid simulation · {host}:{port} · uid {} (EXPOSED, no auth)",
            euid()
        )
    } else {
        format!("  blastit dev · single-uid simulation · {host}:{port}")
    };
    let noun = if routes == 1 { "route" } else { "routes" };
    format!(
        "{first}\n  Not a sandbox. Production `blastit` refuses to run like this.\n  \
         http · scheduler · dbproxy · {routes} {noun}\n"
    )
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
    let dir = rustix::fs::open(
        path,
        rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
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
/// [`ManifestEdits`] and one atomic write.
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
                if let blastit_dbproxy::ObsVerdict::Refused { redacted, reason } = &obs.verdict {
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

        let source = match std::fs::read_to_string(manifest) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("learn: {}: {err}", manifest.display());
                return 1;
            }
        };
        let parsed = match Manifest::parse_and_validate(&source, uid_base) {
            Ok((manifest, _warnings)) => manifest,
            Err(errors) => {
                eprintln!("learn: {errors}");
                return 1;
            }
        };
        let edits = self.edits(&parsed);
        let routes_added = edits.routes.len();
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
            eprintln!("learn: {}: {err}", manifest.display());
            return 1;
        }

        print_summary(manifest, pretty, routes_added, queries_added);
        if refused > 0 { 1 } else { 0 }
    }

    /// The edits this run proposes, filtered against the grants already there
    /// (`R27`: `--learn` writes only routes and exact `queries`).
    fn edits(&self, manifest: &Manifest) -> ManifestEdits {
        let mut edits = ManifestEdits::default();
        let names: BTreeSet<String> = manifest
            .routes()
            .iter()
            .map(|route| route.name().to_string())
            .collect();

        // Routes: one per name, paths collected from every unrouted request.
        let mut proposed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut used = names.clone();
        for (_, path) in &self.unrouted {
            let (name, pattern) = name_and_pattern(path);
            let mut candidate = name.clone();
            let mut suffix = 2;
            while used.contains(&candidate) {
                candidate = format!("{name}-{suffix}");
                suffix += 1;
            }
            let entry = proposed.entry(candidate).or_default();
            entry.insert(pattern);
        }
        for (name, paths) in proposed {
            used.insert(name.clone());
            edits.routes.push(ProposedRoute {
                name,
                path: paths.into_iter().collect(),
            });
        }

        // Grants: only grantable statements, only routes the manifest declares,
        // and never the reserved learn fallback view (`LEARN_FALLBACK_ROUTE`) —
        // the statements that dedicated fallback slot served belong to unrouted
        // requests rather than to any declared route.
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut grants: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for obs in &self.observations {
            let ObsTarget::Routed { route } = &obs.target else {
                continue;
            };
            if route == LEARN_FALLBACK_ROUTE {
                continue;
            }
            let Some(normalized) = obs.verdict.normalized() else {
                continue;
            };
            let Some(index) = manifest
                .routes()
                .iter()
                .position(|candidate| candidate.name() == route)
            else {
                continue;
            };
            let proxy = ProxyName::new(obs.proxy.clone());
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
            if !seen.insert((route.clone(), obs.proxy.clone(), normalized.to_string())) {
                continue;
            }
            grants
                .entry((route.clone(), obs.proxy.clone()))
                .or_default()
                .push(normalized.to_string());
        }
        for ((route, proxy), queries) in grants {
            edits.grants.push(ProposedGrant {
                route,
                proxy,
                queries,
            });
        }
        edits
    }
}

/// Prints the `learn:` summary line (`§1.3`).
///
/// It goes to stdout in `pretty` and to stderr under `BLASTIT_LOG_FORMAT=json`,
/// where stdout is the event stream alone (`§1.1`).
fn print_summary(manifest: &Path, pretty: bool, routes: usize, queries: usize) {
    let path = std::path::absolute(manifest)
        .unwrap_or_else(|_| manifest.to_path_buf())
        .display()
        .to_string();
    let summary = format!("learn: {routes} routes, {queries} queries added → {path}");
    if pretty {
        println!("{summary}");
    } else {
        eprintln!("{summary}");
    }
}

/// A `(name, path regex)` for one unrouted request (`R28`).
///
/// The name is the first non-empty segment, lowercased, with runs outside
/// `[a-z0-9_-]` collapsed to `-` and every leading or trailing character
/// outside `[a-z0-9]` trimmed; `/` is `root`. Numeric and UUID segments become
/// `[0-9]+` and `[^/]+`, and everything else is regex-escaped so `.` and `+` in a
/// path stay literal.
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
    let pattern = if segments.is_empty() {
        "/".to_string()
    } else {
        let mut out = String::from("/");
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                out.push('/');
            }
            out.push_str(&segment_pattern(segment));
        }
        out
    };
    (name.to_string(), pattern)
}

/// One path segment as a regex.
fn segment_pattern(segment: &str) -> String {
    if segment.chars().all(|character| character.is_ascii_digit()) {
        return "[0-9]+".to_string();
    }
    if is_uuid(segment) {
        return "[^/]+".to_string();
    }
    let mut out = String::with_capacity(segment.len());
    for character in segment.chars() {
        if "\\^$.|?*+()[]{}".contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
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
    fn the_banner_is_exactly_the_documented_three_lines() {
        let host = "127.0.0.1".parse().unwrap();
        assert_eq!(
            banner(&host, 8080, 1, false),
            "  blastit dev · single-uid simulation · 127.0.0.1:8080\n  \
             Not a sandbox. Production `blastit` refuses to run like this.\n  \
             http · scheduler · dbproxy · 1 route\n"
        );
        let two = banner(&host, 9000, 2, false);
        assert!(
            two.ends_with("http · scheduler · dbproxy · 2 routes\n"),
            "{two}"
        );

        let exposed = banner(&host, 8080, 1, true);
        let uid = euid();
        assert_eq!(
            exposed.lines().next().unwrap(),
            format!(
                "  blastit dev · single-uid simulation · 127.0.0.1:8080 · uid {uid} (EXPOSED, no auth)"
            )
        );
        assert!(exposed.contains("1 route\n"), "{exposed}");
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
    fn route_names_and_patterns_follow_the_documented_rules() {
        assert_eq!(name_and_pattern("/"), ("root".to_string(), "/".to_string()));
        assert_eq!(
            name_and_pattern("/products/42/"),
            ("products".to_string(), "/products/[0-9]+".to_string())
        );
        assert_eq!(
            name_and_pattern("/orders/550e8400-e29b-41d4-a716-446655440000"),
            ("orders".to_string(), "/orders/[^/]+".to_string())
        );
        assert_eq!(
            name_and_pattern("/robots.txt"),
            ("robots-txt".to_string(), "/robots\\.txt".to_string())
        );
        assert_eq!(
            name_and_pattern("/api.v2/items+list/"),
            ("api-v2".to_string(), "/api\\.v2/items\\+list".to_string())
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
    fn first_declared_route_and_matching_proxy_name_keep_grants_when_unrouted_exists() {
        let source = "\
version = 1
app = \"app:application\"

[proxy.home]
protocol = \"postgresql\"
input = { url = \"DATABASE_URL\" }

[[route]]
name = \"home\"
path = ['/']

[[route]]
name = \"items\"
path = ['/items']
";
        let (manifest, _) = Manifest::parse_and_validate(source, 20_000).unwrap();
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

        let edits = run.edits(&manifest);
        assert_eq!(edits.routes.len(), 1);
        assert_eq!(edits.routes[0].name, "extra");
        assert_eq!(
            edits.grants,
            vec![
                ProposedGrant {
                    route: "home".to_string(),
                    proxy: "home".to_string(),
                    queries: vec!["SELECT 111".to_string()],
                },
                ProposedGrant {
                    route: "items".to_string(),
                    proxy: "home".to_string(),
                    queries: vec!["SELECT 1".to_string()],
                },
            ],
            "the first declared route `home` keeps its statement while the reserved fallback route is skipped"
        );
    }
}
