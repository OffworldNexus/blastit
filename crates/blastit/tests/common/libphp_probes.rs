//! The `LIBPHP_PROBES` drift check (`MB.3.62`).
//!
//! The PHP legs probe for a usable `libphp` **themselves** — independent of the
//! code under test, which is what lets them tell "this host has no PHP" from
//! "the boot lost the PHP it has" — and that independence is a copy: the leg
//! carries `stage0.rs`'s discovery order as its own table. A copy is a claim that
//! can drift, so `e2e_dev.rs` reads the boot's source at compile time and
//! compares the two tables entry by entry: a change to either side then fails the
//! suite that would otherwise probe a host differently from the boot.
//!
//! The parser is deliberately small and deliberately loud. It reads the
//! declaration as text — `const LIBPHP_PROBES: [LibphpProbe; N] = [ LibphpProbe {
//! name: "…", per_minor_dir: None | Some("phpNN") }, … ];` — and returns a
//! sentence naming what it could not read, so a refactor of that declaration
//! fails the drift test instead of silently comparing an empty list with a full
//! one.

/// One step of the discovery order: a file name, and the per-minor directory it
/// is looked for in (`None`: every directory, in the search's own order).
pub type Step = (String, Option<String>);

/// The text that opens the declaration this module reads.
const DECLARATION: &str = "const LIBPHP_PROBES";

/// The field each probe states its directory constraint with.
const CONSTRAINT: &str = "per_minor_dir:";

/// The `(name, per_minor_dir)` pairs `source`'s `LIBPHP_PROBES` declares, in
/// order.
///
/// # Errors
///
/// A sentence naming the unreadable text when `source` holds no such
/// declaration, when it is not closed by `\n];`, or when a probe's `name` or
/// `per_minor_dir` does not look like a string / `None` / `Some("…")`.
pub fn probes_in(source: &str) -> Result<Vec<Step>, String> {
    let start = source
        .find(DECLARATION)
        .ok_or_else(|| format!("the source declares no `{DECLARATION}`"))?;
    let rest = &source[start..];
    let end = rest
        .find("\n];")
        .ok_or_else(|| format!("`{DECLARATION}` is not closed by `\\n];`"))?;
    let block = &rest[..end];

    let mut steps = Vec::new();
    for chunk in block.split("name:").skip(1) {
        let (name, after_name) = quoted(chunk)?;
        let at = after_name.find(CONSTRAINT).ok_or_else(|| {
            format!(
                "the probe named `{name}` states no `{}`",
                CONSTRAINT.trim_end_matches(':')
            )
        })?;
        let field = after_name[at + CONSTRAINT.len()..].trim_start();
        let per_minor_dir = if let Some(argument) = field.strip_prefix("Some(") {
            Some(quoted(argument)?.0)
        } else if field.starts_with("None") {
            None
        } else {
            return Err(format!(
                "the probe named `{name}` has a `{}` this reader cannot parse: `{}`",
                CONSTRAINT.trim_end_matches(':'),
                leading_line(field)
            ));
        };
        steps.push((name, per_minor_dir));
    }
    if steps.is_empty() {
        return Err(format!(
            "`{DECLARATION}` declares no `name:` field, so it has no probes to compare"
        ));
    }
    Ok(steps)
}

/// The first double-quoted string in `text`, and the text that follows it.
fn quoted(text: &str) -> Result<(String, &str), String> {
    let open = text
        .find('"')
        .ok_or_else(|| format!("no quoted string after `{}`", leading_line(text)))?;
    let after = &text[open + 1..];
    let close = after
        .find('"')
        .ok_or_else(|| format!("an unclosed quoted string in `{}`", leading_line(text)))?;
    Ok((after[..close].to_owned(), &after[close + 1..]))
}

/// `text` up to its first line break, so an error sentence stays one line.
fn leading_line(text: &str) -> &str {
    text.split('\n').next().unwrap_or_default()
}

/// Fails with a sentence naming the first difference between the suite's
/// `probes` and the `LIBPHP_PROBES` that `source` declares.
///
/// # Errors
///
/// Every difference: a changed name, a changed per-minor constraint, a changed
/// order, a different length, or a declaration [`probes_in`] cannot read.
pub fn assert_matches(probes: &[(&str, Option<&str>)], source: &str) -> Result<(), String> {
    let declared = probes_in(source)?;
    let copied: Vec<Step> = probes
        .iter()
        .map(|(name, dir)| ((*name).to_owned(), dir.map(str::to_owned)))
        .collect();
    if declared == copied {
        return Ok(());
    }
    let shared = declared.len().min(copied.len());
    let detail = match (0..shared).find(|&index| declared[index] != copied[index]) {
        Some(index) => format!(
            "entry {index} is {:?} in stage0.rs and {:?} in the suite",
            declared[index], copied[index]
        ),
        None => format!(
            "stage0.rs declares {} entries, the suite probes {}",
            declared.len(),
            copied.len()
        ),
    };
    Err(format!(
        "the suite's probe table has drifted from stage0.rs's `{DECLARATION}`: {detail}\n  \
         stage0.rs: {declared:?}\n  suite:     {copied:?}"
    ))
}
