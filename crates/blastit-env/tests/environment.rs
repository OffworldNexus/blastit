//! The rewrite end to end: ARCHITECTURE.md §5.1 and §5.3, against the examples.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use blastit_env::{EnvError, Prepared, build};
use blastit_manifest::Manifest;

/// A secret that must never appear in an error, a `Debug` or the app
/// environment.
const SENTINEL: &str = "S3NT1NEL-d0-not-log-me";

/// The two-proxy example from ARCHITECTURE.md §5.1, plus the `version` line a
/// whole manifest needs.
const SECTION_5_1: &str = r#"version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

[proxy.analytics]
protocol = "postgresql"
input = { url = "ANALYTICS_URL" }
[proxy.analytics.output]
ANALYTICS_DB_HOST = "{{ socket_dir }}"
ANALYTICS_DB_PORT = "{{ port }}"
ANALYTICS_DB_NAME = "{{ database }}"
ANALYTICS_DB_USER = "{{ user }}"
"#;

/// The platform environment of ARCHITECTURE.md §5.1.
fn section_5_1_platform() -> BTreeMap<String, String> {
    env(&[
        (
            "DATABASE_URL",
            "postgresql://doadmin:s3cr3t@db-xyz.db.ondigitalocean.com:25060/defaultdb?sslmode=require",
        ),
        ("DATABASE_CA_CERT", "-----BEGIN CERTIFICATE-----"),
        (
            "ANALYTICS_URL",
            "postgresql://reader:t0ps3cret@db-abc.db.ondigitalocean.com:25060/analytics?sslmode=require",
        ),
    ])
}

/// Parses a manifest that must parse.
fn parse(source: &str) -> Manifest {
    Manifest::from_toml_str(source).expect("manifest should parse")
}

/// A one-proxy `postgresql` manifest, with `db` as the only proxy.
fn one_proxy(input: &str) -> Manifest {
    parse(&format!(
        "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = {input}\n"
    ))
}

/// A one-proxy `postgresql` manifest with an explicit output table.
fn one_proxy_output(input: &str, output: &str) -> Manifest {
    parse(&format!(
        "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = {input}\n[proxy.db.output]\n{output}\n"
    ))
}

/// A `BTreeMap` environment from a literal list.
fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// One variable of the app environment.
fn app<'a>(prepared: &'a Prepared, name: &str) -> Option<&'a str> {
    prepared.app_env().get(name).map(String::as_str)
}

/// The variables a proxy produced, as string slices.
fn outputs<'a>(prepared: &'a Prepared, proxy: &str) -> Option<Vec<&'a str>> {
    prepared
        .outputs()
        .get(proxy)
        .map(|variables| variables.iter().map(String::as_str).collect())
}

/// Builds and expects failure, returning every rendered error.
fn failure(manifest: &Manifest, platform: &BTreeMap<String, String>) -> Vec<String> {
    let errors = build(manifest, platform, None).expect_err("build should fail");
    let mut rendered = vec![errors.to_string(), format!("{errors:?}")];
    for error in errors.errors() {
        rendered.push(error.to_string());
        rendered.push(format!("{error:?}"));
    }
    rendered
}

/// Asserts no rendered error carries the sentinel.
fn assert_quiet(rendered: &[String]) {
    assert!(!rendered.is_empty());
    for text in rendered {
        assert!(
            !text.contains(SENTINEL),
            "an error leaked the environment: {text}"
        );
    }
}

#[test]
fn architecture_5_1_example_end_to_end() {
    // ARCHITECTURE.md §5.1: platform → app, two proxies, split outputs for
    // "analytics".
    //
    // Ports: §5.2 gives each postgresql proxy `5432 + k`. The only manifest
    // order this crate can see is `Manifest::proxies()` order, which
    // `blastit-manifest` keeps sorted by name (it documents that the order of
    // the sections in the file does not matter). So "analytics" is proxy 0
    // (5432) and "db" is proxy 1 (5433); the doc's example declares "db"
    // first, which would number them the other way round. Everything else —
    // the variable swap, the socket directory, the users, the databases and
    // the split outputs — matches §5.1 verbatim.
    let manifest = parse(SECTION_5_1);
    let prepared = build(&manifest, &section_5_1_platform(), None).expect("builds");

    // The app sees the local endpoints, never the platform's.
    assert_eq!(
        app(&prepared, "DATABASE_URL"),
        Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5433/defaultdb")
    );
    assert_eq!(app(&prepared, "ANALYTICS_DB_HOST"), Some("/proc/self/cwd"));
    assert_eq!(app(&prepared, "ANALYTICS_DB_PORT"), Some("5432"));
    assert_eq!(app(&prepared, "ANALYTICS_DB_NAME"), Some("analytics"));
    assert_eq!(app(&prepared, "ANALYTICS_DB_USER"), Some("blastit"));

    // The inputs are gone; the CA material has no output at all.
    assert!(!prepared.app_env().contains_key("DATABASE_CA_CERT"));
    assert!(!prepared.app_env().contains_key("ANALYTICS_URL"));

    // Both proxies reported the variables they produced.
    assert_eq!(
        outputs(&prepared, "db").as_deref(),
        Some(["DATABASE_URL"].as_slice())
    );
    assert_eq!(
        outputs(&prepared, "analytics").as_deref(),
        Some(
            [
                "ANALYTICS_DB_HOST",
                "ANALYTICS_DB_NAME",
                "ANALYTICS_DB_PORT",
                "ANALYTICS_DB_USER"
            ]
            .as_slice()
        )
    );

    // dbproxy keeps the real credentials.
    let db = prepared.upstream("db").expect("proxy db");
    assert_eq!(db.host(), "db-xyz.db.ondigitalocean.com");
    assert_eq!(db.port(), 25060);
    assert_eq!(db.password().expose(), "s3cr3t");
    assert_eq!(db.database(), "defaultdb");
    assert_eq!(db.sslmode(), Some("require"));
    assert_eq!(
        db.ca_cert().map(|ca| ca.expose()),
        Some("-----BEGIN CERTIFICATE-----")
    );
    let analytics = prepared.upstream("analytics").expect("proxy analytics");
    assert_eq!(analytics.password().expose(), "t0ps3cret");
    assert_eq!(analytics.database(), "analytics");
    assert!(analytics.ca_cert().is_none());

    // The proxies are listed in manifest proxy order.
    let names: Vec<&str> = prepared
        .upstreams()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, vec!["analytics", "db"]);

    // The secrets are not reachable through a `Debug`.
    let debug = format!("{prepared:?}");
    assert!(!debug.contains("s3cr3t"));
    assert!(!debug.contains("t0ps3cret"));
    assert!(debug.contains("Secret(<redacted>)"));
}

#[test]
fn no_output_table_rewrites_the_same_variable_in_place() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }"#);
    let platform = env(&[
        (
            "DATABASE_URL",
            "postgresql://doadmin:s3cr3t@db.example.com:25060/defaultdb?sslmode=require",
        ),
        ("DATABASE_CA_CERT", "PEM"),
        ("PATH", "/usr/bin"),
    ]);
    let prepared = build(&manifest, &platform, None).expect("builds");

    assert_eq!(
        app(&prepared, "DATABASE_URL"),
        Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5432/defaultdb")
    );
    assert!(!prepared.app_env().contains_key("DATABASE_CA_CERT"));
    assert_eq!(app(&prepared, "PATH"), Some("/usr/bin"));
    assert_eq!(
        outputs(&prepared, "db").as_deref(),
        Some(["DATABASE_URL"].as_slice())
    );
}

#[test]
fn the_split_input_form_gets_the_default_outputs() {
    // DigitalOcean's split bindables: no URL, just the parts.
    let manifest = one_proxy(
        r#"{ host = "DB_HOST", port = "DB_PORT", user = "DB_USER", password = "DB_PASSWORD", database = "DB_DATABASE", sslmode = "DB_SSLMODE" }"#,
    );
    let platform = env(&[
        ("DB_HOST", "db.example.com"),
        ("DB_PORT", "25060"),
        ("DB_USER", "doadmin"),
        ("DB_PASSWORD", "s3cr3t"),
        ("DB_DATABASE", "defaultdb"),
        ("DB_SSLMODE", "require"),
    ]);
    let prepared = build(&manifest, &platform, None).expect("builds");

    assert_eq!(app(&prepared, "DB_HOST"), Some("/proc/self/cwd"));
    assert_eq!(app(&prepared, "DB_PORT"), Some("5432"));
    assert_eq!(app(&prepared, "DB_USER"), Some("blastit"));
    assert_eq!(app(&prepared, "DB_DATABASE"), Some("defaultdb"));
    // The local leg is a Unix socket, so TLS is off.
    assert_eq!(app(&prepared, "DB_SSLMODE"), Some("disable"));
    // The password is removed.
    assert!(!prepared.app_env().contains_key("DB_PASSWORD"));
    assert_eq!(
        outputs(&prepared, "db").as_deref(),
        Some(["DB_DATABASE", "DB_HOST", "DB_PORT", "DB_SSLMODE", "DB_USER"].as_slice())
    );

    let upstream = prepared.upstream("db").expect("proxy db");
    assert_eq!(upstream.host(), "db.example.com");
    assert_eq!(upstream.port(), 25060);
    assert_eq!(upstream.user(), "doadmin");
    assert_eq!(upstream.password().expose(), "s3cr3t");
    assert_eq!(upstream.sslmode(), Some("require"));
}

#[test]
fn url_together_with_a_split_slot_aborts_the_boot() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL", host = "DB_HOST" }"#);
    let platform = env(&[
        ("DATABASE_URL", "postgresql://u:p@h/db"),
        ("DB_HOST", "db.example.com"),
    ]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, EnvError::UrlAndSplit { .. }))
    );
}

#[test]
fn a_missing_input_variable_aborts_the_boot_and_names_the_variable() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    let errors = build(&manifest, &BTreeMap::new(), None).expect_err("build should fail");
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors.errors()[0],
        EnvError::MissingVariable { variable, .. } if variable == "DATABASE_URL"
    ));
    assert!(errors.to_string().contains("DATABASE_URL"));
}

#[test]
fn an_unknown_slot_aborts_the_boot() {
    let manifest = one_proxy(r#"{ dsn = "DATABASE_DSN" }"#);
    let platform = env(&[("DATABASE_DSN", "postgresql://u:p@h/db")]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::UnknownSlot { slot, .. } if slot == "dsn"
    )));
}

#[test]
fn a_malformed_url_aborts_the_boot_without_repeating_the_url() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    for bad in [
        "definitely not a URL",
        "mysql://u:p@h/db",
        "postgresql://u:p@h:port/db",
        "postgresql://",
    ] {
        let platform = env(&[("DATABASE_URL", bad)]);
        let errors = build(&manifest, &platform, None).expect_err("build should fail");
        let text = errors.to_string();
        assert!(text.contains("DATABASE_URL"), "{text}");
        assert!(text.contains("is not a valid postgresql URL"), "{text}");
        assert!(!text.contains(bad), "{text}");
    }
}

#[test]
fn a_percent_encoded_password_with_at_and_colon_is_decoded() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    let platform = env(&[(
        "DATABASE_URL",
        "postgresql://user:p%40ss%3Aword%2F%23@db.example.com:25060/defaultdb",
    )]);
    let prepared = build(&manifest, &platform, None).expect("builds");
    assert_eq!(
        prepared.upstream("db").map(|u| u.password().expose()),
        Some("p@ss:word/#")
    );
    // The password never reaches the app.
    assert!(!format!("{prepared:?}").contains("p@ss"));
    assert!(
        !prepared
            .app_env()
            .values()
            .any(|value| value.contains("p@ss"))
    );
}

#[test]
fn database_names_that_need_encoding_are_encoded() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    let platform = env(&[("DATABASE_URL", "postgresql://u:p@h:5432/my%20db%2Fx")]);
    let prepared = build(&manifest, &platform, None).expect("builds");
    assert_eq!(
        app(&prepared, "DATABASE_URL"),
        Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5432/my%20db%2Fx")
    );
    // The `database` field itself is the decoded name.
    assert_eq!(
        prepared.upstream("db").map(|u| u.database()),
        Some("my db/x")
    );
}

#[test]
fn explicit_outputs_can_use_both_filters_and_any_whitespace() {
    let manifest = one_proxy_output(
        r#"{ url = "DATABASE_URL" }"#,
        r#"RAW = "{{ socket_dir }}"
ENCODED = "{{socket_dir|urlencode}}"
SPACED = "{{   port   }}"
MIXED = "db://{{ user }}@{{ socket_dir | urlencode }}:{{ port }}/{{ database }}"
PLAIN = "disable""#,
    );
    let platform = env(&[("DATABASE_URL", "postgresql://u:p@h:5432/defaultdb")]);
    let prepared = build(&manifest, &platform, None).expect("builds");

    assert_eq!(app(&prepared, "RAW"), Some("/proc/self/cwd"));
    assert_eq!(app(&prepared, "ENCODED"), Some("%2Fproc%2Fself%2Fcwd"));
    assert_eq!(app(&prepared, "SPACED"), Some("5432"));
    assert_eq!(
        app(&prepared, "MIXED"),
        Some("db://blastit@%2Fproc%2Fself%2Fcwd:5432/defaultdb")
    );
    assert_eq!(app(&prepared, "PLAIN"), Some("disable"));
    assert_eq!(
        outputs(&prepared, "db").as_deref(),
        Some(["ENCODED", "MIXED", "PLAIN", "RAW", "SPACED"].as_slice())
    );
}

#[test]
fn an_output_template_error_aborts_the_boot() {
    let cases = [
        ("{{ password }}", "unknown field"),
        ("{{ url | lower }}", "unknown filter"),
        ("{{ url", "unclosed"),
        ("{{ }}", "empty field name"),
    ];
    for (template, expected) in cases {
        let manifest =
            one_proxy_output(r#"{ url = "DATABASE_URL" }"#, &format!("X = {template:?}"));
        let platform = env(&[("DATABASE_URL", "postgresql://u:p@h/db")]);
        let errors = build(&manifest, &platform, None).expect_err("build should fail");
        let text = errors.to_string();
        assert!(text.contains(expected), "{template}: {text}");
    }
}

#[test]
fn an_output_may_not_overwrite_a_variable_it_does_not_own() {
    let manifest = one_proxy_output(
        r#"{ url = "DATABASE_URL" }"#,
        r#"SECRET_KEY = "{{ user }}""#,
    );
    let platform = env(&[
        ("DATABASE_URL", "postgresql://u:p@h/db"),
        ("SECRET_KEY", "keep-me"),
    ]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::OutputOverwritesForeign { variable, .. } if variable == "SECRET_KEY"
    )));
}

#[test]
fn an_output_may_redefine_its_own_input() {
    let manifest = one_proxy_output(
        r#"{ url = "DATABASE_URL" }"#,
        r#"DATABASE_URL = "{{ url }}?sslmode=disable""#,
    );
    let platform = env(&[("DATABASE_URL", "postgresql://u:p@h/db")]);
    let prepared = build(&manifest, &platform, None).expect("builds");
    assert_eq!(
        app(&prepared, "DATABASE_URL"),
        Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5432/db?sslmode=disable")
    );
}

#[test]
fn two_proxies_may_not_produce_the_same_variable() {
    let manifest = parse(
        r#"version = 1
[proxy.a]
protocol = "postgresql"
input = { url = "A_URL" }
[proxy.a.output]
SHARED = "{{ user }}"
[proxy.b]
protocol = "postgresql"
input = { url = "B_URL" }
[proxy.b.output]
SHARED = "{{ user }}"
"#,
    );
    let platform = env(&[
        ("A_URL", "postgresql://u:p@h/a"),
        ("B_URL", "postgresql://u:p@h/b"),
    ]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::DuplicateOutput { variable, .. } if variable == "SHARED"
    )));
}

#[test]
fn an_output_may_not_name_a_blastit_variable() {
    let manifest = one_proxy_output(
        r#"{ url = "DATABASE_URL" }"#,
        r#"BLASTIT_SNEAKY = "{{ user }}""#,
    );
    let platform = env(&[("DATABASE_URL", "postgresql://u:p@h/db")]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::ReservedOutput { variable, .. } if variable == "BLASTIT_SNEAKY"
    )));
}

#[test]
fn every_blastit_variable_is_removed_from_the_app_environment() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    let platform = env(&[
        ("DATABASE_URL", "postgresql://u:p@h/db"),
        ("BLASTIT_MANIFEST", "/app/blastit.toml"),
        ("BLASTIT_UID_BASE", "20000"),
        ("BLASTIT_MAX_WORKERS", "4"),
        ("BLASTIT_ENV_STRIP", "PGPASSWORD"),
        ("PGPASSWORD", "s3cr3t"),
        ("PATH", "/usr/bin"),
    ]);
    let prepared = build(&manifest, &platform, None).expect("builds");
    assert_eq!(app(&prepared, "PATH"), Some("/usr/bin"));
    for name in [
        "BLASTIT_MANIFEST",
        "BLASTIT_UID_BASE",
        "BLASTIT_MAX_WORKERS",
        "BLASTIT_ENV_STRIP",
        "PGPASSWORD",
    ] {
        assert!(
            !prepared.app_env().contains_key(name),
            "{name} should be stripped"
        );
    }
}

#[test]
fn the_strip_list_comes_from_the_environment_and_from_the_argument() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    let platform = env(&[
        ("DATABASE_URL", "postgresql://u:p@h/db"),
        ("BLASTIT_ENV_STRIP", " PGPASSWORD,PGHOST ,  PGUSER  "),
        ("PGPASSWORD", "s3cr3t"),
        ("PGHOST", "db.example.com"),
        ("PGUSER", "doadmin"),
        ("SECRET_KEY", "k"),
        ("UPSTREAM_TOKEN", "t"),
        ("PATH", "/usr/bin"),
    ]);

    // The environment's list, on its own.
    let prepared = build(&manifest, &platform, None).expect("builds");
    for name in ["PGPASSWORD", "PGHOST", "PGUSER"] {
        assert!(
            !prepared.app_env().contains_key(name),
            "{name} should be stripped"
        );
    }
    assert_eq!(app(&prepared, "SECRET_KEY"), Some("k"));

    // The explicit list is added to it.
    let prepared = build(&manifest, &platform, Some("SECRET_KEY\tUPSTREAM_TOKEN")).expect("builds");
    for name in [
        "PGPASSWORD",
        "PGHOST",
        "PGUSER",
        "SECRET_KEY",
        "UPSTREAM_TOKEN",
    ] {
        assert!(
            !prepared.app_env().contains_key(name),
            "{name} should be stripped"
        );
    }
    assert_eq!(app(&prepared, "PATH"), Some("/usr/bin"));
}

#[test]
fn a_url_may_not_smuggle_configuration() {
    // Every form of a parameter that could redirect the connection or add a
    // credential is refused, and neither its value nor the URL comes back.
    for query in [
        "password=S3NT1NEL-d0-not-log-me",
        "sslpassword=S3NT1NEL-d0-not-log-me",
        "passfile=%2Frun%2Fsecrets%2Fpgpass",
        "sslkey=%2Frun%2Fsecrets%2Fclient.key",
        "sslcert=%2Frun%2Fsecrets%2Fclient.crt",
        "sslrootcert=%2Frun%2Fsecrets%2Fca.crt",
        "sslcrl=%2Frun%2Fsecrets%2Fca.crl",
        "options=-c%20password%3D-S3NT1NEL-d0-not-log-me",
        "host=elsewhere",
        "hostaddr=203.0.113.9",
        "user=other",
        "dbname=other",
        "port=6432",
        "service=elsewhere",
    ] {
        let url = format!("postgresql://u:p@db.example.com:5432/defaultdb?{query}");
        let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
        let platform = env(&[("DATABASE_URL", url.as_str())]);
        for text in failure(&manifest, &platform) {
            assert!(!text.contains(SENTINEL), "{query}: {text}");
            assert!(!text.contains(&url), "{query}: {text}");
            assert!(text.contains("DATABASE_URL"), "{query}: {text}");
        }
    }
}

#[test]
fn two_failing_proxies_report_both_errors() {
    let manifest = parse(
        r#"version = 1
[proxy.alpha]
protocol = "postgresql"
input = { url = "ALPHA_URL", ca_cert = "MISSING_CA" }
[proxy.beta]
protocol = "postgresql"
input = { host = "BETA_HOST", database = "MISSING_DB" }
"#,
    );
    let platform = env(&[
        ("ALPHA_URL", "postgresql://u:p@h/db"),
        ("BETA_HOST", "db.example.com"),
    ]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert_eq!(errors.len(), 2);
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::MissingVariable { proxy, variable, .. }
            if proxy.as_str() == "alpha" && variable == "MISSING_CA"
    )));
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::MissingVariable { proxy, variable, .. }
            if proxy.as_str() == "beta" && variable == "MISSING_DB"
    )));
    let text = errors.to_string();
    assert!(
        text.contains("MISSING_CA") && text.contains("MISSING_DB"),
        "{text}"
    );
}

#[test]
fn an_unusable_output_variable_name_aborts_the_boot() {
    let manifest = one_proxy_output(
        r#"{ url = "DATABASE_URL" }"#,
        r#""BAD=NAME" = "{{ user }}""#,
    );
    let platform = env(&[("DATABASE_URL", "postgresql://u:p@h/db")]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        EnvError::InvalidVariableName { variable, .. } if variable == "BAD=NAME"
    )));
}

#[test]
fn a_variable_name_is_checked_before_the_proxy_is_used() {
    // An input variable name with an `=` is refused, and the proxy is skipped
    // rather than parsed with a name that could never be an environment key.
    let manifest = one_proxy_output(r#"{ url = "DATABASE=URL" }"#, r#"X = "{{ user }}""#);
    let platform = env(&[("DATABASE=URL", "postgresql://u:p@h/db")]);
    let errors = build(&manifest, &platform, None).expect_err("build should fail");
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, EnvError::InvalidVariableName { .. }))
    );
    // The proxy was skipped, so its output was not reported as missing.
    assert!(
        !errors
            .errors()
            .iter()
            .any(|error| matches!(error, EnvError::MissingVariable { .. }))
    );
}

#[test]
fn a_url_without_a_database_aborts_the_boot() {
    let manifest = one_proxy(r#"{ url = "DATABASE_URL" }"#);
    for url in ["postgresql://u:p@h", "postgresql://u:p@h/"] {
        let platform = env(&[("DATABASE_URL", url)]);
        let errors = build(&manifest, &platform, None).expect_err("build should fail");
        assert!(
            errors
                .errors()
                .iter()
                .any(|error| matches!(error, EnvError::MissingDatabase { .. }))
        );
    }
}

#[test]
fn injected_defaults_do_not_override_values_that_are_already_there() {
    let manifest = parse("version = 1\n");
    let platform = env(&[
        ("LC_ALL", "en_US.UTF-8"),
        ("OMP_NUM_THREADS", "4"),
        ("HOME", "/root"),
        ("TMPDIR", "/tmp"),
    ]);
    let prepared = build(&manifest, &platform, None).expect("builds");

    assert_eq!(app(&prepared, "LC_ALL"), Some("en_US.UTF-8"));
    assert_eq!(app(&prepared, "OMP_NUM_THREADS"), Some("4"));
    assert_eq!(app(&prepared, "MKL_NUM_THREADS"), Some("1"));
    assert_eq!(app(&prepared, "PYTHONDONTWRITEBYTECODE"), Some("1"));
    assert_eq!(app(&prepared, "VECLIB_MAXIMUM_THREADS"), Some("1"));
    // HOME and TMPDIR are per view, so they are left alone.
    assert_eq!(app(&prepared, "HOME"), Some("/root"));
    assert_eq!(app(&prepared, "TMPDIR"), Some("/tmp"));
}

#[test]
fn an_empty_environment_still_gets_the_injected_defaults() {
    let manifest = parse("version = 1\n");
    let prepared = build(&manifest, &BTreeMap::new(), None).expect("builds");
    for (name, value) in blastit_env::INJECTED_DEFAULTS {
        assert_eq!(app(&prepared, name), Some(*value), "{name}");
    }
    assert!(!prepared.app_env().contains_key("HOME"));
    assert!(!prepared.app_env().contains_key("TMPDIR"));
}

#[test]
fn every_error_names_variables_and_never_a_value() {
    // One platform environment that carries the sentinel in every place a
    // mistake could echo it back.
    let platform = env(&[
        (
            "DATABASE_URL",
            "postgresql://u:S3NT1NEL-d0-not-log-me@h:5432/db",
        ),
        ("DATABASE_CA_CERT", SENTINEL),
        ("SECRET_KEY", SENTINEL),
        ("DB_HOST", SENTINEL),
    ]);

    let cases = [
        // `url` together with a split slot
        one_proxy(r#"{ url = "DATABASE_URL", host = "DB_HOST" }"#),
        // a variable that is not set (its name is fine to print)
        one_proxy(r#"{ url = "MISSING_URL" }"#),
        // an unknown slot
        one_proxy(r#"{ url = "DATABASE_URL", dsn = "DATABASE_URL" }"#),
        // an output that overwrites a foreign variable
        one_proxy_output(
            r#"{ url = "DATABASE_URL" }"#,
            r#"SECRET_KEY = "{{ user }}""#,
        ),
        // an output that names a reserved variable
        one_proxy_output(r#"{ url = "DATABASE_URL" }"#, r#"BLASTIT_X = "{{ user }}""#),
        // a template error
        one_proxy_output(
            r#"{ url = "DATABASE_URL" }"#,
            r#"X = "{{ database | lower }}""#,
        ),
    ];

    for manifest in cases {
        assert_quiet(&failure(&manifest, &platform));
    }

    // A URL that does not parse, with the sentinel inside it.
    let bad = env(&[("DATABASE_URL", "postgresql://u:S3NT1NEL-d0-not-log-me@/db")]);
    assert_quiet(&failure(&one_proxy(r#"{ url = "DATABASE_URL" }"#), &bad));

    // A URL whose query carries the sentinel in a parameter blastit refuses.
    for query in [
        "password=S3NT1NEL-d0-not-log-me",
        "options=-c%20password%3DS3NT1NEL-d0-not-log-me",
        "sslkey=S3NT1NEL-d0-not-log-me",
    ] {
        let url = format!("postgresql://u:p@h:5432/db?{query}");
        let bad = env(&[("DATABASE_URL", url.as_str())]);
        assert_quiet(&failure(&one_proxy(r#"{ url = "DATABASE_URL" }"#), &bad));
    }

    // A split port that is not a number.
    let bad_port = env(&[
        ("DB_HOST", "db.example.com"),
        ("DB_PORT", "not-a-port"),
        ("DB_DATABASE", "db"),
    ]);
    assert_quiet(&failure(
        &one_proxy(r#"{ host = "DB_HOST", port = "DB_PORT", database = "DB_DATABASE" }"#),
        &bad_port,
    ));

    // A successful rewrite never carries an input's value into the app
    // environment or into a `Debug`, even though dbproxy keeps it. (Variables
    // that are not a proxy's input are the app's own, and stay visible — that
    // is §5.3.)
    let manifest = one_proxy(r#"{ url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }"#);
    let inputs_only = env(&[
        (
            "DATABASE_URL",
            "postgresql://u:S3NT1NEL-d0-not-log-me@h:5432/db",
        ),
        ("DATABASE_CA_CERT", SENTINEL),
    ]);
    let prepared = build(&manifest, &inputs_only, None).expect("builds");
    assert!(
        !prepared
            .app_env()
            .values()
            .any(|value| value.contains(SENTINEL))
    );
    assert_eq!(
        prepared.upstream("db").map(|u| u.password().expose()),
        Some(SENTINEL)
    );
    assert_eq!(
        prepared
            .upstream("db")
            .and_then(|u| u.ca_cert())
            .map(|ca| ca.expose()),
        Some(SENTINEL)
    );
    assert!(!format!("{prepared:?}").contains(SENTINEL));
}
