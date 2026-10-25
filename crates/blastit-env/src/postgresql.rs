//! The `postgresql` protocol (§5.2).
//!
//! One `[proxy.<name>]` declaration with `protocol = "postgresql"` says which
//! environment variables hold the upstream's real configuration (its *input
//! slots*), and optionally which variables the app should see instead (its
//! *outputs*). This module turns the declaration and the platform environment
//! into an [`Upstream`] — host, port, role, password, database, TLS mode and CA
//! — that dbproxy (§2.2) uses, and into the template fields and default outputs
//! the app's environment is built from.
//!
//! Three properties matter:
//!
//! * **No secret is templatable.** The fields are exactly [`template_fields`]:
//!   the local socket directory, this proxy's local port, the socket path, the
//!   upstream database name and the fixed local user. There is no field for
//!   the password, the upstream host or the CA, so no output can leak them.
//! * **A URL may not smuggle configuration.** Only the parameters on
//!   [`ALLOWED_PARAMETERS`] (plus `keepalives*`) are carried through in
//!   [`Upstream::options`]; anything that would move the connection elsewhere
//!   (`host`, `hostaddr`, `port`, `dbname`, `user`, `service`) or supply another
//!   credential or file (`password`, `sslpassword`, `passfile`, `sslkey`,
//!   `sslcert`, `sslrootcert`, `sslcrl`, `options`) is an error that names the
//!   parameter, never its value.
//! * **Nothing here is this crate's business beyond carrying it.** In
//!   particular, forcing `standard_conforming_strings = on` on every upstream
//!   connection is dbproxy's job (it must, because the SQL policy compares
//!   literal text), not this crate's: the parameters above are handed over
//!   as they were written.
//!
//! ```
//! use std::collections::BTreeMap;
//! use blastit_env::postgresql;
//! use blastit_manifest::Manifest;
//!
//! let manifest = Manifest::from_toml_str(
//!     "version = 1\n\
//!      [proxy.db]\n\
//!      protocol = \"postgresql\"\n\
//!      input = { url = \"DATABASE_URL\", ca_cert = \"DATABASE_CA_CERT\" }\n",
//! )
//! .unwrap();
//!
//! let env = BTreeMap::from([
//!     (
//!         "DATABASE_URL".to_string(),
//!         "postgresql://doadmin:s3cr3t@db.example.com:25060/defaultdb?sslmode=require".to_string(),
//!     ),
//!     ("DATABASE_CA_CERT".to_string(), "-----BEGIN CERTIFICATE-----".to_string()),
//! ]);
//!
//! let proxy = manifest.proxy("db").unwrap();
//! let upstream = postgresql::upstream(proxy, &env).unwrap();
//! assert_eq!(upstream.host(), "db.example.com");
//! assert_eq!(upstream.port(), 25060);
//! assert_eq!(upstream.password().expose(), "s3cr3t");
//! assert_eq!(upstream.sslmode(), Some("require"));
//! assert_eq!(upstream.ca_cert().map(|ca| ca.expose()), Some("-----BEGIN CERTIFICATE-----"));
//! ```

use std::collections::BTreeMap;

use blastit_manifest::Proxy;
use percent_encoding::percent_decode_str;
use url::Url;
use zeroize::Zeroizing;

use crate::error::{EnvError, EnvErrors};
use crate::secret::Secret;
use crate::template::{Fields, urlencode};

/// The input slots the `postgresql` protocol has, in protocol order (§5.2).
///
/// `url` is the usual case; `host`, `port`, `user`, `password`, `database` and
/// `sslmode` are the split form DigitalOcean's bindables provide; `ca_cert` is
/// the optional upstream CA. Setting `url` together with any split slot is an
/// error, and so is setting neither `url` nor `host`.
pub const INPUT_SLOTS: &[&str] = &[
    "url", "host", "port", "user", "password", "database", "sslmode", "ca_cert",
];

/// The slots that belong to the split form, as opposed to `url` (§5.2).
const SPLIT_SLOTS: &[&str] = &["host", "port", "user", "password", "database", "sslmode"];

/// The first local port dbproxy's `postgresql` listeners use (§5.2).
pub const PORT_BASE: u16 = 5432;

/// The port libpq uses when the URL names none.
pub const DEFAULT_PORT: u16 = 5432;

/// The socket directory every view sees (§5.2): each view's working directory
/// is its own socket directory, so the same rendered value is correct in every
/// process.
pub const SOCKET_DIR: &str = "/proc/self/cwd";

/// The user the rendered connection strings name; the proxy ignores it, because
/// identity is the socket the connection arrived on (§2.2, §5.2).
pub const LOCAL_USER: &str = "blastit";

/// The local port of the `postgresql` proxy at `index`: `5432 + index` (§5.2).
///
/// `None` when the index does not fit above [`PORT_BASE`]. The index is the
/// proxy's position among the `postgresql` proxies in the manifest's proxy map.
pub fn local_port(index: usize) -> Option<u16> {
    u16::try_from(index)
        .ok()
        .and_then(|index| PORT_BASE.checked_add(index))
}

/// One `postgresql` proxy's upstream: what dbproxy connects to (§2.2, §5.2).
///
/// The password and the CA are [`Secret`]s, so they cannot be logged, and
/// [`std::fmt::Debug`] prints `Secret(<redacted>)` for them.
#[derive(Clone, Debug)]
pub struct Upstream {
    host: String,
    port: u16,
    user: String,
    password: Secret,
    database: String,
    sslmode: Option<String>,
    ca_cert: Option<Secret>,
    options: Vec<(String, String)>,
}

impl Upstream {
    /// The upstream host, as written, with an IPv6 literal's brackets removed
    /// (`[2001:db8::1]` becomes `2001:db8::1`).
    ///
    /// A multi-host input (`h1,h2`) is rejected when the upstream is parsed, so
    /// this is always a single host. A percent-encoded value is kept as written
    /// — that is how libpq and `dj-database-url` spell a Unix socket directory
    /// (§5.2) — and the reader decodes it, as libpq does.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The upstream port, [`DEFAULT_PORT`] when the input names none.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The upstream role.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The upstream password.
    pub fn password(&self) -> &Secret {
        &self.password
    }

    /// The upstream database name.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// The upstream `sslmode`, when the input named one.
    pub fn sslmode(&self) -> Option<&str> {
        self.sslmode.as_deref()
    }

    /// The upstream CA, when the proxy has one.
    pub fn ca_cert(&self) -> Option<&Secret> {
        self.ca_cert.as_ref()
    }

    /// The query parameters that passed the allowlist, in URL order.
    ///
    /// Carried through as written: what dbproxy does with them (setting
    /// `standard_conforming_strings`, for instance) is dbproxy's business.
    /// Everything else in the URL's query is refused when the upstream is
    /// parsed, so nothing here can redirect the connection or add a
    /// credential.
    pub fn options(&self) -> &[(String, String)] {
        &self.options
    }
}

/// Resolves one `postgresql` proxy's inputs against the environment and parses
/// its upstream (§5.2).
///
/// Every problem is reported as an [`EnvError`]; none of them carries a value
/// from the environment.
pub fn upstream(
    proxy: &Proxy,
    platform_env: &BTreeMap<String, String>,
) -> Result<Upstream, EnvErrors> {
    let mut errors = Vec::new();
    // The raw values are only intermediates: they are zeroized as soon as the
    // upstream is built, and the ones that are secrets end up in a `Secret`.
    let mut values: BTreeMap<&'static str, Zeroizing<String>> = BTreeMap::new();

    for (slot, variable) in proxy.input() {
        if let Some(reason) = crate::error::invalid_name_reason(variable) {
            errors.push(EnvError::InvalidVariableName {
                proxy: proxy.name().clone(),
                variable: variable.clone(),
                reason,
            });
            continue;
        }
        let Some(known) = known_slot(slot) else {
            errors.push(EnvError::UnknownSlot {
                proxy: proxy.name().clone(),
                slot: slot.clone(),
            });
            continue;
        };
        match platform_env.get(variable) {
            Some(value) => {
                values.insert(known, Zeroizing::new(value.clone()));
            }
            None => errors.push(EnvError::MissingVariable {
                proxy: proxy.name().clone(),
                slot: known.to_string(),
                variable: variable.clone(),
            }),
        }
    }

    if proxy.input().contains_key("url") {
        if SPLIT_SLOTS
            .iter()
            .any(|slot| proxy.input().contains_key(*slot))
        {
            errors.push(EnvError::UrlAndSplit {
                proxy: proxy.name().clone(),
            });
        }
    } else if !proxy.input().contains_key("host") {
        errors.push(EnvError::MissingUrlOrHost {
            proxy: proxy.name().clone(),
        });
    }

    if !errors.is_empty() {
        return Err(EnvErrors::new(errors));
    }

    parse(proxy, &values).map_err(EnvErrors::new)
}

/// The template fields a `postgresql` proxy offers (§5.2).
///
/// Exactly [`template_fields`]: the local endpoint and two non-secret facts.
/// There is no field for the password, the upstream host or the CA.
pub fn template_fields(upstream: &Upstream, port: u16) -> Fields {
    let mut fields = Fields::new();
    fields.insert("socket_dir", SOCKET_DIR);
    fields.insert("port", port.to_string());
    fields.insert("socket_path", format!("{SOCKET_DIR}/.s.PGSQL.{port}"));
    fields.insert("database", upstream.database.clone());
    fields.insert("user", LOCAL_USER);
    fields.insert(
        "url",
        format!(
            "postgresql://{LOCAL_USER}@{}:{port}/{}",
            urlencode(SOCKET_DIR),
            urlencode(&upstream.database)
        ),
    );
    fields
}

/// The default outputs of a proxy that has no `output` table (§5.2).
///
/// Per input slot, the same variable name, rendered as the equivalent local
/// value; `sslmode` is `disable`, since the local leg is a Unix socket; and
/// `password` and `ca_cert` are removed (they have no output at all).
pub fn default_outputs(proxy: &Proxy) -> BTreeMap<String, String> {
    let mut outputs = BTreeMap::new();
    for (slot, variable) in proxy.input() {
        let template = match slot.as_str() {
            "url" => "{{ url }}",
            "host" => "{{ socket_dir }}",
            "port" => "{{ port }}",
            "user" => "{{ user }}",
            "database" => "{{ database }}",
            "sslmode" => "disable",
            "password" | "ca_cert" => continue,
            _ => continue,
        };
        outputs.insert(variable.clone(), template.to_string());
    }
    outputs
}

/// The slot's canonical spelling, when the protocol has it.
fn known_slot(slot: &str) -> Option<&'static str> {
    INPUT_SLOTS.iter().copied().find(|known| *known == slot)
}

/// Parses an upstream from resolved slot values.
///
/// Every problem found is returned; nothing here reads the environment again.
fn parse(
    proxy: &Proxy,
    values: &BTreeMap<&'static str, Zeroizing<String>>,
) -> Result<Upstream, Vec<EnvError>> {
    let mut upstream = match values.get("url") {
        Some(raw) => parse_url(proxy, raw.as_str())?,
        None => parse_split(proxy, values)?,
    };
    // `ca_cert` is independent of the form: a URL plus a CA is fine (§5.1).
    upstream.ca_cert = values.get("ca_cert").map(|ca| Secret::new(ca.as_str()));
    Ok(upstream)
}

/// Parses the split form: every slot directly (§5.2).
fn parse_split(
    proxy: &Proxy,
    values: &BTreeMap<&'static str, Zeroizing<String>>,
) -> Result<Upstream, Vec<EnvError>> {
    let slot = |name: &str| name.to_string();
    let variable = |name: &str| proxy.input().get(name).cloned().unwrap_or_default();
    let host = values
        .get("host")
        .map(|host| host.as_str())
        .unwrap_or_default();

    let mut errors = Vec::new();
    if host.contains(',') {
        errors.push(EnvError::MultipleHosts {
            proxy: proxy.name().clone(),
            slot: slot("host"),
            variable: variable("host"),
        });
    }
    if !proxy.input().contains_key("database") {
        errors.push(EnvError::NoDatabaseSlot {
            proxy: proxy.name().clone(),
        });
    } else if values
        .get("database")
        .is_none_or(|database| database.is_empty())
    {
        errors.push(EnvError::MissingDatabase {
            proxy: proxy.name().clone(),
            slot: slot("database"),
            variable: variable("database"),
        });
    }

    let port = match values.get("port") {
        Some(raw) => match raw.parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                errors.push(EnvError::InvalidPort {
                    proxy: proxy.name().clone(),
                    slot: slot("port"),
                    variable: variable("port"),
                });
                DEFAULT_PORT
            }
        },
        None => DEFAULT_PORT,
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Upstream {
        host: host.to_string(),
        port,
        user: values
            .get("user")
            .map(|user| user.as_str())
            .unwrap_or_default()
            .to_string(),
        password: Secret::new(
            values
                .get("password")
                .map(|password| password.as_str())
                .unwrap_or_default(),
        ),
        database: values
            .get("database")
            .map(|database| database.as_str())
            .unwrap_or_default()
            .to_string(),
        sslmode: values.get("sslmode").map(|mode| mode.as_str().to_string()),
        ca_cert: None,
        options: Vec::new(),
    })
}

/// The URL query parameters blastit carries through to dbproxy (§5.2).
///
/// The list is deliberately small: every parameter that would move the
/// connection somewhere else (`host`, `hostaddr`, `port`, `dbname`, `user`,
/// `service`) or supply another credential or file (`password`, `sslpassword`,
/// `passfile`, `sslkey`, `sslcert`, `sslrootcert`, `sslcrl`, `options`) is
/// refused, so a URL cannot smuggle configuration past the proxy. Any
/// `keepalives*` parameter is accepted as well.
pub const ALLOWED_PARAMETERS: &[&str] = &[
    "application_name",
    "channel_binding",
    "connect_timeout",
    "sslmode",
    "sslcompression",
    "target_session_attrs",
];

/// True for the parameters dbproxy may receive.
fn parameter_is_allowed(parameter: &str) -> bool {
    ALLOWED_PARAMETERS.contains(&parameter) || parameter.starts_with("keepalives")
}

/// Parses a libpq URI (`postgres://` or `postgresql://`) (§5.2).
///
/// The user, password and database are percent-decoded; allowlisted query
/// parameters are kept in [`Upstream::options`] except `sslmode`, which is
/// extracted. The host is kept as written, with an IPv6 literal's brackets
/// removed. A malformed URL is reported without ever naming the URL, a
/// password or a parameter value.
fn parse_url(proxy: &Proxy, raw: &str) -> Result<Upstream, Vec<EnvError>> {
    let variable = || proxy.input().get("url").cloned().unwrap_or_default();
    let invalid = || {
        vec![EnvError::InvalidUrl {
            proxy: proxy.name().clone(),
            slot: "url".to_string(),
            variable: variable(),
        }]
    };

    let url = Url::parse(raw).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(invalid());
    }
    let Some(host) = url.host_str() else {
        return Err(invalid());
    };
    // `host_str` keeps an IPv6 literal's brackets; dbproxy wants the address.
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);

    let mut errors = Vec::new();
    if host.contains(',') {
        errors.push(EnvError::MultipleHosts {
            proxy: proxy.name().clone(),
            slot: "url".to_string(),
            variable: variable(),
        });
    }

    let user = decode(url.username()).ok_or_else(invalid)?;
    let password = match url.password() {
        Some(password) => Secret::new(decode(password).ok_or_else(invalid)?),
        None => Secret::new(String::new()),
    };
    let database =
        decode(url.path().strip_prefix('/').unwrap_or(url.path())).ok_or_else(invalid)?;
    if database.is_empty() {
        errors.push(EnvError::MissingDatabase {
            proxy: proxy.name().clone(),
            slot: "url".to_string(),
            variable: variable(),
        });
    }

    let mut sslmode = None;
    let mut options = Vec::new();
    for (key, value) in url.query_pairs() {
        if key == "sslmode" {
            sslmode = Some(value.into_owned());
        } else if parameter_is_allowed(&key) {
            options.push((key.into_owned(), value.into_owned()));
        } else {
            errors.push(EnvError::DisallowedParameter {
                proxy: proxy.name().clone(),
                slot: "url".to_string(),
                variable: variable(),
                parameter: key.into_owned(),
            });
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Upstream {
        host: host.to_string(),
        port: url.port().unwrap_or(DEFAULT_PORT),
        user,
        password,
        database,
        sslmode,
        ca_cert: None,
        options,
    })
}

/// Percent-decodes a URL component; `None` when the result is not UTF-8.
fn decode(value: &str) -> Option<String> {
    percent_decode_str(value)
        .decode_utf8()
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use blastit_manifest::{Manifest, Proxy};

    use super::{
        DEFAULT_PORT, INPUT_SLOTS, LOCAL_USER, PORT_BASE, SOCKET_DIR, default_outputs, local_port,
        template_fields, upstream,
    };
    use crate::error::EnvError;

    /// A secret that must never appear in an error or a `Debug`.
    const SENTINEL: &str = "S3NT1NEL-d0-not-log-me";

    /// Parses a manifest with one proxy and returns its declaration.
    fn proxy(input: &str) -> Proxy {
        let source =
            format!("version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = {input}\n");
        let manifest = Manifest::from_toml_str(&source).expect("manifest parses");
        manifest.proxy("db").expect("proxy db").clone()
    }

    /// An environment from a literal list.
    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn the_protocol_declares_its_slots_and_port_base() {
        assert_eq!(PORT_BASE, 5432);
        assert_eq!(DEFAULT_PORT, 5432);
        assert_eq!(SOCKET_DIR, "/proc/self/cwd");
        assert_eq!(LOCAL_USER, "blastit");
        assert_eq!(
            INPUT_SLOTS,
            &[
                "url", "host", "port", "user", "password", "database", "sslmode", "ca_cert"
            ]
        );
        assert_eq!(local_port(0), Some(5432));
        assert_eq!(local_port(1), Some(5433));
        assert_eq!(local_port(usize::MAX), None);
    }

    #[test]
    fn a_url_is_parsed_fully() {
        let proxy = proxy(r#"{ url = "DATABASE_URL", ca_cert = "CA" }"#);
        let env = env(&[
            (
                "DATABASE_URL",
                "postgresql://doadmin:s3cr3t@db-xyz.db.ondigitalocean.com:25060/defaultdb?sslmode=require&application_name=app",
            ),
            ("CA", "PEM"),
        ]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.host(), "db-xyz.db.ondigitalocean.com");
        assert_eq!(upstream.port(), 25060);
        assert_eq!(upstream.user(), "doadmin");
        assert_eq!(upstream.password().expose(), "s3cr3t");
        assert_eq!(upstream.database(), "defaultdb");
        assert_eq!(upstream.sslmode(), Some("require"));
        assert_eq!(
            upstream.options(),
            &[("application_name".to_string(), "app".to_string())]
        );
        assert_eq!(upstream.ca_cert().map(|ca| ca.expose()), Some("PEM"));
    }

    #[test]
    fn the_postgres_scheme_is_accepted_too_and_the_default_port_applies() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        let env = env(&[("DATABASE_URL", "postgres://user@host/db")]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.port(), 5432);
        assert!(upstream.password().is_empty());
        assert_eq!(upstream.sslmode(), None);
    }

    #[test]
    fn user_password_and_database_are_percent_decoded() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        let env = env(&[(
            "DATABASE_URL",
            "postgresql://user:p%40ss%3Aword@host:5432/my%20db",
        )]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.password().expose(), "p@ss:word");
        assert_eq!(upstream.database(), "my db");
    }

    #[test]
    fn a_malformed_url_is_rejected_without_the_url() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        for bad in [
            "not a url",
            "http://host/db",
            "postgresql://user@/db",
            "postgresql://",
            "postgresql://user:pw@host:notaport/db",
            "postgresql://user:pw@host/%FF",
        ] {
            let env = env(&[("DATABASE_URL", bad)]);
            let errors = upstream(&proxy, &env).expect_err("must be rejected");
            assert_eq!(errors.len(), 1);
            let text = errors.to_string();
            assert!(text.contains("DATABASE_URL"));
            assert!(!text.contains(bad));
        }
    }

    #[test]
    fn the_split_form_needs_a_database() {
        // The slot itself is missing.
        let decl = proxy(r#"{ host = "DB_HOST" }"#);
        let vars = env(&[("DB_HOST", "db.example.com")]);
        let errors = upstream(&decl, &vars).expect_err("must be rejected");
        assert_eq!(
            errors.errors(),
            &[EnvError::NoDatabaseSlot {
                proxy: decl.name().clone(),
            }]
        );

        // The slot is there, but the value is empty.
        let second = proxy(r#"{ host = "DB_HOST", database = "DB_DATABASE" }"#);
        let vars = env(&[("DB_HOST", "db.example.com"), ("DB_DATABASE", "")]);
        let errors = upstream(&second, &vars).expect_err("must be rejected");
        assert_eq!(
            errors.errors(),
            &[EnvError::MissingDatabase {
                proxy: second.name().clone(),
                slot: "database".to_string(),
                variable: "DB_DATABASE".to_string(),
            }]
        );
    }

    #[test]
    fn the_split_form_defaults_the_optional_slots() {
        let proxy = proxy(r#"{ host = "DB_HOST", database = "DB_DATABASE" }"#);
        let env = env(&[("DB_HOST", "db.example.com"), ("DB_DATABASE", "defaultdb")]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.host(), "db.example.com");
        assert_eq!(upstream.port(), 5432);
        assert_eq!(upstream.user(), "");
        assert_eq!(upstream.database(), "defaultdb");
        assert!(upstream.ca_cert().is_none());
        assert!(upstream.options().is_empty());
    }

    #[test]
    fn the_split_form_reads_every_slot() {
        let proxy = proxy(
            r#"{ host = "DB_HOST", port = "DB_PORT", user = "DB_USER", password = "DB_PASSWORD", database = "DB_DATABASE", sslmode = "DB_SSLMODE", ca_cert = "DB_CA_CERT" }"#,
        );
        let env = env(&[
            ("DB_HOST", "db.example.com"),
            ("DB_PORT", "25060"),
            ("DB_USER", "doadmin"),
            ("DB_PASSWORD", "s3cr3t"),
            ("DB_DATABASE", "defaultdb"),
            ("DB_SSLMODE", "verify-full"),
            ("DB_CA_CERT", "PEM"),
        ]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.host(), "db.example.com");
        assert_eq!(upstream.port(), 25060);
        assert_eq!(upstream.user(), "doadmin");
        assert_eq!(upstream.password().expose(), "s3cr3t");
        assert_eq!(upstream.database(), "defaultdb");
        assert_eq!(upstream.sslmode(), Some("verify-full"));
        assert_eq!(upstream.ca_cert().map(|ca| ca.expose()), Some("PEM"));
    }

    #[test]
    fn a_bad_split_port_is_an_error() {
        let proxy = proxy(r#"{ host = "DB_HOST", port = "DB_PORT", database = "DB_DATABASE" }"#);
        let env = env(&[
            ("DB_HOST", "h"),
            ("DB_PORT", "not-a-port"),
            ("DB_DATABASE", "db"),
        ]);
        let errors = upstream(&proxy, &env).expect_err("must be rejected");
        assert_eq!(
            errors.errors(),
            &[EnvError::InvalidPort {
                proxy: proxy.name().clone(),
                slot: "port".to_string(),
                variable: "DB_PORT".to_string(),
            }]
        );
        assert!(!errors.to_string().contains("not-a-port"));
    }

    #[test]
    fn url_query_parameters_must_be_allowlisted() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        for parameter in [
            "password",
            "sslpassword",
            "passfile",
            "sslkey",
            "sslcert",
            "sslrootcert",
            "sslcrl",
            "options",
            "host",
            "hostaddr",
            "user",
            "dbname",
            "port",
            "service",
        ] {
            let url = format!("postgresql://u:p@h:5432/db?{parameter}={SENTINEL}");
            let env = env(&[("DATABASE_URL", url.as_str())]);
            let errors = upstream(&proxy, &env).expect_err("must be rejected");
            assert_eq!(errors.len(), 1, "{parameter}");
            assert!(
                matches!(
                    &errors.errors()[0],
                    EnvError::DisallowedParameter { parameter: found, .. } if found == parameter
                ),
                "{parameter}"
            );
            let text = errors.to_string();
            assert!(text.contains("DATABASE_URL"), "{parameter}: {text}");
            assert!(text.contains(parameter), "{parameter}: {text}");
            // Neither the parameter's value nor the whole URL may come back.
            assert!(!text.contains(SENTINEL), "{parameter}: {text}");
            assert!(!format!("{errors:?}").contains(SENTINEL), "{parameter}");
            assert!(!text.contains(&url), "{parameter}: {text}");
        }
    }

    #[test]
    fn allowlisted_query_parameters_are_kept() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        let env = env(&[(
            "DATABASE_URL",
            "postgresql://u:p@h:5432/db?application_name=a&connect_timeout=5&target_session_attrs=read-write&channel_binding=require&sslcompression=0&keepalives=1&keepalives_idle=30&sslmode=verify-full",
        )]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.sslmode(), Some("verify-full"));
        assert_eq!(
            upstream.options(),
            &[
                ("application_name".to_string(), "a".to_string()),
                ("connect_timeout".to_string(), "5".to_string()),
                ("target_session_attrs".to_string(), "read-write".to_string()),
                ("channel_binding".to_string(), "require".to_string()),
                ("sslcompression".to_string(), "0".to_string()),
                ("keepalives".to_string(), "1".to_string()),
                ("keepalives_idle".to_string(), "30".to_string()),
            ]
        );
    }

    #[test]
    fn a_multi_host_input_is_an_error() {
        let decl = proxy(r#"{ url = "DATABASE_URL" }"#);
        let vars = env(&[("DATABASE_URL", "postgresql://u:p@h1,h2:5432/db")]);
        let errors = upstream(&decl, &vars).expect_err("must be rejected");
        assert!(matches!(
            &errors.errors()[0],
            EnvError::MultipleHosts { .. }
        ));

        let decl = proxy(r#"{ host = "DB_HOST", database = "DB_DATABASE" }"#);
        let vars = env(&[("DB_HOST", "h1,h2"), ("DB_DATABASE", "db")]);
        let errors = upstream(&decl, &vars).expect_err("must be rejected");
        assert!(errors.errors().iter().any(|error| matches!(
            error,
            EnvError::MultipleHosts { slot, .. } if slot == "host"
        )));
    }

    #[test]
    fn an_ipv6_literal_loses_its_brackets() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        let env = env(&[("DATABASE_URL", "postgresql://u:p@[2001:db8::1]:5432/db")]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        assert_eq!(upstream.host(), "2001:db8::1");
        assert_eq!(upstream.port(), 5432);
    }

    #[test]
    fn a_url_without_a_database_is_an_error() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        for url in [
            "postgresql://u:p@h",
            "postgresql://u:p@h/",
            "postgresql://u:p@h/?sslmode=require",
        ] {
            let env = env(&[("DATABASE_URL", url)]);
            let errors = upstream(&proxy, &env).expect_err("must be rejected");
            assert_eq!(errors.len(), 1, "{url}");
            assert!(
                matches!(&errors.errors()[0], EnvError::MissingDatabase { .. }),
                "{url}"
            );
            assert!(!errors.to_string().contains(url));
        }
    }

    #[test]
    fn an_unusable_variable_name_is_an_error() {
        // The manifest already refuses an empty name, so the two names that can
        // reach the protocol are the ones with `=` and a NUL byte (`\u0000` is
        // the TOML escape for it).
        let cases = [("DATABASE=URL", "'='"), (r"DATABASE\u0000URL", "NUL")];
        for (name, expected) in cases {
            let source = format!(
                "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = {{ host = \"{name}\", database = \"DB_DATABASE\" }}\n"
            );
            let manifest = Manifest::from_toml_str(&source).expect("manifest parses");
            let proxy = manifest.proxy("db").expect("proxy db").clone();
            let env = env(&[("DB_DATABASE", "db")]);
            let errors = upstream(&proxy, &env).expect_err("must be rejected");
            assert!(
                errors.errors().iter().any(|error| matches!(
                    error,
                    EnvError::InvalidVariableName { reason, .. } if reason.contains(expected)
                )),
                "{name:?}: {errors}"
            );
        }
    }

    #[test]
    fn url_and_split_slots_conflict() {
        for extra in ["host", "port", "user", "password", "database", "sslmode"] {
            let source = format!(
                "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = {{ url = \"U\", {extra} = \"V\" }}\n"
            );
            let manifest = Manifest::from_toml_str(&source).expect("manifest parses");
            let proxy = manifest.proxy("db").expect("proxy db").clone();
            let env = env(&[("U", "postgresql://u:p@h/db"), ("V", "x")]);
            let errors = upstream(&proxy, &env).expect_err("must be rejected");
            assert!(
                errors
                    .errors()
                    .iter()
                    .any(|error| matches!(error, EnvError::UrlAndSplit { .. }))
            );
        }
    }

    #[test]
    fn neither_url_nor_host_is_an_error() {
        let proxy = proxy(r#"{ database = "DB_NAME" }"#);
        let env = env(&[("DB_NAME", "defaultdb")]);
        let errors = upstream(&proxy, &env).expect_err("must be rejected");
        assert_eq!(
            errors.errors(),
            &[EnvError::MissingUrlOrHost {
                proxy: proxy.name().clone(),
            }]
        );
    }

    #[test]
    fn a_missing_variable_names_the_variable() {
        let proxy = proxy(r#"{ url = "DATABASE_URL" }"#);
        let errors = upstream(&proxy, &BTreeMap::new()).expect_err("must be rejected");
        assert_eq!(
            errors.errors(),
            &[EnvError::MissingVariable {
                proxy: proxy.name().clone(),
                slot: "url".to_string(),
                variable: "DATABASE_URL".to_string(),
            }]
        );
    }

    #[test]
    fn an_unknown_slot_is_an_error() {
        let proxy = proxy(r#"{ url = "U", dsn = "D" }"#);
        let env = env(&[("U", "postgresql://u:p@h/db"), ("D", "x")]);
        let errors = upstream(&proxy, &env).expect_err("must be rejected");
        assert!(errors.errors().iter().any(|error| matches!(
            error,
            EnvError::UnknownSlot { slot, .. } if slot == "dsn"
        )));
    }

    #[test]
    fn template_fields_never_expose_the_password_host_or_ca() {
        let proxy = proxy(r#"{ url = "U", ca_cert = "C" }"#);
        let env = env(&[
            (
                "U",
                "postgresql://u:s3cr3t@upstream.example.com:25060/my%20db",
            ),
            ("C", "PEM"),
        ]);
        let upstream = upstream(&proxy, &env).expect("upstream");
        let fields = template_fields(&upstream, 5433);

        let names: Vec<&str> = fields.iter().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            vec![
                "database",
                "port",
                "socket_dir",
                "socket_path",
                "url",
                "user"
            ]
        );
        assert_eq!(fields.get("socket_dir"), Some("/proc/self/cwd"));
        assert_eq!(
            fields.get("socket_path"),
            Some("/proc/self/cwd/.s.PGSQL.5433")
        );
        assert_eq!(fields.get("port"), Some("5433"));
        assert_eq!(fields.get("database"), Some("my db"));
        assert_eq!(fields.get("user"), Some("blastit"));
        assert_eq!(
            fields.get("url"),
            Some("postgresql://blastit@%2Fproc%2Fself%2Fcwd:5433/my%20db")
        );

        // Nothing reachable from a template mentions a secret.
        let rendered = format!("{fields:?}");
        assert!(!rendered.contains("s3cr3t"));
        assert!(!rendered.contains("upstream.example.com"));
        assert!(!rendered.contains("PEM"));
    }

    #[test]
    fn default_outputs_follow_the_input_slots() {
        let proxy = proxy(r#"{ url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }"#);
        let outputs = default_outputs(&proxy);
        assert_eq!(
            outputs.get("DATABASE_URL").map(String::as_str),
            Some("{{ url }}")
        );
        assert!(!outputs.contains_key("DATABASE_CA_CERT"));
    }

    #[test]
    fn default_outputs_cover_the_split_form() {
        let proxy = proxy(
            r#"{ host = "DB_HOST", port = "DB_PORT", user = "DB_USER", password = "DB_PASSWORD", database = "DB_DATABASE", sslmode = "DB_SSLMODE" }"#,
        );
        let outputs = default_outputs(&proxy);
        assert_eq!(
            outputs.get("DB_HOST").map(String::as_str),
            Some("{{ socket_dir }}")
        );
        assert_eq!(
            outputs.get("DB_PORT").map(String::as_str),
            Some("{{ port }}")
        );
        assert_eq!(
            outputs.get("DB_USER").map(String::as_str),
            Some("{{ user }}")
        );
        assert_eq!(
            outputs.get("DB_DATABASE").map(String::as_str),
            Some("{{ database }}")
        );
        assert_eq!(
            outputs.get("DB_SSLMODE").map(String::as_str),
            Some("disable")
        );
        assert!(!outputs.contains_key("DB_PASSWORD"));
    }
}
