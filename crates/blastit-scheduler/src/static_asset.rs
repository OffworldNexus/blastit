//! Resolving one decoded request path to a regular file strictly beneath a
//! declared static root (`R7`, `R11`, design `MB.2.4a` §4.2, §5.1–§5.2).
//!
//! This module owns the decode-to-segments half of the boundary and the
//! filesystem half. It never builds a header: `rewrite.rs` decides
//! (`MB.2.5.1`) and `static_response.rs` responds (`MB.2.4.2`).
//!
//! The boundary is one `O_PATH|O_DIRECTORY|O_CLOEXEC` dirfd per root, opened
//! once on `manifest_dir.join(root)`, and then `openat2` with
//! [`STATIC_RESOLVE_FLAGS`] for every candidate. A candidate is probed
//! `O_PATH`, classified by `fstat`, and only a regular file is re-opened
//! `O_RDONLY|O_CLOEXEC|O_NONBLOCK|O_NOCTTY`, with the `(st_dev, st_ino)`
//! identity check between the two opens ([`same_inode`]). A special file is
//! therefore never read-opened and a path swapped between the probe and the
//! read answers `Hidden`. Kernels without `openat2` fall back to a
//! per-segment `O_NOFOLLOW` walk ([`Backend::Walk`]) that answers identically.
//!
//! Never `canonicalize()` plus a string prefix: that resolves one path and
//! opens another (TOCTOU), follows symlinks, and is the check the
//! traversal CVE class defeats.

use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;

use blastit_manifest::StaticPolicy;
use rustix::fs::{FileType, Mode, OFlags, ResolveFlags, Stat, fstat, open, openat, openat2};
use rustix::io::Errno;

use crate::path::{path_of_target, percent_decode};

/// The longest decoded request path a static route resolves, in bytes
/// (`R7`). Checked before any syscall, so `ENAMETOOLONG` is unreachable.
pub const MAX_STATIC_PATH_BYTES: usize = 4096;

/// The longest single path segment, in bytes (`R7`, `NAME_MAX`).
pub const MAX_SEGMENT_BYTES: usize = 255;

/// The read chunk a static body is pumped in, in bytes (`R10`).
pub const STATIC_CHUNK_BYTES: usize = 64 * 1024;

/// The default ceiling for one static file, in bytes (`R10`). The only source
/// of `SchedulerConfig::static_max_bytes`: no environment variable, no
/// manifest key.
pub const STATIC_MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The longest `immutable` pattern validation accepts (`R5`).
const MAX_IMMUTABLE_PATTERN_BYTES: usize = 256;

/// Every pid/uid/control byte of a decoded path below this is refused
/// (`R7`): NUL and the C0 range, exactly [`crate::path`]'s rule.
const CONTROL_LIMIT: u8 = 0x20;

/// The exact `RESOLVE_*` composition [`StaticRoot::resolve`] passes to
/// `openat2`. Exported so `critical_static_resolve.rs` can assert the bits
/// instead of restating them (the only unprivileged oracle for the flag set).
///
/// Mounts are deliberately followed: `RESOLVE_NO_XDEV` is not used, because a
/// bind-mounted asset directory under `root` is legitimate and the boundary
/// claimed is "beneath `root` in the path namespace", not one filesystem.
#[doc(hidden)]
pub const STATIC_RESOLVE_FLAGS: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS);

/// Why a decoded request path was refused with `400` (`R7`, design §5.2 rows
/// 1–13).
///
/// [`StaticRefusal::Encoding`] is the one variant [`segments`] cannot itself
/// return: it takes an already-decoded `&str`, and the single percent-decode
/// (`crate::path`) answers `400` for a malformed escape or invalid UTF-8
/// before this function runs. The variant exists so the responder's `400`
/// mapping is one table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticRefusal {
    /// A `..` segment after the one decode.
    Traversal,
    /// A second leading `/` (`//x` or `/%2Fx`), the absolute form a naive
    /// `root + path` join treats as a filesystem path.
    Absolute,
    /// A backslash anywhere.
    Backslash,
    /// A NUL or other C0 control byte.
    Control,
    /// The decoded bytes were not a valid path string (malformed escape,
    /// invalid UTF-8): refused by the decode step, never by [`segments`].
    Encoding,
    /// Over [`MAX_STATIC_PATH_BYTES`].
    TooLong,
    /// One segment over [`MAX_SEGMENT_BYTES`].
    SegmentTooLong,
}

/// What resolving one path found (design §4.2, §4.3 steps 3–11).
#[derive(Debug)]
pub enum StaticOutcome {
    /// An exact file, a directory `index`, or the SPA `index`.
    File(StaticFile),
    /// Nothing at that path: the caller may forward (design §4.3 steps 9–10).
    Missing,
    /// `spa = true` and `root/index` is missing: `404`, final.
    SpaLost,
    /// The entry exists but is not servable: `404`, final.
    Hidden,
    /// The path itself was malformed: `400`, final.
    Refused(StaticRefusal),
}

/// Which of the three answers a [`StaticFile`] is (design §4.5 cache rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerKind {
    /// The request path named the file.
    Exact,
    /// The request path named a directory whose `index` answered.
    DirectoryIndex,
    /// The request path named nothing; the root `index` answered.
    SpaFallback,
}

/// One servable static file, read-open and identity-checked.
#[derive(Debug)]
pub struct StaticFile {
    /// The answering file's URL path, built from the **resolved** segments, so
    /// every alias of one file shares one key: `/assets//app.js` and
    /// `/assets/app.js` have the same `url_path`, `ETag` and cache header.
    pub url_path: String,
    /// Which of the three answers this is.
    pub kind: AnswerKind,
    /// `O_RDONLY`, `fstat`-verified regular file.
    pub file: OwnedFd,
    /// `st_size` at the read open.
    pub len: u64,
    /// The metadata the head is built from.
    pub meta: StaticMeta,
}

/// The metadata of one static file, from the read-open `fstat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticMeta {
    /// `st_size`, equal to [`StaticFile::len`].
    pub size: u64,
    /// `st_mtime`, seconds.
    pub mtime_secs: i64,
    /// `st_mtime`, nanoseconds.
    pub mtime_nsec: u32,
}

/// Why a static root could not be opened (design §4.2, §4.7).
///
/// `run` maps it into `SchedulerError::Protocol("static root <root>:
/// <source>")`, which exits the role: a root the responder cannot open is not
/// a degraded answer, it is a broken declaration.
#[derive(Debug, thiserror::Error)]
#[error("static root {root}: {source}")]
pub struct StaticError {
    /// The root exactly as the manifest wrote it.
    pub root: String,
    /// What `open` failed with.
    pub source: io::Error,
}

/// One `/`-anchored pattern: literal bytes, `*` = any run without `/`.
///
/// The matcher is whole-match and anchored (design §4.5): `*` matches zero or
/// more bytes except `/`, and it is the only metacharacter.
#[derive(Clone, Debug)]
pub struct ImmutableGlob {
    pattern: String,
}

impl ImmutableGlob {
    /// Compiles `pattern`, or `None` when validation should have refused it.
    ///
    /// `None` only if `MB.2.3` let the pattern through: not `/`-anchored,
    /// empty, over the 256-byte ceiling, a control byte, or a `..` segment.
    pub fn new(pattern: &str) -> Option<Self> {
        if pattern.is_empty()
            || pattern.len() > MAX_IMMUTABLE_PATTERN_BYTES
            || !pattern.starts_with('/')
            || pattern.bytes().any(|byte| byte < CONTROL_LIMIT)
            || pattern.split('/').any(|segment| segment == "..")
        {
            return None;
        }
        Some(Self {
            pattern: pattern.to_string(),
        })
    }

    /// Whether `url_path` (the answering file's path) matches whole.
    pub fn matches(&self, url_path: &str) -> bool {
        glob(self.pattern.as_bytes(), url_path.as_bytes())
    }
}

/// `glob`'s recursive whole-match: `*` is the only metacharacter and stops at
/// `/`.
fn glob(pattern: &[u8], path: &[u8]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(b'*') => {
            let rest = &pattern[1..];
            let mut end = 0usize;
            loop {
                if glob(rest, &path[end..]) {
                    return true;
                }
                match path.get(end) {
                    // `*` may be empty, and it never crosses a `/`.
                    Some(b'/') | None => return false,
                    Some(_) => end += 1,
                }
            }
        }
        Some(byte) => path.first() == Some(byte) && glob(&pattern[1..], &path[1..]),
    }
}

/// The two ways a candidate is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// `openat2` with [`STATIC_RESOLVE_FLAGS`]: one syscall, kernel-enforced
    /// boundaries.
    Openat2,
    /// A per-segment `O_NOFOLLOW` `openat` walk, for kernels without
    /// `openat2` (`ENOSYS`/`EINVAL`). Selectable in tests, so it is not dead
    /// code.
    Walk,
}

/// One declared static root: its dirfd and the policy that shapes an answer.
#[derive(Debug)]
pub struct StaticRoot {
    /// The root exactly as declared, for boot-time messages.
    // Read by `SchedulerError::Protocol`'s message and the boot probe
    // (`MB.2.5.2`, `MB.2.6`); this leaf only stores it.
    #[allow(dead_code)]
    pub(crate) root: String,
    /// The one `O_PATH|O_DIRECTORY|O_CLOEXEC` dirfd every lookup starts from.
    pub(crate) dirfd: OwnedFd,
    /// The `index` file, relative to `root` (validation guarantees no
    /// leading `/`, no `.`/`..` segment and no dotfile segment).
    pub(crate) index: String,
    /// Whether a miss answers `index` instead of the app.
    pub(crate) spa: bool,
    /// The `/`-anchored patterns that earn the year-long cache.
    // Read by `static_response.rs`'s cache rule (`MB.2.4.2`).
    #[allow(dead_code)]
    pub(crate) immutable: Vec<ImmutableGlob>,
    /// The per-file ceiling, from `SchedulerConfig::static_max_bytes`
    /// ([`STATIC_MAX_FILE_BYTES`] until `run` overrides it).
    pub(crate) max_bytes: u64,
    /// Which backend this root exercises: fixed by `open_with` from the
    /// kernel's `openat2` support, so every lookup takes one path.
    backend: Backend,
}

/// What an `O_PATH` probe found.
enum Landing {
    /// A regular file within the ceiling, with its `O_PATH` descriptor.
    File(OwnedFd, Stat),
    /// A directory: the `index` lookup owns the next step.
    Directory,
    /// `ENOENT`/`ENOTDIR`: nothing at this path.
    Missing,
    /// Anything else: the path exists but is not servable.
    Hidden,
}

impl StaticRoot {
    /// Opens `root` beneath `manifest_dir`, as the scheduler's uid.
    ///
    /// This is the only place a symlink is followed, and only on the
    /// operator's own declaration (`root = "current/public"` must work).
    pub fn open(manifest_dir: &Path, policy: &StaticPolicy) -> Result<Self, StaticError> {
        Self::open_with(manifest_dir, policy, Backend::Openat2)
    }

    /// [`StaticRoot::open`] with the backend chosen explicitly.
    ///
    /// `#[doc(hidden)] pub`, not `#[cfg(test)]`: an integration test compiles
    /// the crate as a dependency, where `cfg(test)` is off, so the `Walk`
    /// matrix could not run.
    #[doc(hidden)]
    pub fn open_with(
        manifest_dir: &Path,
        policy: &StaticPolicy,
        backend: Backend,
    ) -> Result<Self, StaticError> {
        let path = manifest_dir.join(&policy.root);
        let dirfd = open(
            &path,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|source| StaticError {
            root: policy.root.clone(),
            source: io::Error::from(source),
        })?;
        // A kernel without openat2 is detected once, at boot, so every later
        // lookup takes the same path and the two backends stay comparable.
        let backend = if backend == Backend::Walk || !supports_openat2(&dirfd) {
            Backend::Walk
        } else {
            Backend::Openat2
        };
        Ok(Self {
            root: policy.root.clone(),
            dirfd,
            index: policy.index.clone(),
            spa: policy.spa,
            immutable: policy
                .immutable
                .iter()
                .filter_map(|pattern| ImmutableGlob::new(pattern))
                .collect(),
            max_bytes: STATIC_MAX_FILE_BYTES,
            backend,
        })
    }

    /// Resolves `segments` (already decoded, already split by [`segments`]).
    pub fn resolve(&self, segments: &[&str]) -> StaticOutcome {
        self.resolve_with(segments, None)
    }

    /// [`StaticRoot::resolve`], with a hook between the `O_PATH` probe and the
    /// read open.
    ///
    /// The protected suite uses `Some` to swap the leaf for a different inode
    /// and prove the [`same_inode`] check answers `Hidden` — the only way to
    /// exercise the TOCTOU arm without a real race.
    #[doc(hidden)]
    pub fn resolve_with(&self, segments: &[&str], after_probe: Option<&dyn Fn()>) -> StaticOutcome {
        // `resolve` is `pub`, so the "the walk cannot leave the root" guarantee
        // is enforced here and not only by [`segments`]'s caller: a `..`, an
        // empty segment or an embedded `/` would let the per-segment walk open
        // a directory outside the root (`openat(dir, "..")` succeeds).
        if segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == ".." || segment.contains('/'))
        {
            return StaticOutcome::Hidden;
        }
        let rel = segments.join("/");
        match self.probe(&rel) {
            Landing::File(file, stat) => {
                self.finish(file, stat, &rel, AnswerKind::Exact, after_probe)
            }
            Landing::Directory => {
                let candidate = if rel.is_empty() {
                    self.index.clone()
                } else {
                    format!("{rel}/{}", self.index)
                };
                match self.probe(&candidate) {
                    Landing::File(file, stat) => self.finish(
                        file,
                        stat,
                        &candidate,
                        AnswerKind::DirectoryIndex,
                        after_probe,
                    ),
                    Landing::Hidden => StaticOutcome::Hidden,
                    // A directory with no servable `index` "falls through like
                    // any missing path" (design §4.3 step 7).
                    Landing::Directory | Landing::Missing => self.missed(after_probe),
                }
            }
            Landing::Missing => self.missed(after_probe),
            Landing::Hidden => StaticOutcome::Hidden,
        }
    }

    /// A miss: the SPA fallback, or `Missing` for the caller to forward.
    fn missed(&self, after_probe: Option<&dyn Fn()>) -> StaticOutcome {
        if !self.spa {
            return StaticOutcome::Missing;
        }
        match self.probe(&self.index) {
            Landing::File(file, stat) => self.finish(
                file,
                stat,
                &self.index,
                AnswerKind::SpaFallback,
                after_probe,
            ),
            // A refused index is a `404` either way, and the SPA fallback has
            // no third answer: it is `SpaLost`.
            Landing::Directory | Landing::Missing | Landing::Hidden => StaticOutcome::SpaLost,
        }
    }

    /// The probe→read-open half: run the hook, re-open, and prove identity.
    fn finish(
        &self,
        probe: OwnedFd,
        probe_stat: Stat,
        rel: &str,
        kind: AnswerKind,
        after_probe: Option<&dyn Fn()>,
    ) -> StaticOutcome {
        if let Some(hook) = after_probe {
            hook();
        }
        // The probe stays open until the identity check: the probed inode is
        // pinned, so a reused inode number at the same path cannot satisfy the
        // `(st_dev, st_ino)` comparison below.
        let read_flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
        let Ok(file) = self.open_beneath(rel, read_flags) else {
            // `EACCES`, `EISDIR`, a vanished file: hidden, never served.
            return StaticOutcome::Hidden;
        };
        let Ok(stat) = fstat(&file) else {
            return StaticOutcome::Hidden;
        };
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
            return StaticOutcome::Hidden;
        }
        let identity = same_inode(
            (probe_stat.st_dev, probe_stat.st_ino),
            (stat.st_dev, stat.st_ino),
        );
        drop(probe);
        if !identity {
            return StaticOutcome::Hidden;
        }
        let len = u64::try_from(stat.st_size).unwrap_or(0);
        if len > self.max_bytes {
            return StaticOutcome::Hidden;
        }
        StaticOutcome::File(StaticFile {
            url_path: format!("/{rel}"),
            kind,
            file,
            len,
            meta: StaticMeta {
                size: len,
                mtime_secs: stat.st_mtime,
                mtime_nsec: u32::try_from(stat.st_mtime_nsec).unwrap_or(0),
            },
        })
    }

    /// Classifies one `O_PATH` probe.
    fn probe(&self, rel: &str) -> Landing {
        let descriptor = match self.open_beneath(rel, OFlags::PATH | OFlags::CLOEXEC) {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT | Errno::NOTDIR) => return Landing::Missing,
            // Every other errno is a refusal: a symlink at any depth
            // (`ELOOP`), a permission (`EACCES`), a resource failure, or an
            // errno this release has never heard of (design §5.1's total
            // table).
            Err(_) => return Landing::Hidden,
        };
        let Ok(stat) = fstat(&descriptor) else {
            return Landing::Hidden;
        };
        let len = u64::try_from(stat.st_size).unwrap_or(0);
        match classify(FileType::from_raw_mode(stat.st_mode), len, self.max_bytes) {
            Ok(()) => Landing::File(descriptor, stat),
            Err(StaticOutcome::Missing) => Landing::Directory,
            Err(_) => Landing::Hidden,
        }
    }

    /// `openat2` where the kernel has it, the `O_NOFOLLOW` walk otherwise.
    ///
    /// `walk` adds `O_NOFOLLOW` itself, so a read open is one call with the
    /// read flags and a probe is one with `O_PATH`.
    fn open_beneath(&self, rel: &str, flags: OFlags) -> Result<OwnedFd, Errno> {
        match self.backend {
            Backend::Openat2 => match self.openat2(rel, flags) {
                // A kernel that reports no `openat2` (or rejects the flags)
                // still gets the boundary: the walk. `open` already probed
                // this once, so the arm is defence.
                Err(Errno::NOSYS | Errno::INVAL) => self.walk(rel, flags),
                other => other,
            },
            Backend::Walk => self.walk(rel, flags),
        }
    }

    /// One `openat2` beneath the dirfd; the path is relative by construction.
    fn openat2(&self, rel: &str, flags: OFlags) -> Result<OwnedFd, Errno> {
        let path = if rel.is_empty() { "." } else { rel };
        openat2(
            &self.dirfd,
            path,
            flags,
            Mode::empty(),
            STATIC_RESOLVE_FLAGS,
        )
    }

    /// The per-segment fallback: every intermediate must be a real directory
    /// and the leaf a real entry, all opened `O_NOFOLLOW`.
    ///
    /// A symlink intermediate answers `ELOOP` exactly as
    /// `RESOLVE_NO_SYMLINKS` does on the `openat2` path (an `O_NOFOLLOW` open
    /// that lands on a symlink succeeds, so the type is checked explicitly),
    /// and a non-directory intermediate answers `ENOTDIR` as `openat2` would.
    /// `..`, `.` and empty segments are already gone, so the walk cannot leave
    /// the root.
    fn walk(&self, rel: &str, leaf_flags: OFlags) -> Result<OwnedFd, Errno> {
        let probe = OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut current = openat(&self.dirfd, ".", probe | OFlags::DIRECTORY, Mode::empty())?;
        if rel.is_empty() {
            // The path "/" is the root directory itself (design §4.3 step 6:
            // descend to `index`), never an empty-name lookup.
            return Ok(current);
        }
        let parts: Vec<&str> = rel.split('/').collect();
        let (leaf, parents) = match parts.split_last() {
            Some(split) => split,
            // `split` on `""` never yields an empty slice; keep the total case
            // explicit rather than unreachable.
            None => return Err(Errno::INVAL),
        };
        for part in parents {
            let next = openat(&current, *part, probe, Mode::empty())?;
            let stat = fstat(&next)?;
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::Directory => current = next,
                FileType::Symlink => return Err(Errno::LOOP),
                _ => return Err(Errno::NOTDIR),
            }
        }
        openat(
            &current,
            *leaf,
            leaf_flags | OFlags::NOFOLLOW,
            Mode::empty(),
        )
    }
}

/// Whether this kernel can `openat2` at all (`ENOSYS`), or accepts the resolve
/// flags (`EINVAL` on a kernel that knows the syscall but not one of them).
fn supports_openat2(dirfd: &OwnedFd) -> bool {
    match openat2(
        dirfd,
        ".",
        OFlags::PATH | OFlags::CLOEXEC,
        Mode::empty(),
        STATIC_RESOLVE_FLAGS,
    ) {
        Ok(_) => true,
        Err(Errno::NOSYS | Errno::INVAL) => false,
        // An unrelated errno says nothing about the syscall's existence.
        Err(_) => true,
    }
}

/// The probe→read-open identity check, over plain `(st_dev, st_ino)` tuples so
/// a protected integration test can build every row (`rustix::fs::Stat` is
/// `#[non_exhaustive]` and has no constructor).
#[doc(hidden)]
pub fn same_inode(probe: (u64, u64), read: (u64, u64)) -> bool {
    probe == read
}

/// One decode already happened (`crate::path`). The decoded path must begin
/// with exactly one `/`: a second leading `/` (from `//x` or `/%2Fx`) is
/// [`StaticRefusal::Absolute`], because it is the form a naive `root + path`
/// join treats as an absolute filesystem path.
///
/// Interior `//` collapses and `.` segments are dropped; `..` is refused. No
/// Unicode normalization and no second decode, so `%252e` is the literal text
/// `%2e`.
pub fn segments(decoded: &str) -> Result<Vec<&str>, StaticRefusal> {
    if decoded.len() > MAX_STATIC_PATH_BYTES {
        return Err(StaticRefusal::TooLong);
    }
    if decoded.bytes().any(|byte| byte < CONTROL_LIMIT) {
        return Err(StaticRefusal::Control);
    }
    let rest = match decoded.strip_prefix('/') {
        Some(rest) => rest,
        // A request target is absolute-form; anything else is not a path this
        // resolver has a meaning for, and is refused as the absolute shape.
        None => return Err(StaticRefusal::Absolute),
    };
    if rest.starts_with('/') {
        return Err(StaticRefusal::Absolute);
    }
    if decoded.contains('\\') {
        return Err(StaticRefusal::Backslash);
    }
    let mut segments = Vec::new();
    for segment in rest.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err(StaticRefusal::Traversal);
        }
        if segment.len() > MAX_SEGMENT_BYTES {
            return Err(StaticRefusal::SegmentTooLong);
        }
        segments.push(segment);
    }
    Ok(segments)
}

/// The scheduler's head half of design §4.3 (steps 3–4), on a raw request
/// target: cut at `?`, exactly one percent-decode, [`segments`], then the
/// [`is_dotfile`] rule.
///
/// `Ok` is the segment list to hand to [`StaticRoot::resolve`]; `Err` is the
/// final outcome for that path (`Refused` for a malformed one, `Hidden` for a
/// dotfile). `#[doc(hidden)] pub` so the protected suite table-drives the
/// production decode and dotfile wiring instead of restating them.
#[doc(hidden)]
pub fn head_segments(target: &str) -> Result<Vec<String>, StaticOutcome> {
    let decoded = percent_decode(path_of_target(target.as_bytes()))
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let Some(decoded) = decoded else {
        return Err(StaticOutcome::Refused(StaticRefusal::Encoding));
    };
    let parts = segments(&decoded).map_err(StaticOutcome::Refused)?;
    if is_dotfile(&parts) {
        return Err(StaticOutcome::Hidden);
    }
    Ok(parts.into_iter().map(str::to_string).collect())
}

/// The dotfile rule (design §4.3 step 4): a segment starting with `.` is
/// hidden, unless the first segment is exactly `.well-known`.
///
/// The exemption covers the first segment only, so
/// `/.well-known/acme-challenge/token` is public and `/.well-known/.env` is
/// not. `.` and `..` never reach this rule: [`segments`] has already handled
/// them.
pub fn is_dotfile(segments: &[&str]) -> bool {
    segments.iter().enumerate().any(|(position, segment)| {
        segment.starts_with('.') && !(position == 0 && *segment == ".well-known")
    })
}

/// The whole-file classification, pure: the row a `fstat` result lands in.
///
/// * `Ok(())` — a regular file at or below `max_bytes`: servable.
/// * `Err(StaticOutcome::Missing)` — a directory: not a file at this path, so
///   the caller descends to `index` and, when that is missing too, reports
///   `Missing` (design §4.3 steps 6–7).
/// * `Err(StaticOutcome::Hidden)` — everything else: a FIFO, a socket, a
///   character or block device, a symlink, an unknown filesystem object, or a
///   regular file over `max_bytes`. The `_` arm is this table's total case.
pub fn classify(kind: FileType, len: u64, max_bytes: u64) -> Result<(), StaticOutcome> {
    match kind {
        FileType::RegularFile if len <= max_bytes => Ok(()),
        FileType::RegularFile => Err(StaticOutcome::Hidden),
        FileType::Directory => Err(StaticOutcome::Missing),
        _ => Err(StaticOutcome::Hidden),
    }
}
