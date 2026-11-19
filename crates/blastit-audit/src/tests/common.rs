//! Fixtures and report accessors every `audit` suite shares.

use serde_json::Value;

use crate::audit;

pub(super) const MANIFEST: &str = r#"
postgres $DATABASE_URL

trait session {
    postgres {
        transactions
        query 'SELECT "django_session"."session_key" FROM "django_session" WHERE "django_session"."session_key" = $?'
    }
}

view notes GET, POST "/notes/*" {
    use session
    postgres {
        query 'SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21'
    }
}

view checkout "/checkout/" {
    postgres {
        query 'INSERT INTO "orders" ("id") VALUES ($?)'
    }
}
"#;

/// A `Blastfile` whose only store is `db`, without the session trait.
pub(super) const SIMPLE: &str = r#"
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT "notes"."id" FROM "notes" WHERE "notes"."owner_id" = $? LIMIT 21'
    }
}
"#;

/// Runs the audit on a report and returns the parsed JSON.
pub(super) fn run(manifest: &str, observations: &str) -> Value {
    let json = audit(manifest, observations).expect("the audit runs");
    serde_json::from_str(&json).expect("the report is JSON")
}

/// The observations document for one request.
pub(super) fn document(requests: &str) -> String {
    format!(r#"{{"version": 1, "requests": [{requests}]}}"#)
}

/// The observations document for a request that opened connections.
pub(super) fn document_with_egress(requests: &str, egress: &str) -> String {
    format!(
        r#"{{"version": 1, "requests": [{{"method": "GET", "path_info": "{requests}",
                "nodeid": "tests/test_app.py::test_it", "statements": [], "egress": [{egress}]}}]}}"#
    )
}

/// One observed request.
pub(super) fn request(method: &str, path: &str, statements: &str) -> String {
    format!(
        r#"{{"method": "{method}", "path_info": "{path}",
                "nodeid": "tests/test_app.py::test_it", "transaction_control": false,
                "statements": [{statements}]}}"#
    )
}

/// One observed statement.
pub(super) fn statement(sql: &str) -> String {
    format!(r#"{{"alias": "default", "proxy": "db", "sql": "{sql}"}}"#)
}

/// The route named `name` in a report.
pub(super) fn route<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["routes"]
        .as_array()
        .expect("routes is a list")
        .iter()
        .find(|route| route["name"] == name)
        .expect("the route is in the report")
}

/// The `proxy` block of one route.
pub(super) fn proxy<'a>(report: &'a Value, name: &str) -> &'a Value {
    &route(report, name)["proxies"][0]
}

/// The SQL of every `missing` entry, in report order.
pub(super) fn missing_sql(report: &Value, name: &str) -> Vec<String> {
    proxy(report, name)["missing"]
        .as_array()
        .expect("missing is a list")
        .iter()
        .map(|entry| entry["sql"].as_str().expect("a string").to_string())
        .collect()
}
