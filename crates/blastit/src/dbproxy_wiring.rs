//! The one place a dev, production or `check` boot turns the environment's
//! upstreams into dbproxy's `ProxySpec`s.
//!
//! The three boots differed in exactly one field — the per-store connection cap
//! — and in nothing else, so the mapping lives here once: a field added to
//! [`ProxySpec`] is added here and every boot inherits it, instead of three
//! copies that can drift apart.

use blastit_dbproxy::ProxySpec;

/// Builds dbproxy's proxy table from the prepared upstreams.
///
/// `max_connections` is asked for each proxy's name: a `dev` or production boot
/// answers from its manifest (`Config::max_connections_for`), and `check` answers
/// its fixed one-shot budget. Nothing here reads the environment: `prepared` is
/// the rewrite stage 0 already proved, so this is a pure mapping.
pub(crate) fn proxy_specs(
    prepared: &blastit_env::Prepared,
    max_connections: impl Fn(&str) -> u32,
) -> Vec<ProxySpec> {
    prepared
        .upstreams()
        .iter()
        .map(|(name, upstream)| ProxySpec {
            name: name.as_str().to_string(),
            host: upstream.host().to_string(),
            port: upstream.port(),
            user: upstream.user().to_string(),
            password: upstream.password().expose().to_string().into(),
            database: upstream.database().to_string(),
            sslmode: upstream.sslmode().map(str::to_string),
            ca_cert: upstream.ca_cert().map(|ca| ca.expose().to_string().into()),
            options: upstream.options().to_vec(),
            max_connections: max_connections(name.as_str()),
        })
        .collect()
}
