//! Strict parsing of the manifest TOML (§5.1, §6).
//!
//! Parsing is deliberately literal: unknown keys are errors, `input`, `output`
//! and `queries` stay raw strings, and a grant is only interpreted through the
//! protocol of the proxy it names. Everything that needs the whole document is
//! left to [`crate::validate`].

use std::collections::{BTreeMap, BTreeSet};
use toml::Value;

use crate::error::{Location, ManifestError, ManifestErrors, escape_control};
use crate::model::{
    Grant, Group, Manifest, PgGrant, Protocol, Proxy, ProxyName, Route, StaticPolicy,
    UnresolvedGrant,
};
use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};

/// Keys allowed at the top level of the document.
const TOP_LEVEL_KEYS: &[&str] = &[
    "version", "app", "preload", "groups", "proxy", "group", "route",
];

/// Keys of a `[[route]]`; every other key of a route is a proxy name, so its
/// value is an inline grant.
///
/// `static` is reserved (`R1`): it is always the `[route.static]` block, even
/// when a proxy of that name is declared.
const ROUTE_KEYS: &[&str] = &[
    "name", "path", "methods", "groups", "egress", "prefork", "ingress", "static",
];

/// The index file a `[route.static]` block answers with when it names none.
const DEFAULT_INDEX: &str = "index.html";

/// Keys that are only ever top-level, so finding one inside a table means the
/// author wrote it after the first `[table]` and TOML attached it there (`R8`).
const TOP_LEVEL_ONLY_KEYS: &[&str] = &["version", "app", "preload"];

/// Expected value of an unknown route key, for the error message.
const ROUTE_KEY_EXPECTED: &str = "one of the route keys (`name`, `path`, `methods`, `groups`, \
                                   `egress`, `prefork`, `ingress`, `static`) or a table holding an \
                                   inline grant";

/// Expected values of a route's `path`, for the error message of anything that
/// is neither.
const PATTERN_EXPECTED: &str = "a regex, or an array of them";

/// Expected values of the top-level `preload`, for the error message of
/// anything that is neither.
const PRELOAD_EXPECTED: &str = "a module name, or an array of them";

/// Parses a manifest document.
pub(crate) fn parse(source: &str) -> Result<Manifest, ManifestErrors> {
    let document: toml::Table = match source.parse() {
        Ok(document) => document,
        Err(error) => return Err(ManifestErrors::from(syntax_error(&error, source))),
    };

    // A misplaced top-level key is a scoping mistake, not a value mistake: every
    // other check would read the document the way TOML did, so the walk runs
    // first and reports alone.
    let mut misplaced = Vec::new();
    check_top_level_keys(&document, &mut misplaced);
    if !misplaced.is_empty() {
        return Err(ManifestErrors::from(misplaced));
    }

    let mut parser = Parser::default();
    let version = parser.version(&document);
    let app = parser.app(&document);
    let preload = parser.preload(&document);
    let global_groups = parser
        .string_list(document.get("groups"), "groups")
        .unwrap_or_default();
    parser.declare_proxies(&document);
    let groups = parser.groups(&document);
    let routes = parser.routes(&document);
    for key in document.keys() {
        if !TOP_LEVEL_KEYS.contains(&key.as_str()) {
            parser.error(ManifestError::UnknownKey {
                path: escape_control(key),
            });
        }
    }

    if !parser.errors.is_empty() {
        return Err(ManifestErrors::from(parser.errors));
    }

    Ok(Manifest::assemble(
        version,
        global_groups,
        parser.proxies,
        groups,
        routes,
        parser.unresolved,
    )
    .with_app(app)
    .with_preload(preload))
}

/// Reports every top-level-only key that TOML attached to a table (`R8`).
///
/// TOML attaches `version = 1` written after `[proxy.db]` to that proxy, where
/// it reads as a proxy field and the boot fails with a message about the wrong
/// line. `groups` is top-level and route-level; inside any other table it is
/// the same mistake.
///
/// Only *field* positions are checked. Every name in a manifest belongs to the
/// author — proxy names, group names, the variable names in `input`/`output`,
/// and the table names under `tables` — so `[proxy.app]`, `[group.app.db]`,
/// `[proxy.db.output] app = "…"` and `[route.db.tables] groups = { … }` are all
/// legal. The four keys are reserved where the schema names them, not where the
/// author does.
fn check_top_level_keys(document: &toml::Table, errors: &mut Vec<ManifestError>) {
    let declared = declared_proxies(document);
    for (key, value) in document {
        match (key.as_str(), value) {
            // `[proxy.<name>]`: the name is the author's, the body is a field
            // table, and the names inside `input`/`output` are the author's
            // again.
            ("proxy", Value::Table(proxies)) => {
                for proxy in proxies.values() {
                    check_fields(proxy, errors);
                }
            }
            // `[group.<name>.<proxy>]`: a group name, then a proxy name, then
            // the grant's own fields.
            ("group", Value::Table(groups)) => {
                for grants in groups.values() {
                    let Value::Table(grants) = grants else {
                        continue;
                    };
                    for grant in grants.values() {
                        check_grant(grant, errors);
                    }
                }
            }
            // `[[route]]` elements are the one place `groups` is a field.
            ("route", Value::Array(routes)) => {
                for route in routes {
                    check_route(route, &declared, errors);
                }
            }
            // Everything else is a scalar, a list, or a key the schema does not
            // know; none of them can hold a misplaced top-level key.
            _ => {}
        }
    }
}

/// The proxy names a document declares.
///
/// A route key is either a route key or a proxy name (§6), and a reserved key
/// is only an author-chosen name when it names a declared proxy: `version = 1`
/// below a `[[route]]` is still a misplaced top-level key, while
/// `app = { queries = [] }` next to `[proxy.app]` is a grant.
fn declared_proxies(document: &toml::Table) -> BTreeSet<&str> {
    match document.get("proxy") {
        Some(Value::Table(proxies)) => proxies.keys().map(String::as_str).collect(),
        _ => BTreeSet::new(),
    }
}

/// True for the keys that belong at the top of the document, not inside a
/// table: `version`, `app` and `preload` always, and `groups` everywhere but a
/// `[[route]]` element (`R8`).
///
/// Finding one of them as a *field* means it was written after a `[table]`,
/// where TOML attached it to that table instead of refusing.
fn reserved(key: &str) -> bool {
    TOP_LEVEL_ONLY_KEYS.contains(&key) || key == "groups"
}

/// The keys of one table that holds schema fields: a top-level-only key here
/// was written after a `[table]` and TOML attached it to this one (`R8`).
fn check_fields(value: &Value, errors: &mut Vec<ManifestError>) {
    let Value::Table(table) = value else {
        return;
    };
    for key in table.keys() {
        if reserved(key) {
            errors.push(ManifestError::NotTopLevel {
                key: escape_control(key),
            });
        }
    }
}

/// The body of one `[[route]]`: its route keys are fields, `groups` included,
/// and every other key is a proxy name whose value is an inline grant (§6).
fn check_route(value: &Value, declared: &BTreeSet<&str>, errors: &mut Vec<ManifestError>) {
    let Value::Table(route) = value else {
        return;
    };
    for (key, value) in route {
        match key.as_str() {
            // The route keys that are tables of fields.
            "ingress" => check_fields(value, errors),
            // Reserved like `ingress`: its body is the static block's fields.
            "static" => check_fields(value, errors),
            key if ROUTE_KEYS.contains(&key) => {}
            // A grant is a table, so a reserved key with one is a proxy name
            // only when that proxy is declared.
            key if declared.contains(key) && value.is_table() => check_grant(value, errors),
            key if reserved(key) => errors.push(ManifestError::NotTopLevel {
                key: escape_control(key),
            }),
            _ => check_grant(value, errors),
        }
    }
}

/// The body of a grant: `queries`, `tables`, `functions`… are the schema's keys,
/// and the table names inside `tables` are the author's (Django's `groups` is a
/// table like any other).
fn check_grant(value: &Value, errors: &mut Vec<ManifestError>) {
    let Value::Table(grant) = value else {
        return;
    };
    for (key, value) in grant {
        if reserved(key) {
            errors.push(ManifestError::NotTopLevel {
                key: escape_control(key),
            });
            continue;
        }
        if key == "tables"
            && let Value::Table(tables) = value
        {
            for operations in tables.values() {
                check_fields(operations, errors);
            }
        }
    }
}

/// Renders a TOML parse failure as a single line, with its position.
fn syntax_error(error: &toml::de::Error, source: &str) -> ManifestError {
    let message = match error.span() {
        Some(span) => {
            let (line, column) = position(source, span.start);
            format!(
                "{} (line {line}, column {column})",
                escape_control(error.message())
            )
        }
        None => escape_control(error.message()),
    };
    ManifestError::Syntax { message }
}

/// One-based line and column of a byte offset, for a parse error.
///
/// The offset comes from the TOML parser and is rounded down to a character
/// boundary, so a multi-byte character can never split a slice.
fn position(source: &str, offset: usize) -> (usize, usize) {
    // The last character boundary at or below the offset is the rounded-down
    // one. Walking the string once makes that a property of the text, so no
    // loop can fail to step and spin on a hostile offset.
    let limit = offset.min(source.len());
    let offset = source
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|&end| end <= limit)
        .last()
        .unwrap_or(0);
    debug_assert!(source.is_char_boundary(offset));
    let head = source.get(..offset).unwrap_or_default();
    let line = head.matches('\n').count() + 1;
    let column = head
        .rfind('\n')
        .map_or(offset, |newline| offset - newline - 1)
        + 1;
    (line, column)
}

/// Walks the document, collecting every problem it finds.
///
/// The proxies are read first, because a grant is only meaningful through the
/// protocol of the proxy it names; the order of the sections in the file does
/// not matter.
#[derive(Default)]
struct Parser {
    proxies: BTreeMap<ProxyName, Proxy>,
    /// Every name a `[proxy.<name>]` header mentioned, even when the
    /// declaration itself failed: a grant on such a proxy is a consequence of
    /// that error, not a second, separate one.
    declared: BTreeSet<String>,
    errors: Vec<ManifestError>,
    unresolved: Vec<UnresolvedGrant>,
}

impl Parser {
    /// Records a problem.
    fn error(&mut self, error: ManifestError) {
        self.errors.push(error);
    }

    /// `version`, which must be present and equal to `1` (§6).
    fn version(&mut self, document: &toml::Table) -> u32 {
        match document.get("version") {
            None => {
                self.error(ManifestError::MissingVersion);
                1
            }
            Some(Value::Integer(1)) => 1,
            Some(Value::Integer(other)) => {
                self.error(ManifestError::UnsupportedVersion {
                    found: other.to_string(),
                });
                1
            }
            Some(other) => {
                self.error(ManifestError::WrongType {
                    path: "version".to_string(),
                    expected: "the integer 1",
                    found: type_name(other),
                });
                1
            }
        }
    }

    /// The optional top-level `app`: the WSGI callable, kept exactly as written
    /// (`R7`).
    ///
    /// The manifest deliberately does not judge the shape here. `BLASTIT_APP`
    /// overrides this key, so the caller that merges the two is the one that
    /// can say the final value must be `module:attribute`; validation repeats
    /// the check for the manifest value so a file with a dot instead of a colon
    /// fails at boot even when it is the only source.
    fn app(&mut self, document: &toml::Table) -> Option<String> {
        let app = self.as_str(document.get("app")?, "app")?;
        if app.is_empty() {
            self.error(ManifestError::Empty {
                path: "app".to_string(),
            });
            return None;
        }
        Some(app)
    }

    /// The optional top-level `preload`: a module name, or a list of them
    /// (`R7`).
    ///
    /// A string is a list of one, exactly like `path`. Duplicates keep the
    /// first position: a module imported once before the fork gains nothing
    /// from being named twice.
    fn preload(&mut self, document: &toml::Table) -> Vec<String> {
        let Some(value) = document.get("preload") else {
            return Vec::new();
        };
        let list = match value {
            Value::String(module) => vec![module.clone()],
            Value::Array(_) => match self.string_list(Some(value), "preload") {
                Some(list) => list,
                None => return Vec::new(),
            },
            other => {
                self.error(ManifestError::WrongType {
                    path: "preload".to_string(),
                    expected: PRELOAD_EXPECTED,
                    found: type_name(other),
                });
                return Vec::new();
            }
        };

        let mut modules = Vec::with_capacity(list.len());
        for (index, module) in list.into_iter().enumerate() {
            if module.is_empty() {
                self.error(ManifestError::Empty {
                    path: format!("preload[{index}]"),
                });
            } else if !modules.contains(&module) {
                modules.push(module);
            }
        }
        modules
    }

    /// The `[proxy.<name>]` declarations.
    fn declare_proxies(&mut self, document: &toml::Table) {
        let Some(value) = document.get("proxy") else {
            return;
        };
        let Some(table) = self.as_table(value, "proxy") else {
            return;
        };

        for (name, value) in table {
            let path = format!("proxy.{}", escape_control(name));
            if name.is_empty() {
                self.error(ManifestError::Empty {
                    path: "proxy name".to_string(),
                });
                continue;
            }
            self.declared.insert(name.clone());

            let Some(fields) = self.as_table(value, &path) else {
                continue;
            };
            let mut protocol = None;
            let mut has_protocol = false;
            let mut input = BTreeMap::new();
            let mut output = BTreeMap::new();
            for (key, value) in fields {
                let key_path = format!("{path}.{}", escape_control(key));
                match key.as_str() {
                    "protocol" => {
                        has_protocol = true;
                        protocol = self.as_str(value, &key_path);
                    }
                    "input" => self.string_map(
                        value,
                        &key_path,
                        "input slot",
                        Some("input variable name"),
                        &mut input,
                    ),
                    "output" => {
                        self.string_map(value, &key_path, "output variable name", None, &mut output)
                    }
                    _ => self.error(ManifestError::UnknownKey { path: key_path }),
                }
            }

            let Some(raw_protocol) = protocol else {
                if !has_protocol {
                    self.error(ManifestError::MissingKey {
                        path: format!("{path}.protocol"),
                    });
                }
                continue;
            };
            let Some(protocol) = Protocol::parse(&raw_protocol) else {
                self.error(ManifestError::UnknownProtocol {
                    proxy: name.clone(),
                    protocol: raw_protocol,
                });
                continue;
            };

            let name = ProxyName::from(name.clone());
            self.proxies
                .insert(name.clone(), Proxy::new(name, protocol, input, output));
        }
    }

    /// The `[group.<name>.<proxy>]` grants (§6).
    fn groups(&mut self, document: &toml::Table) -> BTreeMap<String, Group> {
        let mut groups = BTreeMap::new();

        let Some(value) = document.get("group") else {
            return groups;
        };
        let Some(table) = self.as_table(value, "group") else {
            return groups;
        };

        for (name, value) in table {
            let path = format!("group.{}", escape_control(name));
            if name.is_empty() {
                self.error(ManifestError::Empty {
                    path: "group name".to_string(),
                });
                continue;
            }
            let Some(fields) = self.as_table(value, &path) else {
                continue;
            };

            let location = Location::Group(name.clone());
            let mut grants = BTreeMap::new();
            for (proxy, value) in fields {
                let grant_path = format!("{path}.{}", escape_control(proxy));
                if let Some((proxy, grant)) =
                    self.resolve_grant(&grant_path, value, proxy, location.clone())
                {
                    grants.insert(proxy, grant);
                }
            }
            groups.insert(name.clone(), Group::new(name.clone(), grants));
        }

        groups
    }

    /// The `[[route]]` entries, in document order, which is matching order
    /// (§6).
    fn routes(&mut self, document: &toml::Table) -> Vec<Route> {
        let mut routes = Vec::new();

        let Some(value) = document.get("route") else {
            return routes;
        };
        let Some(array) = self.as_array(value, "route") else {
            return routes;
        };

        for (index, value) in array.iter().enumerate() {
            let path = format!("route[{index}]");
            let Some(fields) = self.as_table(value, &path) else {
                continue;
            };

            let mut name = None;
            let mut has_name = false;
            let mut patterns = None;
            let mut has_path = false;
            let mut methods = None;
            let mut groups = None;
            let mut egress = false;
            let mut prefork = false;
            let mut ingress = None;
            let mut static_policy = None;
            for (key, value) in fields {
                let key_path = format!("{path}.{}", escape_control(key));
                match key.as_str() {
                    "name" => {
                        has_name = true;
                        name = self.as_str(value, &key_path);
                    }
                    "path" => {
                        has_path = true;
                        patterns = self.patterns(value, &key_path);
                    }
                    "methods" => methods = self.string_list(Some(value), &key_path),
                    "groups" => groups = self.string_list(Some(value), &key_path),
                    "egress" => {
                        if let Some(value) = self.as_bool(value, &key_path) {
                            egress = value;
                        }
                    }
                    "prefork" => {
                        if let Some(value) = self.as_bool(value, &key_path) {
                            prefork = value;
                        }
                    }
                    "ingress" => ingress = self.ingress(value, &key_path),
                    "static" => static_policy = self.static_policy(value, &key_path),
                    _ => {}
                }
            }
            if !has_name {
                self.error(ManifestError::MissingKey {
                    path: format!("{path}.name"),
                });
            }
            if !has_path {
                self.error(ManifestError::MissingKey {
                    path: format!("{path}.path"),
                });
            }
            let name = name.unwrap_or_default();

            let location = Location::Route {
                index,
                name: name.clone(),
            };
            let mut grants = BTreeMap::new();
            for (key, value) in fields {
                if ROUTE_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let grant_path = format!("{path}.{}", escape_control(key));
                if let Some((proxy, grant)) =
                    self.resolve_grant(&grant_path, value, key, location.clone())
                {
                    grants.insert(proxy, grant);
                }
            }

            routes.push(
                Route::new(
                    name,
                    patterns.unwrap_or_default(),
                    methods,
                    groups.unwrap_or_default(),
                    egress,
                    prefork,
                    grants,
                )
                .with_ingress(ingress)
                .with_static(static_policy),
            );
        }

        routes
    }

    /// Resolves one grant through the protocol of the proxy it names.
    ///
    /// Returns `None` when the grant cannot be interpreted: the proxy is not
    /// declared at all (recorded in `unresolved` for validation to report), the
    /// proxy's own declaration failed, or the value is not a table.
    fn resolve_grant(
        &mut self,
        path: &str,
        value: &Value,
        proxy_name: &str,
        location: Location,
    ) -> Option<(ProxyName, Grant)> {
        let Some(protocol) = self.proxies.get(proxy_name).map(Proxy::protocol) else {
            if !self.declared.contains(proxy_name) {
                if value.is_table() {
                    self.unresolved
                        .push(UnresolvedGrant::new(ProxyName::from(proxy_name), location));
                } else {
                    self.error(ManifestError::WrongType {
                        path: path.to_string(),
                        expected: ROUTE_KEY_EXPECTED,
                        found: type_name(value),
                    });
                }
            }
            return None;
        };

        let name = ProxyName::from(proxy_name);
        let table = self.as_table(value, path)?;
        let grant = self.grant(protocol, table, path)?;
        Some((name, grant))
    }

    /// Parses a grant's contents according to the protocol that defines them
    /// (§5.2, §5.3).
    fn grant(&mut self, protocol: Protocol, table: &toml::Table, path: &str) -> Option<Grant> {
        match protocol {
            Protocol::Postgresql => Some(Grant::Postgresql(self.pg_grant(table, path))),
        }
    }

    /// `queries`, `transaction_control`, `unrestricted`, `tables` and
    /// `functions`: the grant contents of `postgresql` (§5.2, §6, D7).
    fn pg_grant(&mut self, table: &toml::Table, path: &str) -> PgGrant {
        let mut queries = Vec::new();
        let mut transaction_control = false;
        let mut unrestricted = false;
        let mut tables: BTreeMap<String, TableGrant> = BTreeMap::new();
        let mut functions: BTreeSet<String> = BTreeSet::new();
        for (key, value) in table {
            let key_path = format!("{path}.{}", escape_control(key));
            match key.as_str() {
                "queries" => {
                    if let Some(list) = self.string_list(Some(value), &key_path) {
                        queries = list;
                    }
                }
                "transaction_control" => {
                    if let Some(value) = self.as_bool(value, &key_path) {
                        transaction_control = value;
                    }
                }
                // Whether a *group* is allowed to carry it is decided by
                // validation, not here: the parser only reads the document.
                "unrestricted" => {
                    if let Some(value) = self.as_bool(value, &key_path) {
                        unrestricted = value;
                    }
                }
                "tables" => self.table_grants(value, &key_path, &mut tables),
                "functions" => {
                    if let Some(list) = self.string_list(Some(value), &key_path) {
                        for name in list {
                            if name.is_empty() {
                                self.error(ManifestError::Empty {
                                    path: format!("{key_path}: a function name"),
                                });
                            } else {
                                functions.insert(name);
                            }
                        }
                    }
                }
                _ => self.error(ManifestError::UnknownKey { path: key_path }),
            }
        }
        PgGrant::new(queries, transaction_control)
            .with_unrestricted(unrestricted)
            .with_tables(TableGrants::new(tables, functions))
    }

    /// The `tables` of a grant: one entry per table, each with the operations
    /// it grants (§6, D7).
    fn table_grants(
        &mut self,
        value: &Value,
        path: &str,
        tables: &mut BTreeMap<String, TableGrant>,
    ) {
        let Some(entries) = self.as_table(value, path) else {
            return;
        };
        for (name, value) in entries {
            let table_path = format!("{path}.{}", escape_control(name));
            if name.is_empty() {
                self.error(ManifestError::Empty {
                    path: format!("{path}: a table name"),
                });
                continue;
            }
            let Some(operations) = self.as_table(value, &table_path) else {
                continue;
            };
            let mut grant = TableGrant::default();
            for (operation, value) in operations {
                let operation_path = format!("{table_path}.{}", escape_control(operation));
                match operation.as_str() {
                    "select" => grant.select = self.columns(value, &operation_path),
                    "insert" => grant.insert = self.columns(value, &operation_path),
                    "update" => grant.update = self.columns(value, &operation_path),
                    "delete" => {
                        if let Some(value) = self.as_bool(value, &operation_path) {
                            grant.delete = value;
                        }
                    }
                    _ => self.error(ManifestError::UnknownTableOperation {
                        path: operation_path,
                    }),
                }
            }
            tables.insert(name.clone(), grant);
        }
    }

    /// The columns one operation of a table grant names: the string `"*"`, or a
    /// list of column names (§6, D7).
    fn columns(&mut self, value: &Value, path: &str) -> Option<Columns> {
        match value {
            Value::String(all) if all == "*" => Some(Columns::All),
            Value::String(_) => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "the string \"*\" (every column) or a list of column names",
                    found: type_name(value),
                });
                None
            }
            Value::Array(_) => {
                let list = self.string_list(Some(value), path)?;
                let mut columns = BTreeSet::new();
                for column in list {
                    if column == "*" {
                        self.error(ManifestError::StarredColumn {
                            path: path.to_string(),
                        });
                        continue;
                    }
                    if column.is_empty() {
                        self.error(ManifestError::Empty {
                            path: format!("{path}: a column name"),
                        });
                        continue;
                    }
                    columns.insert(column);
                }
                Some(Columns::List(columns))
            }
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "the string \"*\" (every column) or a list of column names",
                    found: type_name(other),
                });
                None
            }
        }
    }

    /// A route's `[route.ingress]`: the body codec and whether `raw` is
    /// explicitly allowed (`R11`).
    ///
    /// `body` is required — a table that declares nothing would read like a
    /// policy while enforcing none. `allow_raw` exists only on `raw`: on any
    /// other codec it is an unknown key rather than a setting that quietly does
    /// nothing.
    fn ingress(&mut self, value: &Value, path: &str) -> Option<IngressPolicy> {
        let table = self.as_table(value, path)?;
        let mut has_body = false;
        let mut body = None;
        let mut allow_raw = None;
        for (key, value) in table {
            let key_path = format!("{path}.{}", escape_control(key));
            match key.as_str() {
                "body" => {
                    has_body = true;
                    body = self.body_codec(value, &key_path);
                }
                // Decided once `body` is known: the table is walked in key
                // order, so `allow_raw` arrives before `body`.
                "allow_raw" => allow_raw = Some((value, key_path)),
                _ => self.error(ManifestError::UnknownKey { path: key_path }),
            }
        }

        let Some(codec) = body else {
            if !has_body {
                self.error(ManifestError::MissingKey {
                    path: format!("{path}.body"),
                });
            }
            return None;
        };

        let allow_raw = match allow_raw {
            None => false,
            Some((value, key_path)) => {
                if codec == BodyCodec::Raw {
                    self.as_bool(value, &key_path).unwrap_or(false)
                } else {
                    self.error(ManifestError::UnknownKey { path: key_path });
                    false
                }
            }
        };
        Some(IngressPolicy {
            body: codec,
            allow_raw,
        })
    }

    /// The `body` of a `[route.ingress]` table: `json`, `form`, `none` or
    /// `raw`.
    fn body_codec(&mut self, value: &Value, path: &str) -> Option<BodyCodec> {
        let codec = self.as_str(value, path)?;
        match codec.as_str() {
            "json" => Some(BodyCodec::Json),
            "form" => Some(BodyCodec::Form),
            "none" => Some(BodyCodec::None),
            "raw" => Some(BodyCodec::Raw),
            _ => {
                self.error(ManifestError::UnknownBodyCodec {
                    path: path.to_string(),
                    found: codec,
                });
                None
            }
        }
    }

    /// A route's `[route.static]`: the directory served and its three optional
    /// keys (`R1`).
    ///
    /// Only the shape is read here. The rules a value has to obey — a relative
    /// `root`, an `index` beneath it, `/`-anchored patterns — belong to
    /// validation, so one bad value never hides the others.
    fn static_policy(&mut self, value: &Value, path: &str) -> Option<StaticPolicy> {
        let table = self.as_table(value, path)?;
        let mut root = None;
        let mut has_root = false;
        let mut index = None;
        let mut spa = false;
        let mut immutable = Vec::new();
        for (key, value) in table {
            let key_path = format!("{path}.{}", escape_control(key));
            match key.as_str() {
                "root" => {
                    has_root = true;
                    root = self.as_str(value, &key_path);
                }
                "index" => index = self.as_str(value, &key_path),
                "spa" => {
                    if let Some(value) = self.as_bool(value, &key_path) {
                        spa = value;
                    }
                }
                "immutable" => {
                    if let Some(list) = self.string_list(Some(value), &key_path) {
                        immutable = list;
                    }
                }
                _ => self.error(ManifestError::UnknownKey { path: key_path }),
            }
        }
        if !has_root {
            self.error(ManifestError::MissingKey {
                path: format!("{path}.root"),
            });
        }

        let root = root?;
        Some(StaticPolicy {
            root,
            index: index.unwrap_or_else(|| DEFAULT_INDEX.to_string()),
            spa,
            immutable,
        })
    }

    /// A route's `path`: a list of regexes, or the one regex a bare string
    /// holds.
    ///
    /// A list is the manifest's own form (`path = ['/products/.*']`), and
    /// ARCHITECTURE.md writes the catch-all as a string (`path = ".*"`) in more
    /// than one place — a string is a list of one in every configuration
    /// language a reader has met, and refusing it costs a boot and buys nothing
    /// but a workaround. What is *not* accepted is still refused by name:
    /// anything that is neither a string nor a list says so, and a list with
    /// something else in it names the entry.
    fn patterns(&mut self, value: &Value, path: &str) -> Option<Vec<String>> {
        match value {
            Value::String(pattern) => Some(vec![pattern.clone()]),
            Value::Array(_) => self.string_list(Some(value), path),
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: PATTERN_EXPECTED,
                    found: type_name(other),
                });
                None
            }
        }
    }

    /// A TOML list of strings, such as `groups`, `methods` or `path`.
    fn string_list(&mut self, value: Option<&Value>, path: &str) -> Option<Vec<String>> {
        let value = value?;
        let array = self.as_array(value, path)?;
        let mut list = Vec::with_capacity(array.len());
        for (index, item) in array.iter().enumerate() {
            if let Some(item) = self.as_str(item, &format!("{path}[{index}]")) {
                list.push(item);
            }
        }
        Some(list)
    }

    /// A TOML table of strings, such as `input` and `output`.
    ///
    /// `key_label` names an empty key; `value_label` is `Some` when the values
    /// must be non-empty too (an environment variable name), and `None` when
    /// they may be empty (a template).
    fn string_map(
        &mut self,
        value: &Value,
        path: &str,
        key_label: &str,
        value_label: Option<&str>,
        map: &mut BTreeMap<String, String>,
    ) {
        let Some(table) = self.as_table(value, path) else {
            return;
        };
        for (key, value) in table {
            if key.is_empty() {
                self.error(ManifestError::Empty {
                    path: key_label.to_string(),
                });
                continue;
            }
            let entry_path = format!("{path}.{}", escape_control(key));
            let Some(entry) = self.as_str(value, &entry_path) else {
                continue;
            };
            if entry.is_empty()
                && let Some(label) = value_label
            {
                self.error(ManifestError::Empty {
                    path: format!("{entry_path} ({label})"),
                });
                continue;
            }
            map.insert(key.clone(), entry);
        }
    }

    /// A required string.
    fn as_str(&mut self, value: &Value, path: &str) -> Option<String> {
        match value {
            Value::String(text) => Some(text.clone()),
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "a string",
                    found: type_name(other),
                });
                None
            }
        }
    }

    /// A required boolean.
    fn as_bool(&mut self, value: &Value, path: &str) -> Option<bool> {
        match value {
            Value::Boolean(flag) => Some(*flag),
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "a boolean",
                    found: type_name(other),
                });
                None
            }
        }
    }

    /// A required table.
    fn as_table<'a>(&mut self, value: &'a Value, path: &str) -> Option<&'a toml::Table> {
        match value {
            Value::Table(table) => Some(table),
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "a table",
                    found: type_name(other),
                });
                None
            }
        }
    }

    /// A required array.
    fn as_array<'a>(&mut self, value: &'a Value, path: &str) -> Option<&'a Vec<Value>> {
        match value {
            Value::Array(array) => Some(array),
            other => {
                self.error(ManifestError::WrongType {
                    path: path.to_string(),
                    expected: "an array",
                    found: type_name(other),
                });
                None
            }
        }
    }
}

/// The TOML type of a value, for error messages.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Boolean(_) => "a boolean",
        Value::Datetime(_) => "a datetime",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::position;

    #[test]
    fn position_is_one_based() {
        assert_eq!(position("abc", 0), (1, 1));
        assert_eq!(position("abc", 2), (1, 3));
        assert_eq!(position("a\nbc", 3), (2, 2));
        assert_eq!(position("a\nbc", 99), (2, 3));
    }

    #[test]
    fn position_rounds_down_to_a_char_boundary() {
        // "café": the `é` occupies bytes 3 and 4, so byte 4 is inside it.
        assert_eq!(position("café", 4), (1, 4));
        assert_eq!(position("café", 5), (1, 6));
        assert_eq!(position("é", 1), (1, 1));
        assert_eq!(position("a\né", 3), (2, 1));
        // Far past the end: clamped to the length of the text.
        assert_eq!(position("é", 99), (1, 3));
    }
}
