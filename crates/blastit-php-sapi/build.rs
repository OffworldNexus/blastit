//! Discovery of the host's PHP development toolchain, at build time only.
//!
//! A machine without PHP must still build this crate: the shim is a C artefact
//! built by `make php-sapi`, and the Rust side is only ABI declarations plus
//! the security types. So this script **never** fails the build. When it finds
//! a usable `php-config` it sets the `php_toolchain` cfg (and the
//! `BLASTIT_PHP_CONFIG` / `BLASTIT_PHP_VERSION` env vars); when it does not, it
//! prints the one warning that says how to get one, and the crate compiles
//! without the ABI surface. `tests/critical_shim_contract.rs` asserts both
//! worlds, so the degrade is tested rather than assumed.
//!
//! Search order (the plan's "Toolchain reality" section): `PHP_CONFIG`, then
//! `php-config`, `php-config8.4`, `php-config8.3`, `php-config8.2` on `PATH`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The unversioned name first, then one per supported minor, newest first.
const CANDIDATES: [&str; 4] = [
    "php-config",
    "php-config8.4",
    "php-config8.3",
    "php-config8.2",
];

/// The minors this crate has an ABI for, newest first.
const SUPPORTED: [&str; 3] = ["8.4", "8.3", "8.2"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=c/blastit_php_sapi.c");
    println!("cargo:rerun-if-changed=c/include/blastit_php_abi_v1.h");
    println!("cargo:rerun-if-env-changed=PHP_CONFIG");
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rustc-check-cfg=cfg(php_toolchain)");

    match find_toolchain() {
        Some((path, version)) => {
            println!("cargo:rustc-cfg=php_toolchain");
            println!("cargo:rustc-env=BLASTIT_PHP_CONFIG={}", path.display());
            println!("cargo:rustc-env=BLASTIT_PHP_VERSION={version}");
        }
        None => {
            println!(
                "cargo:warning=no PHP development toolchain found (looked for PHP_CONFIG, then \
                 php-config, php-config8.4, php-config8.3, php-config8.2 on PATH): the C SAPI shim \
                 is not built and the ABI surface is not compiled. Install it with `apt install \
                 php8.4-dev libphp8.4-embed` (Debian/Ubuntu), `apk add php84-dev php84-embed` \
                 (Alpine) or `dnf install php84-php-devel php-embedded` (Fedora/RHEL), or set \
                 PHP_CONFIG=/path/to/php-config8.4. `make php-sapi-e2e` builds and exercises the \
                 shim in its container."
            );
        }
    }
}

/// The first working `php-config` and the PHP version it reports.
fn find_toolchain() -> Option<(PathBuf, String)> {
    if let Some(explicit) = env::var_os("PHP_CONFIG") {
        let path = PathBuf::from(explicit);
        if let Some(version) = probe(&path) {
            return Some((path, version));
        }
        // An explicit PHP_CONFIG that does not work is a typo, not a reason to
        // fall back silently to whatever else is on PATH: say so, then look.
        println!(
            "cargo:warning=PHP_CONFIG={} does not report a supported PHP version; falling back to PATH",
            path.display()
        );
    }

    let path = env::var_os("PATH")?;
    for candidate in CANDIDATES {
        for dir in env::split_paths(&path) {
            let exe = dir.join(candidate);
            if let Some(version) = probe(&exe) {
                return Some((exe, version));
            }
        }
    }

    None
}

/// The version `php-config` reports, when it names a supported minor.
fn probe(exe: &Path) -> Option<String> {
    if !exe.is_file() {
        return None;
    }
    let output = Command::new(exe).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    let version = version.trim().to_owned();
    let minor = version.split('.').take(2).collect::<Vec<_>>().join(".");
    SUPPORTED.contains(&minor.as_str()).then_some(version)
}
