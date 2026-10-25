//! The app environment and each proxy's upstream configuration (§5.1, §5.3).
//!
//! [`build`] is stage 0's single rewrite, run once as root, before the
//! `execve` barrier (§4). It is a pure function of the manifest and the
//! platform environment: the same two inputs give the same
//! [`Prepared`] in every process that derives it, which matters because
//! frameworks read their database settings once, in init, before the fork
//! (§5.2).
//!
//! The rewrite, in order (§5.3):
//!
//! 1. **Strip.** Every `BLASTIT_*` variable, every proxy input variable, and
//!    every name in `BLASTIT_ENV_STRIP` (comma- or whitespace-separated) leave
//!    the app's environment.
//! 2. **Render.** Each proxy's outputs — explicit, or the protocol's defaults —
//!    are rendered against the fields the protocol offers and added.
//! 3. **Inject.** The process-safety defaults ([`INJECTED_DEFAULTS`]) are added
//!    unless the variable is already set.
//!
//! `HOME` and `TMPDIR` are *not* injected here: each view gets a private
//! scratch directory, which only the process launcher knows, so they are set
//! per view, later (§5.3, §2.5).
//!
//! Nothing in the result carries a secret. [`Prepared::upstreams`] holds the
//! real credentials, wrapped in [`Secret`](crate::Secret), so they can only be
//! read on purpose; [`Prepared::app_env`] never holds one, because every proxy
//! input is removed and no template field can expose one (§5.1).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;

use blastit_manifest::{Manifest, Protocol, ProxyName};

use crate::error::{EnvError, EnvErrors, invalid_name_reason};
use crate::postgresql::{self, Upstream};
use crate::template::Template;

/// `BLASTIT_ENV_STRIP`: further variables to remove, comma- or
/// whitespace-separated (§5.3).
pub const STRIP_VAR: &str = "BLASTIT_ENV_STRIP";

/// The prefix of every variable blastit keeps to itself (§5.3).
pub const RESERVED_PREFIX: &str = "BLASTIT_";

/// The process-safety defaults injected after the rewrite, unless already set
/// (§5.3). `HOME` and `TMPDIR` are absent on purpose: they are per view.
pub const INJECTED_DEFAULTS: &[(&str, &str)] = &[
    ("OPENBLAS_NUM_THREADS", "1"),
    ("OMP_NUM_THREADS", "1"),
    ("MKL_NUM_THREADS", "1"),
    ("NUMEXPR_NUM_THREADS", "1"),
    ("VECLIB_MAXIMUM_THREADS", "1"),
    ("PYTHONDONTWRITEBYTECODE", "1"),
    ("LC_ALL", "C.UTF-8"),
];

/// The result of stage 0's rewrite (§5.3).
///
/// [`std::fmt::Debug`] is safe to log: the upstream credentials are [`Secret`]s
/// (which print `Secret(<redacted>)`) and the app environment holds no secret
/// by construction.
///
/// [`Secret`]: crate::Secret
#[derive(Clone, Debug)]
pub struct Prepared {
    app_env: BTreeMap<String, String>,
    upstreams: Vec<(ProxyName, Upstream)>,
    outputs: BTreeMap<ProxyName, Vec<String>>,
}

impl Prepared {
    /// The environment the application will see, secrets removed (§5.3).
    ///
    /// This is the child's **whole** environment: the launcher must pass it to
    /// `execve` as `envp`, or set it with `clearenv` + `setenv`, rather than
    /// merging it into the parent's. Merging would put back every variable the
    /// rewrite removed — the proxy inputs, `BLASTIT_*` and the strip list — and
    /// the credentials with them.
    pub fn app_env(&self) -> &BTreeMap<String, String> {
        &self.app_env
    }

    /// Each proxy's upstream, in manifest proxy order (§2.2, §5.2).
    pub fn upstreams(&self) -> &[(ProxyName, Upstream)] {
        &self.upstreams
    }

    /// One proxy's upstream, by name.
    pub fn upstream(&self, proxy: &str) -> Option<&Upstream> {
        self.upstreams
            .iter()
            .find(|(name, _)| name.as_str() == proxy)
            .map(|(_, upstream)| upstream)
    }

    /// The variable names each proxy produced, by proxy name (§5.1).
    pub fn outputs(&self) -> &BTreeMap<ProxyName, Vec<String>> {
        &self.outputs
    }
}

/// Rewrites the platform environment into the app environment and each proxy's
/// upstream (§5.1, §5.3).
///
/// `strip` is an extra list of variables to remove, comma- or
/// whitespace-separated; it is *added to* whatever the platform environment's
/// `BLASTIT_ENV_STRIP` names. Every problem is collected, so one boot reports
/// them all; no message carries a value from `platform_env`.
pub fn build(
    manifest: &Manifest,
    platform_env: &BTreeMap<String, String>,
    strip: Option<&str>,
) -> Result<Prepared, EnvErrors> {
    let mut strip_names: BTreeSet<String> = BTreeSet::new();
    if let Some(extra) = strip {
        strip_names.extend(split_strip(extra));
    }
    if let Some(from_env) = platform_env.get(STRIP_VAR) {
        strip_names.extend(split_strip(from_env));
    }

    let input_variables: BTreeSet<&str> = manifest
        .proxies()
        .values()
        .flat_map(|proxy| proxy.input().values().map(String::as_str))
        .collect();

    // 1. Strip.
    let mut app_env = BTreeMap::new();
    for (variable, value) in platform_env {
        if variable.starts_with(RESERVED_PREFIX)
            || input_variables.contains(variable.as_str())
            || strip_names.contains(variable)
        {
            continue;
        }
        app_env.insert(variable.clone(), value.clone());
    }

    // 2. Render, and parse every upstream on the way.
    let mut errors: Vec<EnvError> = Vec::new();
    let mut upstreams: Vec<(ProxyName, Upstream)> = Vec::new();
    let mut outputs: BTreeMap<ProxyName, Vec<String>> = BTreeMap::new();
    let mut produced: BTreeMap<String, ProxyName> = BTreeMap::new();

    let mut index = 0usize;
    for (name, proxy) in manifest.proxies() {
        match proxy.protocol() {
            Protocol::Postgresql => {
                let port = postgresql::local_port(index);
                index += 1;
                let Some(port) = port else {
                    errors.push(EnvError::TooManyProxies {
                        proxy: name.clone(),
                    });
                    continue;
                };

                let mut names_are_usable = true;
                for variable in proxy.input().values() {
                    if let Some(reason) = invalid_name_reason(variable) {
                        errors.push(EnvError::InvalidVariableName {
                            proxy: name.clone(),
                            variable: variable.clone(),
                            reason,
                        });
                        names_are_usable = false;
                    }
                }
                if !names_are_usable {
                    continue;
                }

                let upstream = match postgresql::upstream(proxy, platform_env) {
                    Ok(upstream) => upstream,
                    Err(found) => {
                        errors.extend(found.into_errors());
                        continue;
                    }
                };
                let fields = postgresql::template_fields(&upstream, port);
                let table = if proxy.output().is_empty() {
                    postgresql::default_outputs(proxy)
                } else {
                    proxy.output().clone()
                };

                let mut produced_here = Vec::new();
                for (variable, source) in &table {
                    if let Some(reason) = invalid_name_reason(variable) {
                        errors.push(EnvError::InvalidVariableName {
                            proxy: name.clone(),
                            variable: variable.clone(),
                            reason,
                        });
                        continue;
                    }
                    if variable.starts_with(RESERVED_PREFIX) {
                        errors.push(EnvError::ReservedOutput {
                            proxy: name.clone(),
                            variable: variable.clone(),
                        });
                        continue;
                    }
                    if let Some(first) = produced.get(variable) {
                        if first != name {
                            errors.push(EnvError::DuplicateOutput {
                                variable: variable.clone(),
                                first: first.clone(),
                                second: name.clone(),
                            });
                        }
                        continue;
                    }
                    if platform_env.contains_key(variable)
                        && !proxy.input().values().any(|input| input == variable)
                    {
                        errors.push(EnvError::OutputOverwritesForeign {
                            proxy: name.clone(),
                            variable: variable.clone(),
                        });
                        continue;
                    }

                    let template = match Template::parse(source) {
                        Ok(template) => template,
                        Err(error) => {
                            errors.push(EnvError::Template {
                                proxy: name.clone(),
                                variable: variable.clone(),
                                error,
                            });
                            continue;
                        }
                    };
                    match template.render(&fields) {
                        Ok(rendered) => {
                            app_env.insert(variable.clone(), rendered);
                            produced.insert(variable.clone(), name.clone());
                            produced_here.push(variable.clone());
                        }
                        Err(error) => errors.push(EnvError::Template {
                            proxy: name.clone(),
                            variable: variable.clone(),
                            error,
                        }),
                    }
                }

                outputs.insert(name.clone(), produced_here);
                upstreams.push((name.clone(), upstream));
            }
        }
    }

    if !errors.is_empty() {
        return Err(EnvErrors::new(errors));
    }

    // 3. Inject.
    for (variable, value) in INJECTED_DEFAULTS {
        app_env
            .entry((*variable).to_string())
            .or_insert_with(|| (*value).to_string());
    }

    Ok(Prepared {
        app_env,
        upstreams,
        outputs,
    })
}

/// Converts an OS environment into the UTF-8 map [`build`] wants (§5.3).
///
/// A variable whose *value* is not valid UTF-8 is dropped, unless blastit needs
/// it — it is a proxy input, or `BLASTIT_ENV_STRIP` — in which case the boot
/// aborts with [`EnvError::NonUtf8Variable`], naming the variable and not its
/// value. A variable whose *name* is not valid UTF-8 can never be referenced by
/// the manifest, so it is dropped.
pub fn platform_env_from_os(
    manifest: &Manifest,
    vars: &[(OsString, OsString)],
) -> Result<BTreeMap<String, String>, EnvErrors> {
    let needed: BTreeSet<&str> = manifest
        .proxies()
        .values()
        .flat_map(|proxy| proxy.input().values().map(String::as_str))
        .chain(std::iter::once(STRIP_VAR))
        .collect();

    let mut env = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, value) in vars {
        let Some(name) = name.to_str() else {
            continue;
        };
        match value.to_str() {
            Some(value) => {
                env.insert(name.to_string(), value.to_string());
            }
            None if needed.contains(name) => errors.push(EnvError::NonUtf8Variable {
                variable: name.to_string(),
            }),
            None => {}
        }
    }

    if errors.is_empty() {
        Ok(env)
    } else {
        Err(EnvErrors::new(errors))
    }
}

/// Reads the current process environment and rewrites it (§5.3).
///
/// A convenience wrapper for stage 0; the rest of this crate stays pure.
pub fn build_from_process(manifest: &Manifest, strip: Option<&str>) -> Result<Prepared, EnvErrors> {
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let platform_env = platform_env_from_os(manifest, &vars)?;
    build(manifest, &platform_env, strip)
}

/// Splits a strip list on commas and whitespace.
fn split_strip(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split(|character: char| character == ',' || character.is_whitespace())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;
    use std::ffi::OsString;

    use blastit_manifest::Manifest;

    use super::{INJECTED_DEFAULTS, build, platform_env_from_os, split_strip};

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn manifest(source: &str) -> Manifest {
        Manifest::from_toml_str(source).expect("manifest parses")
    }

    #[test]
    fn split_strip_accepts_commas_and_whitespace() {
        assert_eq!(
            split_strip(" A, B  C ,,D\n").collect::<Vec<_>>(),
            ["A", "B", "C", "D"]
        );
    }

    #[test]
    fn injected_defaults_skip_home_and_tmpdir() {
        let names: Vec<&str> = INJECTED_DEFAULTS.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"LC_ALL"));
        assert!(!names.contains(&"HOME"));
        assert!(!names.contains(&"TMPDIR"));
    }

    #[test]
    fn a_non_utf8_input_value_is_an_error_and_others_are_dropped() {
        let manifest = manifest(
            "version = 1\n[proxy.db]\nprotocol = \"postgresql\"\ninput = { url = \"DATABASE_URL\" }\n",
        );
        let vars = vec![
            (
                OsString::from("DATABASE_URL"),
                OsString::from("postgresql://u:p@h/db"),
            ),
            (OsString::from("PATH"), invalid_utf8()),
        ];

        // A variable blastit does not need is dropped.
        let env = platform_env_from_os(&manifest, &vars).expect("converts");
        assert_eq!(
            env.get("DATABASE_URL").map(String::as_str),
            Some("postgresql://u:p@h/db")
        );
        assert_eq!(env.get("PATH"), None);

        // An input variable is needed, so its bad value aborts the boot.
        let vars = vec![(OsString::from("DATABASE_URL"), invalid_utf8())];
        let errors = platform_env_from_os(&manifest, &vars).expect_err("must be rejected");
        assert!(errors.to_string().contains("DATABASE_URL"));
    }

    #[test]
    fn a_non_utf8_variable_name_is_dropped() {
        let manifest = manifest("version = 1\n");
        let vars = vec![(invalid_utf8(), OsString::from("value"))];
        let env = platform_env_from_os(&manifest, &vars).expect("converts");
        assert!(env.is_empty());
    }

    #[test]
    fn a_manifest_with_no_proxies_keeps_the_environment() {
        let manifest = manifest("version = 1\n");
        let prepared = build(&manifest, &env(&[("PATH", "/usr/bin")]), None).expect("builds");
        assert_eq!(
            prepared.app_env().get("PATH").map(String::as_str),
            Some("/usr/bin")
        );
        assert_eq!(
            prepared.app_env().get("LC_ALL").map(String::as_str),
            Some("C.UTF-8")
        );
        assert!(prepared.upstreams().is_empty());
        assert!(prepared.outputs().is_empty());
    }

    #[test]
    fn the_explicit_strip_list_is_added_to_the_environment_one() {
        let manifest = manifest("version = 1\n");
        let platform = env(&[
            ("SECRET_KEY", "k"),
            ("PGPASSWORD", "p"),
            ("BLASTIT_ENV_STRIP", "PGPASSWORD, PGHOST, PGUSER"),
        ]);
        let prepared = build(&manifest, &platform, Some("SECRET_KEY,EXTRA")).expect("builds");
        for name in [
            "SECRET_KEY",
            "PGPASSWORD",
            "PGHOST",
            "PGUSER",
            "EXTRA",
            "BLASTIT_ENV_STRIP",
        ] {
            assert!(
                !prepared.app_env().contains_key(name),
                "{name} should be stripped"
            );
        }
    }

    /// A byte string that is not valid UTF-8.
    fn invalid_utf8() -> OsString {
        use std::os::unix::ffi::OsStringExt as _;
        OsString::from_vec(vec![0xFF, 0xFE])
    }
}
