//! The `entrypoint php` discovery gate (`ADR-0010` §3/§4.1, design §4.4 D5).
//!
//! Stage 0 resolves a host NTS `libphp` and the shim for its `(minor, arch,
//! libc)` tuple **before a listener binds**, and it never loads anything: the
//! library is opened, `fstat`ed and read as an ELF `.dynsym` table, and
//! `dlopen` happens inside a view worker. Nothing in this module names a SAPI
//! type, the worker module, the pin module or the loader crate — the only load
//! in the tree is the loader's, reached from the worker and the fork alone, and
//! a source-level grep over this file is the structural oracle that keeps it so
//! (design §7, `R5`). That is what lets `init` answer "can this host embed
//! PHP?" on a host
//! where the worker could never start, and why the refusal is a boot sentence
//! instead of a first-request fault.
//!
//! # The order is the order an operator can act on
//!
//! The library (missing, a directory, not an ELF, ZTS, no `php_version`, no
//! version this build can read) and then the shim for the tuple. Every one of
//! those refusals is fatal in **both** scopes: `ADR-0010` §1 gives PHP one
//! backend, so a host that cannot serve a declared `entrypoint php` file gets
//! §4.1's package sentence — in `blastit dev` and `blastit check` as much as in
//! a deployment. There is no development mode that serves a PHP route another
//! way.
//!
//! # What this module is told, and what it never sees
//!
//! [`PhpSearch`] (where to look), the manifest (what the file declares) and — for
//! a deployment — a [`Deployment`]: the uid that must own every path, the
//! directory the manifest came from, and **two** provenance callbacks the
//! composition root supplies over `blastit::host`. Two, not one: the object
//! decision is ownership + file capabilities + the zero-writable
//! `DT_RPATH`/`DT_RUNPATH` scan, the tree decision is ownership only, and a
//! single callback would silently drop the search-path refusal (design §4.4 D5).
//!
//! # Reachable from `init` without a `dlopen`
//!
//! Everything here is `std` and [`blastit_sys::elf`] (the shared,
//! bounds-checked ELF reader: header, section header table, section bytes) plus
//! the manifest data model. `cargo tree -p blastit-view-php` still carries
//! `blastit-php-sapi`, because the crate's worker half needs it — the
//! enforceable invariant is "no `dlopen` before the fork", proved by the
//! `critical_php_embed` canary legs and by the source-level test that this
//! module's text names no loader.

use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use blastit_manifest::Manifest;
use blastit_sys::elf::{Class, Elf, SHF_ALLOC, SHF_WRITE, SectionHeader};

/// The distro file names a host NTS `libphp` is looked for under, newest first
/// (`ADR-0010` §3).
///
/// A fixed list rather than a directory scan: the order *is* the preference, so
/// a `libphp8.5.so` somebody dropped into `/usr/local/lib` never changes which
/// library an 8.4 host embeds. Both spellings the distros install are here: the
/// dotted Debian/Ubuntu name and the **packed** Alpine 8.3 one (`libphp83.so`;
/// Alpine's 8.4 and 8.5 state their minor in `/usr/lib/php84` and
/// `/usr/lib/php85` instead, `MB.3.59`), so a host with `BLASTIT_LIBPHP` unset
/// reaches Alpine's package without a copy.
///
/// The unversioned `libphp.so` is last here because on its own it states no
/// minor; the per-minor directory it sits in states one, which is why
/// `LIBPHP_PROBES` also tries it there, under that minor (`MB.3.61`).
pub const LIBPHP_CANDIDATES: [&str; 10] = [
    "libphp8.5.so",
    "libphp-8.5.so",
    "libphp85.so",
    "libphp8.4.so",
    "libphp-8.4.so",
    "libphp84.so",
    "libphp8.3.so",
    "libphp-8.3.so",
    "libphp83.so",
    "libphp.so",
];

/// One step of the discovery order: a file name, and which of
/// [`PhpSearch::lib_dirs`] it is looked for in (`MB.3.61`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibphpProbe {
    /// The file name, as [`LIBPHP_CANDIDATES`] spells it.
    name: &'static str,
    /// `None`: every directory, in [`PhpSearch::lib_dirs`] order. `Some("php84")`:
    /// only the per-minor embed directories whose last component is that name, so
    /// a `libphp.so` there is the 8.4 library the *path* names.
    per_minor_dir: Option<&'static str>,
}

/// The order the search walks: **version-major**, then spelling
/// (`MB.3.61`).
///
/// Each minor's own spellings — dotted, hyphenated, packed, and the per-minor
/// `libphp.so` — are all tried before the next minor's, so the newest *library*
/// wins rather than the first name in a list. That is what makes a host with two
/// embed packages installed serve from the newest one: Alpine's `php83-embed`
/// puts `libphp83.so` in `/usr/lib` and `php85-embed` puts `libphp.so` in
/// `/usr/lib/php85`, and a name-major walk preferred `libphp83.so` — an older
/// library — while the 8.5 library beside it would serve.
///
/// The unversioned `libphp.so` in the plain directories stays last: its minor is
/// only known from the descriptor's link target or the object's own bytes, so it
/// is a fallback, not a preference.
pub const LIBPHP_PROBES: [LibphpProbe; 13] = [
    LibphpProbe {
        name: "libphp8.5.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.5.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp85.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php85"),
    },
    LibphpProbe {
        name: "libphp8.4.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.4.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp84.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php84"),
    },
    LibphpProbe {
        name: "libphp8.3.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp-8.3.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp83.so",
        per_minor_dir: None,
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: Some("php83"),
    },
    LibphpProbe {
        name: "libphp.so",
        per_minor_dir: None,
    },
];

/// The distributions' PHP embed packages, named once so every refusal says the
/// same thing (`ADR-0010` §4.1).
const PHP_PACKAGES: &str = "install the PHP embed package — Debian/Ubuntu: \
     apt install libphp-embed, Alpine: apk add php85-embed, Fedora/RHEL: \
     dnf install php-embedded";

/// The minors this build ships a shim for, as one sentence.
///
/// `docs/frameworks/php.md` states the same three minors and nothing else; a
/// host on another minor is refused with exactly this sentence
/// ([`PhpRefusal::UnsupportedVersion`]).
const SUPPORTED_PHP: &str = "PHP 8.3, 8.4 and 8.5";

/// The one override `ADR-0010` §3 allows.
const PHP_OVERRIDE: &str = "set BLASTIT_LIBPHP=/path/to/libphp.so";

/// `ADR-0010` §4.1's sentence for a **stage-1 probe** refusal.
///
/// The stage-0 gate builds the same shape from the same two halves
/// (`present_refusal`); this is the post-barrier twin, so a shim that will not
/// load, an ABI this build does not speak, a minor that disagrees with the
/// library, or a `worker_init` that hangs reads exactly like a discovery
/// refusal and names the same fix.
pub fn php_probe_refusal(
    route: &str,
    entry: &str,
    problem: &str,
    version: Option<PhpVersion>,
) -> String {
    present_refusal(
        &format!("route {route:?}: entrypoint php {entry:?}: "),
        problem,
        version,
    )
}

/// The compile-time target's architecture, as the shim name spells it.
///
/// Deliberately a second spelling of the tuple in `crates/blastit-php-sapi`
/// (`Minor::shim_file()`): that crate is an *optional* dependency of this
/// binary (`php` feature) while this discovery gate compiles on every build, so
/// its tuple cannot be imported here unconditionally (and the no-load guard
/// below bans naming the loader crate at all).
/// `the_shim_name_matches_the_adr_literal` pins this spelling to the ADR-0010
/// §4.1 literals, so the two cannot drift.
const fn target_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

/// The compile-time target's libc, as the shim name spells it (`ADR-0010` §4).
///
/// Pinned by `the_shim_name_matches_the_adr_literal` for the same reason as
/// [`target_arch`].
const fn target_libc() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    }
}

/// `x86_64-gnu`: the half of the ABI tuple that is fixed at compile time.
pub fn target_tuple() -> String {
    format!("{}-{}", target_arch(), target_libc())
}

/// `x86_64-linux-gnu`: the multi-arch directory name a distro installs into.
fn multiarch_triplet() -> String {
    format!("{}-linux-{}", target_arch(), target_libc())
}

/// The shim for one host version: `libblastit_php84-x86_64-gnu.so`.
pub fn shim_name(version: PhpVersion) -> String {
    format!("libblastit_php{}-{}.so", version.number(), target_tuple())
}

/// The multi-arch library directories a distro may put `libphp` in, in order
/// (`ADR-0010` §3).
///
/// The per-minor directories come after the plain `/usr/lib` and before the
/// `/usr/local` half: Alpine installs 8.4 as `/usr/lib/php84/libphp.so` and 8.5
/// as `/usr/lib/php85/libphp.so`, and with `BLASTIT_LIBPHP` unset a fixed name
/// alone cannot reach them — the directory is half of the package's spelling
/// (`MB.3.59`). `/usr/local/lib/php8x` is the twin of a copy an operator put
/// there.
///
/// The directory order is *within* one minor: [`LIBPHP_PROBES`] picks the minor
/// first, so `/usr/lib/php85` outranks `/usr/lib/libphp84.so` even though it is
/// listed later (`MB.3.61`).
pub fn library_dirs() -> Vec<PathBuf> {
    let triplet = multiarch_triplet();
    [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
        "/usr/lib/php85".to_string(),
        "/usr/lib/php84".to_string(),
        "/usr/lib/php83".to_string(),
        format!("/usr/local/lib/{triplet}"),
        "/usr/local/lib".to_string(),
        "/usr/local/lib/php85".to_string(),
        "/usr/local/lib/php84".to_string(),
        "/usr/local/lib/php83".to_string(),
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Whether the discovery may look in the development shim directory.
///
/// `ADR-0010` §4 and `R8`: `BLASTIT_PHP_SHIM_DIR` and `target/php-sapi` are the
/// development and `check` seams. A deployed boot takes the shims the artifact
/// ships — beside the executable — and nothing an environment variable names,
/// because a shim is code that runs inside every PHP worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhpShimScope {
    /// `blastit`: beside the executable only.
    Deployment,
    /// `blastit dev` and `blastit check`: `BLASTIT_PHP_SHIM_DIR`, then beside
    /// the executable, then the `target/php-sapi` directory `make php-sapi`
    /// writes.
    Development,
}

/// Everything the `entrypoint php` discovery looks at, injectable so a test never
/// depends on the host's `/usr/lib`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhpSearch {
    /// `BLASTIT_LIBPHP`: when set, the only candidate.
    pub libphp: Option<PathBuf>,
    /// The directories a shim is looked for in, in order.
    pub shim_dirs: Vec<PathBuf>,
    /// The directories the fixed names in [`LIBPHP_CANDIDATES`] are looked for
    /// in, in order.
    pub lib_dirs: Vec<PathBuf>,
}

impl PhpSearch {
    /// The real directories for `scope`, with an explicit override.
    pub fn new(libphp: Option<PathBuf>, shim_dir: Option<PathBuf>, scope: PhpShimScope) -> Self {
        let mut shim_dirs = Vec::new();
        if scope == PhpShimScope::Development {
            shim_dirs.extend(shim_dir);
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            shim_dirs.push(dir.to_path_buf());
        }
        // `make php-sapi` writes `target/php-sapi`; finding it from the running
        // executable is what makes it independent of the working directory a
        // developer started the boot in.
        if scope == PhpShimScope::Development
            && let Ok(exe) = std::env::current_exe()
            && let Some(target) = exe
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
        {
            shim_dirs.push(target.join("php-sapi"));
        }
        Self {
            libphp,
            shim_dirs,
            lib_dirs: library_dirs(),
        }
    }
}

/// What the static `.dynsym` inspection says about one shared object
/// (`ADR-0010` §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZendAbi {
    /// `executor_globals` is exported and `tsrm_startup` is not: the
    /// Non-Thread-Safe build, the only one an embedded worker may load.
    Nts,
    /// `tsrm_startup` is exported: a thread-safe (ZTS) build.
    Zts,
    /// Neither symbol is exported: not a `libphp` at all.
    NotLibphp,
}

/// Why the `entrypoint php` gate refused, before the sentence names the route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhpRefusal {
    /// No `BLASTIT_LIBPHP` and no candidate file: nothing to embed.
    Missing {
        /// The directories that were searched.
        searched: Vec<PathBuf>,
    },
    /// `BLASTIT_LIBPHP` is not an absolute path.
    NotAbsolute {
        /// The value, as given.
        path: PathBuf,
    },
    /// The file could not be opened, or is not a regular file.
    NotAFile {
        /// The candidate.
        path: PathBuf,
        /// The kernel's reason.
        detail: String,
    },
    /// The file is not an ELF shared object this build can inspect.
    NotElf {
        /// The candidate.
        path: PathBuf,
        /// What the walk found instead.
        detail: String,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object is a ZTS (thread-safe) `libphp`.
    ThreadSafe {
        /// The candidate.
        path: PathBuf,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object exports neither Zend global, so it is not a `libphp`.
    NotLibphp {
        /// The candidate.
        path: PathBuf,
        /// The version its name states, when it states one.
        version: Option<PhpVersion>,
    },
    /// The object's `.dynsym` has no `php_version` **name**, so the worker
    /// cannot resolve the symbol it reads its minor from at load time
    /// (`MB.3.59`). An object that exports the name but whose body this walk
    /// cannot read is [`Self::NoVersion`], not this (`MB.3.61`).
    NoPhpVersion {
        /// The candidate.
        path: PathBuf,
        /// The version the path itself states, when it states one — named in
        /// the sentence so an operator sees that the file name does not help.
        named: Option<PhpVersion>,
    },
    /// The host's `libphp` is a PHP minor this build ships no shim for.
    ///
    /// The supported set is stated once, in `SUPPORTED_PHP`, and the refusal
    /// is that sentence: a user does not have to learn `.dynsym` inspection to
    /// hear that their PHP is too old (or too new).
    UnsupportedVersion {
        /// The minor the object or its path states.
        version: PhpVersion,
    },
    /// The path states two different minors, or the path and the object's own
    /// bytes disagree, so no single shim can be chosen.
    VersionConflict {
        /// The candidate.
        path: PathBuf,
        /// Every minor the path states, with the spelling that stated it.
        stated: Vec<(PhpVersion, String)>,
        /// What the object's compile-time version string says, when readable.
        measured: Option<PhpVersion>,
    },
    /// `php_version` is exported in the `.dynsym`, but the version it returns
    /// could not be read from the object — no readable `return PHP_VERSION;`
    /// body, an `STT_NOTYPE` symbol, or `st_size == 0` — and the path states no
    /// minor either (`MB.3.61`).
    NoVersion {
        /// The candidate.
        path: PathBuf,
    },
    /// The host version is known but no shim for its tuple was found.
    NoShim {
        /// The NTS `libphp` that was found.
        libphp: PathBuf,
        /// Its version.
        version: PhpVersion,
        /// The file name that was looked for.
        shim: String,
        /// The directories that were searched.
        searched: Vec<PathBuf>,
    },
}

/// `(PHP 8.4, x86_64-gnu)` and the shim that tuple needs.
///
/// The two halves `ADR-0010` §4.1 requires every version-specific refusal to
/// name, built in one place so they cannot disagree.
fn abi_tuple(version: Option<PhpVersion>) -> Option<(String, String)> {
    let version = version?;
    Some((version.dotted(), shim_name(version)))
}

/// The sentence an `entrypoint php` boot fails with (`ADR-0010` §4.1).
///
/// Both shapes name the route and its `entry`, the distro package that provides
/// the library, and `BLASTIT_LIBPHP`. The version-specific shape also names the
/// `(PHP X.Y, <arch>-<libc>)` tuple and the shim that tuple needs, because
/// "wrong minor" and "no shim" are the two failures an operator can fix by
/// installing a package, and neither is diagnosable without the tuple.
pub fn php_refusal_message(route: &str, entry: &str, refusal: &PhpRefusal) -> String {
    // Every shape names the route **and** its `entry`: a manifest with two PHP
    // routes must let the operator tell which front controller the host cannot
    // serve, and the entry is the only thing that distinguishes them.
    let prefix = format!("route {route:?}: entrypoint php {entry:?}: ");
    match refusal {
        PhpRefusal::Missing { searched } => format!(
            "{prefix}no usable NTS libphp was found (searched BLASTIT_LIBPHP, then {} in {}); \
             {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            LIBPHP_CANDIDATES.join(", "),
            display_paths(searched),
        ),
        PhpRefusal::NotAbsolute { path } => format!(
            "{prefix}BLASTIT_LIBPHP ({}) is not an absolute path, so the library the \
             boot inspected and the one a worker loads could differ; {PHP_PACKAGES}, or \
             {PHP_OVERRIDE}",
            path.display()
        ),
        PhpRefusal::NotAFile { path, detail } => format!(
            "{prefix}BLASTIT_LIBPHP ({}) cannot be opened or is not a regular file \
             ({detail}); {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            path.display()
        ),
        PhpRefusal::NotElf {
            path,
            detail,
            version,
        } => present_refusal(
            &prefix,
            &format!("{} is not an ELF shared object ({detail})", path.display()),
            *version,
        ),
        PhpRefusal::ThreadSafe { path, version } => present_refusal(
            &prefix,
            &format!(
                "{} is a ZTS (thread-safe) libphp, and the embedded worker needs the NTS \
                 build of the same version",
                path.display()
            ),
            *version,
        ),
        PhpRefusal::NotLibphp { path, version } => present_refusal(
            &prefix,
            &format!(
                "{} exports neither executor_globals nor tsrm_startup, so it is not a PHP \
                 embed library",
                path.display()
            ),
            *version,
        ),
        PhpRefusal::NoPhpVersion { path, named } => {
            let name = named.map_or_else(String::new, |version| {
                format!(
                    " (its file name states PHP {}, but a file name cannot replace the symbol)",
                    version.dotted()
                )
            });
            format!(
                "{prefix}{} is an NTS libphp whose `.dynsym` exports no `php_version` — the \
                 symbol the embedded worker resolves before `php_module_startup` — so the worker \
                 cannot determine its minor at load time and no shim can be chosen for it{name}; \
                 {PHP_PACKAGES}, or {PHP_OVERRIDE}",
                path.display()
            )
        }
        PhpRefusal::UnsupportedVersion { version } => format!(
            "{prefix}PHP {} is not supported — Blastit supports {SUPPORTED_PHP}",
            version.dotted()
        ),
        PhpRefusal::VersionConflict {
            path,
            stated,
            measured,
        } => {
            let mut candidates: Vec<String> = stated
                .iter()
                .map(|(version, spelling)| format!("{spelling} states PHP {}", version.dotted()))
                .collect();
            if let Some(version) = measured {
                candidates.push(format!(
                    "the `php_version` string the object carries states PHP {}",
                    version.dotted()
                ));
            }
            format!(
                "{prefix}{} states more than one PHP minor — {} — so no single shim can be \
                 chosen for it; give it one versioned name (libphp8.4.so, libphp-8.4.so, …), \
                 keep the distro's directory and file name consistent, or {PHP_OVERRIDE}; \
                 {PHP_PACKAGES}",
                path.display(),
                candidates.join(", ")
            )
        }
        PhpRefusal::NoVersion { path } => format!(
            "{prefix}{} is an NTS libphp that exports `php_version` in its `.dynsym`, but the \
             version that symbol returns could not be read from its sections (no readable \
             `return PHP_VERSION;` body — a symbol typed `STT_NOTYPE` or sized `0` is one), and \
             neither its file name nor its directory states a PHP major.minor, so no shim can \
             be chosen for it; give it a versioned name (libphp8.4.so, libphp-8.4.so, …), or \
             {PHP_OVERRIDE}; {PHP_PACKAGES}",
            path.display()
        ),
        PhpRefusal::NoShim {
            libphp,
            version,
            shim,
            searched,
        } => format!(
            "{prefix}no shim {shim} was found for the NTS libphp {} (looked in {}); \
             this host is (PHP {}, {}), and {shim} is what that tuple needs; {PHP_PACKAGES}, \
             or {PHP_OVERRIDE}",
            libphp.display(),
            display_paths(searched),
            version.dotted(),
            target_tuple(),
        ),
    }
}

/// The version-specific half of [`php_refusal_message`]: the problem, then the
/// tuple, the shim, the package and the override.
fn present_refusal(prefix: &str, problem: &str, version: Option<PhpVersion>) -> String {
    match abi_tuple(version) {
        Some((dotted, shim)) => format!(
            "{prefix}{problem}; this host is (PHP {dotted}, {}), whose embedded shim \
             is {shim}; {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            target_tuple(),
        ),
        None => format!(
            "{prefix}{problem}; this host is (PHP ?, {}), so no shim can be chosen \
             until the object's version is known; {PHP_PACKAGES}, or {PHP_OVERRIDE}",
            target_tuple(),
        ),
    }
}

/// The directories as one comma-separated list, for a refusal.
fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<String>>()
        .join(", ")
}

/// What a boot's `entrypoint php` gate says before a listener may bind.
#[derive(Debug)]
pub enum PhpBoot {
    /// No file declares `entrypoint php`: nothing to discover.
    Absent,
    /// The host can embed PHP: this is the NTS `libphp` and the shim every PHP
    /// route's worker will load, recorded as `(path, st_dev, st_ino)` so stage 1
    /// re-opens exactly these files (`ADR-0010` §3).
    Proceed(PhpRuntime),
    /// The host cannot embed PHP: the sentence names the route, the package,
    /// `BLASTIT_LIBPHP` and — when it is known — the `(PHP X.Y, <arch>-<libc>)`
    /// tuple.
    Refused(String),
}

impl PhpBoot {
    /// The sentence this gate refuses with, or `None` when it proceeds.
    ///
    /// Public because the composition root's own cases (and `check`'s) read the
    /// sentence as a value: a gate that answers "refused" without a sentence an
    /// operator can act on is a defect, and this is how they notice.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Absent | Self::Proceed(_) => None,
            Self::Refused(message) => Some(message),
        }
    }
}

/// What a **deployment** boot checks that `dev` and `check` do not: the uid that
/// must own every path, the directory the manifest was read from, and the two
/// provenance decisions (`ADR-0010` §3, design `R7`/`B2-R5`, §4.4 D5).
///
/// The decisions arrive as callbacks because `blastit::host`'s `Provenance` is
/// the composition root's type and must not cross into a view crate. **Two**
/// callbacks, deliberately: `is_protected_object` is the public whole object
/// decision (ownership, file capabilities **and** the zero-writable
/// `DT_RPATH`/`DT_RUNPATH` scan) and `is_protected_tree` is the ownership-only
/// tree claim. One callback would silently drop the search-path refusal, which
/// is the half a route uid can exploit by pointing the loader at its own
/// directory.
///
/// The object callback is handed the claim's **noun** (`the NTS libphp`, `the
/// embedded shim`) as well as its path, because only the callback can build the
/// whole sentence's subject and path together. Formatting `{noun}: {detail}`
/// here duplicated the noun (`the NTS libphp: the file (…) could be replaced`,
/// the sentence `MB.36.5` recorded): the noun now reaches `Provenance` as the
/// `Claim`'s name and [`deployment_refusal`] adds only the route prefix.
#[derive(Clone)]
pub struct Deployment<'a> {
    /// The uid init runs as: root, or init's own in `rootless` mode.
    pub init_uid: u32,
    /// The directory the `Blastfile` came from — every PHP worker's `cwd` and
    /// `include_path` (`ADR-0010` §2).
    pub manifest_dir: PathBuf,
    /// Authorises one object a worker loads: ownership, no file capabilities, no
    /// writable `DT_RPATH`/`DT_RUNPATH` entry. Takes the claim's noun and path.
    pub is_protected_object: &'a dyn Fn(&str, &Path) -> Result<(), String>,
    /// Authorises one tree a worker reads: ownership and permissions only.
    pub is_protected_tree: &'a dyn Fn(&Path) -> Result<(), String>,
}

/// The same gate for a caller with the search and — for a deployment — the
/// provenance to authorise spelled out.
///
/// There is no per-view budget here: `ADR-0012` §12.3 removed `workers`, so the
/// only cap is the deployment's, and it reaches the scheduler, not this gate.
pub fn php_boot_in(
    manifest: &Manifest,
    search: &PhpSearch,
    deployment: Option<&Deployment<'_>>,
) -> PhpBoot {
    let Some((route, entry, _workers)) = first_php_route(manifest) else {
        return PhpBoot::Absent;
    };
    match discover_php(search) {
        Ok(runtime) => {
            // The authorisation comes after discovery — it needs the *resolved*
            // paths — and before the record is handed to stage 1, so a
            // deployment whose pair or tree a route uid could replace never
            // reaches `bind_view_sockets` (`R7`, `B2-R5`).
            if let Some(deployment) = deployment
                && let Some(message) =
                    deployment_refusal(manifest, deployment, &runtime, route, entry)
            {
                return PhpBoot::Refused(message);
            }
            PhpBoot::Proceed(runtime)
        }
        Err(refusal) => PhpBoot::Refused(php_refusal_message(route, entry, &refusal)),
    }
}

/// `R7`/`B2-R5`: the sentence a deployment boot refuses this pair and tree with.
///
/// The **pair** is checked first and keeps `ADR-0010` §4.1's shape — the route,
/// its `entry` (**what the object is**), the offending path, the fix, the
/// `(PHP X.Y, <arch>-<libc>)` tuple, the distro package and `BLASTIT_LIBPHP` —
/// because every way an operator can fix a library is one of those names. The
/// **tree** is a different sentence: nothing about it is fixed by installing a
/// package, so it names what the path is (the directory holding the
/// `Blastfile`, an `entrypoint php`, a `static` root), the directory itself, and
/// the `chown`/`chmod` that fixes it.
///
/// The two decisions come from the caller ([`Deployment`]); this function only
/// formats, so the sentence stays in one place.
pub fn deployment_refusal(
    manifest: &Manifest,
    deployment: &Deployment<'_>,
    runtime: &PhpRuntime,
    route: &str,
    entry: &str,
) -> Option<String> {
    let prefix = format!("route {route:?}: entrypoint php {entry:?}: ");
    for (what, path) in [
        ("the NTS libphp", runtime.libphp.as_path()),
        ("the embedded shim", runtime.shim.as_path()),
    ] {
        // The callback owns the subject: `Provenance` names the claim it was
        // given (`the NTS libphp (…) could be replaced…`), so the sentence reads
        // once instead of `the NTS libphp: the file (…) could be replaced`.
        if let Err(detail) = (deployment.is_protected_object)(what, path) {
            return Some(present_refusal(&prefix, &detail, Some(runtime.version)));
        }
    }
    // One claim at a time, in this order: the first refusal wins, but a claim
    // whose path is not there gets the missing-path sentence (see
    // [`missing_claim`]).
    for (what, path) in php_trees(manifest, deployment) {
        if let Some(message) = missing_claim(&what, &path) {
            return Some(message);
        }
        if let Err(detail) = (deployment.is_protected_tree)(&path) {
            return Some(format!("{what}: {detail}"));
        }
    }
    None
}

/// `R13`: the sentence a manifest-named path that is **not there** refuses with.
///
/// Asked before the deployment's tree decision for the same claim because that
/// check's sentence for a missing path — "it cannot be inspected (No such file
/// or directory)" — reads as a permission problem and sends the operator to
/// `chmod` a file that does not exist. A path the manifest names is *missing* or
/// it is *unprotected*; this is the missing half, and a path that is there but
/// unreadable still goes to the provenance check, which is the one that can say
/// what is wrong with it.
///
/// The test is [`std::fs::metadata`], the same call the composition root's
/// `facts_of` makes, so "missing" here means exactly the `NotFound` that call
/// reports — which **includes a dangling symlink** (`metadata` follows the link,
/// and the target is what the boot would read). That is reported as missing on
/// purpose: it is the sentence an operator can act on, while "cannot be
/// inspected" would name the link rather than the file that is absent. Any other
/// errno — `EACCES`, a symlink loop — is *not* missing and stays with the
/// provenance sentence, which is the one that can say what is wrong with the
/// path.
fn missing_claim(what: &str, path: &Path) -> Option<String> {
    let missing = matches!(
        std::fs::metadata(path),
        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound
    );
    missing.then(|| {
        format!(
            "{what} ({}) is missing (no such file or directory): the manifest names a path this \
             boot cannot read, so a deployment would hand its workers nothing; create it, or \
             correct the path the manifest names",
            path.display()
        )
    })
}

/// The tree claims a deployment boot authorises: the directory the workers'
/// `cwd`/`include_path` is, every `entrypoint php`, and every
/// `[route.static] root` (`ADR-0010` §3, design `B2-R5`).
///
/// A worker can `include` any file those paths reach, so a route uid that can
/// write one of them chooses PHP code the worker will run — which is why the
/// rule is the same one `host::plain_is_protected` applies to the binary a child
/// executes, minus the file-capability half (a tree is read, not loaded).
///
/// `(what, path)`, not the root's `Claim`: a `blastit::host` type may not cross
/// into a view crate (design §4.4 D2), and a refusal only needs the two halves.
fn php_trees(manifest: &Manifest, deployment: &Deployment<'_>) -> Vec<(String, PathBuf)> {
    let mut claims = vec![(
        "the directory holding the Blastfile (the PHP worker's cwd and include_path)".to_string(),
        deployment.manifest_dir.clone(),
    )];
    // The file's one front controller, under the first view's name: every view
    // reaches it (§12.2), so the claim is one file, not one per view.
    if let (Some(php), Some(route)) = (manifest.php(), manifest.routes().first()) {
        claims.push((
            format!("route {:?}: entrypoint php {:?}", route.name(), php.entry),
            deployment.manifest_dir.join(&php.entry),
        ));
    }
    // A mount is the file's, not a route's (§9.1), so the claim names the
    // prefix it answers under.
    for mount in manifest.mounts() {
        claims.push((
            format!("static {:?}: root {:?}", mount.prefix, mount.policy.root),
            deployment.manifest_dir.join(&mount.policy.root),
        ));
    }
    claims
}

/// The file's one front controller: the first view's name, its `entry` and its
/// (always absent) `workers` cap.
///
/// A `Blastfile` declares `entrypoint php` once for the whole file (§12.2), so
/// every view reaches it; the gate names the first view and the entry, which is
/// the shape `ADR-0010` §4.1's sentence has. A file with no view has nothing to
/// route to, so the gate is absent. `workers` is `None` by construction: the
/// language has no `workers` keyword (`ADR-0012` §12.3), so the per-view budget
/// is the deployment's alone and no boot can be refused for one the file names.
fn first_php_route(manifest: &Manifest) -> Option<(&str, &str, Option<u32>)> {
    let php = manifest.php()?;
    let route = manifest.routes().first()?;
    Some((route.name(), php.entry.as_str(), php.workers))
}

/// Resolves the NTS `libphp` and its shim, or names why not (`ADR-0010` §3).
///
/// `BLASTIT_LIBPHP` is the only candidate when set — a boot never falls back to
/// a distro copy the operator did not name. Otherwise [`LIBPHP_PROBES`] is
/// walked version-major — every 8.5 spelling, then every 8.4 one, then 8.3 —
/// each in every directory of [`PhpSearch::lib_dirs`], so the *library's*
/// version wins over both its name and its directory (`MB.3.61`).
///
/// # Both records are the *resolved* path
///
/// Stage 0 may follow symlinks (`/usr/lib/libphp8.so -> libphp8.4.so` is a
/// normal distro layout), but stage 1 re-opens each object
/// `O_NOFOLLOW`-strictly so a link cannot be swapped in between the two
/// stages. Recording the path as given therefore makes a legitimate symlink
/// die in stage 1 with `ELOOP` — after the banner, with no §4.1 sentence. So
/// the record holds `canonicalize`'s answer for both objects: the two stages
/// open the same real file, a link in the distro keeps working, and a link
/// planted after stage 0 is still refused (`ENOENT`/`ELOOP` on the real name
/// is the inode record's job, not the link's).
pub fn discover_php(search: &PhpSearch) -> Result<PhpRuntime, PhpRefusal> {
    let path = match &search.libphp {
        Some(path) => {
            if !path.is_absolute() {
                return Err(PhpRefusal::NotAbsolute { path: path.clone() });
            }
            path.clone()
        }
        None => match first_existing(search) {
            Some(path) => path,
            None => {
                // No supported spelling was found. An installed *unsupported*
                // minor is a different failure from "no libphp at all", and the
                // fix is a different package — so say which one it is rather
                // than sending the operator to look for a library they have.
                if let Some(version) = installed_unsupported(search) {
                    return Err(PhpRefusal::UnsupportedVersion { version });
                }
                return Err(PhpRefusal::Missing {
                    searched: search.lib_dirs.clone(),
                });
            }
        },
    };
    let mut file = std::fs::File::open(&path).map_err(|err| PhpRefusal::NotAFile {
        path: path.clone(),
        detail: err.to_string(),
    })?;
    let metadata = file.metadata().map_err(|err| PhpRefusal::NotAFile {
        path: path.clone(),
        detail: err.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(PhpRefusal::NotAFile {
            path: path.clone(),
            detail: "not a regular file".to_string(),
        });
    }
    // The path stage 1 will re-open. A name that cannot be resolved is a §4.1
    // refusal here, never an `ELOOP` after the banner.
    let path = resolve(&path)?;
    // Three static statements, in the order `MB.3.59` fixes them: the file
    // name (the fast path), the path's per-minor directory, then the object's
    // own bytes. None of them loads anything.
    let named = version_of(&path, &file);
    let stated = path_versions(&path);
    let facts = inspect_object(&mut file).map_err(|detail| PhpRefusal::NotElf {
        path: path.clone(),
        detail,
        version: named.or_else(|| stated.first().map(|stated| stated.version)),
    })?;
    let hinted = named
        .or_else(|| stated.first().map(|stated| stated.version))
        .or(facts.version);
    match facts.abi {
        ZendAbi::Nts => {}
        ZendAbi::Zts => {
            return Err(PhpRefusal::ThreadSafe {
                path: path.clone(),
                version: hinted,
            });
        }
        ZendAbi::NotLibphp => {
            return Err(PhpRefusal::NotLibphp {
                path: path.clone(),
                version: hinted,
            });
        }
    }
    // A minor this build has no shim for is refused by name before the
    // `.dynsym` story. The object's own `php_version` body is the most
    // authoritative answer; the path's file name or per-minor directory is the
    // fallback for an object whose body the walk cannot read.
    let stated_version = facts
        .version
        .or(named)
        .or_else(|| stated.first().map(|stated| stated.version));
    if let Some(version) = stated_version
        && !version.supported()
    {
        return Err(PhpRefusal::UnsupportedVersion { version });
    }
    // The worker resolves `php_version` before it can start anything, so an
    // object whose `.dynsym` has no such **name** is refused here — whatever
    // its file name says (`MB.3.59`). Presence is the name alone: an odd object
    // that exports it as `STT_NOTYPE` or with no size is refused later, and for
    // the reason that is actually true (`MB.3.61`).
    if !facts.exports_php_version {
        return Err(PhpRefusal::NoPhpVersion {
            path: path.clone(),
            named,
        });
    }
    let version = choose_version(&path, &stated, facts.version)?;
    // One spelling, the `ADR-0010` §4.1 one: `make php-sapi` writes the same
    // `libblastit_php<minor>-<arch>-<libc>.so` that `Minor::shim_file()` names,
    // so a developer who has just built a shim is found here with no
    // `BLASTIT_PHP_SHIM_DIR` (`MB.3.35`). This lookup never accepts two
    // spellings for the same tuple.
    let shim = shim_name(version);
    let shim_path = search
        .shim_dirs
        .iter()
        .map(|dir| dir.join(&shim))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| PhpRefusal::NoShim {
            libphp: path.clone(),
            version,
            shim,
            searched: search.shim_dirs.clone(),
        })?;
    let shim_metadata = std::fs::metadata(&shim_path).map_err(|err| PhpRefusal::NotAFile {
        path: shim_path.clone(),
        detail: err.to_string(),
    })?;
    // The shim's own identity, recorded beside the library's: stage 1 re-opens
    // both and compares each against the record (`ADR-0010` §3), because a shim
    // is code that runs inside every PHP worker and a path is not an identity.
    let shim_path = resolve(&shim_path)?;
    Ok(PhpRuntime {
        libphp: path,
        dev: metadata.dev(),
        ino: metadata.ino(),
        version,
        shim: shim_path,
        shim_dev: shim_metadata.dev(),
        shim_ino: shim_metadata.ino(),
    })
}

/// The path a record stores: `canonicalize`'s answer, or a §4.1 refusal.
///
/// [`PhpRefusal::NotAFile`] carries the sentence an operator can act on — the
/// route, its `entry`, the path, the reason, the distro package and
/// `BLASTIT_LIBPHP` — which is the whole point: a link that cannot be resolved
/// must read as "this host cannot embed PHP", not as `errno` in a worker.
fn resolve(path: &Path) -> Result<PathBuf, PhpRefusal> {
    std::fs::canonicalize(path).map_err(|err| PhpRefusal::NotAFile {
        path: path.to_path_buf(),
        detail: format!("its path cannot be resolved ({err})"),
    })
}

/// The first probe of [`LIBPHP_PROBES`] that exists, in its own order.
///
/// Version-major, then spelling (`MB.3.61`): the *minor* is preferred, and the
/// name only breaks a tie within it, so a host carrying two embed packages
/// serves from the newer one instead of preferring whichever name came first.
fn first_existing(search: &PhpSearch) -> Option<PathBuf> {
    LIBPHP_PROBES.iter().find_map(|probe| {
        search
            .lib_dirs
            .iter()
            .filter(|dir| match probe.per_minor_dir {
                None => true,
                Some(minor) => dir.file_name() == Some(OsStr::new(minor)),
            })
            .map(|dir| dir.join(probe.name))
            .find(|path| path.is_file())
    })
}

/// The newest unsupported PHP minor installed in [`PhpSearch::lib_dirs`], when
/// no supported one was found.
///
/// This is a **refusal** scan, not a discovery one: it runs only after
/// [`first_existing`] found nothing, so it cannot change which library a host
/// with a supported package serves. Its one job is to turn "no usable NTS
/// `libphp` was found" into the true sentence for a host whose PHP is simply not
/// a minor this build ships — the version comes from the file name
/// (`libphp8.2.so`, `libphp82.so`) or, for an unversioned `libphp.so`, from the
/// per-minor directory it sits in (`/usr/lib/php82`).
fn installed_unsupported(search: &PhpSearch) -> Option<PhpVersion> {
    let mut found: Vec<PhpVersion> = Vec::new();
    for dir in &search.lib_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let directory_minor = dir
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(PhpVersion::parse_directory);
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let version = entry
                .file_name()
                .to_str()
                .and_then(PhpVersion::parse)
                .or(directory_minor);
            if let Some(version) = version
                && !version.supported()
            {
                found.push(version);
            }
        }
    }
    found.into_iter().max()
}

/// The version the file's name states, or the name its descriptor resolves to.
///
/// `libphp.so` is usually a symlink to `libphp8.4.so`, and a descriptor's link
/// target says so even when the path does not — a `readlink`, never a load.
/// Both spellings `PhpVersion::parse` knows are accepted, so Alpine's packed
/// `libphp83.so` is a name that states a version (`MB.3.59`).
fn version_of(path: &Path, file: &std::fs::File) -> Option<PhpVersion> {
    let named = path
        .file_name()
        .and_then(OsStr::to_str)
        .and_then(PhpVersion::parse);
    named.or_else(|| {
        let link = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
        link.file_name()
            .and_then(OsStr::to_str)
            .and_then(PhpVersion::parse)
    })
}

/// One minor the **path** states, and the spelling that stated it — for the
/// conflict sentence, so an operator sees which half to rename.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StatedVersion {
    /// The minor.
    version: PhpVersion,
    /// The file name or directory component, as written.
    spelling: String,
}

/// Every PHP minor the path itself states (`MB.3.59`), deduplicated.
///
/// Two spellings are the distro's: the file name (`libphp83.so`,
/// `libphp8.4.so`) and a per-minor parent directory (`/usr/lib/php84`,
/// `/usr/local/lib/php83`). A path with the same minor twice keeps one entry;
/// a path with two different minors still lists both, so [`choose_version`]
/// can refuse rather than pick one.
fn path_versions(path: &Path) -> Vec<StatedVersion> {
    let mut stated: Vec<StatedVersion> = Vec::new();
    let mut push = |version: PhpVersion, spelling: String| {
        if !stated.iter().any(|known| known.version == version) {
            stated.push(StatedVersion { version, spelling });
        }
    };
    if let Some(name) = path.file_name().and_then(OsStr::to_str)
        && let Some(version) = PhpVersion::parse(name)
    {
        push(version, format!("`{name}`"));
    }
    // The immediate parent only: `…/php84/libphp.so` is Alpine's own shape,
    // while a `php84` further up a developer's tree is not a statement about
    // this object.
    if let Some(component) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
        && let Some(version) = PhpVersion::parse_directory(component)
    {
        push(version, format!("the directory `{component}`"));
    }
    stated
}

/// The one minor the path and the object's own bytes agree on, or the refusal
/// that names every candidate (`MB.3.59`).
///
/// Two different statements are never resolved by a preference: the shim is
/// chosen by this number, and a wrong one is a boot that loads a foreign ABI.
/// So disagreement is a refusal naming each spelling, the `.dynsym` fact and
/// `BLASTIT_LIBPHP`; stating nothing at all is the same sentence with no
/// candidates.
fn choose_version(
    path: &Path,
    stated: &[StatedVersion],
    measured: Option<PhpVersion>,
) -> Result<PhpVersion, PhpRefusal> {
    let mut distinct: Vec<PhpVersion> = stated.iter().map(|stated| stated.version).collect();
    if let Some(measured) = measured
        && !distinct.contains(&measured)
    {
        distinct.push(measured);
    }
    if distinct.len() > 1 {
        return Err(PhpRefusal::VersionConflict {
            path: path.to_path_buf(),
            stated: stated
                .iter()
                .map(|stated| (stated.version, stated.spelling.clone()))
                .collect(),
            measured,
        });
    }
    distinct
        .first()
        .copied()
        .ok_or_else(|| PhpRefusal::NoVersion {
            path: path.to_path_buf(),
        })
}

/// The largest section this walk will read into memory: a `libphp`'s `.dynsym`
/// is tens of kilobytes, and this is the bound that keeps a hostile file from
/// being an allocation.
const MAX_SECTION_BYTES: u64 = 64 * 1024 * 1024;

/// `SHT_DYNSYM`: the dynamic symbol table.
///
/// Spelled once, in [`blastit_sys::elf`], and re-exported here because the
/// boot's fixtures build objects out of these names.
pub const SHT_DYNSYM: u32 = blastit_sys::elf::SHT_DYNSYM;
/// `SHT_STRTAB`: a string table.
pub const SHT_STRTAB: u32 = blastit_sys::elf::SHT_STRTAB;
/// `EM_X86_64`: the machine whose `lea`-based `php_version` body is read.
pub const EM_X86_64: u16 = 62;
/// `EM_AARCH64`: the machine whose `adrp`/`add` body is read.
pub const EM_AARCH64: u16 = 183;

/// What the bounded, load-free walk says about one object (`ADR-0010` §3,
/// `MB.3.59`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObjectFacts {
    /// Which Zend build the `.dynsym` names.
    abi: ZendAbi,
    /// Whether `php_version` is exported — the symbol the worker resolves before
    /// `php_module_startup`.
    ///
    /// Presence comes from the **name** in the `.dynsym` and nothing else: an
    /// object that exports it as `STT_NOTYPE`, or with `st_size == 0`, does
    /// export it, and refusing it as "exports no `php_version`" was a false
    /// sentence (`MB.3.61`). What the body says is [`ObjectFacts::version`].
    exports_php_version: bool,
    /// The compile-time version `php_version()` returns, read from the string
    /// its body points at; `None` when the body is not the shape this walk
    /// understands.
    version: Option<PhpVersion>,
}

/// Reads the `.dynsym` of `file` and — when the path stated no minor — the
/// compile-time version the object carries (`ADR-0010` §3, `MB.3.59`).
///
/// No `dlopen`, no `mmap`: [`blastit_sys::elf`] `seek`s/`read`s the header, the
/// section header table and one section at a time with every offset and length
/// bounds-checked, and this walk adds only what a Zend object means — the
/// `.dynsym`, its linked string table, the `php_version` body (bounded) and the
/// string it returns (bounded). A file that is not a little-endian ELF, has no
/// `.dynsym`, or is truncated is an `Err` naming what was wrong.
fn inspect_object(file: &mut std::fs::File) -> Result<ObjectFacts, String> {
    let mut elf = Elf::parse(file).map_err(|err| err.to_string())?;
    let (dynsym, strtab_index) = find_dynsym(&mut elf)?;
    let strtab = find_strtab(&mut elf, strtab_index)?;
    let symbols = elf
        .section_bytes(&dynsym, MAX_SECTION_BYTES)
        .map_err(|err| err.to_string())?;
    let names = elf
        .section_bytes(&strtab, MAX_SECTION_BYTES)
        .map_err(|err| err.to_string())?;
    let entsize = usize::try_from(dynsym.entsize)
        .map_err(|_| "the .dynsym entry size does not fit this build".to_string())?;
    let symbols = scan_symbols(elf.class(), &symbols, entsize, &names)?;
    let abi = match (symbols.executor_globals, symbols.tsrm_startup) {
        (true, false) => ZendAbi::Nts,
        (false, false) => ZendAbi::NotLibphp,
        (_, true) => ZendAbi::Zts,
    };
    let version = symbols
        .php_version_body
        .and_then(|symbol| php_version_string(&mut elf, symbol));
    Ok(ObjectFacts {
        abi,
        exports_php_version: symbols.exports_php_version,
        version,
    })
}

/// The `.dynsym` section and the index of its string table.
///
/// The walk is this module's — "which Zend symbols does this object export" —
/// while the header, the section table and every bounds check are
/// [`blastit_sys::elf`]'s.
fn find_dynsym(elf: &mut Elf<impl Read + Seek>) -> Result<(SectionHeader, usize), String> {
    for index in 0..elf.section_count() {
        let section = elf.section(index).map_err(|err| err.to_string())?;
        if section.kind != SHT_DYNSYM {
            continue;
        }
        let link = usize::try_from(section.link)
            .map_err(|_| "the .dynsym string-table index does not fit this build".to_string())?;
        return Ok((section, link));
    }
    Err("it has no .dynsym section".to_string())
}

/// The string table `index` names, checked to be one.
fn find_strtab(elf: &mut Elf<impl Read + Seek>, index: usize) -> Result<SectionHeader, String> {
    if index >= elf.section_count() {
        return Err("the .dynsym string-table index is past the section table".to_string());
    }
    let section = elf
        .section(index)
        .map_err(|_| "the .dynstr section header is truncated".to_string())?;
    if section.kind != SHT_STRTAB {
        return Err("the .dynsym string table is not a string table".to_string());
    }
    Ok(section)
}

/// The dynamic symbols the walk cares about (`ADR-0010` §3, `MB.3.59`).
struct DynSymbols {
    /// `executor_globals` is exported.
    executor_globals: bool,
    /// `tsrm_startup` is exported (a ZTS build).
    tsrm_startup: bool,
    /// The `.dynsym` exports the **name** `php_version`: the symbol the worker's
    /// loader resolves before `php_module_startup`. Presence is the name alone,
    /// whatever the symbol is typed or sized (`MB.3.61`).
    exports_php_version: bool,
    /// `php_version`'s `(st_value, st_size)` — its *body*, and a separate fact:
    /// set only for a symbol typed `STT_FUNC` whose fields the walk could read.
    php_version_body: Option<(u64, u64)>,
}

/// Which of the symbols above the dynamic table exports.
fn scan_symbols(
    class: Class,
    dynsym: &[u8],
    entsize: usize,
    strtab: &[u8],
) -> Result<DynSymbols, String> {
    let minimum = match class {
        Class::Elf64 => 24usize,
        Class::Elf32 => 16usize,
    };
    if entsize < minimum {
        return Err("a .dynsym entry is too small to be a symbol".to_string());
    }
    let mut symbols = DynSymbols {
        executor_globals: false,
        tsrm_startup: false,
        exports_php_version: false,
        php_version_body: None,
    };
    for index in 0..dynsym.len() / entsize {
        let at = index.saturating_mul(entsize);
        let Some(name) = le_u32(dynsym, at).and_then(|offset| {
            usize::try_from(offset)
                .ok()
                .map(|offset| c_str(strtab, offset))
        }) else {
            continue;
        };
        match name {
            b"executor_globals" => symbols.executor_globals = true,
            b"tsrm_startup" => symbols.tsrm_startup = true,
            b"php_version" => {
                // Presence is the name alone (`MB.3.61`): an object that exports
                // `php_version` as `STT_NOTYPE`, or with `st_size == 0`, still
                // exports it, and saying otherwise is a false sentence. The body
                // is a second fact, read only when it is the shape the version
                // decode understands.
                symbols.exports_php_version = true;
                if symbol_is_function(class, dynsym, at)
                    && let (Some(value), Some(size)) = (
                        symbol_value(class, dynsym, at),
                        symbol_size(class, dynsym, at),
                    )
                {
                    symbols.php_version_body = Some((value, size));
                }
            }
            _ => {}
        }
    }
    Ok(symbols)
}

/// `st_info`'s type nibble: `STT_FUNC`, the only kind the worker can call.
fn symbol_is_function(class: Class, dynsym: &[u8], at: usize) -> bool {
    let info = match class {
        Class::Elf64 => dynsym.get(at.saturating_add(4)).copied(),
        Class::Elf32 => dynsym.get(at.saturating_add(12)).copied(),
    };
    info.is_some_and(|info| info & 0x0f == 2)
}

/// `st_value`: where the symbol's code or data lives.
fn symbol_value(class: Class, dynsym: &[u8], at: usize) -> Option<u64> {
    match class {
        Class::Elf64 => le_u64(dynsym, at.checked_add(8)?),
        Class::Elf32 => le_u32(dynsym, at.checked_add(4)?).map(u64::from),
    }
}

/// `st_size`: how many bytes the symbol's body occupies.
fn symbol_size(class: Class, dynsym: &[u8], at: usize) -> Option<u64> {
    match class {
        Class::Elf64 => le_u64(dynsym, at.checked_add(16)?),
        Class::Elf32 => le_u32(dynsym, at.checked_add(8)?).map(u64::from),
    }
}

/// The compile-time version `php_version()` returns, read statically
/// (`MB.3.59`).
///
/// The function is `return PHP_VERSION;` in every PHP 8.3+ build: one
/// PC-relative `lea` (`x86_64`) or `adrp`/`add` pair (`aarch64`) whose target
/// is the string in the object's own read-only data — on Alpine it is the
/// `8.4.27` inside the `X-Powered-By: PHP/8.4.27` literal, which is why a
/// search for a bare version string would miss it. Decoding the body is
/// precise: it reads exactly the constant the loader will receive, and
/// anything this walk does not understand is `None` (a refusal), never a guess
/// at a neighbouring library's version.
fn php_version_string(
    elf: &mut Elf<impl Read + Seek>,
    (value, size): (u64, u64),
) -> Option<PhpVersion> {
    // 32 bytes is more than the longest body either decoder reads; a bigger
    // symbol is not a `return <constant>;` and is left to the refusal.
    let len = usize::try_from(size.min(32)).ok().filter(|len| *len > 0)?;
    let body_offset = vaddr_to_offset(elf, value)?;
    let body = elf.read(body_offset, len).ok()?;
    let string_vaddr = match elf.machine() {
        EM_X86_64 => decode_x86_64(&body, value)?,
        EM_AARCH64 => decode_aarch64(&body, value)?,
        _ => return None,
    };
    let string_offset = vaddr_to_offset(elf, string_vaddr)?;
    let available = usize::try_from(elf.file_len().checked_sub(string_offset)?).ok()?;
    let wanted = available.min(32);
    if wanted == 0 {
        return None;
    }
    let bytes = elf.read(string_offset, wanted).ok()?;
    let end = bytes.iter().position(|byte| *byte == 0)?;
    let text = std::str::from_utf8(bytes.get(..end)?).ok()?;
    version_prefix(text)
}

/// The file offset of `vaddr`, from the section that contains it.
///
/// A section's `sh_addr` is its load address and `sh_offset` its file offset;
/// the difference is the mapping, so no program header is needed.
fn vaddr_to_offset(elf: &mut Elf<impl Read + Seek>, vaddr: u64) -> Option<u64> {
    for index in 0..elf.section_count() {
        let section = elf.section(index).ok()?;
        // A body and the constant it returns live in mapped, read-only
        // sections; a pointer into `.data` or `.bss` is not the shape this
        // walk understands.
        if section.flags & SHF_ALLOC == 0 || section.flags & SHF_WRITE != 0 {
            continue;
        }
        let end = section.addr.checked_add(section.size)?;
        if vaddr >= section.addr && vaddr < end {
            return section.offset.checked_add(vaddr.checked_sub(section.addr)?);
        }
    }
    None
}

/// `"8.4.27"` -> PHP 8.4: the first two dot-separated components.
fn version_prefix(text: &str) -> Option<PhpVersion> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse::<u8>().ok()?;
    let minor = parts.next()?.parse::<u8>().ok()?;
    Some(PhpVersion::new(major, minor))
}

/// The target of `php_version`'s body on `x86_64`, when it is the
/// `lea reg,[rip+disp]` a `return <constant>;` compiles to.
pub fn decode_x86_64(body: &[u8], at: u64) -> Option<u64> {
    // `endbr64` is what `-fcf-protection` prefixes to the function.
    let cursor = if body.starts_with(&[0xf3, 0x0f, 0x1e, 0xfa]) {
        4
    } else {
        0
    };
    let rex = *body.get(cursor)?;
    if rex & 0xf8 != 0x48 {
        return None;
    }
    if *body.get(cursor.checked_add(1)?)? != 0x8d {
        return None;
    }
    let modrm = *body.get(cursor.checked_add(2)?)?;
    // mod = 00, rm = 101: `[rip + disp32]`.
    if modrm & 0xc7 != 0x05 {
        return None;
    }
    let at_disp = cursor.checked_add(3)?;
    let disp = i32::from_le_bytes(
        body.get(at_disp..at_disp.checked_add(4)?)?
            .try_into()
            .ok()?,
    );
    let next = at.checked_add(u64::try_from(at_disp.checked_add(4)?).ok()?)?;
    Some(next.wrapping_add_signed(i64::from(disp)))
}

/// The target of `php_version`'s body on `aarch64`: `adrp` + `add` with the
/// low 12 bits, the PIC sequence a `return <constant>;` compiles to.
pub fn decode_aarch64(body: &[u8], at: u64) -> Option<u64> {
    // `bti c` is what `-mbranch-protection` prefixes to the function.
    let mut cursor: usize = if u32::from_le_bytes(body.get(..4)?.try_into().ok()?) == 0xd503_245f {
        4
    } else {
        0
    };
    let adrp = u32::from_le_bytes(body.get(cursor..cursor.checked_add(4)?)?.try_into().ok()?);
    // ADRP: op=1, 10000 in bits 28..24, Rd, immhi, immlo.
    if adrp & 0x9f00_0000 != 0x9000_0000 {
        return None;
    }
    let rd = adrp & 0x1f;
    let immlo = i64::from((adrp >> 29) & 0x3);
    let immhi = i64::from((adrp >> 5) & 0x7_ffff);
    let pages = sign_extend((immhi << 2) | immlo, 21) << 12;
    let page = at.checked_add(u64::try_from(cursor).ok()?)? & !0xfff;
    let base = page.wrapping_add_signed(pages);
    cursor = cursor.checked_add(4)?;
    let add = u32::from_le_bytes(body.get(cursor..cursor.checked_add(4)?)?.try_into().ok()?);
    // ADD (immediate), 64-bit: sf=1, op=0, S=0, 100010, sh, imm12, Rn, Rd.
    if add & 0x7f80_0000 != 0x1100_0000 {
        return None;
    }
    if add & 0x1f != rd || (add >> 5) & 0x1f != rd {
        return None;
    }
    let shift = (add >> 22) & 1;
    let imm12 = u64::from((add >> 10) & 0xfff);
    base.checked_add(imm12.checked_shl(12 * shift)?)
}

/// `value` as a `bits`-wide two's-complement integer.
fn sign_extend(value: i64, bits: u32) -> i64 {
    let shift = 64u32.saturating_sub(bits);
    (value << shift) >> shift
}

/// The NUL-terminated string at `offset`, or an empty slice.
fn c_str(bytes: &[u8], offset: usize) -> &[u8] {
    let Some(rest) = bytes.get(offset..) else {
        return &[];
    };
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    rest.get(..end).unwrap_or_default()
}

/// A little-endian `u32` at `offset`, or `None` past the end.
fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

/// A little-endian `u64` at `offset`, or `None` past the end.
fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(slice))
}

/// The PHP version a host `libphp` is, `major.minor` (`ADR-0010` §3).
///
/// Stage 0 derives it statically from the object it opened — its file name, its
/// per-minor directory, or the compile-time version string the object carries —
/// because the alternative (calling `php_version()`) is a `dlopen`, and a
/// `dlopen` in stage 0 is exactly what `ADR-0010` §3 forbids. It is therefore
/// *what the file says it is*, and the `(major, minor)` is checked against the
/// shim's own `blastit_php_minor()` again in stage 1, where a wrong answer
/// becomes a refusal rather than a crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhpVersion {
    major: u8,
    minor: u8,
}

impl PhpVersion {
    /// The version `major.minor`, when both fit a `u8`.
    #[must_use]
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    /// The version a `libphp` file name states, in every spelling a distro
    /// ships: `libphp8.4.so`, `libphp-8.4.so`, a soname such as
    /// `libphp8.4.so.0`, and the **packed** name Alpine's 8.3 package installs
    /// — `libphp83.so` (`MB.3.59`; Alpine's 8.4 states the minor in its
    /// `/usr/lib/php84` directory instead).
    ///
    /// The packed form needs exactly two digits before the first dot, so
    /// `libphp8.so` (major only) and `libphp841.so` are still `None`. `None`
    /// for a name that carries no version (`libphp.so`): the shim is chosen by
    /// this number, and stating nothing is a refusal rather than a guess.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.rsplit('/').next()?;
        let rest = name.strip_prefix("libphp")?;
        let rest = rest.strip_prefix('-').unwrap_or(rest);
        let mut parts = rest.split('.');
        let first = parts.next()?;
        let second = parts.next();
        let major_is_digits = !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit());
        if let (Some(second), true) = (second, major_is_digits)
            && let Some(minor) = digits_prefix(second)
        {
            return Some(Self {
                major: first.parse().ok()?,
                minor,
            });
        }
        Self::packed(first)
    }

    /// The version `php84` (Alpine's per-minor directory) or the digits of a
    /// packed file name state: exactly two ASCII digits, `major` then `minor`.
    ///
    /// `None` for anything else — `php`, `php8`, `php844`, `20240924` — so a
    /// directory that is not per-minor never becomes a version.
    #[must_use]
    pub fn packed(digits: &str) -> Option<Self> {
        let bytes = digits.as_bytes();
        if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(Self {
            major: bytes.first()?.checked_sub(b'0')?,
            minor: bytes.get(1)?.checked_sub(b'0')?,
        })
    }

    /// The version a path component states when it is a per-minor directory:
    /// `php84` (Alpine's `/usr/lib/php84`), `php83`.
    #[must_use]
    pub fn parse_directory(component: &str) -> Option<Self> {
        Self::packed(component.strip_prefix("php")?)
    }

    /// The major version.
    #[must_use]
    pub const fn major(self) -> u8 {
        self.major
    }

    /// The minor version.
    #[must_use]
    pub const fn minor(self) -> u8 {
        self.minor
    }

    /// `"8.4"`, for messages, `(PHP 8.4, …)` tuples and package names.
    #[must_use]
    pub fn dotted(self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    /// The version `"8.4"` states, the form the stage-1 document carries.
    #[must_use]
    pub fn from_dotted(text: &str) -> Option<Self> {
        let (major, minor) = text.split_once('.')?;
        Some(Self {
            major: major.parse().ok()?,
            minor: minor.parse().ok()?,
        })
    }

    /// Whether this build ships a shim for this minor (`ADR-0010` §4.1).
    ///
    /// The supported set is `8.3`, `8.4` and `8.5` — the minors
    /// `crates/blastit-php-sapi` has an ABI for. A host on any other minor is
    /// refused with one sentence naming that set, never with the `.dynsym`
    /// story of a symbol it may or may not export.
    #[must_use]
    pub const fn supported(self) -> bool {
        self.major == 8 && self.minor >= 3 && self.minor <= 5
    }

    /// `"84"`, the `libblastit_php84-<arch>-<libc>.so` infix.
    #[must_use]
    pub fn number(self) -> String {
        format!("{}{}", self.major, self.minor)
    }
}

/// The leading decimal run of `text`, as a byte value (`"4.27"` -> `4`).
///
/// One component only: a dotted version's minor is the run before the next
/// dot, and a run that does not fit a `u8` is not a version this build states.
fn digits_prefix(text: &str) -> Option<u8> {
    let run = text.split(|c: char| !c.is_ascii_digit()).next()?;
    if run.is_empty() {
        return None;
    }
    run.parse().ok()
}

/// What stage 0 resolved for `entrypoint php` before a listener was bound
/// (`ADR-0010` §3).
///
/// Both objects — the NTS `libphp` and the shim for its tuple — carry their
/// identity (`dev`, `ino`) beside their path because stage 1 re-opens each
/// `O_RDONLY | O_CLOEXEC | O_NOFOLLOW` and compares the two: a path that
/// changed between the stages is a boot refusal, never a load of whatever is
/// there now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhpRuntime {
    /// The absolute path of the NTS `libphp` stage 0 opened and inspected.
    pub libphp: PathBuf,
    /// `st_dev` of that inode.
    pub dev: u64,
    /// `st_ino` of that inode.
    pub ino: u64,
    /// The version the resolved library is: from its file name, its per-minor
    /// directory, or its own compile-time version string (see [`PhpVersion`]).
    pub version: PhpVersion,
    /// The absolute path of the shim for `(version, target arch, target libc)`.
    pub shim: PathBuf,
    /// `st_dev` of the shim's inode.
    pub shim_dev: u64,
    /// `st_ino` of the shim's inode.
    pub shim_ino: u64,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// The distro spellings, one table: `PhpVersion` is what chooses the shim,
    /// so a name read wrongly is a boot that loads a foreign ABI.
    #[test]
    fn a_version_is_read_from_the_distro_spellings() {
        for (name, expected) in [
            ("libphp8.4.so", PhpVersion::new(8, 4)),
            ("libphp-8.4.so", PhpVersion::new(8, 4)),
            ("libphp8.3.so", PhpVersion::new(8, 3)),
            ("libphp8.4.so.0", PhpVersion::new(8, 4)),
            // Alpine's packed names and per-minor directories (`MB.3.59`).
            ("libphp83.so", PhpVersion::new(8, 3)),
            ("libphp84.so", PhpVersion::new(8, 4)),
            ("/usr/lib/libphp83.so", PhpVersion::new(8, 3)),
            (
                "/usr/lib/x86_64-linux-gnu/libphp8.5.so",
                PhpVersion::new(8, 5),
            ),
        ] {
            assert_eq!(PhpVersion::parse(name), Some(expected), "{name}");
        }
        for name in ["libphp.so", "libphp8.so", "php8.4.so", "libphp.so.8"] {
            assert_eq!(PhpVersion::parse(name), None, "{name}");
        }
        // A packed spelling is exactly two digits, never a run of them.
        assert_eq!(PhpVersion::parse("libphp841.so"), None);
        for (component, expected) in [
            ("php84", PhpVersion::new(8, 4)),
            ("php83", PhpVersion::new(8, 3)),
        ] {
            assert_eq!(
                PhpVersion::parse_directory(component),
                Some(expected),
                "{component}"
            );
        }
        for component in ["php", "php8", "php844", "php84x", "20240924"] {
            assert_eq!(PhpVersion::parse_directory(component), None, "{component}");
        }
        assert_eq!(PhpVersion::new(8, 4).dotted(), "8.4");
        assert_eq!(PhpVersion::new(8, 4).number(), "84");
        assert_eq!(PhpVersion::from_dotted("8.4"), Some(PhpVersion::new(8, 4)));
        assert_eq!(PhpVersion::from_dotted("8"), None);
    }
}
