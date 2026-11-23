// The one Rust reader of `packaging/languages.tsv` (`MB.67.3`, `MB.75.1`).
//
// An include-only fragment, not a crate: every `build.rs` and every freshness
// test spells
//
//     include!("../../packaging/languages.rs.inc"); // tests: one `../` deeper
//
// so the one declared list of language rows has one parse rule in Rust, beside
// the bash loader (`packaging/languages.sh`) and the Python loader
// (`scripts/languages.py`). `std` only, on purpose: a build script must not drag
// a dependency into `Cargo.lock`.

/// One language row: the six tab-separated fields of one non-comment line.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    language: String,
    version: String,
    slug: String,
    deb_runtime: String,
    rpm_runtime: String,
    /// The `channels` column, verbatim: `stable`, `nightly` or `stable nightly`.
    channels: String,
}

/// The `slug` column: the shim's file-name infix (`php84`, `py314`).
#[allow(dead_code)]
impl Row {
    /// The `slug` column, exactly as the TSV spells it.
    fn slug(&self) -> &str {
        &self.slug
    }

    /// The digits of the `version` column: `"3.13"` -> `"313"`, `"8.4"` -> `"84"`.
    fn digits(&self) -> String {
        digits(&self.version)
    }

    /// Whether this row ships in `channel` (`"stable"` or `"nightly"`).
    ///
    /// This is the one *selection* rule, used by packaging/docs generators. The
    /// Rust runtime never calls it: the same binary serves every channel, so its
    /// tables are all-row (see `rows_of`/`versions`).
    fn in_channel(&self, channel: &str) -> bool {
        self.channels.split(' ').any(|declared| declared == channel)
    }
}

/// Whether `row` ships in `channel`, for a caller holding `&Row`.
#[allow(dead_code)]
fn in_channel(row: &Row, channel: &str) -> bool {
    row.in_channel(channel)
}

/// The header a valid file starts with (its first non-comment line).
#[allow(dead_code)]
const LANGUAGE_HEADER: [&str; 6] = [
    "language",
    "version",
    "slug",
    "deb-runtime",
    "rpm-runtime",
    "channels",
];

/// The only legal `channels` values: the two channels, or both with `stable` first.
#[allow(dead_code)]
const CHANNEL_COMBINATIONS: [&str; 3] = ["stable", "nightly", "stable nightly"];

/// The names the TSV reserves for the row-less core package and its header.
#[allow(dead_code)]
const RESERVED_LANGUAGES: [&str; 3] = ["core", "blastit", "language"];

/// Parses `text`, or names every problem in file order.
///
/// The rules are the ones `scripts/languages.py` states: a header line, six
/// fields per row, `<major>.<minor>` versions, `[a-z0-9_]` slugs, no `(none)`
/// runtime for a shim, no spec/control syntax in a `Recommends:` value, a
/// `channels` value of `stable`, `nightly` or `stable nightly`, and no
/// package or slug twice.
#[allow(dead_code)]
fn parse(text: &str) -> Result<Vec<Row>, String> {
    let mut rows: Vec<Row> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut header_seen = false;
    let mut packages: Vec<String> = Vec::new();
    let mut slugs: Vec<String> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if !header_seen {
            header_seen = true;
            if fields != LANGUAGE_HEADER.to_vec() {
                problems.push(format!(
                    "{number}: the header is {line:?}, expected {:?}",
                    LANGUAGE_HEADER.join("\t")
                ));
            }
            continue;
        }
        if fields.len() != 6 {
            problems.push(format!(
                "{number}: {} fields, a row is exactly 6 \
                 (language<TAB>version<TAB>slug<TAB>deb-runtime<TAB>rpm-runtime<TAB>channels)",
                fields.len()
            ));
            continue;
        }
        let row = Row {
            language: fields[0].to_string(),
            version: fields[1].to_string(),
            slug: fields[2].to_string(),
            deb_runtime: fields[3].to_string(),
            rpm_runtime: fields[4].to_string(),
            channels: fields[5].to_string(),
        };
        let package = format!("blastit-{}-{}", row.language, row.version);
        let parts: Vec<&str> = row.version.split('.').collect();
        if parts.len() != 2
            || parts
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            problems.push(format!(
                "{number}: {package} has version {:?}; expected <major>.<minor> like 8.10",
                row.version
            ));
        }
        if row.slug.is_empty()
            || !row
                .slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            problems.push(format!(
                "{number}: {package} has slug {:?}; expected lowercase letters, digits or '_'",
                row.slug
            ));
        }
        if RESERVED_LANGUAGES.contains(&row.language.as_str()) {
            problems.push(format!(
                "{number}: {:?} is not a language ({} are reserved)",
                row.language,
                RESERVED_LANGUAGES.join(", ")
            ));
        }
        if !CHANNEL_COMBINATIONS.contains(&row.channels.as_str()) {
            problems.push(format!(
                "{number}: {package} has channels {:?}; expected one of {} (space-separated, \
                 `stable` first, never empty, never repeated)",
                row.channels,
                CHANNEL_COMBINATIONS
                    .iter()
                    .map(|value| format!("{value:?}"))
                    .collect::<Vec<String>>()
                    .join(", ")
            ));
        }
        for (field, value) in [
            ("deb-runtime", &row.deb_runtime),
            ("rpm-runtime", &row.rpm_runtime),
        ] {
            if value.is_empty() || value == "(none)" || value.trim() != value {
                problems.push(format!(
                    "{number}: {package} needs a {field} package name for `Recommends:`"
                ));
            }
            if value
                .bytes()
                .any(|byte| matches!(byte, b'%' | b'#' | b'{' | b'}'))
            {
                problems.push(format!(
                    "{number}: {package} has {field} {value:?} with spec/control syntax; a \
                     runtime column is a package name, never spec syntax"
                ));
            }
        }
        if packages.contains(&package) {
            problems.push(format!("{number}: package {package} is declared twice"));
        } else {
            packages.push(package);
        }
        if slugs.contains(&row.slug) {
            problems.push(format!(
                "{number}: slug {:?} is declared twice",
                row.slug
            ));
        } else {
            slugs.push(row.slug.clone());
        }
        rows.push(row);
    }
    if !header_seen {
        problems.push("no header line".to_string());
    } else if rows.is_empty() {
        problems.push("declares no language rows".to_string());
    }
    if problems.is_empty() {
        Ok(rows)
    } else {
        Err(problems.join("\n"))
    }
}

/// Reads and parses `path`.
#[allow(dead_code)]
fn load(path: &std::path::Path) -> Result<Vec<Row>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    parse(&text).map_err(|err| format!("{}: {err}", path.display()))
}

/// The rows of one language, newest first, compared per component.
///
/// A version is a string sorted by numeric components, never a float: `8.10`
/// sorts after `8.9`. Callers that emit a name from `slug`/`digits` take the
/// row, never a re-split of the version string.
#[allow(dead_code)]
fn rows_of<'a>(rows: &'a [Row], language: &str) -> Vec<&'a Row> {
    let mut found: Vec<&Row> = rows
        .iter()
        .filter(|row| row.language == language)
        .collect();
    found.sort_by_cached_key(|row| std::cmp::Reverse(version_key(&row.version)));
    found
}

/// The versions of one language, newest first, compared per component.
#[allow(dead_code)]
fn versions<'a>(rows: &'a [Row], language: &str) -> Vec<&'a str> {
    rows_of(rows, language)
        .iter()
        .map(|row| row.version.as_str())
        .collect()
}

/// The sort key of a version: its components as integers.
#[allow(dead_code)]
fn version_key(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The digits of a dotted version: `"3.13"` -> `"313"`, `"8.4"` -> `"84"`.
#[allow(dead_code)]
fn digits(version: &str) -> String {
    version.chars().filter(char::is_ascii_digit).collect()
}

/// `items` in ascending version order, the order a sentence reads.
#[allow(dead_code)]
fn ascending<'a>(items: &[&'a str]) -> Vec<&'a str> {
    let mut ordered = items.to_vec();
    ordered.sort_by_cached_key(|version| version_key(version));
    ordered
}

/// `versions` as one sentence: `PHP 8.3, 8.4, 8.5 and 8.6`.
#[allow(dead_code)]
fn sentence(prefix: &str, items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [only] => format!("{prefix} {only}"),
        [rest @ .., last] => format!("{prefix} {} and {last}", rest.join(", ")),
    }
}
