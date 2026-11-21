//! The `postgresql` protocol (§5.2).
//!
//! One `postgres` store says which environment variables hold the upstream's
//! real configuration (its *input slots*): `postgres $DATABASE_URL` is the
//! one-line form, `postgres { host $DB_HOST … }` the split one. This module
//! turns the store and the platform environment into an [`Upstream`] — host,
//! port, role, password, database, TLS mode and CA — that dbproxy (§2.2) uses,
//! and into the template fields and default outputs the app's environment is
//! built from. A `Blastfile` has no `output` spelling: every input variable
//! keeps its name and gets the local value (§7.5).
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
//! let manifest = Manifest::from_blastfile_str(
//!     "postgres {\n\
//!          host $DB_HOST\n\
//!          port $DB_PORT\n\
//!          user $DB_USER\n\
//!          password $DB_PASSWORD\n\
//!          database $DB_DATABASE\n\
//!          sslmode $DB_SSLMODE\n\
//!          ca_cert $DB_CA_CERT\n\
//!      }\n",
//! )
//! .unwrap();
//!
//! let env = BTreeMap::from([
//!     ("DB_HOST".to_string(), "db.example.com".to_string()),
//!     ("DB_PORT".to_string(), "25060".to_string()),
//!     ("DB_USER".to_string(), "doadmin".to_string()),
//!     ("DB_PASSWORD".to_string(), "s3cr3t".to_string()),
//!     ("DB_DATABASE".to_string(), "defaultdb".to_string()),
//!     ("DB_SSLMODE".to_string(), "require".to_string()),
//!     ("DB_CA_CERT".to_string(), "-----BEGIN CERTIFICATE-----".to_string()),
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
///
/// # The shape of `url`
///
/// `postgresql://blastit@/shop?host=/proc/self/cwd&port=5432` — the socket
/// directory in a **query parameter**, not in the authority. Both spellings are
/// legal, and only this one is read by every consumer: libpq (and so
/// `psycopg.connect(url)`) resolves `host` from the query when there is no
/// authority, `dj-database-url` puts the unknown parameters in Django's
/// `OPTIONS` (which its PostgreSQL backend passes through to
/// `psycopg.connect`), and SQLAlchemy's `postgresql://` and
/// `postgresql+psycopg://` dialects hand `host`/`port` to the driver as keyword
/// arguments.
///
/// The percent-encoded-authority spelling
/// (`postgresql://blastit@%2Fproc%2Fself%2Fcwd:5432/shop`) is what libpq and
/// `dj-database-url` were built to read, and SQLAlchemy does not decode it: it
/// passes the literal `%2Fproc%2Fself%2Fcwd` as the host, which the driver then
/// puts through DNS. One spelling that every consumer resolves is worth more
/// than a shorter one that one of them does not.
pub fn template_fields(upstream: &Upstream, port: u16) -> Fields {
    template_fields_in(upstream, port, SOCKET_DIR)
}

/// [`template_fields`] with a caller-chosen socket directory.
///
/// The runtime uses [`SOCKET_DIR`], each view's own directory. `blastit check`
/// binds every proxy's socket under `<runtime_dir>/pg` instead, so its
/// `swapped_env` has to render that directory — never `/proc/self/cwd`, which
/// would point at the pytest process's working directory and fail to connect.
pub(crate) fn template_fields_in(upstream: &Upstream, port: u16, socket_dir: &str) -> Fields {
    let mut fields = Fields::new();
    fields.insert("socket_dir", socket_dir);
    fields.insert("port", port.to_string());
    fields.insert("socket_path", format!("{socket_dir}/.s.PGSQL.{port}"));
    fields.insert("database", upstream.database.clone());
    fields.insert("user", LOCAL_USER);
    fields.insert(
        "url",
        format!(
            "postgresql://{LOCAL_USER}@/{}?host={socket_dir}&port={port}",
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
