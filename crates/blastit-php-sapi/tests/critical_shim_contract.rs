//! Protected critical test: the `php2shell` surface is a type error, not a filter.
//!
//! Every assertion has both halves: the allowed entry and the refused one.
//! `PHP_VALUE`, `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `HTTP_PROXY` and the
//! `Proxy:` header exist in the FastCGI/CGI world (CVE-2012-1823,
//! CVE-2024-4577/8926/8927, httpoxy) and must be *unrepresentable* here — the C
//! shim re-checks them too, which `make php-sapi-e2e` proves against real PHP.
//!
//! This file also pins the honest degradation: on a host with no PHP toolchain
//! the crate compiles, the ABI surface does not, and the refusal names
//! `php-config` and the package to install.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use blastit_php_sapi::toolchain::Env;
use blastit_php_sapi::{
    DISABLED_FUNCTIONS, DropReason, Egress, HeaderOutcome, IniError, IniPolicy, Minor, PINNED_INI,
    ReservedVar, ServerVar, ServerVarError, ServerVars, Toolchain, ToolchainError,
};

const C_SHIM: &str = include_str!("../c/blastit_php_sapi.c");
const C_HEADER: &str = include_str!("../c/include/blastit_php_abi_v1.h");

#[test]
fn reserved_server_names_are_unrepresentable() {
    for (raw, reserved) in [
        ("PHP_VALUE", ReservedVar::PhpValue),
        ("php_admin_value", ReservedVar::PhpAdminValue),
        ("REDIRECT_STATUS", ReservedVar::RedirectStatus),
        ("http_proxy", ReservedVar::HttpProxy),
    ] {
        assert_eq!(
            ServerVar::new(raw, "1"),
            Err(ServerVarError::Reserved(reserved)),
            "{raw} must not be constructible"
        );
        let mut vars = ServerVars::new();
        assert_eq!(
            vars.insert(raw, "1"),
            Err(ServerVarError::Reserved(reserved))
        );
        assert!(vars.is_empty());
    }

    // The allowed case, so the refusals above are not "everything fails".
    let var = ServerVar::new("HTTP_X_FORWARDED_FOR", "203.0.113.7").unwrap();
    assert_eq!(var.name(), "HTTP_X_FORWARDED_FOR");
    assert_eq!(var.value(), "203.0.113.7");
}

#[test]
fn a_proxy_header_never_becomes_http_proxy() {
    let mut vars = ServerVars::new();
    assert_eq!(
        vars.push_header("Proxy", "http://attacker.example:8080"),
        HeaderOutcome::Dropped(DropReason::Httpoxy)
    );
    assert_eq!(
        vars.push_header("proxy", "http://attacker.example:8080"),
        HeaderOutcome::Dropped(DropReason::Httpoxy)
    );
    assert!(vars.is_empty(), "httpoxy: no entry was registered");

    // A header that does not derive a reserved name registers normally: the
    // reserved gate is the exact four names, and no header can derive one
    // (`HTTP_` + anything is never `HTTP_PROXY`).
    assert_eq!(
        vars.push_header("PHP-Value", "allow_url_include=1"),
        HeaderOutcome::Registered
    );
    assert_eq!(vars.as_slice()[0].name(), "HTTP_PHP_VALUE");
    // And an ordinary header does register, deriving the CGI name exactly.
    assert_eq!(
        vars.push_header("X-Forwarded-For", "203.0.113.7"),
        HeaderOutcome::Registered
    );
    assert_eq!(vars.len(), 2);
    assert_eq!(vars.as_slice()[1].name(), "HTTP_X_FORWARDED_FOR");

    // Field names are tokens: a space or a colon is refused before mapping.
    for bad in ["Bad Header", "X:Y", "", "X\r\nY"] {
        assert_eq!(
            vars.push_header(bad, "1"),
            HeaderOutcome::Dropped(DropReason::InvalidName),
            "{bad:?} is not a field name"
        );
    }
}

#[test]
fn server_var_names_values_and_budget_are_bounded() {
    assert_eq!(
        ServerVar::new("lower", "1"),
        Err(ServerVarError::InvalidName)
    );
    assert_eq!(ServerVar::new("A-B", "1"), Err(ServerVarError::InvalidName));
    assert_eq!(
        ServerVar::new("OK", "line\nInjected: 1"),
        Err(ServerVarError::InvalidValue)
    );
    assert_eq!(
        ServerVar::new("OK", "nul\0byte"),
        Err(ServerVarError::InvalidValue)
    );
    let long_name = "A".repeat(128);
    assert_eq!(
        ServerVar::new(&long_name, "1"),
        Err(ServerVarError::InvalidName)
    );

    let mut vars = ServerVars::new();
    vars.insert("SCRIPT_FILENAME", "/srv/app/public/index.php")
        .unwrap();
    assert_eq!(
        vars.insert("SCRIPT_FILENAME", "/etc/passwd"),
        Err(ServerVarError::Duplicate)
    );
    for index in 0..(ServerVars::MAX - 1) {
        vars.insert(&format!("V{index}"), "x").unwrap();
    }
    assert_eq!(vars.len(), ServerVars::MAX);
    assert_eq!(vars.insert("OVERFLOW", "x"), Err(ServerVarError::TooMany));
}

#[test]
fn ini_lockdown_pins_every_system_directive() {
    let policy = IniPolicy::new(
        Path::new("/srv/app"),
        Path::new("/run/blastit/view"),
        30,
        Egress::Denied,
    )
    .unwrap();
    let rendered = policy.as_str();
    for (key, value) in PINNED_INI {
        assert!(
            rendered.contains(&format!("{key}={value}\n")),
            "{key} is missing from the lockdown"
        );
    }
    assert!(rendered.contains("open_basedir=/srv/app:/run/blastit/view\n"));
    assert!(rendered.contains("session.save_path=/run/blastit/view\n"));
    assert!(rendered.contains("upload_tmp_dir=/run/blastit/view\n"));
    assert!(rendered.contains("sys_temp_dir=/run/blastit/view\n"));
    assert!(rendered.contains("max_execution_time=30\n"));
    assert_eq!(policy.entry_count(), PINNED_INI.len() + 6);
    // `disable_functions` is ADR-0010 §2 **verbatim**: every name, in order, so
    // a `pcntl_*`/`posix_*` omission (which is how a script escapes the view's
    // process group) is a test failure, not a silent narrowing.
    let disabled = rendered
        .lines()
        .find_map(|line| line.strip_prefix("disable_functions="))
        .expect("disable_functions is pinned");
    assert_eq!(disabled, DISABLED_FUNCTIONS);
    for function in [
        "exec",
        "shell_exec",
        "proc_open",
        "pcntl_exec",
        "pcntl_fork",
        "pcntl_forkx",
        "pcntl_signal",
        "posix_kill",
        "posix_getpgid",
        "posix_setsid",
        "posix_setpgid",
        "dl",
    ] {
        assert!(
            disabled.split(',').any(|name| name == function),
            "{function} is not disabled"
        );
    }
    assert_eq!(disabled.split(',').count(), 16);
}

#[test]
fn allow_url_fopen_follows_the_declared_egress_both_ways() {
    // ADR-0010 §2 pins `allow_url_fopen=0` when the route has no egress, and it
    // is the INI half only: `curl`, `sockets` and PDO open their own sockets
    // regardless. The seccomp filter is the wall; this must still be shut.
    let denied = IniPolicy::new(
        Path::new("/srv/app"),
        Path::new("/run/view"),
        30,
        Egress::Denied,
    )
    .unwrap();
    assert!(denied.as_str().contains("allow_url_fopen=0\n"));
    assert!(!denied.as_str().contains("allow_url_fopen=1\n"));

    // And the other half is explicit, so the effective value never depends on
    // the host's `php.ini` (`On` in Debian's default).
    let allowed = IniPolicy::new(
        Path::new("/srv/app"),
        Path::new("/run/view"),
        30,
        Egress::Allowed,
    )
    .unwrap();
    assert!(allowed.as_str().contains("allow_url_fopen=1\n"));
    assert!(!allowed.as_str().contains("allow_url_fopen=0\n"));

    // Everything else is identical: egress changes one directive, not the wall.
    let strip = |policy: &IniPolicy| {
        policy
            .as_str()
            .lines()
            .filter(|line| !line.starts_with("allow_url_fopen="))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&denied), strip(&allowed));
}

#[test]
fn ini_policy_refuses_to_render_an_injection() {
    let app = Path::new("/srv/app");
    assert_eq!(
        IniPolicy::new(
            Path::new("srv/app"),
            Path::new("/run/view"),
            30,
            Egress::Denied
        ),
        Err(IniError::Relative {
            key: "open_basedir"
        })
    );
    // A newline in a value is how a FastCGI `PHP_VALUE` would inject a second
    // directive; here it refuses the boot instead of rendering it.
    assert_eq!(
        IniPolicy::new(
            Path::new("/srv/app\nallow_url_include=1"),
            Path::new("/run/view"),
            30,
            Egress::Denied
        ),
        Err(IniError::ControlByte {
            key: "open_basedir"
        })
    );
    assert_eq!(
        IniPolicy::new(app, Path::new("/run/view"), 0, Egress::Denied),
        Err(IniError::Timeout)
    );
    // A `:` in a directory would render two `open_basedir` entries — one often
    // empty, i.e. "anywhere" — and every request would 500 on the warning that
    // `display_errors=0` hides. Refuse the boot, never widen the box.
    assert_eq!(
        IniPolicy::new(app, Path::new("/run/view:x"), 30, Egress::Denied),
        Err(IniError::Separator {
            key: "open_basedir"
        })
    );
    assert_eq!(
        IniPolicy::new(
            Path::new("/srv/app:/etc"),
            Path::new("/run/view"),
            30,
            Egress::Denied
        ),
        Err(IniError::Separator {
            key: "open_basedir"
        })
    );
    assert!(IniPolicy::new(app, Path::new("/run/view"), 30, Egress::Denied).is_ok());
}

// The shim-minor *refusal* is not tested here. `blastit_php_minor()` is only
// comparable against a real `libphp`'s `php_version()`, and that comparison is
// the worker's boot gate (`MB.3.13`: refuse a shim whose minor does not match
// the resolved libphp before the first request). This crate supplies
// `Minor::accepts`/`code` for that gate, and the probe already asserts that the
// shim it loaded reports a minor this build supports — the real half. A test
// that only compared the predicate with itself would prove nothing.

#[test]
fn php_versions_parse_only_where_a_shim_exists() {
    assert_eq!(Minor::parse("8.4.27"), Some(Minor::Php84));
    assert_eq!(Minor::parse("8.3.11\n"), Some(Minor::Php83));
    assert_eq!(Minor::parse("8.2"), Some(Minor::Php82));
    for refused in ["8.5.0", "7.4.33", "9.0.0", "", "php", "8", "8.x.1"] {
        assert_eq!(Minor::parse(refused), None, "{refused:?} must be refused");
    }
}

/// A fake path: no environment but the one the test supplies.
struct FakeEnv {
    path: Option<String>,
    php_config: Option<String>,
}

impl Env for FakeEnv {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "PATH" => self.path.clone(),
            "PHP_CONFIG" => self.php_config.clone(),
            _ => None,
        }
    }
}

/// Makes every [`FakeConfig`] directory unique inside this process, even when
/// two tests construct one in the same nanosecond.
static NEXT_FAKE_CONFIG: AtomicU64 = AtomicU64::new(0);

/// A temporary directory holding one fake `php-config`, removed on drop.
struct FakeConfig {
    dir: PathBuf,
    script: PathBuf,
}

impl FakeConfig {
    fn new(version: &str, include_dir: bool) -> Self {
        // `pid` separates test processes, the counter separates tests (and
        // threads) in this one, and the timestamp keeps a directory from a
        // crashed run with a recycled pid from being reused with stale
        // contents. Two tests once shared a pid+nanos name, and one `Drop`
        // deleted the other's script mid-run.
        let unique = format!(
            "blastit-php-sapi-{}-{}-{}",
            std::process::id(),
            NEXT_FAKE_CONFIG.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if include_dir {
            std::fs::create_dir_all(dir.join("include")).unwrap();
        }
        let config = Self {
            script: dir.join("php-config8.4"),
            dir,
        };
        config.write_script(version);
        config
    }

    /// (Re)writes the fake `php-config` body; `--version` prints `version`.
    ///
    /// A test that needs an executable file which is *not* a program
    /// overwrites this with plain text; the `0o755` mode is kept by
    /// `std::fs::write` truncating in place.
    fn write_script(&self, version: &str) {
        let body = format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) echo {version} ;;\n  --include-dir) echo {} ;;\n  *) exit 1 ;;\nesac\n",
            self.dir.join("include").display()
        );
        std::fs::write(&self.script, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&self.script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn path(&self) -> String {
        self.dir.display().to_string()
    }

    fn script(&self) -> &Path {
        &self.script
    }
}

impl Drop for FakeConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_missing_toolchain_names_php_config_and_the_package() {
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(String::new()),
        php_config: None,
    })
    .unwrap_err();
    assert_eq!(error, ToolchainError::NotFound);
    let message = error.to_string();
    for needle in ["php-config", "libphp8.4-embed", "PHP_CONFIG"] {
        assert!(
            message.contains(needle),
            "the refusal must name {needle}: {message}"
        );
    }

    // A PHP_CONFIG that does not exist is a typo, not a silent fallback.
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(String::new()),
        php_config: Some("/nonexistent/php-config".to_owned()),
    })
    .unwrap_err();
    assert!(matches!(error, ToolchainError::NotFound));
}

#[test]
fn an_unsupported_minor_is_refused_by_name() {
    let fake = FakeConfig::new("8.5.0", true);
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .unwrap_err();
    match error {
        ToolchainError::UnsupportedVersion { version, .. } => assert_eq!(version, "8.5.0"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn a_spawn_failure_is_not_reported_as_not_found() {
    // The real race behind this file's flake: the script exists and is
    // executable, but the kernel refuses to start it (`ETXTBSY` while a write
    // handle is open — the fd table is process-wide, so even another test's
    // just-forked child inherits the handle `FakeConfig::new` opens between
    // `write` and its first exec — or `EAGAIN` when `fork` hits the process
    // limit). "Could not ask" must never read as "no toolchain installed".
    let fake = FakeConfig::new("8.4.27", true);
    let script = fake.script().to_path_buf();
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&script)
        .expect("hold the fake php-config open for writing");
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .unwrap_err();
    assert!(
        matches!(&error, ToolchainError::Spawn { path, .. } if path == &script),
        "a spawn failure must be its own error, got {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(&script.display().to_string()),
        "the refusal must name the candidate: {message}"
    );
    assert!(
        message.contains("PHP_CONFIG"),
        "the refusal must name the override: {message}"
    );
    assert!(
        !message.contains("no php-config found"),
        "a spawn failure is not a missing toolchain: {message}"
    );

    // The typed refusal is not a permanent lie: release the writer and the
    // same path is a usable toolchain (the allowed half).
    drop(writer);
    let toolchain = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .expect("the same php-config is usable once the writer is gone");
    assert_eq!(toolchain.minor(), Minor::Php84);
    assert_eq!(toolchain.php_config(), script.as_path());
}

#[test]
fn a_text_file_named_php_config_is_not_a_spawn_failure() {
    // A file that exists and is executable but is not a program — no ELF
    // header, no `#!` — fails `exec` with `ENOEXEC`. That is a fact about the
    // file, not a busy host: it stays `NotExecutable` (skipped like any other
    // PATH entry), never `Spawn` with advice to retry.
    let fake = FakeConfig::new("8.4.27", true);
    std::fs::write(fake.script(), "not a program\n").unwrap();
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .unwrap_err();
    assert_eq!(
        error,
        ToolchainError::NotFound,
        "a text file is a bad candidate, not an unanswered one"
    );
    assert!(
        error.to_string().contains("no php-config found"),
        "a text file must not produce the spawn sentence: {error}"
    );

    // The allowed half: the same path is a usable toolchain once it is a
    // `php-config` program again.
    fake.write_script("8.4.27");
    let toolchain = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .expect("the same path is a usable toolchain once it is a php-config script");
    assert_eq!(toolchain.minor(), Minor::Php84);
}

#[test]
fn a_fake_toolchain_is_discovered_on_path() {
    let fake = FakeConfig::new("8.4.27", true);
    let toolchain = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: None,
    })
    .expect("a php8.4 toolchain on PATH is usable");
    assert_eq!(toolchain.minor(), Minor::Php84);
    assert_eq!(toolchain.version(), "8.4.27");
    // ADR-0010 §4.1: the shipped name is
    // `libblastit_php<major><minor>-<arch>-<libc>.so`, so the literal is per
    // supported `(arch, libc)` tuple: swapping either one in `shim_file()`
    // cannot pass.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        #[cfg(all(target_arch = "x86_64", not(target_env = "musl")))]
        const SHIM: &str = "libblastit_php84-x86_64-gnu.so";
        #[cfg(all(target_arch = "x86_64", target_env = "musl"))]
        const SHIM: &str = "libblastit_php84-x86_64-musl.so";
        #[cfg(all(target_arch = "aarch64", not(target_env = "musl")))]
        const SHIM: &str = "libblastit_php84-aarch64-gnu.so";
        #[cfg(all(target_arch = "aarch64", target_env = "musl"))]
        const SHIM: &str = "libblastit_php84-aarch64-musl.so";
        assert_eq!(toolchain.shim_file(), SHIM);
    }
    assert!(toolchain.include_dir().is_dir());
    assert!(toolchain.php_config().ends_with("php-config8.4"));

    // PHP_CONFIG wins over PATH, and a toolchain without headers is refused.
    let other = FakeConfig::new("8.3.11", true);
    let toolchain = Toolchain::discover_with(&FakeEnv {
        path: Some(fake.path()),
        php_config: Some(other.dir.join("php-config8.4").display().to_string()),
    })
    .unwrap();
    assert_eq!(toolchain.minor(), Minor::Php83);

    let headerless = FakeConfig::new("8.4.27", false);
    let error = Toolchain::discover_with(&FakeEnv {
        path: Some(headerless.path()),
        php_config: None,
    })
    .unwrap_err();
    assert!(matches!(error, ToolchainError::IncludeDir { .. }));
}

#[test]
fn the_c_shim_is_self_contained_and_guarded() {
    // Linkage: no libphp at link time, no host path baked in.
    for forbidden in ["-lphp", "-lphp8", "/usr/lib", "/usr/include", "/etc/php"] {
        assert!(
            !C_SHIM.contains(forbidden),
            "the shim must not contain {forbidden:?}"
        );
    }
    // The bailout guard is the reason this file exists.
    for required in ["zend_first_try", "zend_catch", "zend_end_try"] {
        assert!(C_SHIM.contains(required), "the shim is missing {required}");
    }
    // The C side re-checks the reserved names, independently of the Rust types.
    for reserved in [
        "PHP_VALUE",
        "PHP_ADMIN_VALUE",
        "REDIRECT_STATUS",
        "HTTP_PROXY",
    ] {
        assert!(
            C_SHIM.contains(reserved),
            "the shim does not re-check {reserved}"
        );
    }
    // The INI lockdown is startup-stage, not request-derived.
    assert!(C_SHIM.contains("php_ini_ignore_cwd = 1"));
    assert!(C_SHIM.contains("ini_entries"));

    // `sapi_startup()` nulls `ini_entries` (`main/SAPI.c`), so the lockdown must
    // be assigned after it, on the struct `php_module_startup` copies.
    let startup = C_SHIM
        .find("sapi_startup(&bt_sapi)")
        .expect("the shim calls sapi_startup");
    let lockdown = C_SHIM
        .find("bt_sapi.ini_entries = cfg->ini")
        .expect("the shim assigns the lockdown");
    assert!(
        startup < lockdown,
        "the INI lockdown is assigned before sapi_startup, which nuls it"
    );

    // `cfg->ini` is the ABI's only lockdown channel: a NULL must be refused
    // **before** `sapi_startup`, never defaulted to the host's php.ini (the
    // attempt-1 failure in a different costume). The probe also calls
    // `worker_init` with a NULL ini and requires `-1`; this pins the order.
    let null_check = C_SHIM
        .find("cfg->ini == NULL")
        .expect("the shim refuses a NULL ini");
    assert!(
        null_check < startup,
        "a NULL ini config must be refused before sapi_startup"
    );

    // The ABI the header declares is the one this crate speaks.
    assert!(C_HEADER.contains("#define BLASTIT_PHP_ABI_VERSION 1u"));
    assert!(C_HEADER.contains("#define BLASTIT_PHP_ABI_MAJOR 8"));
    for entry in [
        "blastit_php_abi_version",
        "blastit_php_minor",
        "blastit_php_worker_init",
        "blastit_php_worker_shutdown",
        "blastit_php_request",
    ] {
        assert!(C_HEADER.contains(entry), "the header is missing {entry}");
    }
}

#[test]
fn the_build_reports_one_consistent_world() {
    match (
        blastit_php_sapi::toolchain_present(),
        blastit_php_sapi::build_toolchain_version(),
        blastit_php_sapi::build_php_config(),
    ) {
        (true, Some(version), Some(config)) => {
            assert!(
                Minor::parse(version).is_some(),
                "cfg(php_toolchain) with an unsupported version {version}"
            );
            assert!(!config.is_empty());
        }
        (false, None, None) => {
            // The PHP-free host: the crate compiled, the ABI surface did not,
            // and `Toolchain::discover` is what tells the user what to install.
            assert!(!blastit_php_sapi::toolchain_present());
        }
        other => panic!("build.rs set an inconsistent world: {other:?}"),
    }
}

#[cfg(any(php_toolchain, feature = "probe"))]
#[test]
fn the_rust_abi_mirror_has_the_c_layout() {
    use blastit_php_sapi::abi::{
        ABI_MAJOR, ABI_VERSION, ConfigV1, OUTCOME_BAILOUT, OUTCOME_OK, RequestV1, ResultV1,
        ServerVarV1, SinkV1,
    };
    use core::mem::{offset_of, size_of};

    assert_eq!(ABI_VERSION, 1);
    assert_eq!(ABI_MAJOR, 8);
    assert_eq!(OUTCOME_OK, 0);
    assert_eq!(OUTCOME_BAILOUT, 1);
    // Same numbers as the `_Static_assert`s in c/blastit_php_sapi.c.
    assert_eq!(size_of::<ResultV1>(), 32);
    assert_eq!(offset_of!(ResultV1, cpu_ns), 16);
    assert_eq!(offset_of!(ResultV1, body_bytes), 24);
    assert_eq!(size_of::<SinkV1>(), 4 * size_of::<usize>());
    assert_eq!(offset_of!(SinkV1, head), 2 * size_of::<usize>());
    assert_eq!(offset_of!(RequestV1, content_length), 7 * 8);
    assert_eq!(offset_of!(RequestV1, server_vars), 9 * 8);
    assert_eq!(offset_of!(RequestV1, body_len), 12 * 8);
    assert_eq!(size_of::<ServerVarV1>(), 4 * size_of::<usize>());
    assert_eq!(size_of::<ConfigV1>(), 2 * size_of::<usize>());
}
