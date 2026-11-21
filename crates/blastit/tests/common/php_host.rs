//! The host's NTS `libphp` and the shim for its tuple, resolved **without** the
//! code under test.
//!
//! A suite may not learn "this host can embed PHP" from the boot it is grading:
//! a discovery regression then reads exactly like a host with no PHP, and the
//! refusal branch grades a decision the boot never made. This probe therefore
//! walks `stage0.rs`'s candidate names with raw `std` file access, reads the
//! `.dynsym` strings as bytes (NTS = `executor_globals`, no `tsrm_startup`,
//! `php_version` present), takes the minor from the file name, and looks for the
//! shim in the three directories a `dev` boot searches.
//!
//! [`PROBES`] is a **copy** of the boot's discovery order. A copy can drift, so
//! `e2e_dev.rs` compares it with `blastit-view-php/src/discovery.rs` at compile
//! time (`the_php_probe_table_matches_the_boot`): a change to either side fails
//! instead of letting the probe walk a host the boot would walk differently.

use std::path::{Path, PathBuf};

/// The library and shim this host can embed.
pub struct HostPhp {
    /// The real file a boot is handed (canonicalised, so a symlinked distro
    /// package is named by its target).
    pub libphp: PathBuf,
    /// The `libblastit_php<minor>-<arch>-<libc>.so` shim for its tuple.
    pub shim: PathBuf,
}

/// The discovery order this probe walks: a file name, and the per-minor
/// directory it is looked for in (`None`: every directory below, in order).
///
/// Version-major, then spelling: the *minor* is preferred and the name only
/// breaks a tie inside it, so a host that ships two embed packages probes the
/// library the boot serves (the newest) rather than the first name in a list.
pub const PROBES: [(&str, Option<&str>); 17] = [
    ("libphp8.6.so", None),
    ("libphp-8.6.so", None),
    ("libphp86.so", None),
    ("libphp.so", Some("php86")),
    ("libphp8.5.so", None),
    ("libphp-8.5.so", None),
    ("libphp85.so", None),
    ("libphp.so", Some("php85")),
    ("libphp8.4.so", None),
    ("libphp-8.4.so", None),
    ("libphp84.so", None),
    ("libphp.so", Some("php84")),
    ("libphp8.3.so", None),
    ("libphp-8.3.so", None),
    ("libphp83.so", None),
    ("libphp.so", Some("php83")),
    ("libphp.so", None),
];

/// The library and shim this host can embed, or `None` when it ships neither.
pub fn host_php() -> Option<HostPhp> {
    let libphp = libphp_path()?;
    // A `.dynsym` name is a plain string in the file; reading the bytes beats
    // duplicating an ELF walk, and it is still independent of `blastit`.
    let bytes = std::fs::read(&libphp).ok()?;
    if !contains(&bytes, b"executor_globals") || contains(&bytes, b"tsrm_startup") {
        return None;
    }
    // An object that exports no `php_version` — the symbol the worker resolves
    // before `php_module_startup` — is refused by stage 0. The exact name needs
    // its NUL: `php_version_compare` exists in every version and must not count.
    if !contains(&bytes, b"php_version\0") {
        return None;
    }
    let (major, minor) = minor_of(&libphp)?;
    let shim = shim_path(major, minor)?;
    Some(HostPhp { libphp, shim })
}

/// Whether `haystack` holds `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `BLASTIT_LIBPHP` when it is a file, else [`PROBES`] walked over the library
/// directories — the resolved path, canonicalised.
fn libphp_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let triplet = format!("{}-linux-{libc}", std::env::consts::ARCH);
    let dirs = [
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
    ];
    for (name, minor) in PROBES {
        let found = dirs
            .iter()
            .filter(|dir| match minor {
                Some(minor) => {
                    Path::new(dir).file_name().and_then(|name| name.to_str()) == Some(minor)
                }
                None => true,
            })
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file());
        if let Some(path) = found {
            return path.canonicalize().ok();
        }
    }
    None
}

/// The `(major, minor)` the path states: a dotted name (`libphp8.4.so`,
/// `libphp-8.4.so`), a packed name (`libphp83.so`), or a per-minor directory
/// (`/usr/lib/php84`).
fn minor_of(path: &Path) -> Option<(u32, u32)> {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(dotted) = dotted_of(name)
    {
        return Some(dotted);
    }
    let packed = |component: &str| -> Option<(u32, u32)> {
        let rest = component
            .strip_prefix("libphp")
            .or_else(|| component.strip_prefix("php"))?;
        let stem = rest.split('.').next()?;
        let bytes = stem.as_bytes();
        if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some((
            u32::from(bytes.first()? - b'0'),
            u32::from(bytes.get(1)? - b'0'),
        ))
    };
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(packed) = packed(name)
    {
        return Some(packed);
    }
    path.ancestors()
        .skip(1)
        .filter_map(Path::file_name)
        .filter_map(|component| component.to_str())
        .find_map(packed)
}

/// The dotted spelling: `libphp8.4.so`, `libphp-8.4.so`, `libphp8.4.so.0`.
fn dotted_of(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("libphp")?;
    let mut rest = rest.strip_prefix('-').unwrap_or(rest);
    let major = digits(&mut rest)?;
    rest = rest.strip_prefix('.')?;
    let minor = digits(&mut rest)?;
    Some((major, minor))
}

/// The leading decimal run, consumed from `rest`.
fn digits(rest: &mut &str) -> Option<u32> {
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let value = rest.get(..len)?.parse().ok()?;
    *rest = rest.get(len..)?;
    Some(value)
}

/// `libblastit_php<minor>-<arch>-<libc>.so` in the directories a `dev` boot
/// looks in: `BLASTIT_PHP_SHIM_DIR`, the executable's directory, then the
/// `target/php-sapi` that `make php-sapi` writes.
fn shim_path(major: u32, minor: u32) -> Option<PathBuf> {
    let name = shim_name(major * 10 + minor);
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_blastit"));
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BLASTIT_PHP_SHIM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = exe.parent() {
        dirs.push(dir.to_path_buf());
    }
    if let Some(target) = exe
        .ancestors()
        .find(|dir| dir.file_name() == Some(std::ffi::OsStr::new("target")))
    {
        dirs.push(target.join("php-sapi"));
    }
    dirs.into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

/// The shim's file name for a packed minor (`84`), spelled by the target tuple.
fn shim_name(minor: u32) -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    };
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    format!("libblastit_php{minor}-{arch}-{libc}.so")
}
