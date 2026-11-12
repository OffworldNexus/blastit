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

use std::collections::HashSet;
use std::fmt;
use std::io::{Read, Write};

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use blastit_sqlpolicy::{Allowlist, TableGrants};

use crate::error::DbproxyError;
use crate::upstream::SslPlan;

/// Largest configuration accepted, in bytes.
///
/// A manifest's grants are the bulk of it; 16 MiB is far above any real
/// manifest, and it keeps a bogus length prefix from allocating wildly.
pub const MAX_CONFIG_BYTES: usize = 16 * 1024 * 1024;

/// The route name a shared `--learn`/`check` listener uses.
///
/// One socket per proxy serves every route while dbproxy watches the wire: the
/// control channel ([`crate::ObsTarget`]) says which route, or which unrouted
/// request, the statements that follow belong to. The wildcard is accepted only
/// outside [`ListenerMode::Enforce`], where no policy decision is taken on it
/// (§4.2 D.3).
pub const WILDCARD_ROUTE: &str = "*";

/// What a listener does with the statements it sees.
///
/// The production runtime is [`ListenerMode::Enforce`] and nothing else:
/// [`ListenerMode::Learn`] (`blastit dev --learn`) and
/// [`ListenerMode::DynamicCheck`] (`blastit check`) exist so one pass of a suite
/// cannot be short-circuited by a missing grant (R26, ADR-0003, ADR-0004).
/// Both learning modes always answer `Allow` and report what they saw over
/// `obs_fd`; neither emits `sql.denied`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerMode {
    /// Refuse anything the grants do not allow, with `42501` (§6).
    #[default]
    Enforce,
    /// `blastit dev --learn`: allow everything, observe everything.
    Learn,
    /// `blastit check`: allow everything, observe everything, and switch the
    /// upstream database per run with `set_db`.
    DynamicCheck,
}

impl ListenerMode {
    /// Whether the listener takes policy decisions at all.
    ///
    /// A learning listener answers `Allow` unconditionally, so this is also
    /// "might emit `sql.denied`".
    pub fn enforces(self) -> bool {
        matches!(self, Self::Enforce)
    }

    /// Whether a shared listener may carry [`WILDCARD_ROUTE`].
    pub fn allows_wildcard_route(self) -> bool {
        !self.enforces()
    }
}

/// The effective grant of one (route, proxy) pair, as `blastit check` knows it
/// before any request has run.
///
/// A shared check listener has one socket per proxy and therefore cannot carry
/// the grants itself: this is the same subset of a [`ListenerSpec`] minus its
/// `fd` and `uid`, derived solely from `Manifest::effective_grants` (§4.2 D.3,
/// `I2-M4`). A listener whose route is [`WILDCARD_ROUTE`] evaluates against the
/// entry its current [`crate::ObsTarget`] names, and against an empty allowlist
/// when the name is unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutePolicySpec {
    /// Route name, exactly as the manifest writes it.
    pub route: String,
    /// Proxy the route reaches.
    pub proxy: String,
    /// The route's effective queries, matched exactly after normalisation.
    #[serde(default)]
    pub queries: Vec<String>,
    /// Whether the route may also run single transaction-control statements.
    #[serde(default)]
    pub transaction_control: bool,
    /// Whether the route may run *any* statement on this proxy.
    #[serde(default)]
    pub unrestricted: bool,
    /// The table, column and function grants of the route's effective grant.
    #[serde(default)]
    pub tables: TableGrants,
}

impl RoutePolicySpec {
    /// The allowlist this policy describes.
    pub(crate) fn allowlist(&self) -> Allowlist {
        if self.unrestricted {
            Allowlist::unrestricted()
        } else {
            Allowlist::new(&self.queries, self.transaction_control).with_tables(self.tables.clone())
        }
    }
}

/// Everything dbproxy needs: the proxies it may reach, and the listeners it
/// serves.
///
/// The listener at index *i* carries tag `i` in every hook call, which is how a
/// connection is tied back to its route and proxy.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DbproxyConfig {
    /// The upstream databases, by proxy name.
    pub proxies: Vec<ProxySpec>,
    /// The listeners init bound, in the order their fds were handed over.
    pub listeners: Vec<ListenerSpec>,
    /// What the listeners do with the statements they see.
    #[serde(default)]
    pub mode: ListenerMode,
    /// Whether a listener may declare `uid == 0`.
    ///
    /// Root is never a view (`ARCHITECTURE.md` §2.2), so a `uid == 0` listener
    /// is refused unless this is set. Only a root CI container — where the
    /// whole test process tree runs as root and there is no other uid to give a
    /// view — sets it (ADR-0001).
    #[serde(default)]
    pub allow_root_listener: bool,
    /// The effective grants of every route, for a shared learning listener
    /// (§4.2 D.3).
    #[serde(default)]
    pub route_policies: Vec<RoutePolicySpec>,
    /// The write end of the internal observation pipe (`--learn`/`check`).
    ///
    /// One JSON `WireSqlObs` per line, never on stdout: the public event stream
    /// stays limited to the documented events
    /// (`docs/reference/log-events.md`, ADR-0003). dbproxy owns the descriptor
    /// after the handover.
    #[serde(default)]
    pub obs_fd: Option<i32>,
    /// The dbproxy end of the JSON-line control socket (`push_routed`,
    /// `push_unrouted`, `pop`, `set_db`; `5s` deadline, `4096` B per line).
    ///
    /// dbproxy owns the descriptor after the handover.
    #[serde(default)]
    pub check_ctl_fd: Option<i32>,
}

impl DbproxyConfig {
    /// The enforcing runtime's configuration: the proxies init resolved, and the
    /// listeners it bound for them.
    ///
    /// Every listener comes from [`ListenerSpec::enforcing`]: at runtime the
    /// manifest's effective grants, carried on the listener, are the authority
    /// (§6).
    pub fn enforcing(proxies: Vec<ProxySpec>, listeners: Vec<ListenerSpec>) -> Self {
        Self {
            proxies,
            listeners,
            ..Self::default()
        }
    }

    /// Checks the parts of the configuration that do not depend on a socket:
    /// unique proxy names, known `sslmode` values, and listeners that name a
    /// declared proxy and a route.
    pub fn validate(&self) -> Result<(), DbproxyError> {
        self.validate_proxies()?;
        self.validate_serving(&self.listeners)?;
        self.validate_learning()
    }

    /// Checks the listeners that are actually served.
    ///
    /// [`run_with_listeners`](crate::run_with_listeners) is handed listeners
    /// that never appear in `self.listeners`, so the runtime validates the
    /// sockets it really has through this, and [`DbproxyConfig::validate`]
    /// validates `self.listeners` through it.
    pub fn validate_serving(&self, listeners: &[ListenerSpec]) -> Result<(), DbproxyError> {
        for (index, listener) in listeners.iter().enumerate() {
            self.validate_listener(listener, index)?;
        }
        // A learning listener is one socket per proxy, so a second wildcard
        // listener for the same proxy would serve the same rows twice.
        let mut wildcard_proxies = HashSet::new();
        for listener in listeners
            .iter()
            .filter(|listener| listener.route == WILDCARD_ROUTE)
        {
            if !wildcard_proxies.insert(listener.proxy.as_str()) {
                return Err(DbproxyError::Config(format!(
                    "two wildcard listeners reach proxy {:?}",
                    listener.proxy
                )));
            }
        }
        Ok(())
    }

    /// Checks everything that does not depend on a socket: the proxy table.
    pub fn validate_proxies(&self) -> Result<(), DbproxyError> {
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
        Ok(())
    }

    /// Checks one listener against the proxy table and the listener mode.
    ///
    /// This is the per-listener half of [`DbproxyConfig::validate`], called for
    /// the listeners that are actually served — by [`DbproxyConfig::validate`]
    /// for `self.listeners`, and by the runtime for the sockets it was handed
    /// directly.
    pub fn validate_listener(
        &self,
        listener: &ListenerSpec,
        index: usize,
    ) -> Result<(), DbproxyError> {
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
        if listener.route == WILDCARD_ROUTE && !self.mode.allows_wildcard_route() {
            return Err(DbproxyError::Config(format!(
                "listener {index} uses the wildcard route {WILDCARD_ROUTE:?}, which only a \
                 learning listener may carry (set mode to learn or dynamic_check)"
            )));
        }
        if listener.uid == 0 && !self.allow_root_listener {
            return Err(DbproxyError::Config(format!(
                "listener {index} (route {:?}) declares uid 0; root is never a view \
                 (allow_root_listener is only for a root CI container)",
                listener.route
            )));
        }
        Ok(())
    }

    /// Checks the learning-only parts of the configuration: the route policies
    /// and the two internal descriptors.
    pub fn validate_learning(&self) -> Result<(), DbproxyError> {
        for (name, fd) in [("obs_fd", self.obs_fd), ("check_ctl_fd", self.check_ctl_fd)] {
            match fd {
                Some(fd) if fd < 0 => {
                    return Err(DbproxyError::Config(format!(
                        "{name} is the negative descriptor {fd}"
                    )));
                }
                Some(_) if self.mode.enforces() => {
                    return Err(DbproxyError::Config(format!(
                        "{name} is only meaningful in learn or dynamic_check mode, not enforce"
                    )));
                }
                _ => {}
            }
        }
        for (index, policy) in self.route_policies.iter().enumerate() {
            if policy.route.is_empty() || policy.route == WILDCARD_ROUTE {
                return Err(DbproxyError::Config(format!(
                    "route policy {index} names {:?}, which is not a route name",
                    policy.route
                )));
            }
            if !self.proxies.iter().any(|p| p.name == policy.proxy) {
                return Err(DbproxyError::Config(format!(
                    "route policy {index} (route {:?}) names the unknown proxy {:?}",
                    policy.route, policy.proxy
                )));
            }
            if self.route_policies[..index]
                .iter()
                .any(|other| other.route == policy.route && other.proxy == policy.proxy)
            {
                return Err(DbproxyError::Config(format!(
                    "two route policies cover route {:?} on proxy {:?}",
                    policy.route, policy.proxy
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

impl ListenerSpec {
    /// A listener that enforces the grants it carries (§6).
    ///
    /// This is the runtime's listener mode: the route name, uid and grants here
    /// are the authority, exactly as the manifest's effective grants wrote them.
    ///
    /// `fd` starts at `0`. Init fills it in once the listener is bound, because
    /// the descriptor number the child sees is only known after the fork.
    pub fn enforcing(
        route: impl Into<String>,
        proxy: impl Into<String>,
        uid: u32,
        queries: Vec<String>,
        transaction_control: bool,
        unrestricted: bool,
        tables: TableGrants,
    ) -> Self {
        Self {
            fd: 0,
            route: route.into(),
            proxy: proxy.into(),
            uid,
            queries,
            transaction_control,
            unrestricted,
            tables,
        }
    }
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
///     ..DbproxyConfig::default()
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
            ..DbproxyConfig::default()
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
    fn a_wildcard_listener_belongs_to_a_learning_mode_only() {
        let mut learning = config();
        learning.listeners[0].route = WILDCARD_ROUTE.to_string();
        assert!(
            learning
                .validate()
                .expect_err("enforce has no wildcard listener")
                .to_string()
                .contains("wildcard"),
            "the message names the wildcard route"
        );

        learning.mode = ListenerMode::DynamicCheck;
        assert!(learning.validate().is_ok());

        // One socket per proxy: a second wildcard listener for the same proxy
        // would serve the same rows twice.
        let mut duplicated = learning.clone();
        let mut second = duplicated.listeners[1].clone();
        second.route = WILDCARD_ROUTE.to_string();
        duplicated.listeners.push(second);
        assert!(
            duplicated
                .validate()
                .expect_err("two wildcard listeners on one proxy")
                .to_string()
                .contains("two wildcard listeners")
        );

        learning.mode = ListenerMode::Learn;
        assert!(learning.validate().is_ok());
    }

    #[test]
    fn a_root_listener_needs_the_explicit_opt_in() {
        let mut root = config();
        root.listeners[0].uid = 0;
        assert!(
            root.validate()
                .expect_err("root is not a view")
                .to_string()
                .contains("uid 0")
        );
        root.allow_root_listener = true;
        assert!(root.validate().is_ok());
        // The opt-in does not loosen any other listener.
        root.listeners[1].uid = 0;
        assert!(root.validate().is_ok());
    }

    #[test]
    fn the_two_internal_descriptors_belong_to_a_learning_mode() {
        let mut enforce = config();
        enforce.obs_fd = Some(3);
        assert!(
            enforce
                .validate()
                .expect_err("obs_fd in enforce")
                .to_string()
                .contains("only meaningful")
        );
        enforce.obs_fd = None;
        enforce.check_ctl_fd = Some(3);
        assert!(
            enforce
                .validate()
                .expect_err("ctl fd in enforce")
                .to_string()
                .contains("only meaningful")
        );

        enforce.check_ctl_fd = None;
        enforce.mode = ListenerMode::Learn;
        enforce.obs_fd = Some(3);
        enforce.check_ctl_fd = Some(4);
        assert!(enforce.validate().is_ok(), "learning mode takes both");

        let mut negative = enforce.clone();
        negative.obs_fd = Some(-1);
        assert!(
            negative
                .validate()
                .expect_err("a negative descriptor")
                .to_string()
                .contains("negative")
        );
    }

    #[test]
    fn route_policies_name_a_real_route_on_a_declared_proxy_once() {
        let mut config = config();
        config.listeners[0].route = WILDCARD_ROUTE.to_string();
        config.mode = ListenerMode::DynamicCheck;
        let policy = RoutePolicySpec {
            route: "product-detail".to_string(),
            proxy: "db".to_string(),
            queries: vec!["SELECT 1".to_string()],
            ..RoutePolicySpec::default()
        };
        config.route_policies = vec![policy.clone()];
        assert!(config.validate().is_ok());

        let mut duplicated = config.clone();
        duplicated.route_policies.push(policy.clone());
        assert!(
            duplicated
                .validate()
                .expect_err("two policies for one pair")
                .to_string()
                .contains("two route policies")
        );

        let mut unknown = config.clone();
        unknown.route_policies[0].proxy = "nope".to_string();
        assert!(
            unknown
                .validate()
                .expect_err("an unknown proxy")
                .to_string()
                .contains("unknown proxy")
        );

        let mut wildcard = config.clone();
        wildcard.route_policies[0].route = WILDCARD_ROUTE.to_string();
        assert!(
            wildcard
                .validate()
                .expect_err("a wildcard route is not a route")
                .to_string()
                .contains("not a route name")
        );

        let mut empty = config;
        empty.route_policies[0].route = String::new();
        assert!(
            empty
                .validate()
                .expect_err("an empty route")
                .to_string()
                .contains("not a route name")
        );
    }

    #[test]
    fn the_listener_mode_round_trips_through_its_wire_spelling() {
        for (mode, spelling) in [
            (ListenerMode::Enforce, "\"enforce\""),
            (ListenerMode::Learn, "\"learn\""),
            (ListenerMode::DynamicCheck, "\"dynamic_check\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).expect("serialised"), spelling);
            assert_eq!(
                serde_json::from_str::<ListenerMode>(spelling).expect("deserialised"),
                mode
            );
        }
        assert_eq!(ListenerMode::default(), ListenerMode::Enforce);
        assert!(ListenerMode::Enforce.enforces());
        assert!(!ListenerMode::Learn.enforces());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{"name":"db","host":"h","port":1,"user":"u","password":"p","database":"d","max_connections":1,"nope":true}"#;
        let error = serde_json::from_str::<ProxySpec>(json).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}
