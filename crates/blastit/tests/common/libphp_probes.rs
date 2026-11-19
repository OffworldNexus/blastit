//! The `LIBPHP_PROBES` drift check (`MB.3.62`).
//!
//! Both PHP suites probe for a usable `libphp` **themselves** — independent of
//! the code under test, which is what lets them tell "this host has no PHP" from
//! "the boot lost the PHP it has" — and that independence is a copy: each suite
//! carries `stage0.rs`'s discovery order as its own table. A copy is a claim that
//! can drift, so both suites read `stage0.rs`'s source at compile time and
//! compare the two tables entry by entry: a change to either side then fails the
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

#[cfg(test)]
mod tests {
    use super::{assert_matches, probes_in};

    /// The declaration the suites' tables are copies of, read at compile time.
    const STAGE0: &str = include_str!("../../../blastit-view-php/src/discovery.rs");

    /// The table `critical_php_embed.rs` and `critical_php_backend.rs` both
    /// carry, written out here so this module's own tests have an oracle that is
    /// not read out of `stage0.rs`: 17 entries, minor-major, each minor's own
    /// spellings before the next minor's, and the unversioned `libphp.so` last
    /// with no directory constraint.
    const EXPECTED: [(&str, Option<&str>); 17] = [
        ("libphp8.6.so", None),
        ("libphp-8.6.so", None),
        ("libphp86.so", None),
        ("libphp.so", Some("php86")),
        ("libphp8.5.so", None),
        ("libphp-8.5.so", None),
        ("libphp85.so", None),
        ("libphp.so", Some("php85")),
        ("libphp8.4.so", None),
        ("libphp-8.4.so", None),
        ("libphp84.so", None),
        ("libphp.so", Some("php84")),
        ("libphp8.3.so", None),
        ("libphp-8.3.so", None),
        ("libphp83.so", None),
        ("libphp.so", Some("php83")),
        ("libphp.so", None),
    ];

    /// The reader finds every entry, the per-minor constraints included, and the
    /// final unconstrained `libphp.so` stays unconstrained.
    #[test]
    fn the_declaration_reads_as_seventeen_ordered_probes() {
        let declared = probes_in(STAGE0).expect("stage0.rs's LIBPHP_PROBES reads");
        assert_eq!(declared.len(), EXPECTED.len(), "{declared:?}");
        assert_eq!(
            declared.last().map(|step| step.1.as_deref()),
            Some(None),
            "the final entry is the unconstrained `libphp.so` fallback: {declared:?}"
        );
        assert_matches(&EXPECTED, STAGE0).expect("stage0.rs's order");
    }

    /// The reader is a reader, not a rubber stamp: a declaration it cannot parse
    /// is an error, never an empty list that matches an empty table.
    #[test]
    fn an_unreadable_declaration_is_an_error() {
        assert!(probes_in("fn nothing_here() {}").is_err());
        assert!(probes_in("const LIBPHP_PROBES: [Probe; 0] = [];").is_err());
        assert!(
            probes_in("const LIBPHP_PROBES: [P; 1] = [\n    P { name: \"libphp.so\" },\n];")
                .is_err(),
            "a probe with no `per_minor_dir` must not read as `None`"
        );
    }

    /// The green assertion above is only worth anything if a change goes red, so
    /// each class of drift is proved on the **real** declaration, mutated: a
    /// dropped per-minor constraint, the fallback turned into a constraint, and
    /// two entries out of order.
    #[test]
    fn a_changed_entry_is_reported_at_its_index() {
        let dropped_constraint = STAGE0.replacen(CONSTRAINT_SOME_86, CONSTRAINT_NONE, 1);
        assert_ne!(
            dropped_constraint, STAGE0,
            "the source states `Some(\"php86\")`"
        );
        assert!(
            assert_matches(&EXPECTED, &dropped_constraint)
                .is_err_and(|drift| drift.contains("entry 3")),
            "`libphp.so` outside `/usr/lib/php86` must be entry 3's difference"
        );

        let fallback_constrained = constrain_the_fallback(STAGE0);
        assert_ne!(
            fallback_constrained, STAGE0,
            "the fallback is unconstrained"
        );
        assert!(
            assert_matches(&EXPECTED, &fallback_constrained)
                .is_err_and(|drift| drift.contains("entry 16")),
            "a constraint on the final `libphp.so` must be entry 16's difference"
        );

        let reordered = STAGE0
            .replacen("name: \"libphp8.6.so\",", "name: \"libphp86.so\",", 1)
            .replacen("name: \"libphp-8.6.so\",", "name: \"libphp8.6.so\",", 1);
        assert_ne!(
            reordered, STAGE0,
            "the first two entries are dotted then hyphenated"
        );
        assert!(
            assert_matches(&EXPECTED, &reordered).is_err_and(|drift| drift.contains("entry 0")),
            "a swapped pair must be reported at its first index"
        );
    }

    /// The `per_minor_dir` field of the newest per-minor entry, as `stage0.rs`
    /// spells it.
    const CONSTRAINT_SOME_86: &str = "per_minor_dir: Some(\"php86\")";

    /// The same field unconstrained.
    const CONSTRAINT_NONE: &str = "per_minor_dir: None";

    /// `source` with the constraint on its **last** `name: "libphp.so"` turned
    /// into a per-minor one — the fallback entry, changed but still readable.
    fn constrain_the_fallback(source: &str) -> String {
        let at = source
            .rfind("name: \"libphp.so\"")
            .expect("the last probe is the unversioned `libphp.so`");
        let tail = &source[at..];
        let field = tail
            .find(CONSTRAINT_NONE)
            .expect("the fallback states its constraint");
        let mut out = String::with_capacity(source.len());
        out.push_str(&source[..at + field]);
        out.push_str("per_minor_dir: Some(\"php83\")");
        out.push_str(&source[at + field + CONSTRAINT_NONE.len()..]);
        out
    }
}
