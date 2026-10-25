//! dbproxy against a real PostgreSQL (`ROADMAP.md` B3).
//!
//! Every test but the configuration round-trip needs `BLASTIT_TEST_PG_URL` — CI
//! sets `postgresql://blastit:ci-not-secret@127.0.0.1:5432/blastit`, and a local
//! container serves the same credentials on port 55433. Without it the tests
//! print a note and pass, except under CI, where an unset variable is a
//! failure: silently skipping the only tests that exercise the upstream would
//! make CI lie.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpListener;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blastit_dbproxy::{
    DbproxyConfig, Emit, ListenerSpec, ProxySpec, read_config, run_with_listeners, write_config,
};
use blastit_events::{Event, EventKind, PgRefuseReason, SqlKind, encode_json};
use tokio_postgres::error::SqlState;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage, SimpleQueryRow};

/// A password that must never appear in an event or an error.
const SENTINEL: &str = "blastit-b3-sentinel-never-log-me";

/// The PostgreSQL to test against, or `None` when the test must skip.
fn pg_url() -> Option<String> {
    match std::env::var("BLASTIT_TEST_PG_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            assert!(
                std::env::var("CI").is_err(),
                "BLASTIT_TEST_PG_URL must be set in CI: the PostgreSQL integration tests would \
                 otherwise be skipped and CI would be lying"
            );
            eprintln!(
                "BLASTIT_TEST_PG_URL is not set; skipping the PostgreSQL integration tests \
                 (start a container, for example \
                 `docker run -e POSTGRES_USER=blastit -e POSTGRES_PASSWORD=ci-not-secret \
                 -e POSTGRES_DB=blastit -p 55433:5432 postgres:18`, and set \
                 BLASTIT_TEST_PG_URL=postgresql://blastit:ci-not-secret@127.0.0.1:55433/blastit)"
            );
            None
        }
    }
}

/// The upstream a test URL points at.
#[derive(Clone)]
struct Upstream {
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

fn upstream(url: &str) -> Upstream {
    let config: tokio_postgres::Config = url.parse().expect("a libpq URL");
    let host = match config.get_hosts().first() {
        Some(tokio_postgres::config::Host::Tcp(host)) => host.clone(),
        other => panic!("BLASTIT_TEST_PG_URL must be a TCP URL, got {other:?}"),
    };
    Upstream {
        host,
        port: config.get_ports().first().copied().unwrap_or(5432),
        user: config.get_user().unwrap_or("postgres").to_string(),
        password: config
            .get_password()
            .map(|password| String::from_utf8_lossy(password).into_owned())
            .unwrap_or_default(),
        database: config.get_dbname().unwrap_or("postgres").to_string(),
    }
}

/// The uid this test runs as. `/proc/self` is owned by it, and it is the uid
/// the kernel attaches to the process's `SCM_CREDENTIALS`.
fn current_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .expect("/proc/self is readable")
        .uid()
}

/// A unique, short scratch directory (Unix socket paths are limited to about
/// 108 bytes).
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("blastit-b3-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

/// A table name unique to one test: the tests share a database and run in
/// parallel.
fn table_for(test: &str) -> String {
    format!("b3_{test}")
}

/// Binds `.s.PGSQL.<port>` for a test, with `SO_PASSCRED` set *before* any
/// client can connect: the kernel only stamps the credentials of a message sent
/// before `accept()` if the listener already had the flag.
fn bind(directory: &Path, port: u16) -> StdUnixListener {
    let listener = StdUnixListener::bind(directory.join(format!(".s.PGSQL.{port}")))
        .expect("the test listener binds");
    pregresql::proxy::stream::set_passcred(&listener).expect("SO_PASSCRED");
    listener
}

/// A proxy process, running in this test's runtime.
struct Proxy {
    directory: PathBuf,
    events: Arc<Mutex<Vec<Event>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("the event log").clone()
    }

    /// The one event matching `predicate`, or a panic naming what was seen.
    fn event(&self, predicate: impl Fn(&Event) -> bool, what: &str) -> Event {
        let events = self.events();
        match events.iter().find(|event| predicate(event)) {
            Some(event) => event.clone(),
            None => panic!("no {what} event; saw {events:#?}"),
        }
    }

    /// The only event matching `predicate`; a second one is a failure.
    fn denied(&self, route: &str) -> blastit_events::SqlDenied {
        let events = self.events();
        let mut denials: Vec<&Event> = events
            .iter()
            .filter(|event| {
                event.route() == Some(route) && matches!(event.kind(), EventKind::SqlDenied(_))
            })
            .collect();
        assert_eq!(denials.len(), 1, "want one denial on {route}: {events:#?}");
        match denials.remove(0).kind() {
            EventKind::SqlDenied(denied) => denied.clone(),
            _ => unreachable!("filtered for sql.denied"),
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Starts dbproxy over `listeners`.
fn start(
    directory: PathBuf,
    proxies: Vec<ProxySpec>,
    listeners: Vec<(StdUnixListener, ListenerSpec)>,
) -> Proxy {
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let emit: Emit = {
        let events = Arc::clone(&events);
        Arc::new(move |event: Event| {
            events.lock().expect("the event log").push(event);
        })
    };
    let config = DbproxyConfig {
        proxies,
        // The listeners are handed over directly, as `run_with_listeners`
        // documents.
        listeners: Vec::new(),
    };
    let task = tokio::spawn(async move {
        let _ = run_with_listeners(config, listeners, emit).await;
    });
    Proxy {
        directory,
        events,
        task,
    }
}

/// Connects through the proxy at `<directory>/.s.PGSQL.<port>`.
async fn connect(directory: &Path, port: u16) -> Result<Client, tokio_postgres::Error> {
    let mut config = tokio_postgres::Config::new();
    config.host_path(directory);
    config.port(port);
    config.user("blastit");
    config.dbname("blastit");
    let (client, connection) = config.connect(NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Connects straight to the upstream, for setup and for checking side effects.
async fn direct(upstream: &Upstream) -> Client {
    let mut config = tokio_postgres::Config::new();
    config.host(&upstream.host);
    config.port(upstream.port);
    config.user(&upstream.user);
    config.password(&upstream.password);
    config.dbname(&upstream.database);
    let (client, connection) = config.connect(NoTls).await.expect("the upstream connects");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn proxy_spec(name: &str, upstream: &Upstream, max_connections: u32) -> ProxySpec {
    ProxySpec {
        name: name.to_string(),
        host: upstream.host.clone(),
        port: upstream.port,
        user: upstream.user.clone(),
        password: upstream.password.clone().into(),
        database: upstream.database.clone(),
        // libpq's default: try TLS, fall back to plaintext, do not verify.
        sslmode: None,
        ca_cert: None,
        options: Vec::new(),
        max_connections,
    }
}

fn listener_spec(
    route: &str,
    proxy: &str,
    uid: u32,
    queries: &[&str],
    transaction_control: bool,
) -> ListenerSpec {
    ListenerSpec {
        // Ignored by `run_with_listeners`: the listener is already owned.
        fd: -1,
        route: route.to_string(),
        proxy: proxy.to_string(),
        uid,
        queries: queries.iter().map(|query| query.to_string()).collect(),
        transaction_control,
    }
}

/// The SQLSTATE tokio-postgres saw, when the error came from a server.
fn sqlstate(error: &tokio_postgres::Error) -> Option<String> {
    error.code().map(SqlState::code).map(str::to_string)
}

/// The server's own message, when the error came from a server.
fn message(error: &tokio_postgres::Error) -> String {
    error
        .as_db_error()
        .map(|db| db.message().to_string())
        .unwrap_or_else(|| error.to_string())
}

fn simple_rows(messages: &[SimpleQueryMessage]) -> Vec<&SimpleQueryRow> {
    messages
        .iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .collect()
}

/// Creates the table the policy tests read, with two rows.
async fn setup_items(upstream: &Upstream, table: &str) {
    let client = direct(upstream).await;
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {table}; \
             CREATE TABLE {table} (id int primary key, name text); \
             INSERT INTO {table} VALUES (1, 'alpha'), (2, 'beta')"
        ))
        .await
        .expect("the fixture table is created");
}

#[tokio::test(flavor = "multi_thread")]
async fn granted_statements_run_over_both_protocols_and_the_session_is_plain() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("granted");
    setup_items(&upstream, &items).await;

    let one = format!("SELECT id, name FROM {items} WHERE id = $1");
    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("granted");
    let listener = bind(&directory, 5432);
    let spec = listener_spec(
        "product-detail",
        "db",
        current_uid(),
        &[
            one.as_str(),
            all.as_str(),
            "SHOW standard_conforming_strings",
        ],
        false,
    );
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(listener, spec)],
    );

    let client = connect(&directory, 5432)
        .await
        .expect("the proxy serves its listener");

    // The extended protocol, with a parameter bound separately from the SQL.
    let rows = client
        .query(&one, &[&1i32])
        .await
        .expect("the grant covers this statement");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<_, i32>("id"), 1);
    assert_eq!(rows[0].get::<_, &str>("name"), "alpha");

    // The simple protocol goes through the same policy.
    let messages = client
        .simple_query(&all)
        .await
        .expect("the grant covers this statement");
    assert_eq!(simple_rows(&messages).len(), 2);

    // §6 assumes `standard_conforming_strings = on`; dbproxy forces it on the
    // upstream session, and the server reports it back to the view.
    let messages = client
        .simple_query("SHOW standard_conforming_strings")
        .await
        .expect("the grant covers this statement");
    assert_eq!(
        simple_rows(&messages)[0].get(0),
        Some("on"),
        "the upstream session must be forced to standard_conforming_strings=on"
    );

    assert!(
        proxy
            .events()
            .iter()
            .all(|event| !matches!(event.kind(), EventKind::SqlDenied(_))),
        "a granted statement must not be denied: {:#?}",
        proxy.events()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ungranted_statement_is_denied_with_42501_and_logged_normalised() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("ungranted");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("denied");
    let listener = bind(&directory, 5432);
    let spec = listener_spec(
        "product-detail",
        "db",
        current_uid(),
        &[all.as_str()],
        false,
    );
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(listener, spec)],
    );
    let client = connect(&directory, 5432).await.expect("connects");

    // Sent with extra whitespace, so the event has to carry the *normalised*
    // text rather than what went over the wire.
    let error = client
        .query(&format!("  SELECT   name\nFROM {items}  "), &[])
        .await
        .expect_err("the statement is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
    assert!(
        message(&error).contains("product-detail"),
        "the refusal must name the route: {}",
        message(&error)
    );

    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::SqlDenied(_)),
        "sql.denied",
    );
    assert_eq!(event.route(), Some("product-detail"));
    let EventKind::SqlDenied(denied) = event.kind() else {
        unreachable!("filtered for sql.denied")
    };
    assert_eq!(denied.sql(), format!("SELECT name FROM {items}"));
    assert_eq!(denied.kind(), &SqlKind::Parse);
    assert_eq!(denied.proxy(), "db");
    assert_eq!(denied.sqlstate(), "42501");
    assert!(!denied.in_transaction());
    assert_eq!(
        denied.pid(),
        std::process::id(),
        "the pid is the view process's, from SCM_CREDENTIALS"
    );
    // The normalised SQL carries no parameter values, by construction.
    assert!(!encode_json(&event).contains("alpha"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_statement_never_reaches_the_upstream() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("side_effect");
    let probe = table_for("probe");
    setup_items(&upstream, &items).await;
    let direct_client = direct(&upstream).await;
    direct_client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {probe}; CREATE TABLE {probe} (v text)"
        ))
        .await
        .expect("the probe table is created");

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("side-effect");
    let listener = bind(&directory, 5432);
    let spec = listener_spec("checkout", "db", current_uid(), &[all.as_str()], false);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(listener, spec)],
    );
    let client = connect(&directory, 5432).await.expect("connects");

    let error = client
        .query(
            &format!("INSERT INTO {probe} (v) VALUES ($1)"),
            &[&"denied"],
        )
        .await
        .expect_err("the insert is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    // The side-effect table is empty, seen through a connection that does not
    // go through the proxy at all.
    let count = direct_client
        .query_one(&format!("SELECT count(*) FROM {probe}"), &[])
        .await
        .expect("the probe table is readable");
    assert_eq!(
        count.get::<_, i64>(0),
        0,
        "a denied statement must never reach the server"
    );
    assert!(
        proxy
            .events()
            .iter()
            .any(|event| matches!(event.kind(), EventKind::SqlDenied(_)))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn transaction_control_is_granted_only_where_it_is_listed() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("transactions");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("transactions");
    let granted = bind(&directory, 5432);
    let refused = bind(&directory, 5433);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![
            (
                granted,
                listener_spec("session", "db", current_uid(), &[all.as_str()], true),
            ),
            (
                refused,
                listener_spec("about", "db", current_uid(), &[all.as_str()], false),
            ),
        ],
    );

    let mut client = connect(&directory, 5432).await.expect("connects");
    let transaction = client
        .transaction()
        .await
        .expect("BEGIN is granted on this route");
    transaction.commit().await.expect("COMMIT is granted too");

    let mut client = connect(&directory, 5433).await.expect("connects");
    let outcome = client.transaction().await;
    let error = match outcome {
        Ok(_) => panic!("BEGIN must not be granted on this route"),
        Err(error) => error,
    };
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    let denied = proxy.denied("about");
    assert!(
        matches!(denied.sql(), "BEGIN" | "START TRANSACTION"),
        "the denial must be the transaction-control statement, got {:?}",
        denied.sql()
    );
    assert!(!denied.in_transaction());
    assert!(
        proxy
            .events()
            .iter()
            .all(|event| event.route() != Some("session")),
        "the route that grants transaction control must not be denied anything"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_multi_statement_simple_query_is_denied() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("multi_statement");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("multi-statement");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec("catalog", "db", current_uid(), &[all.as_str()], false),
        )],
    );
    let client = connect(&directory, 5432).await.expect("connects");

    let error = client
        .simple_query(&format!("{all}; DELETE FROM {items}"))
        .await
        .expect_err("the second statement is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    let remaining = client
        .simple_query(&all)
        .await
        .expect("the grant still matches on its own");
    assert_eq!(simple_rows(&remaining).len(), 2, "nothing was deleted");
    let denied = proxy.denied("catalog");
    assert_eq!(denied.sql(), format!("{all}; DELETE FROM {items}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_routes_on_one_proxy_have_separate_allowlists() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("two_routes");
    setup_items(&upstream, &items).await;

    let both = format!("SELECT id, name FROM {items}");
    let id_only = format!("SELECT id FROM {items}");
    let directory = scratch("two-routes");
    let catalog = bind(&directory, 5432);
    let orders = bind(&directory, 5433);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![
            (
                catalog,
                listener_spec("catalog", "db", current_uid(), &[both.as_str()], false),
            ),
            (
                orders,
                listener_spec("orders", "db", current_uid(), &[id_only.as_str()], false),
            ),
        ],
    );

    let catalog_client = connect(&directory, 5432).await.expect("connects");
    catalog_client
        .query(&both, &[])
        .await
        .expect("the catalog route may read both columns");
    let error = catalog_client
        .query(&id_only, &[])
        .await
        .expect_err("the catalog route may not run the orders statement");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    let orders_client = connect(&directory, 5433).await.expect("connects");
    orders_client
        .query(&id_only, &[])
        .await
        .expect("the orders route may read the id");
    let error = orders_client
        .query(&both, &[])
        .await
        .expect_err("the orders route may not run the catalog statement");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    let events = proxy.events();
    let routes: Vec<&str> = events
        .iter()
        .filter_map(|event| {
            matches!(event.kind(), EventKind::SqlDenied(_))
                .then(|| event.route())
                .flatten()
        })
        .collect();
    assert_eq!(routes, ["catalog", "orders"], "one denial per route");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_from_another_uid_is_refused() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    let directory = scratch("credentials");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec(
                "product-detail",
                "db",
                // Any uid but our own: the connection carries ours.
                current_uid().wrapping_add(1),
                &["SELECT 1"],
                false,
            ),
        )],
    );

    let error = connect(&directory, 5432)
        .await
        .expect_err("the listener does not belong to this uid");
    assert_eq!(sqlstate(&error).as_deref(), Some("28000"));
    assert!(
        !message(&error).contains(&upstream.host),
        "the refusal must not name the upstream: {}",
        message(&error)
    );

    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::PgRefused(_)),
        "pg.refused",
    );
    assert_eq!(event.route(), Some("product-detail"));
    let EventKind::PgRefused(refused) = event.kind() else {
        unreachable!("filtered for pg.refused")
    };
    assert_eq!(refused.proxy(), "db");
    assert_eq!(refused.reason(), &PgRefuseReason::CredentialsMismatch);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_connection_cap_refuses_with_53300_and_releases_its_slot() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("cap");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("cap");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 1)],
        vec![(
            listener,
            listener_spec("catalog", "db", current_uid(), &[all.as_str()], false),
        )],
    );

    let first = connect(&directory, 5432)
        .await
        .expect("the first session fits under the cap");
    let error = connect(&directory, 5432)
        .await
        .expect_err("the second session does not");
    assert_eq!(sqlstate(&error).as_deref(), Some("53300"));

    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::PgRefused(_)),
        "pg.refused",
    );
    assert_eq!(event.route(), Some("catalog"));
    let EventKind::PgRefused(refused) = event.kind() else {
        unreachable!("filtered for pg.refused")
    };
    assert_eq!(refused.proxy(), "db");
    assert_eq!(refused.reason(), &PgRefuseReason::TooManyConnections);

    // The slot comes back when the session ends: the cap is a cap, not a
    // lifetime budget.
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match connect(&directory, 5432).await {
            Ok(client) => {
                drop(client);
                break;
            }
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the connection slot was never released: {error}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_upstream_is_refused_without_naming_it() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    // A port nothing listens on: bound and released, so the kernel hands it to
    // nobody in between.
    let closed = {
        let probe = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
        probe.local_addr().expect("an address").port()
    };
    let directory = scratch("unreachable");
    let listener = bind(&directory, 5432);
    // The proxy dials a port nobody listens on; its password is a sentinel
    // that nothing may ever print.
    let mut spec = proxy_spec("db", &upstream, 4);
    spec.host = "127.0.0.1".to_string();
    spec.port = closed;
    spec.password = SENTINEL.to_string().into();
    let proxy = start(
        directory.clone(),
        vec![spec],
        vec![(
            listener,
            listener_spec("catalog", "db", current_uid(), &["SELECT 1"], false),
        )],
    );

    let error = connect(&directory, 5432)
        .await
        .expect_err("the upstream cannot be reached");
    assert_eq!(sqlstate(&error).as_deref(), Some("08006"));
    assert!(
        !message(&error).contains(SENTINEL),
        "the refusal must not carry the password: {}",
        message(&error)
    );
    assert!(
        !message(&error).contains("127.0.0.1"),
        "the refusal must not name the upstream: {}",
        message(&error)
    );

    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::PgRefused(_)),
        "pg.refused",
    );
    let EventKind::PgRefused(refused) = event.kind() else {
        unreachable!("filtered for pg.refused")
    };
    assert_eq!(refused.reason(), &PgRefuseReason::UpstreamUnreachable);

    for event in proxy.events() {
        let line = encode_json(&event);
        assert!(
            !line.contains(SENTINEL),
            "an event leaked the password: {line}"
        );
        assert!(
            !line.contains(&closed.to_string()),
            "an event named the unreachable upstream: {line}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_query_is_forwarded_upstream() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    let directory = scratch("cancel");
    let listener = bind(&directory, 5432);
    let _proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec(
                "catalog",
                "db",
                current_uid(),
                &["SELECT pg_sleep(10)"],
                false,
            ),
        )],
    );
    let client = connect(&directory, 5432).await.expect("connects");
    let token = client.cancel_token();

    let query = tokio::spawn(async move { client.query("SELECT pg_sleep(10)", &[]).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    token
        .cancel_query(NoTls)
        .await
        .expect("the cancel connects");

    let error = query
        .await
        .expect("the query task finishes")
        .expect_err("the sleep is cancelled");
    assert_eq!(
        sqlstate(&error).as_deref(),
        Some("57014"),
        "a cancel must reach the upstream backend: {}",
        message(&error)
    );
}

#[test]
fn the_configuration_round_trips_and_never_prints_its_secrets() {
    let config = DbproxyConfig {
        proxies: vec![ProxySpec {
            name: "db".to_string(),
            host: "db.internal".to_string(),
            port: 25060,
            user: "doadmin".to_string(),
            password: SENTINEL.to_string().into(),
            database: "defaultdb".to_string(),
            sslmode: Some("verify-full".to_string()),
            ca_cert: Some(
                "-----BEGIN CERTIFICATE-----\nsentinel\n-----END CERTIFICATE-----"
                    .to_string()
                    .into(),
            ),
            options: vec![("application_name".to_string(), "blastit".to_string())],
            max_connections: 8,
        }],
        listeners: vec![listener_spec(
            "product-detail",
            "db",
            21_000,
            &["SELECT 1"],
            false,
        )],
    };

    // Through a real pipe, exactly as init writes it.
    let (reader, mut writer) = std::io::pipe().expect("a pipe");
    let expected = config.clone();
    let writer_thread = std::thread::spawn(move || {
        write_config(&expected, &mut writer).expect("written to the pipe");
    });
    let read = read_config(reader).expect("read back");
    writer_thread.join().expect("the writer thread");
    assert_eq!(read, config);

    // Nothing printed or reported may carry a credential.
    let printed = format!("{:?}", config);
    assert!(!printed.contains(SENTINEL), "{printed}");
    assert!(!printed.contains("sentinel"), "{printed}");
    assert!(printed.contains("<redacted>"), "{printed}");
}
