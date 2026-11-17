//! The hover data: the directive vocabulary an editor shows where the
//! developer is typing (`docs/design/blastfile-lsp-zed.md` §7.3).
//!
//! This module is **data and its one lookup**, nothing else: the table is a
//! transcription of `docs/reference/blastfile.md`, so a one-word fix to the page
//! is a one-word fix here, and the page stays the source. `mod.rs` is still the
//! module §1's boundary table names — this split is data only, and the public
//! entry an editor spawns (`blastit lsp`) is untouched.
//!
//! Hover text is not a second grammar: nothing here lexes, parses or decides
//! what a word means, and `hover_text` answers `None` for every word the table
//! does not carry.

/// The `$IDENT` paragraph, with the name interpolated (§7.2, §7.3).
pub(super) fn env_var_paragraph(name: &str) -> String {
    format!(
        "`${name}` names an environment variable: the value stays in the environment \
         and never in the `Blastfile`, so the file is safe to commit. A quoted string \
         where `$IDENT` is required is a syntax error."
    )
}

/// The markdown paragraph for one word, from the table (§7.2).
pub(super) fn hover_text(word: &str) -> Option<&'static str> {
    HOVER_TABLE
        .iter()
        .find(|(key, _)| *key == word)
        .map(|(_, value)| *value)
}

/// The `$IDENT` shape: `[A-Z_][A-Z0-9_]*` (§7.2).
pub(super) fn is_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_uppercase() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The directive vocabulary, transcribed from `docs/reference/blastfile.md`
/// (§7.3).
///
/// Sorted by key, one paragraph each, ≤ 48 rows and ≤ 600 bytes a value; a
/// duplicate key, an unsorted row or a key the reference page does not spell is
/// a test failure (`mod tests`), and a one-word fix to the page never needs the
/// editor to change its wording. Hover text is data, not a second grammar.
const HOVER_TABLE: &[(&str, &str)] = &[
    (
        "DELETE",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "GET",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "OPTIONS",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "PATCH",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "POST",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "PUT",
        "A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view.",
    ),
    (
        "any",
        "`egress any` opens any IPv4/IPv6 address and port, with no allowlist, and warns at boot.",
    ),
    (
        "app",
        "`app \"shop\"` names the application. One per file, optional; it is a label, never a path or a callable.",
    ),
    (
        "body",
        "The ingress codec: `json`, `form`, `none` (the default) or `raw`. A body sent to a view that declares no `body` is a `413`; `json` and `form` are re-encoded canonically, and anything else is a `400`.",
    ),
    (
        "ca_cert",
        "A store slot for the upstream TLS leg: `sslmode $DB_SSLMODE` names an `$IDENT` holding `disable`, `require`, `verify-ca` or `verify-full` (the app gets `disable`); `ca_cert $DB_CA_CERT` names the PEM.",
    ),
    (
        "database",
        "A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed.",
    ),
    (
        "delete",
        "`select products(id, name)` allows reading those columns; a bare `products` reads any column. `insert`/`update`/`delete` grant those columns or that table the same way, and `update` also allows row locks.",
    ),
    (
        "egress",
        "`egress any` opens any IPv4/IPv6 address and port, with no allowlist, and warns at boot.",
    ),
    (
        "entrypoint",
        "`entrypoint python \"shop.wsgi:application\"` names the callable behind every view; `entrypoint php \"public/index.php\"` names the front controller. One per file.",
    ),
    (
        "fallback",
        "`static` directives: `root` is the directory served (required, relative, no `..`), `index` answers a directory request (default `index.html`), `fallback` answers a path that is no file, on `GET` or `HEAD`.",
    ),
    (
        "form",
        "The ingress codec: `json`, `form`, `none` (the default) or `raw`. A body sent to a view that declares no `body` is a `413`; `json` and `form` are re-encoded canonically, and anything else is a `400`.",
    ),
    (
        "functions",
        "`functions st_distance, unaccent` allows function calls beyond the built-in safe set; `unaccent` reads a rule file, so it must be named.",
    ),
    (
        "host",
        "A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed.",
    ),
    (
        "immutable",
        "`immutable \"/assets/*\"` adds `Cache-Control: public, max-age=31536000, immutable`; `*` stops at `/`, so a nested tree needs one glob per depth.",
    ),
    (
        "index",
        "`static` directives: `root` is the directory served (required, relative, no `..`), `index` answers a directory request (default `index.html`), `fallback` answers a path that is no file, on `GET` or `HEAD`.",
    ),
    (
        "insert",
        "`select products(id, name)` allows reading those columns; a bare `products` reads any column. `insert`/`update`/`delete` grant those columns or that table the same way, and `update` also allows row locks.",
    ),
    (
        "json",
        "The ingress codec: `json`, `form`, `none` (the default) or `raw`. A body sent to a view that declares no `body` is a `413`; `json` and `form` are re-encoded canonically, and anything else is a `400`.",
    ),
    (
        "none",
        "The ingress codec: `json`, `form`, `none` (the default) or `raw`. A body sent to a view that declares no `body` is a `413`; `json` and `form` are re-encoded canonically, and anything else is a `400`.",
    ),
    (
        "password",
        "A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed.",
    ),
    (
        "port",
        "A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed.",
    ),
    (
        "postgres",
        "`postgres $DATABASE_URL` declares the default store; `postgres analytics $ANALYTICS_URL` a second one. The file holds the variable name, never the value.",
    ),
    (
        "prefork",
        "`prefork` forks the worker at startup instead of on the first request.",
    ),
    (
        "preload",
        "`preload \"shop.urls\", \"shop.signals\"` imports each module once before the worker forks, so every request shares one copy. The block holds `preload` only.",
    ),
    (
        "query",
        "`query 'SELECT …'` allows one exact statement; repeat the directive for more. It is trusted as written: read it before pasting.",
    ),
    (
        "raw",
        "`body raw` passes the request body through unvalidated — the loud opt-out. `blastit dev` serves it with a warning, `check` reports `raw_ingress`, and a production boot refuses the file until you add `unvalidated`.",
    ),
    (
        "regex",
        "`regex '/v[0-9]+/items'` matches the whole path in linear time, with no lookaround or backreferences.",
    ),
    (
        "root",
        "`static` directives: `root` is the directory served (required, relative, no `..`), `index` answers a directory request (default `index.html`), `fallback` answers a path that is no file, on `GET` or `HEAD`.",
    ),
    (
        "select",
        "`select products(id, name)` allows reading those columns; a bare `products` reads any column. `insert`/`update`/`delete` grant those columns or that table the same way, and `update` also allows row locks.",
    ),
    (
        "sslmode",
        "A store slot for the upstream TLS leg: `sslmode $DB_SSLMODE` names an `$IDENT` holding `disable`, `require`, `verify-ca` or `verify-full` (the app gets `disable`); `ca_cert $DB_CA_CERT` names the PEM.",
    ),
    (
        "static",
        "`static \"/assets\" { root \"public/assets\" }` serves regular files under a prefix before any view answers; the longest matching prefix wins.",
    ),
    (
        "trait",
        "`trait session { postgres { … } }` holds store grants only. A view attaches it with `use session`; a trait used nowhere warns at boot.",
    ),
    (
        "transactions",
        "`transactions` allows `BEGIN`, `COMMIT`, `ROLLBACK` and savepoints on that store.",
    ),
    (
        "unrestricted",
        "`postgres unrestricted` allows every statement on that store, so `check` can no longer tell you what the view needed. It is not a fix for a missing grant, and it warns at boot.",
    ),
    (
        "unvalidated",
        "`body raw unvalidated` is the acknowledgement: production serves it, with a boot warning. The two legitimate uses are a file upload and a raw-signature webhook.",
    ),
    (
        "update",
        "`select products(id, name)` allows reading those columns; a bare `products` reads any column. `insert`/`update`/`delete` grant those columns or that table the same way, and `update` also allows row locks.",
    ),
    (
        "use",
        "`use session, catalog` attaches those traits. The grant is the union of the view's own blocks and every attached trait.",
    ),
    (
        "user",
        "A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed.",
    ),
    (
        "view",
        "`view products GET \"/products\", \"/products/{id:int}\"` is one sandbox: one worker, one set of permissions. Views are tried in the order written, and the first match wins.",
    ),
];

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{HOVER_TABLE, hover_text};

    /// H6: the table is bounded, sorted, duplicate-free, and every key is a word
    /// of the reference page — the page is the source, this is a transcription.
    #[test]
    fn hover_table_is_bounded_and_transcribed() {
        assert!(HOVER_TABLE.len() <= 48, "{} rows", HOVER_TABLE.len());
        let page = include_str!("../../../../docs/reference/blastfile.md");
        let mut previous = "";
        for (key, value) in HOVER_TABLE {
            assert!(!key.is_empty(), "an empty key");
            assert!(
                key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{key}"
            );
            assert!(value.len() <= 600, "{key} is {} bytes", value.len());
            assert!(!value.is_empty(), "{key} has no text");
            assert!(previous < *key, "{key} is out of order after {previous}");
            previous = key;
            assert!(page.contains(key), "{key} is not in the reference page");
        }
        let unrestricted = hover_text("unrestricted").unwrap();
        assert!(unrestricted.contains("every statement"));
        assert!(unrestricted.contains("warns at boot"));
        // The `$IDENT` paragraph is computed, never a row: its one spelling is
        // what keeps `$ANALYTICS_URL` from being a second vocabulary.
        assert!(hover_text("$ANALYTICS_URL").is_none());
    }
}
