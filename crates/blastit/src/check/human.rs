//! The human `§1.3` rendering of a check report.
//!
//! Every finding names the facts `--json` carries for it, so the human and the
//! machine form cannot drift: the statement a `missing_grant`/`dead_grant` is
//! about, the proxy it ran on, the MIME essences a body finding observed, and
//! the `Blastfile` node its `fix` edits — the file's own nouns, never a retired
//! `route`/`group` id (§13.3). Counts are singular or plural as written.

use blastit_ingress::BodyCodec;

use super::findings::{Finding, Fix, Report};
/// Renders the report as the human `§1.3` block.
///
/// A clean report is the one-line form with `, clean`; a report with findings is
/// the header, one line per finding (with the statements a grant is about
/// indented under it), and the `--json` footer.
pub fn human(report: &Report) -> String {
    let mut out = header(report);
    if report.findings.is_empty() {
        out.push_str(", clean\n");
        return out;
    }
    out.push('\n');
    let mut at = 0;
    while at < report.findings.len() {
        match &report.findings[at] {
            Finding::MissingGrant { route, proxy, .. } => {
                let mut shown: Vec<(String, Fix)> = Vec::new();
                while let Some(Finding::MissingGrant {
                    route: same,
                    proxy: same_proxy,
                    sql,
                    fix,
                    ..
                }) = report.findings.get(at)
                {
                    if same != route || same_proxy != proxy {
                        break;
                    }
                    shown.push((sql.clone(), fix.clone()));
                    at += 1;
                }
                out.push_str(&format!(
                    "  view {route}: {} {} not granted on proxy {proxy}\n",
                    shown.len(),
                    plural(shown.len(), "statement")
                ));
                for (sql, fix) in shown {
                    statement(&mut out, &sql, &fix);
                }
            }
            Finding::DeadGrant {
                route,
                group,
                proxy,
                ..
            } => {
                let mut shown: Vec<(String, Fix)> = Vec::new();
                while let Some(Finding::DeadGrant {
                    route: same,
                    group: same_group,
                    proxy: same_proxy,
                    sql,
                    fix,
                    ..
                }) = report.findings.get(at)
                {
                    if same != route || same_group != group || same_proxy != proxy {
                        break;
                    }
                    shown.push((sql.clone(), fix.clone()));
                    at += 1;
                }
                let owner = match group {
                    Some(group) => format!("trait {group}"),
                    None => format!("view {route}"),
                };
                out.push_str(&format!(
                    "  {owner}: {} {} never used on proxy {proxy}\n",
                    shown.len(),
                    plural(shown.len(), "grant")
                ));
                for (sql, fix) in shown {
                    statement(&mut out, &sql, &fix);
                }
            }
            Finding::UnexercisedRoute { route, fix, .. } => {
                out.push_str(&format!("  view {route}: never exercised\n"));
                out.push_str(&format!("      {}\n", fix_line(fix)));
                at += 1;
            }
            Finding::MissingIngress {
                route,
                observed,
                fix,
                ..
            } => {
                out.push_str(&format!(
                    "  view {route}: takes a body and declares no ingress validation; observed {}\n",
                    mimes(observed)
                ));
                out.push_str(&format!("      {}\n", fix_line(fix)));
                at += 1;
            }
            Finding::IngressMismatch {
                route,
                declared,
                observed,
                fix,
                ..
            } => {
                out.push_str(&format!(
                    "  view {route}: declared body {} does not match observed {}\n",
                    codec_word(*declared),
                    mimes(observed)
                ));
                out.push_str(&format!("      {}\n", fix_line(fix)));
                at += 1;
            }
            Finding::RawIngress { route, fix, .. } => {
                out.push_str(&format!(
                    "  view {route}: declares body raw without `unvalidated`\n"
                ));
                out.push_str(&format!("      {}\n", fix_line(fix)));
                at += 1;
            }
            Finding::UnroutedRequest {
                method, path, fix, ..
            } => {
                out.push_str(&format!("  request {method} {path}: no view matches\n"));
                out.push_str(&format!("      {}\n", fix_line(fix)));
                at += 1;
            }
            Finding::NoObservations { .. } => {
                out.push_str("  suite exercised 0 views or 0 proxy statements\n");
                out.push_str(
                    "      fix: run the suite against this app (does DATABASE_URL point at \
                     blastit?)\n",
                );
                at += 1;
            }
            Finding::ProxyUnused { proxy, .. } => {
                out.push_str(&format!(
                    "  proxy {proxy}: declared, 0 statements observed\n"
                ));
                // The declaration the finding is about is a store (`postgres
                // <name> $URL`), and the file has no `proxy <name>` directive:
                // the sentence names the store, the same noun `--json` documents
                // for `proxy_unused`. The `observe` action carries no single
                // edit, so both forms leave the choice open rather than name a
                // node the file cannot produce.
                out.push_str("      fix: remove the store, or add the call that uses it\n");
                at += 1;
            }
        }
    }
    out.push_str(&format!(
        "blastit check: {} {} · `--json` for the machine-readable list\n",
        report.findings.len(),
        plural(report.findings.len(), "problem")
    ));
    out
}

/// One statement line and the fix line beneath it.
///
/// The statement is what the developer has to recognise, and the fix line is the
/// `Blastfile` node and action that answer it — the two facts `--json` carries
/// as `sql` and `fix`.
fn statement(out: &mut String, sql: &str, fix: &Fix) {
    out.push_str(&format!("    {sql}\n"));
    out.push_str(&format!("      {}\n", fix_line(fix)));
}

/// The `fix:` line of one finding, in `Blastfile` nouns.
///
/// `to` is the node the action edits (§13.3) and is printed verbatim, so the
/// line names exactly what `--json`'s `fix.to` names; an action that edits no
/// node says what to write instead of where. No action is ever printed as a
/// machine id alone.
fn fix_line(fix: &Fix) -> String {
    match (fix.action, fix.to.as_deref()) {
        ("add_query" | "add_group", Some(to)) => format!("fix: add it to {to}"),
        ("remove_query", Some(to)) => format!("fix: remove it from {to}"),
        ("add_route", _) => "fix: add a `view` whose pattern matches this request".to_string(),
        ("add_test", _) => "fix: add a test that reaches it".to_string(),
        // `body raw` is the one codec a view has to acknowledge by name, so the
        // node-only form (`fix_raw_ingress`) names both the codec and the node.
        ("declare_ingress" | "fix_ingress", Some(to)) if fix.codecs.is_none() => {
            format!("fix: write `body raw unvalidated` on {to}")
        }
        ("declare_ingress" | "fix_ingress", _) => {
            let codecs = codecs_text(fix);
            if codecs.is_empty() {
                "fix: declare the codec the body needs".to_string()
            } else {
                format!("fix: write {codecs}")
            }
        }
        // `observe` is the one action that proposes nothing: the statement is
        // redacted or outside what `--learn` may grant, so the author decides.
        ("observe", _) => {
            "fix: none — add the grant by hand once you know the statement".to_string()
        }
        // Unreachable today (every action above is named): stay a sentence
        // rather than printing a machine id at a developer.
        _ => "fix: none — see this finding's action in `--json`".to_string(),
    }
}

/// The codecs a `declare_ingress`/`fix_ingress` fix proposes, as the file
/// writes them: `body json`, `body raw unvalidated`, or `body json or body
/// form` when the observed bodies need different codecs.
fn codecs_text(fix: &Fix) -> String {
    let Some(codecs) = fix.codecs.as_ref() else {
        return String::new();
    };
    let unvalidated = fix.allow_raw == Some(true);
    let mut words: Vec<String> = codecs
        .iter()
        .map(|codec| format!("`{}`", body_text(*codec, unvalidated)))
        .collect();
    match words.len() {
        0 => String::new(),
        1 => words.remove(0),
        _ => {
            let last = words.pop().unwrap_or_default();
            format!("{} or {last}", words.join(", "))
        }
    }
}

/// How a `body` directive writes one codec (§10.1); `raw` is the only one that
/// takes `unvalidated`.
fn body_text(codec: BodyCodec, unvalidated: bool) -> &'static str {
    match codec {
        BodyCodec::Json => "body json",
        BodyCodec::Form => "body form",
        BodyCodec::None => "body none",
        BodyCodec::Raw if unvalidated => "body raw unvalidated",
        BodyCodec::Raw => "body raw",
    }
}

/// The bare codec word, for `declared body <word>`.
fn codec_word(codec: BodyCodec) -> &'static str {
    match codec {
        BodyCodec::Json => "json",
        BodyCodec::Form => "form",
        BodyCodec::None => "none",
        BodyCodec::Raw => "raw",
    }
}

/// The MIME essences one body finding observed, in the order `--json` prints
/// them.
fn mimes(observed: &[String]) -> String {
    if observed.is_empty() {
        return "nothing".to_string();
    }
    observed.join(", ")
}

/// The `blastit check: …` header line, without `, clean`.
///
/// Every count is written in its own number (`1 request`, `2 requests`, `1
/// statement`, `1/1 view`, `3/4 views`) and the third noun is `view`, the noun a
/// `Blastfile` writes and every finding line below uses. `--json` keeps
/// `routes` as its machine key: a document never pluralises.
fn header(report: &Report) -> String {
    let mut line = format!(
        "blastit check: {} {}, {} {}, {}/{} {}",
        report.requests,
        plural(report.requests as usize, "request"),
        report.statements,
        plural(report.statements as usize, "statement"),
        report.routes.exercised,
        report.routes.total,
        plural(report.routes.total as usize, "view"),
    );
    if report.proxies.len() > 1 {
        line.push_str(&format!(", {} proxies", report.proxies.len()));
    }
    line
}

/// `singular` or its plural, for every count the human forms write.
///
/// `--json` never goes through this: a machine key is a key, and `"1 problems"`
/// is not a thing a document has. This is for the sentences a developer reads
/// (`1 grant`, `2 grants`, `1 query`, `3 queries`).
pub fn plural(count: usize, singular: &str) -> String {
    if count == 1 {
        return singular.to_string();
    }
    // A `-y` after a consonant takes `-ies` (`query` → `queries`); every other
    // noun this module writes just takes `s`.
    let consonant_y = singular.ends_with('y')
        && !singular
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|character| "aeiou".contains(character));
    if consonant_y {
        let stem = singular.get(..singular.len() - 1).unwrap_or(singular);
        format!("{stem}ies")
    } else {
        format!("{singular}s")
    }
}
