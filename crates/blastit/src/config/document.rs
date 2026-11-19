//! The stage-1 document's writing half (§4): [`Config::to_json`].

use std::time::Duration;

use serde_json::{Map, Value};

use super::settings::Config;

impl Config {
    /// The configuration as a JSON document, for stage 1 (§4).
    ///
    /// It holds no secret: proxy variable *names*, counts, sizes and timeouts.
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("port".into(), Value::from(self.port));
        object.insert("server_port".into(), Value::from(self.server_port));
        object.insert(
            "app".into(),
            self.app.clone().map_or(Value::Null, Value::from),
        );
        object.insert(
            "preload".into(),
            Value::Array(self.preload.iter().cloned().map(Value::from).collect()),
        );
        object.insert(
            "manifest".into(),
            Value::from(self.manifest.to_string_lossy().into_owned()),
        );
        object.insert(
            "manifest_dir".into(),
            Value::from(self.manifest_dir.to_string_lossy().into_owned()),
        );
        // `ADR-0010` §3: the two settings stage 0's discovery read, and what it
        // resolved. A path is not a secret, and stage 1 needs both the path and
        // the inode identity to re-open exactly the file stage 0 inspected.
        object.insert(
            "libphp".into(),
            self.libphp
                .as_ref()
                .map(|path| Value::from(path.to_string_lossy().into_owned()))
                .unwrap_or(Value::Null),
        );
        object.insert(
            "php_shim_dir".into(),
            self.php_shim_dir
                .as_ref()
                .map(|path| Value::from(path.to_string_lossy().into_owned()))
                .unwrap_or(Value::Null),
        );
        #[cfg(feature = "php")]
        {
            object.insert(
                "php".into(),
                match &self.php {
                    None => Value::Null,
                    Some(runtime) => {
                        let mut php = Map::new();
                        php.insert(
                            "libphp".into(),
                            Value::from(runtime.libphp.to_string_lossy().into_owned()),
                        );
                        php.insert("dev".into(), Value::from(runtime.dev));
                        php.insert("ino".into(), Value::from(runtime.ino));
                        php.insert("version".into(), Value::from(runtime.version.dotted()));
                        php.insert(
                            "shim".into(),
                            Value::from(runtime.shim.to_string_lossy().into_owned()),
                        );
                        php.insert("shim_dev".into(), Value::from(runtime.shim_dev));
                        php.insert("shim_ino".into(), Value::from(runtime.shim_ino));
                        Value::Object(php)
                    }
                },
            );
        }
        // §2.4: the Python handoff stage 1 re-verifies. `null` for a PHP-only or
        // static-only boot, which never searched for an interpreter.
        object.insert(
            "python".into(),
            match &self.python {
                None => Value::Null,
                Some(python) => {
                    let mut handoff = Map::new();
                    handoff.insert("py_minor".into(), Value::from(python.py_minor));
                    handoff.insert(
                        "python_home".into(),
                        Value::from(python.python_home.to_string_lossy().into_owned()),
                    );
                    handoff.insert(
                        "sys_path".into(),
                        Value::Array(
                            python
                                .sys_path
                                .iter()
                                .map(|entry| Value::from(latin1(entry)))
                                .collect(),
                        ),
                    );
                    handoff.insert("lib_fd".into(), Value::from(python.lib.0));
                    handoff.insert("lib_dev".into(), Value::from(python.lib.1));
                    handoff.insert("lib_ino".into(), Value::from(python.lib.2));
                    handoff.insert("shim_fd".into(), Value::from(python.shim.0));
                    handoff.insert("shim_dev".into(), Value::from(python.shim.1));
                    handoff.insert("shim_ino".into(), Value::from(python.shim.2));
                    Value::Object(handoff)
                }
            },
        );
        object.insert(
            "script_name".into(),
            self.script_name.clone().map_or(Value::Null, Value::from),
        );
        object.insert("log_format".into(), Value::from(self.log_format.as_str()));
        object.insert("log_level".into(), Value::from(self.log_level.as_str()));
        object.insert("uid_base".into(), Value::from(self.uid_base));
        object.insert("max_workers".into(), Value::from(self.max_workers));
        object.insert(
            "view_max_workers".into(),
            self.view_max_workers.map_or(Value::Null, Value::from),
        );
        object.insert("queue_size".into(), Value::from(self.queue_size as u64));
        object.insert(
            "queue_timeout_ms".into(),
            Value::from(millis(self.queue_timeout)),
        );
        object.insert(
            "request_timeout_ms".into(),
            Value::from(millis(self.request_timeout)),
        );
        object.insert("stuck_threads".into(), Value::from(self.stuck_threads));
        object.insert("view_spawn".into(), Value::from(self.view_spawn.as_str()));
        object.insert(
            "view_idle_timeout_ms".into(),
            Value::from(millis(self.view_idle_timeout)),
        );
        object.insert(
            "view_max_requests".into(),
            Value::from(self.view_max_requests),
        );
        object.insert(
            "max_view_processes".into(),
            self.max_view_processes.map_or(Value::Null, Value::from),
        );
        object.insert(
            "view_mem_estimate".into(),
            Value::from(self.view_mem_estimate),
        );
        object.insert(
            "mem_high_watermark_percent".into(),
            Value::from(self.mem_high_watermark_percent),
        );
        object.insert("immortalize".into(), Value::from(self.immortalize.as_str()));
        object.insert(
            "max_header_bytes".into(),
            Value::from(self.max_header_bytes as u64),
        );
        object.insert("max_body_bytes".into(), Value::from(self.max_body_bytes));
        object.insert(
            "header_timeout_ms".into(),
            Value::from(millis(self.header_timeout)),
        );
        object.insert(
            "keepalive_timeout_ms".into(),
            Value::from(millis(self.keepalive_timeout)),
        );
        object.insert("fair_queuing".into(), Value::from(self.fair_queuing));
        object.insert(
            "fq_outer_prefix_v4".into(),
            Value::from(self.fq_outer_prefix_v4),
        );
        object.insert(
            "fq_outer_prefix_v6".into(),
            Value::from(self.fq_outer_prefix_v6),
        );
        object.insert("fq_outer_max".into(), Value::from(self.fq_outer_max as u64));
        object.insert(
            "fq_inner_max_per_outer".into(),
            Value::from(self.fq_inner_max_per_outer as u64),
        );
        object.insert("fq_inner_max".into(), Value::from(self.fq_inner_max as u64));
        object.insert("fq_burst_ms".into(), Value::from(millis(self.fq_burst)));
        object.insert(
            "fq_max_debt_ms".into(),
            Value::from(millis(self.fq_max_debt)),
        );
        object.insert(
            "fq_debt_ttl_ms".into(),
            Value::from(millis(self.fq_debt_ttl)),
        );
        object.insert(
            "fq_inner_headers".into(),
            Value::Array(
                self.fq_inner_headers
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "fq_inner_cookies".into(),
            Value::Array(
                self.fq_inner_cookies
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "fq_inner_include_ip".into(),
            Value::from(self.fq_inner_include_ip),
        );
        object.insert(
            "client_ip_header".into(),
            Value::from(self.client_ip_header.clone()),
        );
        object.insert(
            "trusted_proxies".into(),
            Value::Array(
                self.trusted_proxies
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "warnings".into(),
            Value::Array(
                self.warnings
                    .iter()
                    .map(|warning| Value::from(warning.as_str()))
                    .collect(),
            ),
        );
        object.insert("health_path".into(), Value::from(self.health_path.clone()));
        let connections = self
            .proxy_max_connections
            .iter()
            .map(|(name, cap)| (name.clone(), Value::from(*cap)))
            .collect();
        object.insert("proxy_max_connections".into(), Value::Object(connections));
        object.insert(
            "env_strip".into(),
            self.env_strip.clone().map_or(Value::Null, Value::from),
        );
        object.insert("seccomp".into(), Value::from(self.seccomp));
        object.insert(
            "graceful_timeout_ms".into(),
            Value::from(millis(self.graceful_timeout)),
        );
        object.insert(
            "warmup".into(),
            self.warmup.clone().map_or(Value::Null, Value::from),
        );
        object.insert("frame_max".into(), Value::from(self.frame_max as u64));
        object.insert("backlog".into(), Value::from(self.backlog));
        object.insert(
            "max_connections".into(),
            Value::from(self.max_connections as u64),
        );
        let outputs = self
            .proxy_outputs
            .iter()
            .map(|(name, names)| {
                (
                    name.clone(),
                    Value::Array(names.iter().cloned().map(Value::from).collect()),
                )
            })
            .collect();
        object.insert("proxy_outputs".into(), Value::Object(outputs));
        // D6: the mode init detected, the capabilities it holds, and whether
        // the children's bounding set can be emptied. `root` is written as a
        // plain mode with no list; the rootless case carries the names, never a
        // value of any kind.
        object.insert(
            "privilege".into(),
            match &self.privilege {
                None => Value::Null,
                Some(privilege) => Value::from(privilege.mode.as_str()),
            },
        );
        object.insert(
            "capabilities".into(),
            Value::Array(
                self.privilege
                    .as_ref()
                    .map(|privilege| privilege.capabilities.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .cloned()
                    .map(Value::from)
                    .collect(),
            ),
        );
        object.insert(
            "bounding_set".into(),
            match self
                .privilege
                .as_ref()
                .map(|privilege| privilege.bounding_set_dropped)
            {
                None => Value::Null,
                Some(dropped) => Value::from(dropped),
            },
        );
        object.insert(
            "sys_path".into(),
            Value::Array(
                self.sys_path
                    .iter()
                    .map(|entry| Value::from(latin1(entry)))
                    .collect(),
            ),
        );
        // D2: the directory `blastit` was started in, in bytes like `sys.path`
        // — stage 1 puts it first on `sys.path`, which is what makes a project
        // that is not installed importable without `PYTHONPATH` (§5.3).
        object.insert(
            "cwd".into(),
            match &self.cwd {
                None => Value::Null,
                Some(dir) => Value::from(latin1(dir)),
            },
        );
        // E1: which preset is in force, what it supplied and what an explicit
        // variable overrode. Every value here is one of blastit's own, so the
        // document stays free of the operator's environment.
        object.insert(
            "preset".into(),
            match &self.preset {
                None => Value::Null,
                Some(choice) => {
                    let mut preset = Map::new();
                    preset.insert("name".into(), Value::from(choice.name.clone()));
                    preset.insert(
                        "supplied".into(),
                        Value::Object(
                            choice
                                .supplied
                                .iter()
                                .map(|(name, value)| (name.clone(), Value::from(value.clone())))
                                .collect(),
                        ),
                    );
                    preset.insert(
                        "overridden".into(),
                        Value::Array(choice.overridden.iter().cloned().map(Value::from).collect()),
                    );
                    Value::Object(preset)
                }
            },
        );
        Value::Object(object)
    }
}

/// Milliseconds as a floating-point number, so sub-second values survive.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// A byte string as the JSON string the configuration document can carry.
///
/// JSON has no byte strings, and a `sys.path` entry is bytes (`os.fsencode`):
/// one byte per character is the identity for the ASCII of every ordinary path
/// and round-trips the rest exactly, where a UTF-8 conversion would refuse a
/// path that is not valid UTF-8.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}
