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
use std::os::fd::IntoRawFd;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use blastit_dbproxy::{
    DbproxyConfig, Emit, ListenerSpec, ProxySpec, read_config, run_with_listeners, run_with_ready,
    write_config,
};
use blastit_events::{Event, EventKind, PgRefuseReason, SqlKind, encode_json};
use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::ToSql;
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
        unrestricted: false,
        tables: TableGrants::default(),
    }
}

/// A listener whose grant is a table grant rather than a list of statements
/// (§6, D7).
fn table_listener(uid: u32, tables: TableGrants) -> ListenerSpec {
    ListenerSpec {
        // Ignored by `run_with_listeners`: the listener is already owned.
        fd: -1,
        route: "restaurants".to_string(),
        proxy: "db".to_string(),
        uid,
        queries: Vec::new(),
        transaction_control: false,
        unrestricted: false,
        tables,
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
async fn an_unrestricted_route_runs_anything_and_its_neighbour_still_gets_42501() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("unrestricted");
    setup_items(&upstream, &items).await;
    let probe = table_for("unrestricted_probe");
    let direct_client = direct(&upstream).await;
    direct_client
        .batch_execute(&format!("DROP TABLE IF EXISTS {probe}"))
        .await
        .expect("the probe table is gone");

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("unrestricted");
    let backoffice = bind(&directory, 5432);
    let catalog = bind(&directory, 5433);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![
            (
                backoffice,
                // `[route.db] unrestricted = true` (ARCHITECTURE.md §6): no
                // queries at all, and everything is allowed.
                ListenerSpec {
                    unrestricted: true,
                    ..listener_spec("backoffice", "db", current_uid(), &[], false)
                },
            ),
            (
                catalog,
                listener_spec("catalog", "db", current_uid(), &[all.as_str()], false),
            ),
        ],
    );

    // The unrestricted route runs statements no listed grant could cover, and
    // its session is still a session through dbproxy.
    let mut admin = connect(&directory, 5432).await.expect("connects");
    admin
        .execute(&format!("CREATE TABLE {probe} (v text)"), &[])
        .await
        .expect("DDL is allowed on an unrestricted route");
    admin
        .query(
            &format!("INSERT INTO {probe} (v) VALUES ($1) RETURNING v"),
            &[&"from-the-back-office"],
        )
        .await
        .expect("an insert is allowed too");
    let transaction = admin.transaction().await.expect("BEGIN as well");
    transaction
        .query(&format!("SELECT id, name FROM {items}"), &[])
        .await
        .expect("and any query");
    transaction.commit().await.expect("COMMIT");

    // The neighbouring route keeps its own list: the same statements are
    // refused there, with 42501.
    let reader = connect(&directory, 5433).await.expect("connects");
    for sql in [
        format!("CREATE TABLE {probe} (v text)"),
        format!("INSERT INTO {probe} (v) VALUES ($1)"),
        "BEGIN".to_string(),
    ] {
        let error = reader
            .query(&sql, &[&"denied"])
            .await
            .expect_err("the catalog route may not run this");
        assert_eq!(sqlstate(&error).as_deref(), Some("42501"), "{sql}");
    }
    reader
        .query(&all, &[])
        .await
        .expect("its own grant still works");

    // The unrestricted route's work reached the server, and the denied
    // statements did not.
    let count = direct_client
        .query_one(&format!("SELECT count(*) FROM {probe}"), &[])
        .await
        .expect("the probe table is readable")
        .get::<_, i64>(0);
    assert_eq!(count, 1);

    // Nothing is logged for the unrestricted route: there is nothing to deny.
    // The reader's three refusals are, one per statement.
    let events = proxy.events();
    assert!(
        events
            .iter()
            .all(|event| event.route() != Some("backoffice")),
        "an unrestricted route logs no per-statement event: {events:#?}"
    );
    let denied = events
        .iter()
        .filter(|event| {
            event.route() == Some("catalog") && matches!(event.kind(), EventKind::SqlDenied(_))
        })
        .count();
    assert_eq!(denied, 3, "{events:#?}");
    for event in &events {
        let line = encode_json(event);
        assert!(
            !line.contains(&upstream.user) && !line.contains("ci-not-secret"),
            "an event leaked the upstream identity: {line}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shapes_a_django_app_sends_match_their_grant() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("shapes");
    setup_items(&upstream, &items).await;

    let one = format!("SELECT id, name FROM {items} WHERE id IN ($1)");
    let insert = format!("INSERT INTO {items} (id, name) VALUES ($1, $2), ($3, $4)");
    let directory = scratch("shapes");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec(
                "notes",
                "db",
                current_uid(),
                &[one.as_str(), insert.as_str()],
                false,
            ),
        )],
    );
    let client = connect(&directory, 5432).await.expect("connects");

    // `__in` and `prefetch_related` grow the list, and every added placeholder
    // renumbers the ones after it.
    let statement = format!("SELECT id, name FROM {items} WHERE id IN ($1, $2, $3)");
    let rows = client
        .query(&statement, &[&1i32, &2i32, &99i32])
        .await
        .expect("the length of the list is not part of the statement");
    assert_eq!(rows.len(), 2);
    let rows = client
        .query(&one, &[&1i32])
        .await
        .expect("one placeholder is the same statement");
    assert_eq!(rows.len(), 1);

    // `bulk_create` sends one row per object.
    let statement = format!("INSERT INTO {items} (id, name) VALUES ($1, $2), ($3, $4), ($5, $6)");
    let inserted = client
        .execute(&statement, &[&10i32, &"x", &11i32, &"y", &12i32, &"z"])
        .await
        .expect("the number of rows is not part of the statement");
    assert_eq!(inserted, 3);
    let count = direct(&upstream)
        .await
        .query_one(&format!("SELECT count(*) FROM {items}"), &[])
        .await
        .expect("the table is readable")
        .get::<_, i64>(0);
    assert_eq!(count, 5, "the three rows were inserted");

    // A statement whose shape differs is still a different statement.
    let error = client
        .query(
            &format!("SELECT id, name FROM {items} WHERE name IN ($1, $2)"),
            &[&"alpha", &"beta"],
        )
        .await
        .expect_err("a different statement is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
    let denied = proxy.denied("notes");
    assert_eq!(
        denied.sql(),
        format!("SELECT id, name FROM {items} WHERE name IN ($?, ...)"),
        "the log carries the shape form, so it can be pasted into the manifest"
    );
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

//
// Tests that need bytes tokio-postgres would never send, or a raw fd.
//

/// A hand-built `StartupMessage`.
fn startup_packet(params: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&196_608i32.to_be_bytes());
    for (name, value) in params {
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
    }
    body.push(0);
    let mut packet = ((body.len() + 4) as i32).to_be_bytes().to_vec();
    packet.extend_from_slice(&body);
    packet
}

/// A hand-built `CancelRequest`.
fn cancel_request(pid: i32, secret: i32) -> Vec<u8> {
    let mut packet = 16i32.to_be_bytes().to_vec();
    packet.extend_from_slice(&80_877_102i32.to_be_bytes());
    packet.extend_from_slice(&pid.to_be_bytes());
    packet.extend_from_slice(&secret.to_be_bytes());
    packet
}

/// `SSLRequest` (80877103).
const SSL_REQUEST: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 47];
/// `GSSENCRequest` (80877104).
const GSSENC_REQUEST: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 48];

/// A connection to a listener, for speaking to dbproxy by hand.
async fn open(directory: &Path, port: u16) -> UnixStream {
    UnixStream::connect(directory.join(format!(".s.PGSQL.{port}")))
        .await
        .expect("the listener accepts")
}

/// Everything the proxy says back before it goes quiet or closes.
async fn read_until_quiet(stream: &mut UnixStream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = [0u8; 8192];
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), stream.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(read)) => out.extend_from_slice(&buffer[..read]),
        }
    }
    out
}

/// Sends `bytes` on a fresh connection and returns the answer.
async fn raw_exchange(directory: &Path, port: u16, bytes: &[u8]) -> Vec<u8> {
    let mut stream = open(directory, port).await;
    stream.write_all(bytes).await.expect("the proxy reads");
    read_until_quiet(&mut stream).await
}

/// Whether `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_leg_refuses_encryption_with_n_and_then_serves_plain_startup() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("tls_negotiation");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("tls-negotiation");
    let listener = bind(&directory, 5432);
    let _proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec("catalog", "db", current_uid(), &[all.as_str()], false),
        )],
    );

    // A view's environment renders `sslmode=disable`, but a driver that tries
    // to encrypt first is answered exactly like a server without TLS.
    assert_eq!(raw_exchange(&directory, 5432, &SSL_REQUEST).await, b"N");
    assert_eq!(raw_exchange(&directory, 5432, &GSSENC_REQUEST).await, b"N");

    // ... and a client that takes the `N` and continues in plaintext gets a
    // session.
    let mut stream = open(&directory, 5432).await;
    stream.write_all(&SSL_REQUEST).await.expect("written");
    let mut answer = [0u8; 1];
    stream.read_exact(&mut answer).await.expect("an answer");
    assert_eq!(answer, [b'N']);
    stream
        .write_all(&startup_packet(&[
            ("user", "blastit"),
            ("database", "blastit"),
            ("client_encoding", "UTF8"),
        ]))
        .await
        .expect("written");
    let answer = read_until_quiet(&mut stream).await;
    assert_eq!(
        answer.first().copied(),
        Some(b'R'),
        "the session starts: {answer:?}"
    );
    assert!(
        answer.contains(&b'Z'),
        "the session is announced: {answer:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_documented_startup_parameters_are_accepted() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let directory = scratch("startup-parameters");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            listener_spec("product-detail", "db", current_uid(), &["SELECT 1"], false),
        )],
    );

    // The parameters a view may send: identity (ignored upstream), the
    // application name (passed on), and a UTF-8 client encoding (dropped).
    let answer = raw_exchange(
        &directory,
        5432,
        &startup_packet(&[
            ("user", "blastit"),
            ("database", "blastit"),
            ("application_name", "django"),
            ("client_encoding", "UTF8"),
        ]),
    )
    .await;
    assert_eq!(answer.first().copied(), Some(b'R'), "accepted: {answer:?}");
    assert!(answer.contains(&b'Z'));

    // Everything else is refused, in particular anything that could reshape the
    // upstream session.
    let value = "a-value-that-must-not-be-quoted";
    let base = [("user", "blastit"), ("database", "blastit")];
    let forbidden: Vec<(&str, Vec<(&str, &str)>)> = vec![
        ("options", vec![("options", value)]),
        ("search_path", vec![("search_path", value)]),
        ("role", vec![("role", value)]),
        (
            "session_authorization",
            vec![("session_authorization", value)],
        ),
        ("replication", vec![("replication", value)]),
        (
            "default_transaction_isolation",
            vec![("default_transaction_isolation", value)],
        ),
        (
            "default_transaction_read_only",
            vec![("default_transaction_read_only", value)],
        ),
        ("extra_float_digits", vec![("extra_float_digits", value)]),
        ("_pq_.anything", vec![("_pq_.anything", value)]),
        // A `client_encoding` that is not UTF-8:
        (
            "client_encoding=LATIN1",
            vec![("client_encoding", "LATIN1")],
        ),
    ];

    for (name, extra) in &forbidden {
        let mut params: Vec<(&str, &str)> = base.to_vec();
        params.extend(extra.iter().copied());
        let answer = raw_exchange(&directory, 5432, &startup_packet(&params)).await;
        assert_eq!(
            answer.first().copied(),
            Some(b'E'),
            "{name}: an ErrorResponse, got {answer:?}"
        );
        assert!(
            contains(&answer, b"28000"),
            "{name}: the refusal is 28000, got {answer:?}"
        );
        assert!(
            !contains(&answer, value.as_bytes()),
            "{name}: the refusal must not quote a value: {answer:?}"
        );
    }

    // One event per refused connection, with the route and the reason.
    let refusals = proxy
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event.kind(),
                EventKind::PgRefused(refused)
                    if refused.reason() == &PgRefuseReason::Other("startup_parameter".to_string())
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(refusals.len(), forbidden.len());
    for event in &refusals {
        assert_eq!(event.route(), Some("product-detail"));
    }
    for event in proxy.events() {
        let line = encode_json(&event);
        assert!(!line.contains(value), "an event quoted a value: {line}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unrestricted_route_still_refuses_forbidden_startup_parameters() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("unrestricted_startup");
    setup_items(&upstream, &items).await;

    let directory = scratch("unrestricted-startup");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(
            listener,
            // A route that may run anything at all on this proxy.
            ListenerSpec {
                unrestricted: true,
                ..listener_spec("backoffice", "db", current_uid(), &[], false)
            },
        )],
    );

    // `unrestricted` is about *statements*: the session is still dbproxy's, so
    // what a client claims in its startup packet still cannot reshape the
    // upstream session it gets (§2.2).
    let value = "a-value-that-must-not-be-quoted";
    let forbidden = [
        "options",
        "search_path",
        "role",
        "session_authorization",
        "default_transaction_read_only",
    ];
    for name in forbidden {
        let answer = raw_exchange(
            &directory,
            5432,
            &startup_packet(&[("user", "blastit"), ("database", "blastit"), (name, value)]),
        )
        .await;
        assert_eq!(
            answer.first().copied(),
            Some(b'E'),
            "{name}: an ErrorResponse, got {answer:?}"
        );
        assert!(
            contains(&answer, b"28000"),
            "{name}: the refusal is 28000, got {answer:?}"
        );
        assert!(
            !contains(&answer, value.as_bytes()),
            "{name}: the refusal must not quote a value: {answer:?}"
        );
    }

    // One `pg.refused` per connection, with the route and the reason, and not a
    // single `sql.denied`: the policy never saw a statement.
    let events = proxy.events();
    let refusals = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                EventKind::PgRefused(refused)
                    if refused.reason() == &PgRefuseReason::Other("startup_parameter".to_string())
            )
        })
        .count();
    assert_eq!(refusals, forbidden.len(), "{events:#?}");
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.kind(), EventKind::SqlDenied(_))),
        "{events:#?}"
    );

    // ... and a client that sends nothing extra gets a session in which the
    // policy really does allow anything.
    let client = connect(&directory, 5432).await.expect("connects");
    client
        .simple_query(&format!("SELECT count(*) FROM {items}"))
        .await
        .expect("an unrestricted route may run this");
    assert!(
        proxy
            .events()
            .iter()
            .all(|event| !matches!(event.kind(), EventKind::SqlDenied(_))),
        "nothing is logged per statement"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn every_connection_to_a_closed_port_is_refused_by_dbroxy() {
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

    // Four connections in a row: each is refused with dbproxy's own message,
    // whatever the probe cache remembers, and each is logged once.
    for attempt in 1..=4 {
        let error = connect(&directory, 5432)
            .await
            .expect_err("the upstream cannot be reached");
        assert_eq!(
            sqlstate(&error).as_deref(),
            Some("08006"),
            "attempt {attempt}"
        );
        let text = format!("{error} {error:?}");
        for leaked in [SENTINEL, "127.0.0.1", &closed.to_string(), &upstream.user] {
            assert!(
                !text.contains(leaked),
                "attempt {attempt} leaked {leaked:?}: {text}"
            );
        }
    }

    let events = proxy.events();
    let refusals = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                EventKind::PgRefused(refused)
                    if refused.reason() == &PgRefuseReason::UpstreamUnreachable
            )
        })
        .count();
    assert_eq!(refusals, 4, "one pg.refused per connection: {events:#?}");
    for event in &events {
        let line = encode_json(event);
        assert!(
            !line.contains(SENTINEL),
            "an event leaked the password: {line}"
        );
        assert!(
            !line.contains(&closed.to_string()),
            "an event named the unreachable upstream: {line}"
        );
        assert!(
            !line.contains(&upstream.user),
            "an event named the upstream role: {line}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upstream_authentication_failure_is_dbroxys_message() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    let directory = scratch("upstream-auth");
    let listener = bind(&directory, 5432);
    // The role exists and the port is open; only the password is wrong, so
    // PostgreSQL's own refusal names the role and pregresql would relay it.
    let mut spec = proxy_spec("db", &upstream, 4);
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
        .expect_err("the proxy refuses the session");
    assert_eq!(sqlstate(&error).as_deref(), Some("08006"));
    let text = format!("{error} {error:?}");
    for leaked in [SENTINEL, upstream.user.as_str(), upstream.host.as_str()] {
        assert!(!text.contains(leaked), "leaked {leaked:?}: {text}");
    }
    assert!(
        message(&error).contains("could not be reached"),
        "dbproxy's own message: {}",
        message(&error)
    );

    // The event says which side failed, and never in the server's words.
    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::PgRefused(_)),
        "pg.refused",
    );
    let EventKind::PgRefused(refused) = event.kind() else {
        unreachable!("filtered for pg.refused")
    };
    assert_eq!(
        refused.reason(),
        &PgRefuseReason::Other("upstream_authentication_failed".to_string())
    );
    for event in proxy.events() {
        let line = encode_json(&event);
        assert!(
            !line.contains(SENTINEL),
            "an event leaked the password: {line}"
        );
        assert!(
            !line.contains(&upstream.user),
            "an event named the upstream role: {line}"
        );
        assert!(
            !line.contains(&upstream.host),
            "an event named the upstream host: {line}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_from_another_route_or_uid_is_refused() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    let directory = scratch("cancel-scope");
    let catalog = bind(&directory, 5432);
    let orders = bind(&directory, 5433);
    let stranger = bind(&directory, 5434);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![
            (
                catalog,
                listener_spec("catalog", "db", current_uid(), &["SELECT 1"], false),
            ),
            (
                orders,
                listener_spec("orders", "db", current_uid(), &["SELECT 1"], false),
            ),
            (
                stranger,
                listener_spec(
                    "stranger",
                    "db",
                    // Any uid but ours.
                    current_uid().wrapping_add(1),
                    &["SELECT 1"],
                    false,
                ),
            ),
        ],
    );

    // A cancel sent to another route: the registry is the listener's own, so
    // the key cannot be found there and nothing is forwarded.
    let answer = raw_exchange(&directory, 5433, &cancel_request(1_000_000, 42)).await;
    assert!(
        answer.is_empty(),
        "a cancel is answered with nothing: {answer:?}"
    );

    // A cancel from a uid that does not own the listener is refused before the
    // key is even looked up.
    let answer = raw_exchange(&directory, 5434, &cancel_request(1_000_000, 42)).await;
    assert!(answer.is_empty(), "{answer:?}");

    let events = proxy.events();
    let unknown = events
        .iter()
        .find(|event| {
            matches!(
                event.kind(),
                EventKind::PgRefused(refused)
                    if refused.reason() == &PgRefuseReason::Other("cancel_unknown_key".to_string())
            )
        })
        .expect("cancel_unknown_key");
    assert_eq!(unknown.route(), Some("orders"));
    let mismatch = events
        .iter()
        .find(|event| {
            matches!(
                event.kind(),
                EventKind::PgRefused(refused)
                    if refused.reason() == &PgRefuseReason::CredentialsMismatch
            )
        })
        .expect("credentials_mismatch");
    assert_eq!(mismatch.route(), Some("stranger"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_copy_is_logged_as_a_copy() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("copy");
    setup_items(&upstream, &items).await;

    let all = format!("SELECT id, name FROM {items}");
    let directory = scratch("copy");
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
        .simple_query(&format!("COPY (SELECT id, name FROM {items}) TO STDOUT"))
        .await
        .expect_err("a COPY is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
    let denied = proxy.denied("catalog");
    assert_eq!(denied.kind(), &SqlKind::Copy);
    assert!(
        denied.sql().starts_with("COPY"),
        "the normalised statement: {}",
        denied.sql()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_application_name_is_the_only_parameter_forwarded() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);

    let directory = scratch("application-name");
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
                &["SHOW application_name"],
                false,
            ),
        )],
    );

    // A client's `application_name` reaches the upstream session; everything
    // else it might have sent was either refused or dropped.
    let mut config = tokio_postgres::Config::new();
    config.host_path(&directory);
    config.port(5432);
    config.user("blastit");
    config.dbname("blastit");
    config.application_name("blastit-b3-test-app");
    let (client, connection) = config.connect(NoTls).await.expect("connects");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let messages = client
        .simple_query("SHOW application_name")
        .await
        .expect("the grant covers this statement");
    assert_eq!(
        simple_rows(&messages)[0].get(0),
        Some("blastit-b3-test-app")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_is_called_once_the_listeners_are_adopted() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let items = table_for("ready");
    setup_items(&upstream, &items).await;
    let all = format!("SELECT id, name FROM {items}");

    let directory = scratch("ready");
    let listener = bind(&directory, 5432);
    // init hands dbproxy a raw fd; the test does the same with a duplicate, so
    // `run_with_ready` owns what it adopts.
    let fd = listener.try_clone().expect("a duplicate").into_raw_fd();
    let spec = ListenerSpec {
        fd,
        ..listener_spec("catalog", "db", current_uid(), &[all.as_str()], false)
    };

    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let emit: Emit = {
        let events = Arc::clone(&events);
        Arc::new(move |event: Event| {
            events.lock().expect("the event log").push(event);
        })
    };
    let config = DbproxyConfig {
        proxies: vec![proxy_spec("db", &upstream, 4)],
        listeners: vec![spec],
    };
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let calls = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&calls);
    let task = tokio::spawn(async move {
        let ready = move || {
            counted.fetch_add(1, Ordering::SeqCst);
            let _ = ready_tx.send(());
        };
        let _ = run_with_ready(config, emit, ready).await;
    });

    // `ready` runs once the listeners are adopted and checked: a client that
    // connects after it is served straight away.
    ready_rx.await.expect("dbproxy reports ready");
    let client = connect(&directory, 5432).await.expect("connects");
    client
        .query(&all, &[])
        .await
        .expect("the grant covers this statement");
    task.abort();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "ready is called once");
    assert!(
        events
            .lock()
            .expect("the event log")
            .iter()
            .all(|event| !matches!(event.kind(), EventKind::SqlDenied(_))),
        "nothing was denied"
    );
}

/// A route whose grant is a table grant, against the real PostgreSQL (D7, §6):
/// the shape a Django list endpoint sends runs whatever its filters are, and a
/// column the grant does not name is refused before it reaches the server.
#[tokio::test(flavor = "multi_thread")]
async fn table_grants_run_a_django_shaped_join_and_refuse_a_column() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let product = table_for("d7_product");
    let category = table_for("d7_category");
    let client = direct(&upstream).await;
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {product}; DROP TABLE IF EXISTS {category}; \
             CREATE TABLE {category} (id int primary key, name text); \
             CREATE TABLE {product} (id int primary key, name text, price numeric, \
                                      category_id int references {category}(id)); \
             INSERT INTO {category} VALUES (1, 'food'); \
             INSERT INTO {product} VALUES (1, 'bread', 2.5, 1), (2, 'milk', 1.5, 1)"
        ))
        .await
        .expect("the fixture tables are created");

    let tables = TableGrants::new(
        [
            (
                product.clone(),
                TableGrant {
                    select: Some(Columns::list(["id", "name", "category_id"])),
                    insert: Some(Columns::list(["id", "name", "category_id"])),
                    ..TableGrant::default()
                },
            ),
            (
                category.clone(),
                TableGrant {
                    select: Some(Columns::list(["id", "name"])),
                    ..TableGrant::default()
                },
            ),
        ],
        [],
    );
    let directory = scratch("tables");
    let listener = bind(&directory, 5432);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![(listener, table_listener(current_uid(), tables))],
    );
    let client = connect(&directory, 5432).await.expect("connects");

    // One join, four filters: four statements, none of them listed anywhere.
    let join = format!(
        r#"SELECT "{product}"."id", "{product}"."name", "{category}"."name" FROM "{product}" INNER JOIN "{category}" ON ("{product}"."category_id" = "{category}"."id") WHERE "#
    );
    for filter in [
        format!(r#""{product}"."id" = $1"#),
        format!(r#""{product}"."name" LIKE $1"#),
        format!(r#"("{product}"."name" LIKE $1 AND "{product}"."category_id" = $2)"#),
        format!(r#""{category}"."name" = $1"#),
    ] {
        let statement = format!("{join}{filter} ORDER BY \"{product}\".\"id\" ASC LIMIT 21");
        let text = "bread";
        let one = 1i32;
        let params: Vec<&(dyn ToSql + Sync)> = if filter.contains(r#""id" = $1"#) {
            vec![&one]
        } else if filter.contains("$2") {
            vec![&text, &one]
        } else {
            vec![&text]
        };
        // The row count does not matter: what matters is that the statement
        // reached PostgreSQL instead of a 42501.
        client
            .query(&statement, &params)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    // An INSERT the grant covers (the `price` column is not in it), and one it
    // does not.
    let insert =
        format!(r#"INSERT INTO "{product}" ("id", "name", "category_id") VALUES ($1, $2, $3)"#);
    client
        .execute(&insert, &[&3i32, &"eggs", &1i32])
        .await
        .expect("the grant's columns are written");
    let error = client
        .execute(
            &format!(r#"INSERT INTO "{product}" ("id", "name", "category_id", "price") VALUES ($1, $2, $3, $4)"#),
            &[&4i32, &"cheese", &1i32, &9.5],
        )
        .await
        .expect_err("a column the grant does not name is refused");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));

    // Reading a column the grant does not name is refused too, and the log says
    // which one.
    let error = client
        .query(
            &format!(r#"SELECT "{product}"."price" FROM "{product}""#),
            &[],
        )
        .await
        .expect_err("a column the grant does not name is refused");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
    let event = proxy.event(
        |event| {
            matches!(event.kind(), EventKind::SqlDenied(denied)
                if denied.sql().starts_with("SELECT") && denied.sql().contains("price"))
        },
        "the denied price SELECT",
    );
    let EventKind::SqlDenied(denied) = event.kind() else {
        unreachable!("filtered for sql.denied")
    };
    assert_eq!(
        denied.reason(),
        format!("select on {product}.price not granted")
    );
    assert_eq!(
        denied.sql(),
        format!(r#"SELECT "{product}"."price" FROM "{product}""#)
    );

    // A table the grant does not name at all is refused as well.
    let error = client
        .query("SELECT name FROM pg_class", &[])
        .await
        .expect_err("another table is not granted");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
}

/// A locking clause hidden behind a derived relation reaches the base table
/// (D7): `SELECT 1 FROM (SELECT 1 FROM t) s FOR UPDATE` locks `t`'s rows even
/// though the outer statement reads no column of `t`, so a grant without
/// `update` on `t` must refuse it — and one with `update` on **any one column**
/// must let it run (D7b), which is what PostgreSQL asks for to lock a row.
#[tokio::test(flavor = "multi_thread")]
async fn table_grants_refuse_a_lock_hidden_behind_a_subquery() {
    let Some(url) = pg_url() else { return };
    let upstream = upstream(&url);
    let table = table_for("d7_locked");
    let client = direct(&upstream).await;
    client
        .batch_execute(&format!(
            "DROP TABLE IF EXISTS {table}; \
             CREATE TABLE {table} (id int primary key, name text); \
             INSERT INTO {table} VALUES (1, 'alpha'), (2, 'beta')"
        ))
        .await
        .expect("the fixture table is created");

    let read_only = TableGrants::new(
        [(
            table.clone(),
            TableGrant {
                select: Some(Columns::All),
                ..TableGrant::default()
            },
        )],
        [],
    );
    // One column is enough to lock the rows: `"*"` is not needed (D7b).
    let mut locking = read_only.clone();
    locking.tables.entry(table.clone()).or_default().update = Some(Columns::list(["name"]));

    let directory = scratch("lock");
    let listener = bind(&directory, 5432);
    let other = bind(&directory, 5433);
    let proxy = start(
        directory.clone(),
        vec![proxy_spec("db", &upstream, 4)],
        vec![
            (listener, table_listener(current_uid(), read_only)),
            (other, table_listener(current_uid(), locking)),
        ],
    );

    let sql = format!("SELECT 1 FROM (SELECT 1 FROM {table}) s FOR UPDATE");

    // Without `update`, the statement is refused before it reaches the server.
    let client = connect(&directory, 5432).await.expect("connects");
    let error = client
        .query(&sql, &[])
        .await
        .expect_err("a lock through a subquery needs update");
    assert_eq!(sqlstate(&error).as_deref(), Some("42501"));
    let event = proxy.event(
        |event| matches!(event.kind(), EventKind::SqlDenied(denied) if denied.sql() == sql),
        "the denied lock",
    );
    let EventKind::SqlDenied(denied) = event.kind() else {
        unreachable!("filtered for sql.denied")
    };
    assert_eq!(
        denied.reason(),
        format!("update on {table} (any column, to lock its rows) not granted")
    );

    // With `update` on one column, it reaches PostgreSQL and takes the lock.
    let client = connect(&directory, 5433).await.expect("connects");
    client
        .query(&sql, &[])
        .await
        .expect("one column is enough for the grant to cover the lock");
}
