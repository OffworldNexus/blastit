//! `blastit._blastit`: the pyo3 extension a wheel installs (D2, §2.1, §4).
//!
//! The module is deliberately tiny and one function wide. Everything blastit
//! does lives in the `blastit` library; the only thing a wheel changes about it
//! is where the program image comes from:
//!
//! * the console script runs on the interpreter the wheel was installed into —
//!   the project's virtualenv — so the application's dependencies are importable
//!   for the same reason they are for `python app.py`;
//! * stage 0 re-executes that interpreter for stage 1 (`python -I -m blastit`,
//!   the arguments the standalone binary passes to `/proc/self/exe`) and carries
//!   the interpreter's `sys.path` across the barrier with it;
//! * dbproxy is `execve`'d as the Python-free binary the wheel ships, because
//!   the process holding the credentials must run no interpreter at all.
//!
//! Importing the module starts nothing: no thread, no interpreter, no file is
//! touched until the `main` function is called. Tools that never serve anything
//! (`pip`, `uv`, `python -c 'import blastit'`) run on such a wheel, and stage 1
//! must find the process exactly as its parent left it.
//!
//! Two more functions serve the pytest plugin (D3b): `normalize`, blastit's own
//! SQL normalisation, and `audit`, which diffs what a test suite observed
//! against what a manifest grants. Both are pure: they parse, they compare, they
//! return — no file, no connection, nothing to clean up.

mod audit;

use std::path::PathBuf;

use blastit::{Host, cli};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Runs blastit on this interpreter: the whole console script (§4).
///
/// `argv` is `sys.argv[1:]`, the `--key=value` arguments [`cli::main`] parses;
/// `executable` is `sys.executable`, the interpreter stage 0 re-executes for
/// stage 1; `dbproxy` is the path of the Python-free `blastit-dbproxy` binary
/// the wheel ships next to this module.
///
/// # Errors
///
/// [`PyValueError`] when the installation is incoherent — an empty
/// `sys.executable`, or a `dbproxy` binary that is not there. That is the one
/// failure the wheel itself can cause, and Python names it better than a
/// half-started sandbox can.
///
/// # Returns
///
/// The process exit status. Stage 0 and stage 1 never return at all: the server
/// becomes this process, or it fails and exits itself.
#[pyfunction]
fn main(py: Python<'_>, argv: Vec<String>, executable: String, dbproxy: String) -> PyResult<u8> {
    let executable = PathBuf::from(executable);
    if executable.as_os_str().is_empty() {
        return Err(PyValueError::new_err(
            "sys.executable is empty: blastit has no interpreter to re-execute for stage 1",
        ));
    }
    let dbproxy = PathBuf::from(dbproxy);
    if !dbproxy.is_file() {
        return Err(PyValueError::new_err(format!(
            "the dbproxy binary this installation ships is missing: {}",
            dbproxy.display()
        )));
    }
    Ok(cli::main(
        Host::wheel(
            executable,
            dbproxy,
            plain_interpreter(py)?,
            interpreter_path(py)?,
        ),
        argv,
        Some(py),
    ))
}

/// Every name of the interpreter a **child** of init must use, if this
/// installation can name them (D6/B1).
///
/// A rootless wheel install starts blastit from a dedicated copy of the
/// interpreter that carries the boot's file capabilities, and `sys.executable`
/// is then that copy: running it is what turns any process of init's uid into
/// `init` (`setuid(0)` and all). The project's own interpreter — the one a
/// developer's `python`, `manage.py` or a worker uses — is `sys.prefix/bin/python`,
/// and that is the file a child gets. It is the *same* file as `sys.executable`
/// under an ambient-capability deployment, which is a perfectly good deployment:
/// nothing about that file grants anything then.
///
/// The list holds **every** name that interpreter can be started by: `python`,
/// `python3`, `python3.X`, `sys._base_executable`, and the target
/// `sys.executable` resolves to. A capability is a property of the file, and a
/// file reachable under several names is exactly how `setcap python3.13` leaks
/// into `python` (`host::placement` refuses any name that carries one).
///
/// Empty when this interpreter cannot name such a file. The boot then refuses
/// only if the running file turns out to carry file capabilities, which is
/// exactly the case where the name is needed — and `placement` is the one that
/// says so, with the recipe.
fn plain_interpreter(py: Python<'_>) -> PyResult<Vec<PathBuf>> {
    let sys = py.import("sys")?;
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    };
    if let Ok(prefix) = sys.getattr("prefix")?.extract::<String>() {
        let bin = PathBuf::from(prefix).join("bin");
        push(bin.join("python"));
        push(bin.join("python3"));
        // `python3.X`, for whichever X this interpreter is.
        let version = sys.getattr("version_info")?;
        let major: u32 = version.getattr("major")?.extract()?;
        let minor: u32 = version.getattr("minor")?.extract()?;
        push(bin.join(format!("python{major}.{minor}")));
    }
    if let Ok(base) = sys.getattr("_base_executable")?.extract::<String>() {
        push(PathBuf::from(base));
    }
    if let Ok(executable) = sys.getattr("executable")?.extract::<String>() {
        let executable = PathBuf::from(executable);
        // The target of the symlink chain, so a capability on the *real* file
        // is seen even when it is named something else.
        if let Ok(resolved) = executable.canonicalize() {
            push(resolved);
        }
        push(executable);
    }
    Ok(candidates
        .into_iter()
        .filter(|path| path.is_file())
        .collect())
}

/// Normalises one SQL text the way blastit compares it to a grant (§6).
///
/// The rules are [`blastit_sqlpolicy::normalize`]'s: trim, drop trailing `;`,
/// replace every comment by a single space, collapse whitespace outside quoted
/// regions, keep quoted strings, identifiers and dollar-quoted text exactly as
/// written, and rewrite the shapes a live query changes on every call — every
/// placeholder becomes `$?`, so a grant for one page size fits them all.
///
/// # Returns
///
/// The normalised text, which is what an allowlist stores and compares.
#[pyfunction]
fn normalize(sql: &str) -> String {
    audit::normalize_sql(sql)
}

/// Diffs what a test suite observed against what a manifest grants (D3b, §6).
///
/// `manifest_toml` is the whole `blastit.toml`; `observations_json` is the
/// document the pytest plugin writes (the [`audit`] module documents its shape
/// and the shape of the report). The observations carry the `PATH_INFO` the app
/// saw, which is what the scheduler routed on, so no mount prefix is involved.
///
/// Routing is blastit's own: [`blastit_manifest::RoutingTable`] on the
/// method and path, [`blastit_manifest::Manifest::effective_grants`] per
/// `(route, proxy)`, [`blastit_sqlpolicy::Allowlist::check`] on the
/// normalised SQL, and [`blastit_manifest::Route::egress`] for what a route may
/// reach on the network. Nothing about the manifest is re-implemented here.
///
/// # Errors
///
/// [`PyValueError`] when the manifest is invalid, when the observations do not
/// have the documented shape, or when a statement cannot be attributed to a
/// proxy the manifest declares. A failure is a problem with what was handed
/// over, never a partially written report.
///
/// # Returns
///
/// The report, pretty-printed JSON.
#[pyfunction]
#[pyo3(name = "audit")]
fn audit_report(manifest_toml: &str, observations_json: &str) -> PyResult<String> {
    audit::audit(manifest_toml, observations_json)
        .map_err(|error| PyValueError::new_err(error.to_string()))
}

/// Analyses one SQL statement the way a table grant is checked (D7, §6).
///
/// The statement is parsed by PostgreSQL's own parser and resolved to the
/// tables, columns, operations and functions it touches — or refused with the
/// reason, when it is not one statement of a kind a table grant can describe.
///
/// # Returns
///
/// Pretty-printed JSON in the shape of the report's `table_grants`:
///
/// ```json
/// {
///   "kind": "select",
///   "tables": {
///     "restaurants_restaurant": {"select": ["id", "name"]},
///     "restaurants_cuisinetype": {"select": "*"}
///   },
///   "functions": ["unaccent"]
/// }
/// ```
///
/// `tables` is the smallest `[route.<proxy>.tables]` grant that covers the
/// statement; `functions` lists only the calls beyond the built-in safe set
/// ([`blastit_sqlpolicy::SAFE_FUNCTIONS`]), which is what a grant has to write.
///
/// # Errors
///
/// [`PyValueError`] with the same one-line reason a `sql.denied` event carries.
#[pyfunction]
#[pyo3(name = "analyze")]
fn analyze_statement(sql: &str) -> PyResult<String> {
    audit::analyze_statement(sql).map_err(PyValueError::new_err)
}

/// The smallest table grant covering a set of statements (D3c, §6).
///
/// `blastit._blastit.analyze` answers for one statement; this answers for the
/// statements a whole log denied, which is what `blastit-manifest fold
/// --prefer-tables` needs: the union is taken by blastit's own
/// [`blastit_sqlpolicy::TableGrants::covering`], so a row lock asks for
/// `update` on a column the route already reads rather than on `"*"`, and the
/// statements no table grant can describe come back separately.
///
/// `prefix` is the manifest section the rendered `toml` names, for example
/// `route.db` — the same string the pytest plugin's report uses.
///
/// # Returns
///
/// Pretty-printed JSON with `tables`, `functions`, `toml` and `unanalysable`
/// (each entry with its `sql` and the `reason` a `sql.denied` event would
/// carry). Statements this build cannot analyse are reported, never dropped:
/// they have to stay exact `queries`.
///
/// # Errors
///
/// [`PyValueError`] only when the result cannot be rendered as JSON, which
/// cannot happen in practice.
#[pyfunction]
#[pyo3(name = "covering")]
fn covering_statements(statements: Vec<String>, prefix: String) -> PyResult<String> {
    audit::covering(&statements, &prefix).map_err(PyValueError::new_err)
}

/// Classifies one statement with PostgreSQL's own parser, without analysing it
/// (D3c, §6).
///
/// The statement is parsed by libpg_query and named for what it is — `select`,
/// `insert`, `update` and `delete` are the four kinds a grant can hold, and
/// everything else (DDL, `SET`, `COPY`, `DO`, `CALL`, `LOCK`, `GRANT`,
/// `TRUNCATE`, `VACUUM`, `LISTEN`, the transaction commands) is `other`.
///
/// It exists because the analysis refuses those statements for *other* reasons
/// too (a system catalogue, an unknown function, a construct it does not walk),
/// and a tool that offers to write a grant into a manifest has to know the
/// difference: `SELECT … FROM pg_type` is a statement a `queries` entry may
/// hold, and `SET ROLE …` is not.
///
/// # Returns
///
/// Pretty-printed JSON:
///
/// ```json
/// {"class": "select", "label": "SELECT", "plain": true}
/// {"class": "other", "label": "SET", "plain": false}
/// ```
///
/// # Errors
///
/// [`PyValueError`] with the reason a `sql.denied` event would carry: the text
/// is not valid SQL, holds no statement, or holds more than one — a grant is one
/// statement, so a caller has to refuse the text rather than pick one.
#[pyfunction]
#[pyo3(name = "classify")]
fn classify_statement(sql: &str) -> PyResult<String> {
    audit::classify(sql).map_err(PyValueError::new_err)
}

/// The interpreter's `sys.path`, as bytes, for stage 1 to restore (§4).
///
/// `os.fsencode` is what makes this exact: a path is bytes, and one that is not
/// valid UTF-8 — a home directory in a legacy encoding, say — is a path like any
/// other. Reading Python's text `sys.path` through Rust would drop it.
///
/// # Errors
///
/// Whatever the interpreter raises while reading `sys.path`.
fn interpreter_path(py: Python<'_>) -> PyResult<Vec<Vec<u8>>> {
    let fsencode = py.import("os")?.getattr("fsencode")?;
    let mut entries = Vec::new();
    for entry in py.import("sys")?.getattr("path")?.try_iter()?.flatten() {
        // One entry that `fsencode` refuses — an `int`, an object that is not a
        // path — is not worth failing a boot over.
        if let Ok(bytes) = fsencode
            .call1((entry,))
            .and_then(|bytes| bytes.extract::<Vec<u8>>())
        {
            entries.push(bytes);
        }
    }
    Ok(entries)
}

/// The `blastit._blastit` module: the console script's `main`, and the
/// functions the pytest plugin and the manifest tools call (D3b, D3c).
///
/// # Errors
///
/// [`PyErr`] when the module cannot be built, which is a pyo3 failure rather
/// than anything blastit can act on.
#[pymodule]
fn _blastit(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(main, module)?)?;
    module.add_function(wrap_pyfunction!(normalize, module)?)?;
    module.add_function(wrap_pyfunction!(audit_report, module)?)?;
    module.add_function(wrap_pyfunction!(analyze_statement, module)?)?;
    module.add_function(wrap_pyfunction!(covering_statements, module)?)?;
    module.add_function(wrap_pyfunction!(classify_statement, module)?)?;
    Ok(())
}
