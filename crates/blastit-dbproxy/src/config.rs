//! dbproxy's configuration, as init writes it across `secrets.pipe`
//! (`ARCHITECTURE.md` §3, §4).
//!
//! dbproxy is `execve`'d with an empty environment, so everything it knows
//! arrives here: the real upstream credentials of every `postgresql` proxy, and
//! one entry per listener init bound (its fd, which route it belongs to, which
//! view uid may use it, and the SQL that route may run).
//!
//! The wire format is a `u32` big-endian length followed by that many bytes of
//! JSON ([`write_config`] / [`read_config`]). Both the bytes written and the
//! bytes read are held in [`Zeroizing`] buffers, so a plaintext password lives
//! in main memory only for as long as the value that uses it, and
//! [`ProxySpec`]'s [`Debug`] implementation redacts passwords and CA bundles.

use std::fmt;
use std::io::{Read, Write};

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use blastit_sqlpolicy::TableGrants;

use crate::error::DbproxyError;
use crate::upstream::SslPlan;

/// Largest configuration accepted, in bytes.
///
/// A manifest's grants are the bulk of it; 16 MiB is far above any real
/// manifest, and it keeps a bogus length prefix from allocating wildly.
pub const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

/// Everything dbproxy needs: the proxies it may reach, and the listeners it
/// serves.
///
/// The listener at index *i* carries tag `i` in every hook call, which is how a
/// connection is tied back to its route and proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbproxyConfig {
    /// The upstream databases, by proxy name.
    pub proxies: Vec<ProxySpec>,
    /// The listeners init bound, in the order their fds were handed over.
    pub listeners: Vec<ListenerSpec>,
}

impl DbproxyConfig {
    /// Checks the parts of the configuration that do not depend on a socket:
    /// unique proxy names, known `sslmode` values, and listeners that name a
    /// declared proxy and a route.
    pub fn validate(&self) -> Result<(), DbproxyError> {
        for (index, proxy) in self.proxies.iter().enumerate() {
            if proxy.name.is_empty() {
                return Err(DbproxyError::Config(format!("proxy {index} has no name")));
            }
            if proxy.name == "all" || proxy.name == "default" {
                return Err(DbproxyError::Config(format!(
                    "proxy name {:?} is reserved",
                    proxy.name
                )));
            }
            if self.proxies[..index]
                .iter()
                .any(|other| other.name == proxy.name)
            {
                return Err(DbproxyError::Config(format!(
                    "two proxies are named {:?}",
                    proxy.name
                )));
            }
            SslPlan::parse(proxy.sslmode.as_deref())?;
        }
        for (index, listener) in self.listeners.iter().enumerate() {
            if listener.route.is_empty() {
                return Err(DbproxyError::Config(format!(
                    "listener {index} has no route"
                )));
            }
            if !self.proxies.iter().any(|p| p.name == listener.proxy) {
                return Err(DbproxyError::Config(format!(
                    "listener {index} (route {:?}) names the unknown proxy {:?}",
                    listener.route, listener.proxy
                )));
            }
        }
        Ok(())
    }
}

/// One `postgresql` proxy's real upstream identity.
///
/// The password and the CA bundle are the two secrets of the pair: both are
/// redacted by [`Debug`], and neither ever appears in an error, an event or a
/// message to a view.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxySpec {
    /// Proxy name from the manifest (`[proxy.<name>]`).
    pub name: String,
    /// Upstream host.
    pub host: String,
    /// Upstream port.
    pub port: u16,
    /// Role dbproxy authenticates as upstream.
    pub user: String,
    /// Password for that role.
    pub password: Zeroizing<String>,
    /// Database to request upstream.
    pub database: String,
    /// libpq `sslmode` (`disable`, `allow`, `prefer`, `require`, `verify-ca`,
    /// `verify-full`); `None` is libpq's default, `prefer`.
    pub sslmode: Option<String>,
    /// PEM bundle of CAs to verify the upstream server against, when the mode
    /// asks for verification.
    pub ca_cert: Option<Zeroizing<String>>,
    /// Extra upstream startup parameters, sent in addition to the proxy's
    /// identity. `user` and `database` here are ignored: the client can never
    /// steer who dbproxy logs in as.
    pub options: Vec<(String, String)>,
    /// Maximum number of upstream sessions this proxy may hold at once.
    /// `0` refuses every new connection.
    pub max_connections: u32,
}

impl fmt::Debug for ProxySpec {
    /// Prints everything but the password and the CA bundle.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxySpec")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &Redacted)
            .field("database", &self.database)
            .field("sslmode", &self.sslmode)
            .field("ca_cert", &self.ca_cert.as_ref().map(|_| Redacted))
            .field("options", &self.options)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

/// The placeholder [`Debug`] prints instead of a credential.
#[derive(Clone, Copy)]
struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// One listener init bound for one (route, proxy) pair.
///
/// The listener's file permissions (and the directory's) already restrict who
/// may `connect()`; [`ListenerSpec::uid`] is the second check, against the
/// `SCM_CREDENTIALS` the kernel attaches to the client's first message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerSpec {
    /// The listening socket's fd, inherited across dbproxy's `execve`. Ignored
    /// by [`run_with_listeners`](crate::run_with_listeners), which is handed
    /// listeners that are already owned.
    pub fd: i32,
    /// Route this listener belongs to (a manifest route name).
    pub route: String,
    /// Proxy this listener reaches.
    pub proxy: String,
    /// The only uid allowed to use this listener. `0` matches nothing: root is
    /// never a view, so a root client is refused like any other stranger.
    pub uid: u32,
    /// The route's effective queries (§6), matched exactly after
    /// normalisation.
    pub queries: Vec<String>,
    /// Whether the route may also run single transaction-control statements.
    pub transaction_control: bool,
    /// Whether the route may run *any* statement on this proxy (§6,
    /// `unrestricted = true`). The route still reaches the database through
    /// dbproxy, and still never sees the credentials.
    #[serde(default)]
    pub unrestricted: bool,
    /// The table, column and function grants of the route's effective grant
    /// (§6, D7). A statement no query lists is parsed with PostgreSQL's parser
    /// and allowed when every table, column and function it touches is here.
    #[serde(default)]
    pub tables: TableGrants,
}

/// Writes `cfg` as a length-prefixed JSON document.
///
/// `u32` big-endian length of the JSON, then the JSON itself. The JSON is built
/// in a [`Zeroizing`] buffer, because it holds every proxy's password.
pub fn write_config<W: Write>(cfg: &DbproxyConfig, mut out: W) -> Result<(), DbproxyError> {
    let json = Zeroizing::new(serde_json::to_vec(cfg).map_err(config_error)?);
    let length = u32::try_from(json.len()).map_err(|_| {
        DbproxyError::Config(format!(
            "the configuration is {} bytes, more than the {MAX_CONFIG_BYTES}-byte limit",
            json.len()
        ))
    })?;
    if json.len() > MAX_CONFIG_BYTES {
        return Err(DbproxyError::Config(format!(
            "the configuration is {} bytes, more than the {MAX_CONFIG_BYTES}-byte limit",
            json.len()
        )));
    }
    out.write_all(&length.to_be_bytes())?;
    out.write_all(&json)?;
    out.flush()?;
    Ok(())
}

/// Reads what [`write_config`] wrote, reading exactly one document.
///
/// The bytes read are held in a [`Zeroizing`] buffer that is wiped once the
/// document has been parsed. A length of zero, or one above
/// [`MAX_CONFIG_BYTES`], is refused before any allocation happens.
///
/// ```
/// use blastit_dbproxy::{read_config, write_config, DbproxyConfig, ProxySpec};
///
/// let config = DbproxyConfig {
///     proxies: vec![ProxySpec {
///         name: "db".to_string(),
///         host: "db.internal".to_string(),
///         port: 25060,
///         user: "doadmin".to_string(),
///         password: "the-real-one".to_string().into(),
///         database: "defaultdb".to_string(),
///         sslmode: Some("require".to_string()),
///         ca_cert: None,
///         options: vec![("application_name".to_string(), "blastit".to_string())],
///         max_connections: 8,
///     }],
///     listeners: Vec::new(),
/// };
///
/// let mut bytes = Vec::new();
/// write_config(&config, &mut bytes).expect("written");
/// assert_eq!(read_config(std::io::Cursor::new(bytes)).expect("read"), config);
/// ```
pub fn read_config<R: Read>(mut input: R) -> Result<DbproxyConfig, DbproxyError> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 {
        return Err(DbproxyError::Config(
            "the configuration document is empty".to_string(),
        ));
    }
    if length > MAX_CONFIG_BYTES {
        return Err(DbproxyError::Config(format!(
            "the configuration claims {length} bytes, more than the {MAX_CONFIG_BYTES}-byte limit"
        )));
    }
    let mut buffer = Zeroizing::new(vec![0u8; length]);
    input.read_exact(&mut buffer)?;
    let config: DbproxyConfig = serde_json::from_slice(&buffer).map_err(config_error)?;
    Ok(config)
}

/// Turns a `serde_json` failure into an error that names no value: position
/// information only, never a snippet of the document.
fn config_error(error: serde_json::Error) -> DbproxyError {
    DbproxyError::Config(format!(
        "the configuration is not valid JSON ({error}; line {}, column {})",
        error.line(),
        error.column()
    ))
}

impl Serialize for ProxySpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ProxySpec", FIELD_COUNT)?;
        map.serialize_field("name", &self.name)?;
        map.serialize_field("host", &self.host)?;
        map.serialize_field("port", &self.port)?;
        map.serialize_field("user", &self.user)?;
        map.serialize_field("password", self.password.as_str())?;
        map.serialize_field("database", &self.database)?;
        map.serialize_field("sslmode", &self.sslmode)?;
        map.serialize_field("ca_cert", &self.ca_cert.as_ref().map(|pem| pem.as_str()))?;
        map.serialize_field("options", &self.options)?;
        map.serialize_field("max_connections", &self.max_connections)?;
        map.end()
    }
}

/// Number of fields [`ProxySpec`] serialises.
const FIELD_COUNT: usize = 10;

/// The field names [`ProxySpec`] accepts, for error messages.
const FIELDS: &[&str] = &[
    "name",
    "host",
    "port",
    "user",
    "password",
    "database",
    "sslmode",
    "ca_cert",
    "options",
    "max_connections",
];

/// A JSON string whose value must never be echoed in an error message.
struct Secret(String);

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SecretVisitor;

        impl Visitor<'_> for SecretVisitor {
            type Value = Secret;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Secret, E> {
                Ok(Secret(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Secret, E> {
                Ok(Secret(value))
            }

            // Numbers carry their value in serde's own error rendering, so
            // they are refused with dbproxy's own wording instead.
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Secret, E> {
                Err(E::custom("the value must be a JSON string"))
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Secret, E> {
                Err(E::custom("the value must be a JSON string"))
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Secret, E> {
                Err(E::custom("the value must be a JSON string"))
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Secret, E> {
                Err(E::custom("the value must be a JSON string"))
            }
        }

        deserializer.deserialize_any(SecretVisitor)
    }
}

impl<'de> Deserialize<'de> for ProxySpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ProxyVisitor;

        impl<'de> Visitor<'de> for ProxyVisitor {
            type Value = ProxySpec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a dbproxy proxy table")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ProxySpec, A::Error> {
                let mut name: Option<String> = None;
                let mut host: Option<String> = None;
                let mut port: Option<u16> = None;
                let mut user: Option<String> = None;
                let mut password: Option<Zeroizing<String>> = None;
                let mut database: Option<String> = None;
                let mut sslmode: Option<String> = None;
                let mut ca_cert: Option<Zeroizing<String>> = None;
                let mut options: Option<Vec<(String, String)>> = None;
                let mut max_connections: Option<u32> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "name" => name = Some(map.next_value()?),
                        "host" => host = Some(map.next_value()?),
                        "port" => port = Some(map.next_value()?),
                        "user" => user = Some(map.next_value()?),
                        "password" => {
                            password = Some(Zeroizing::new(map.next_value::<Secret>()?.0));
                        }
                        "database" => database = Some(map.next_value()?),
                        "sslmode" => sslmode = map.next_value()?,
                        "ca_cert" => {
                            ca_cert = map
                                .next_value::<Option<Secret>>()?
                                .map(|secret| Zeroizing::new(secret.0));
                        }
                        "options" => options = Some(map.next_value()?),
                        "max_connections" => max_connections = Some(map.next_value()?),
                        other => return Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }

                Ok(ProxySpec {
                    name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                    host: host.ok_or_else(|| de::Error::missing_field("host"))?,
                    port: port.ok_or_else(|| de::Error::missing_field("port"))?,
                    user: user.ok_or_else(|| de::Error::missing_field("user"))?,
                    password: password.ok_or_else(|| de::Error::missing_field("password"))?,
                    database: database.ok_or_else(|| de::Error::missing_field("database"))?,
                    sslmode,
                    ca_cert,
                    options: options.unwrap_or_default(),
                    max_connections: max_connections
                        .ok_or_else(|| de::Error::missing_field("max_connections"))?,
                })
            }
        }

        deserializer.deserialize_struct("ProxySpec", FIELDS, ProxyVisitor)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A password that must never show up in output.
    const SENTINEL: &str = "sentinel-password-3f9a";
    /// A CA that must never show up in output.
    const CA_SENTINEL: &str = "sentinel-ca-7b2c";

    fn config() -> DbproxyConfig {
        DbproxyConfig {
            proxies: vec![
                ProxySpec {
                    name: "db".to_string(),
                    host: "db.internal".to_string(),
                    port: 25060,
                    user: "doadmin".to_string(),
                    password: Zeroizing::new(SENTINEL.to_string()),
                    database: "defaultdb".to_string(),
                    sslmode: Some("require".to_string()),
                    ca_cert: Some(Zeroizing::new(format!(
                        "-----BEGIN CERTIFICATE-----\n{CA_SENTINEL}\n-----END CERTIFICATE-----"
                    ))),
                    options: vec![("application_name".to_string(), "blastit".to_string())],
                    max_connections: 8,
                },
                ProxySpec {
                    name: "analytics".to_string(),
                    host: "analytics.internal".to_string(),
                    port: 25060,
                    user: "reader".to_string(),
                    password: Zeroizing::new(SENTINEL.to_string()),
                    database: "analytics".to_string(),
                    sslmode: None,
                    ca_cert: None,
                    options: vec![],
                    max_connections: 0,
                },
            ],
            listeners: vec![
                ListenerSpec {
                    fd: 3,
                    route: "product-detail".to_string(),
                    proxy: "db".to_string(),
                    uid: 21_000,
                    queries: vec!["SELECT 1".to_string()],
                    transaction_control: false,
                    unrestricted: false,
                    tables: TableGrants::default(),
                },
                ListenerSpec {
                    fd: 4,
                    route: "checkout".to_string(),
                    proxy: "db".to_string(),
                    uid: 21_001,
                    queries: vec![],
                    transaction_control: true,
                    unrestricted: false,
                    tables: TableGrants::default(),
                },
            ],
        }
    }

    #[test]
    fn round_trips_through_a_pipe() {
        let cfg = config();
        let (reader, mut writer) = std::io::pipe().expect("a pipe");
        let expected = cfg.clone();
        let handle = std::thread::spawn(move || write_config(&expected, &mut writer));
        let read = read_config(reader).expect("the configuration round-trips");
        assert_eq!(read, cfg);
        assert!(handle.join().expect("the writer thread").is_ok());
    }

    #[test]
    fn round_trips_through_a_byte_sink() {
        let cfg = config();
        let mut sink = Vec::new();
        write_config(&cfg, &mut sink).expect("written");
        assert_eq!(
            u32::from_be_bytes(sink[..4].try_into().expect("four bytes")) as usize,
            sink.len() - 4
        );
        assert_eq!(read_config(std::io::Cursor::new(sink)).expect("read"), cfg);
    }

    #[test]
    fn debug_redacts_passwords_and_cas() {
        let printed = format!("{:?}", config());
        assert!(!printed.contains(SENTINEL), "{printed}");
        assert!(!printed.contains(CA_SENTINEL), "{printed}");
        assert!(printed.contains("<redacted>"), "{printed}");
        assert!(printed.contains("db.internal"), "{printed}");
    }

    #[test]
    fn serialised_json_carries_the_secrets_because_init_has_to_hand_them_over() {
        let json = serde_json::to_string(&config().proxies[0]).expect("serialised");
        assert!(json.contains(SENTINEL));
        assert!(json.contains(CA_SENTINEL));
    }

    #[test]
    fn a_bad_document_never_echoes_a_value_from_it() {
        // The sentinel appears as the wrong JSON type, and as a truncated
        // document, but neither error may repeat it.
        let document = format!(r#"{{"password": "{SENTINEL}","#).into_bytes();
        let mut framed = (document.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&document);
        let error = read_config(std::io::Cursor::new(framed)).expect_err("malformed");
        assert!(!error.to_string().contains(SENTINEL), "{error}");

        let document = br#"{"name": "db", "password": 12349876}"#;
        // The document's shape is wrong before `DbproxyConfig` is even reached,
        // so the wrong type is checked directly.
        let error = serde_json::from_slice::<ProxySpec>(document).expect_err("wrong type");
        assert!(
            error.to_string().contains("must be a JSON string"),
            "{error}"
        );
        assert!(!error.to_string().contains("1234"), "{error}");

        // A document that is not JSON at all reports a position, not a snippet.
        let document = format!("not json at all: {SENTINEL}").into_bytes();
        let mut framed = (document.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&document);
        let error = read_config(std::io::Cursor::new(framed)).expect_err("not json");
        assert!(!error.to_string().contains(SENTINEL), "{error}");
    }

    #[test]
    fn refuses_documents_that_are_too_large_or_empty() {
        let error = read_config(std::io::Cursor::new(0u32.to_be_bytes())).expect_err("empty");
        assert!(error.to_string().contains("empty"), "{error}");

        let too_large = u32::try_from(MAX_CONFIG_BYTES + 1)
            .expect("fits")
            .to_be_bytes();
        let error = read_config(std::io::Cursor::new(too_large)).expect_err("too large");
        assert!(error.to_string().contains("limit"), "{error}");
    }

    #[test]
    fn validates_proxies_and_listeners() {
        assert!(config().validate().is_ok());

        let mut duplicated = config();
        duplicated.proxies[1].name = "db".to_string();
        assert!(
            duplicated
                .validate()
                .expect_err("duplicate")
                .to_string()
                .contains("two proxies")
        );

        let mut unknown = config();
        unknown.listeners[0].proxy = "nope".to_string();
        assert!(
            unknown
                .validate()
                .expect_err("unknown proxy")
                .to_string()
                .contains("unknown proxy")
        );

        let mut bad_mode = config();
        bad_mode.proxies[0].sslmode = Some("perhaps".to_string());
        assert!(
            bad_mode
                .validate()
                .expect_err("bad sslmode")
                .to_string()
                .contains("unknown sslmode")
        );

        let mut no_route = config();
        no_route.listeners[0].route = String::new();
        assert!(
            no_route
                .validate()
                .expect_err("no route")
                .to_string()
                .contains("no route")
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{"name":"db","host":"h","port":1,"user":"u","password":"p","database":"d","max_connections":1,"nope":true}"#;
        let error = serde_json::from_str::<ProxySpec>(json).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}
