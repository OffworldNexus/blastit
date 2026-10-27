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
    Grant, Group, Manifest, PgGrant, Protocol, Proxy, ProxyName, Route, UnresolvedGrant,
};
use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};

/// Keys allowed at the top level of the document.
const TOP_LEVEL_KEYS: &[&str] = &["version", "groups", "proxy", "group", "route"];

/// Keys of a `[[route]]`; every other key of a route is a proxy name, so its
/// value is an inline grant.
const ROUTE_KEYS: &[&str] = &["name", "path", "methods", "groups", "egress", "prefork"];

/// Expected value of an unknown route key, for the error message.
const ROUTE_KEY_EXPECTED: &str = "one of the route keys (`name`, `path`, `methods`, `groups`, \
                                   `egress`, `prefork`) or a table holding an inline grant";

/// Expected values of a route's `path`, for the error message of anything that
/// is neither.
const PATTERN_EXPECTED: &str = "a regex, or an array of them";

/// Parses a manifest document.
pub(crate) fn parse(source: &str) -> Result<Manifest, ManifestErrors> {
    let document: toml::Table = match source.parse() {
        Ok(document) => document,
        Err(error) => return Err(ManifestErrors::from(syntax_error(&error, source))),
    };

    let mut parser = Parser::default();
    let version = parser.version(&document);
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
    ))
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
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let head = &source[..offset];
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

            routes.push(Route::new(
                name,
                patterns.unwrap_or_default(),
                methods,
                groups.unwrap_or_default(),
                egress,
                prefork,
                grants,
            ));
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
