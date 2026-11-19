//! The stage-1 document's reading half: the JSON contract as `serde` structs.
//!
//! Stage 0 writes the document with [`Config::to_json`], and stage 1 rebuilds
//! the same [`Config`] from it after the `execve` barrier (§4). The field names,
//! the nesting, the optional members and the integer widths here are that
//! contract, so they are spelled once — as a derived [`WireConfig`] — instead of
//! once per field in a hand-written accessor.
//!
//! A `serde` failure is turned back into the [`ConfigError::Document`] the
//! accessors reported: each reader in [`read`] writes the field's own name and
//! the sentence it always gave, so no `serde_json` wording reaches an operator
//! (§8.1).

mod field;
mod read;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::{Deserializer, Error as _};
use serde_json::Value;

#[cfg(feature = "php")]
use blastit_view_php::discovery::{PhpRuntime, PhpVersion};

use self::field::*;
use self::read::*;
use super::document;
use super::preset::PresetChoice;
use super::settings::Config;
use super::types::{ConfigError, Privilege};

/// The document [`Config::to_json`] writes, read back with `serde`.
///
/// Every field name is the JSON key, so nothing is renamed. A member the
/// document only sometimes carries is `#[serde(default)]`: the two PHP settings
/// predate the first documents, `php` is absent when nothing was resolved, and
/// D6's capability names and bounding-set fact are only written once stage 0 has
/// detected them.
#[derive(Deserialize)]
struct WireConfig {
    port: WireU16<PortField>,
    server_port: WireU16<ServerPortField>,
    app: OptText<AppField>,
    preload: Names<PreloadField>,
    manifest: Text<ManifestField>,
    manifest_dir: Text<ManifestDirField>,
    #[serde(default)]
    libphp: OptText<LibphpField>,
    #[serde(default)]
    php_shim_dir: OptText<PhpShimDirField>,
    #[cfg(feature = "php")]
    #[serde(default)]
    php: PhpMember,
    script_name: OptText<ScriptNameField>,
    log_format: LogFmt<LogFormatField>,
    log_level: LevelText<LogLevelField>,
    uid_base: WireU32<UidBaseField>,
    max_workers: WireU32<MaxWorkersField>,
    view_max_workers: OptU32<ViewMaxWorkersField>,
    queue_size: WireUsize<QueueSizeField>,
    queue_timeout_ms: Ms<QueueTimeoutMsField>,
    request_timeout_ms: Ms<RequestTimeoutMsField>,
    stuck_threads: WireU32<StuckThreadsField>,
    view_spawn: Spawn<ViewSpawnField>,
    view_idle_timeout_ms: Ms<ViewIdleTimeoutMsField>,
    view_max_requests: WireU64<ViewMaxRequestsField>,
    max_view_processes: OptU32<MaxViewProcessesField>,
    view_mem_estimate: WireU64<ViewMemEstimateField>,
    mem_high_watermark_percent: Float<MemHighWatermarkPercentField>,
    immortalize: Immortal<ImmortalizeField>,
    max_header_bytes: WireUsize<MaxHeaderBytesField>,
    max_body_bytes: WireU64<MaxBodyBytesField>,
    header_timeout_ms: Ms<HeaderTimeoutMsField>,
    keepalive_timeout_ms: Ms<KeepaliveTimeoutMsField>,
    fair_queuing: Bool<FairQueuingField>,
    fq_outer_prefix_v4: WireU8<FqOuterPrefixV4Field>,
    fq_outer_prefix_v6: WireU8<FqOuterPrefixV6Field>,
    fq_outer_max: WireUsize<FqOuterMaxField>,
    fq_inner_max_per_outer: WireUsize<FqInnerMaxPerOuterField>,
    fq_inner_max: WireUsize<FqInnerMaxField>,
    fq_burst_ms: Ms<FqBurstMsField>,
    fq_max_debt_ms: Ms<FqMaxDebtMsField>,
    fq_debt_ttl_ms: Ms<FqDebtTtlMsField>,
    fq_inner_headers: Names<FqInnerHeadersField>,
    fq_inner_cookies: Names<FqInnerCookiesField>,
    fq_inner_include_ip: Bool<FqInnerIncludeIpField>,
    client_ip_header: Text<ClientIpHeaderField>,
    trusted_proxies: Names<TrustedProxiesField>,
    warnings: Warns<WarningsField>,
    health_path: Text<HealthPathField>,
    proxy_max_connections: Option<ConnCaps<ProxyMaxConnectionsField>>,
    env_strip: OptText<EnvStripField>,
    seccomp: Bool<SeccompField>,
    graceful_timeout_ms: Ms<GracefulTimeoutMsField>,
    warmup: OptText<WarmupField>,
    frame_max: WireUsize<FrameMaxField>,
    backlog: WireI32<BacklogField>,
    max_connections: WireUsize<MaxConnectionsField>,
    proxy_outputs: NameMap<ProxyOutputsField>,
    #[serde(default)]
    privilege: OptText<PrivilegeField>,
    #[serde(default)]
    capabilities: OptNames<CapabilitiesField>,
    #[serde(default)]
    bounding_set: OptBool<BoundingSetField>,
    sys_path: Bytes<SysPathField>,
    cwd: OptBytes<CwdField>,
    preset: PresetMember,
}

impl Config {
    /// Rebuilds the configuration stage 0 wrote with [`Config::to_json`].
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a field that is missing or has the wrong
    /// type; the message names the field, never a value.
    pub fn from_json(value: &Value) -> Result<Self, ConfigError> {
        if !value.is_object() {
            // The accessors read `port` first, and `get` on a document that is
            // not an object reports exactly this.
            return Err(document("port", "the field is missing".to_string()));
        }
        let wire = WireConfig::deserialize(value).map_err(document_error)?;
        wire.into_config()
    }
}

impl WireConfig {
    // Each field is one member of the configuration, and the document cannot
    // carry a struct literal, so the default is what every field falls back to.
    #[allow(clippy::field_reassign_with_default)]
    fn into_config(self) -> Result<Config, ConfigError> {
        let mut config = Config::default();
        config.port = self.port.0;
        config.server_port = self.server_port.0;
        config.app = self.app.0;
        config.preload = self.preload.0;
        config.manifest = PathBuf::from(self.manifest.0);
        config.manifest_dir = PathBuf::from(self.manifest_dir.0);
        config.libphp = self.libphp.0.map(PathBuf::from);
        config.php_shim_dir = self.php_shim_dir.0.map(PathBuf::from);
        #[cfg(feature = "php")]
        {
            config.php = match self.php.0 {
                None => None,
                Some(php) => Some(php.into_runtime()?),
            };
        }
        config.script_name = self.script_name.0;
        config.log_format = self.log_format.0;
        config.log_level = self.log_level.0;
        config.uid_base = self.uid_base.0;
        config.max_workers = self.max_workers.0;
        config.view_max_workers = self.view_max_workers.0;
        config.queue_size = self.queue_size.0;
        config.queue_timeout = self.queue_timeout_ms.0;
        config.request_timeout = self.request_timeout_ms.0;
        config.stuck_threads = self.stuck_threads.0;
        config.view_spawn = self.view_spawn.0;
        config.view_idle_timeout = self.view_idle_timeout_ms.0;
        config.view_max_requests = self.view_max_requests.0;
        config.max_view_processes = self.max_view_processes.0;
        config.view_mem_estimate = self.view_mem_estimate.0;
        config.mem_high_watermark_percent = self.mem_high_watermark_percent.0;
        config.immortalize = self.immortalize.0;
        config.max_header_bytes = self.max_header_bytes.0;
        config.max_body_bytes = self.max_body_bytes.0;
        config.header_timeout = self.header_timeout_ms.0;
        config.keepalive_timeout = self.keepalive_timeout_ms.0;
        config.fair_queuing = self.fair_queuing.0;
        config.fq_outer_prefix_v4 = self.fq_outer_prefix_v4.0;
        config.fq_outer_prefix_v6 = self.fq_outer_prefix_v6.0;
        config.fq_outer_max = self.fq_outer_max.0;
        config.fq_inner_max_per_outer = self.fq_inner_max_per_outer.0;
        config.fq_inner_max = self.fq_inner_max.0;
        config.fq_burst = self.fq_burst_ms.0;
        config.fq_max_debt = self.fq_max_debt_ms.0;
        config.fq_debt_ttl = self.fq_debt_ttl_ms.0;
        config.fq_inner_headers = self.fq_inner_headers.0;
        config.fq_inner_cookies = self.fq_inner_cookies.0;
        config.fq_inner_include_ip = self.fq_inner_include_ip.0;
        config.client_ip_header = self.client_ip_header.0;
        config.trusted_proxies = self.trusted_proxies.0;
        config.warnings = self.warnings.0;
        config.health_path = self.health_path.0;
        config.env_strip = self.env_strip.0;
        config.seccomp = self.seccomp.0;
        config.graceful_timeout = self.graceful_timeout_ms.0;
        config.warmup = self.warmup.0;
        config.frame_max = self.frame_max.0;
        config.backlog = self.backlog.0;
        config.max_connections = self.max_connections.0;
        config.proxy_outputs = self.proxy_outputs.0;
        config.proxy_max_connections = match self.proxy_max_connections {
            Some(caps) => caps.0,
            // The accessor read this member with `get`: a document without it
            // is a document whose member is not an object, and it said so.
            None => {
                return Err(document(
                    "proxy_max_connections",
                    "must be an object".to_string(),
                ));
            }
        };
        config.privilege =
            Privilege::from_parts(self.privilege.0, self.capabilities.0, self.bounding_set.0)?;
        config.preset = self.preset.0;
        config.sys_path = self.sys_path.0;
        config.cwd = self.cwd.0;
        Ok(config)
    }
}

/// The `preset` member: `null`, or the preset stage 0 settled on (E1).
///
/// A member of its own rather than an `Option`, because the document always
/// carries it: the accessors reported a document without it as a missing field,
/// and stage 0 cannot write one.
struct PresetMember(Option<PresetChoice>);

impl<'de> Deserialize<'de> for PresetMember {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self(None));
        }
        PresetWire::deserialize(&value)
            .map(|preset| Self(Some(preset.0)))
            .map_err(|error| D::Error::custom(error.to_string()))
    }
}

/// The `preset` member of the stage 1 document (E1).
///
/// The name is carried as text and is **not** looked up in [`PRESETS`]: what
/// stage 1 reports is what stage 0 decided, so a document written by a build
/// with a preset this one does not know is still a document stage 1 can boot
/// from — and a preset that was removed from the table cannot silently change
/// which defaults a running server was configured with.
///
/// [`PRESETS`]: crate::config::PRESETS
struct PresetWire(PresetChoice);

impl<'de> Deserialize<'de> for PresetWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("preset: must be an object or null"))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| D::Error::custom("preset: must name the preset"))?
            .to_string();
        let supplied = object
            .get("supplied")
            .and_then(Value::as_object)
            .ok_or_else(|| D::Error::custom("preset: `supplied` must be an object"))?
            .iter()
            .map(|(variable, value)| {
                let value = value.as_str().ok_or_else(|| {
                    D::Error::custom("preset: every supplied value must be a string")
                })?;
                Ok((variable.clone(), value.to_string()))
            })
            .collect::<Result<BTreeMap<String, String>, D::Error>>()?;
        let overridden = object
            .get("overridden")
            .and_then(Value::as_array)
            .ok_or_else(|| D::Error::custom("preset: `overridden` must be an array"))?
            .iter()
            .map(|name| {
                name.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| D::Error::custom("preset: `overridden` must name variables"))
            })
            .collect::<Result<Vec<String>, D::Error>>()?;
        Ok(Self(PresetChoice {
            name,
            supplied,
            overridden,
        }))
    }
}

/// The `php` member: absent, `null`, or the resolved backend.
///
/// A member of its own rather than an `Option`, so a document that carries
/// something else there is refused in this module's words — never with `serde`'s
/// idea of the Rust type behind the member.
#[cfg(feature = "php")]
#[derive(Default)]
struct PhpMember(Option<PhpWire>);

#[cfg(feature = "php")]
impl<'de> Deserialize<'de> for PhpMember {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self(None));
        }
        if !value.is_object() {
            return Err(D::Error::custom("php: must be an object or null"));
        }
        PhpWire::deserialize(&value)
            .map(|php| Self(Some(php)))
            .map_err(|error| D::Error::custom(error.to_string()))
    }
}

/// The resolved backend `ADR-0010` §3 records: the path, the inode identity and
/// the matching shim, so stage 1 re-opens the file stage 0 inspected.
#[cfg(feature = "php")]
#[derive(Deserialize)]
struct PhpWire {
    libphp: Text<LibphpField>,
    dev: WireU64<PhpDevField>,
    ino: WireU64<PhpInoField>,
    version: Text<PhpVersionField>,
    shim: Text<PhpShimField>,
    shim_dev: WireU64<PhpShimDevField>,
    shim_ino: WireU64<PhpShimInoField>,
}

#[cfg(feature = "php")]
impl PhpWire {
    fn into_runtime(self) -> Result<PhpRuntime, ConfigError> {
        let version = PhpVersion::from_dotted(&self.version.0)
            .ok_or_else(|| document("php", "version must be `major.minor`".to_string()))?;
        Ok(PhpRuntime {
            libphp: PathBuf::from(self.libphp.0),
            dev: self.dev.0,
            ino: self.ino.0,
            version,
            shim: PathBuf::from(self.shim.0),
            shim_dev: self.shim_dev.0,
            shim_ino: self.shim_ino.0,
        })
    }
}

/// The three members D6 adds to the document (§4), for
/// [`Privilege::from_json`].
///
/// All three are optional: the accessor read them with `get`, so a document
/// written before D6 carries none of them and still means "this boot detected
/// nothing".
#[derive(Deserialize)]
struct PrivilegeParts {
    #[serde(default)]
    privilege: OptText<PrivilegeField>,
    #[serde(default)]
    capabilities: OptNames<CapabilitiesField>,
    #[serde(default)]
    bounding_set: OptBool<BoundingSetField>,
}

impl Privilege {
    /// The same, from the document stage 0 wrote (§4).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Document`] for a mode that is not one of the two, which is
    /// a document stage 0 did not write.
    pub fn from_json(value: &Value) -> Result<Option<Self>, ConfigError> {
        if !value.is_object() {
            // The members are read with `get`, so a value that is not an object
            // has none of them: the "nothing was detected" answer, not an error.
            return Ok(None);
        }
        let parts = PrivilegeParts::deserialize(value).map_err(document_error)?;
        Self::from_parts(
            parts.privilege.0,
            parts.capabilities.0,
            parts.bounding_set.0,
        )
    }
}

/// A `serde` failure as the [`ConfigError::Document`] it always was.
///
/// Every reader writes `<json field>: <reason>`, so the message splits back into
/// the pair `ConfigError::Document` carried before the structs did the reading.
/// The one structural failure `serde` produces itself — a key the document lacks
/// — is given the sentence the accessors gave it.
fn document_error(error: serde_json::Error) -> ConfigError {
    let text = error.to_string();
    if let Some(rest) = text.strip_prefix("missing field `")
        && let Some((field, _)) = rest.split_once('`')
        && let Some(field) = document_field(field)
    {
        return document(field, "the field is missing".to_string());
    }
    match text.split_once(": ") {
        Some((field, reason)) => match document_field(field) {
            Some(field) => document(field, reason.to_string()),
            None => document("document", text),
        },
        None => document("document", text),
    }
}

/// The `&'static str` of a document field the message names.
fn document_field(name: &str) -> Option<&'static str> {
    DOCUMENT_FIELDS.iter().copied().find(|field| *field == name)
}
