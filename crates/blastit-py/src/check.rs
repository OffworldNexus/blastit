//! The `blastit check` / `--learn` functions behind the extension
//! (`R27`, `R32..R38`; `ADR-0004`).
//!
//! Three thin wrappers, so the rules live in one place:
//!
//! * [`check_audit`] is [`blastit::audit_v1::check_audit`]: the JSONL streams
//!   the pytest gate wrote, diffed against the manifest;
//! * [`plan_learn`] is [`blastit::audit_v1::plan_learn_json`]: the routes and
//!   exact queries `--learn` would add;
//! * [`probe_ingress`] is [`blastit::audit_v1::probe_ingress_json`]:
//!   [`blastit_ingress::probe_sample`] over at most 64 KiB of body, which is
//!   all the WSGI gate hands over.
//!
//! Nothing here reads a credential or a body byte out: the sample is a MIME
//! essence and three booleans.

/// Diffs the observations of one `blastit check` run against the manifest.
///
/// # Errors
///
/// The one-line reason a corrupt observations file, invalid manifest or
/// unsupported schema version fails with; the caller raises it as
/// `ValueError`, and `check::run` names the file and exits `2`.
pub(crate) fn check_audit(
    manifest_toml: &str,
    http_obs_jsonl: &str,
    sql_obs_jsonl: &str,
) -> Result<String, String> {
    blastit::audit_v1::check_audit(manifest_toml, http_obs_jsonl, sql_obs_jsonl)
        .map_err(|error| error.to_string())
}

/// Plans the `--learn` edit and renders the `--learn --json` document.
///
/// # Errors
///
/// The same failures as [`check_audit`].
pub(crate) fn plan_learn(
    manifest_toml: &str,
    http_obs_jsonl: &str,
    sql_obs_jsonl: &str,
    dry_run: bool,
) -> Result<String, String> {
    blastit::audit_v1::plan_learn_json(manifest_toml, http_obs_jsonl, sql_obs_jsonl, dry_run)
        .map_err(|error| error.to_string())
}

/// Probes one request body as JSON: `mime`, `truncated`, `valid_json` and
/// `valid_form`.
///
/// `prefix` is the first bytes of the body, already capped by the caller;
/// `total_len` is the body's real length, which is what makes a `> 64 KiB`
/// body report `truncated: true`.
///
/// # Errors
///
/// Only a rendering failure, which cannot happen in practice.
pub(crate) fn probe_ingress(
    content_type: Option<&str>,
    prefix: &[u8],
    total_len: u64,
) -> Result<String, String> {
    blastit::audit_v1::probe_ingress_json(content_type, prefix, total_len)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
        version = 1
        [proxy.db]
        protocol = "postgresql"
        input = { url = "DATABASE_URL" }
        [[route]]
        name = "notes"
        path = ['/notes/.*']
        [route.db]
        queries = ['SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $1']
    "#;

    #[test]
    fn check_audit_answers_the_python_gate() {
        let sql = serde_json::json!({
            "target": {"scope": "routed", "route": "notes"},
            "proxy": "db",
            "kind": "query",
            "sql": r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $?"#,
            "is_tx": false,
            "allowed": true,
            "verdict": {
                "kind": "query",
                "normalized": r#"SELECT "notes"."id" FROM "notes" WHERE "notes"."id" = $?"#,
            },
        })
        .to_string();
        let report = check_audit(MANIFEST, "{\"method\":\"GET\",\"path\":\"/notes/\"}", &sql)
            .expect("the audit runs");
        let report: serde_json::Value = serde_json::from_str(&report).expect("JSON");
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["ok"], true);
        assert_eq!(report["findings"], serde_json::json!([]));
    }

    #[test]
    fn probe_ingress_renders_the_body_sample() {
        let sample = probe_ingress(Some("application/json; charset=utf-8"), b"{}", 2)
            .expect("the sample renders");
        let sample: serde_json::Value = serde_json::from_str(&sample).expect("JSON");
        assert_eq!(sample["mime"], "application/json");
        assert_eq!(sample["valid_json"], true);
        assert_eq!(sample["truncated"], false);
    }
}
