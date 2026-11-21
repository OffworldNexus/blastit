//! The document's field names: one zero-sized marker per JSON key.
//!
//! The name is an associated constant rather than a const generic parameter,
//! because `&'static str` is not a supported const-generic type. Each key is
//! therefore a marker type, the readers in [`super::read`] take it as their type
//! parameter, and a value the document cannot carry names the key the document
//! has — not the Rust field that read it.

/// The JSON name one wire field reports in a [`super::ConfigError::Document`].
pub(super) trait JsonField {
    const NAME: &'static str;
}

macro_rules! json_fields {
    ($($marker:ident = $name:literal),* $(,)?) => {
        $(
            pub(super) struct $marker;

            impl JsonField for $marker {
                const NAME: &'static str = $name;
            }
        )*
    };
}

json_fields! {
    PortField = "port",
    ServerPortField = "server_port",
    AppField = "app",
    PreloadField = "preload",
    ManifestField = "manifest",
    ManifestDirField = "manifest_dir",
    LibphpField = "libphp",
    PhpShimDirField = "php_shim_dir",
    ScriptNameField = "script_name",
    LogFormatField = "log_format",
    LogLevelField = "log_level",
    UidBaseField = "uid_base",
    MaxWorkersField = "max_workers",
    ViewMaxWorkersField = "view_max_workers",
    QueueSizeField = "queue_size",
    QueueTimeoutMsField = "queue_timeout_ms",
    RequestTimeoutMsField = "request_timeout_ms",
    StuckThreadsField = "stuck_threads",
    ViewSpawnField = "view_spawn",
    ViewIdleTimeoutMsField = "view_idle_timeout_ms",
    ViewMaxRequestsField = "view_max_requests",
    MaxViewProcessesField = "max_view_processes",
    ViewMemEstimateField = "view_mem_estimate",
    MemHighWatermarkPercentField = "mem_high_watermark_percent",
    ImmortalizeField = "immortalize",
    MaxHeaderBytesField = "max_header_bytes",
    MaxBodyBytesField = "max_body_bytes",
    HeaderTimeoutMsField = "header_timeout_ms",
    KeepaliveTimeoutMsField = "keepalive_timeout_ms",
    FairQueuingField = "fair_queuing",
    FqOuterPrefixV4Field = "fq_outer_prefix_v4",
    FqOuterPrefixV6Field = "fq_outer_prefix_v6",
    FqOuterMaxField = "fq_outer_max",
    FqInnerMaxPerOuterField = "fq_inner_max_per_outer",
    FqInnerMaxField = "fq_inner_max",
    FqBurstMsField = "fq_burst_ms",
    FqMaxDebtMsField = "fq_max_debt_ms",
    FqDebtTtlMsField = "fq_debt_ttl_ms",
    FqInnerHeadersField = "fq_inner_headers",
    FqInnerCookiesField = "fq_inner_cookies",
    FqInnerIncludeIpField = "fq_inner_include_ip",
    ClientIpHeaderField = "client_ip_header",
    TrustedProxiesField = "trusted_proxies",
    WarningsField = "warnings",
    HealthPathField = "health_path",
    ProxyMaxConnectionsField = "proxy_max_connections",
    EnvStripField = "env_strip",
    SeccompField = "seccomp",
    GracefulTimeoutMsField = "graceful_timeout_ms",
    WarmupField = "warmup",
    FrameMaxField = "frame_max",
    BacklogField = "backlog",
    MaxConnectionsField = "max_connections",
    ProxyOutputsField = "proxy_outputs",
    PrivilegeField = "privilege",
    CapabilitiesField = "capabilities",
    BoundingSetField = "bounding_set",
    SysPathField = "sys_path",
    CwdField = "cwd",
}

#[cfg(feature = "php")]
json_fields! {
    PhpDevField = "dev",
    PhpInoField = "ino",
    PhpVersionField = "version",
    PhpShimField = "shim",
    PhpShimDevField = "shim_dev",
    PhpShimInoField = "shim_ino",
}

json_fields! {
    PyMinorField = "py_minor",
    PythonHomeField = "python_home",
    PythonSysPathField = "sys_path",
    PythonLibFdField = "lib_fd",
    PythonLibDevField = "lib_dev",
    PythonLibInoField = "lib_ino",
    PythonShimFdField = "shim_fd",
    PythonShimDevField = "shim_dev",
    PythonShimInoField = "shim_ino",
}

/// Every JSON key the document carries, for the [`super::ConfigError::Document`]
/// a `serde` failure is reported with.
///
/// A key may repeat — the members of `php` report their own key, exactly as the
/// hand-written accessors did — and the lookup is by value, so the repeat is
/// harmless.
pub(super) const DOCUMENT_FIELDS: &[&str] = &[
    "port",
    "server_port",
    "app",
    "preload",
    "manifest",
    "manifest_dir",
    "libphp",
    "php_shim_dir",
    "php",
    "script_name",
    "log_format",
    "log_level",
    "uid_base",
    "max_workers",
    "view_max_workers",
    "queue_size",
    "queue_timeout_ms",
    "request_timeout_ms",
    "stuck_threads",
    "view_spawn",
    "view_idle_timeout_ms",
    "view_max_requests",
    "max_view_processes",
    "view_mem_estimate",
    "mem_high_watermark_percent",
    "immortalize",
    "max_header_bytes",
    "max_body_bytes",
    "header_timeout_ms",
    "keepalive_timeout_ms",
    "fair_queuing",
    "fq_outer_prefix_v4",
    "fq_outer_prefix_v6",
    "fq_outer_max",
    "fq_inner_max_per_outer",
    "fq_inner_max",
    "fq_burst_ms",
    "fq_max_debt_ms",
    "fq_debt_ttl_ms",
    "fq_inner_headers",
    "fq_inner_cookies",
    "fq_inner_include_ip",
    "client_ip_header",
    "trusted_proxies",
    "warnings",
    "health_path",
    "proxy_max_connections",
    "env_strip",
    "seccomp",
    "graceful_timeout_ms",
    "warmup",
    "frame_max",
    "backlog",
    "max_connections",
    "proxy_outputs",
    "privilege",
    "capabilities",
    "bounding_set",
    "sys_path",
    "cwd",
    "preset",
    "dev",
    "ino",
    "version",
    "shim",
    "shim_dev",
    "shim_ino",
    "python",
    "py_minor",
    "python_home",
    "lib_fd",
    "lib_dev",
    "lib_ino",
    "shim_fd",
];
