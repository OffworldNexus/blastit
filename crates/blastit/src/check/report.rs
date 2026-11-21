//! What a `blastit check` run prints, and the route resolution behind it.
//!
//! Under `--json` the report is one document on stdout and nothing else is
//! (`§1.3`, `R5`); the human form is [`human`]'s. `--learn` prints the proposal,
//! never a partial write, and writes only through the manifest's one editor.

use std::path::Path;

use blastit_dbproxy::{ObsTarget, WireSqlObs};
use blastit_manifest::{Manifest, RoutingTable};

use crate::cli::CheckOptions;
use crate::config::Config;

use super::findings::Report;
use super::human::{human, plural};
use super::learn::{LearnPlan, plan_learn};
use super::obs::CheckObsV1;
use super::{CLEAN, FINDINGS, START_FAILURE};

/// Turns every observed request into the target blastit's own table resolves.
///
/// The gate pushes `Unrouted { method, path }` for every request, because the
/// route is blastit's decision and re-implementing it in Python would be a
/// second, divergent router. Here the same [`RoutingTable`] the audit uses names
/// the route; a path no route matches stays unrouted (`R32`).
pub fn resolve_targets(table: &RoutingTable, manifest: &Manifest, observations: &mut CheckObsV1) {
    let resolved: Vec<WireSqlObs> = observations
        .sql()
        .iter()
        .map(|statement| {
            let ObsTarget::Unrouted { method, path } = &statement.target else {
                return statement.clone();
            };
            let Some(matched) = table.route(method, path) else {
                return statement.clone();
            };
            let Some(route) = manifest.route(matched.route) else {
                return statement.clone();
            };
            WireSqlObs {
                target: ObsTarget::Routed {
                    route: route.name().to_string(),
                },
                ..statement.clone()
            }
        })
        .collect();
    *observations = match CheckObsV1::new(
        observations.schema_version(),
        observations.http().to_vec(),
        resolved,
    ) {
        Ok(updated) => updated,
        // Unreachable: the version was the accepted one a moment ago.
        Err(_) => return,
    };
}

/// Writes the report, human or one JSON document (`§1.3`, `R5`).
pub fn emit_report(report: &Report, json: bool) -> Result<(), String> {
    if json {
        let document = report.to_json().map_err(|error| error.to_string())?;
        println!("{document}");
    } else {
        print!("{}", human(report));
    }
    Ok(())
}

/// `check --learn`: the proposal, its hints, and the one write (`R29`).
pub fn learn(
    config: &Config,
    source: &str,
    manifest: &Manifest,
    observations: &CheckObsV1,
    options: &CheckOptions,
) -> u8 {
    let plan = match plan_learn(manifest, observations) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("blastit check: {error}");
            return START_FAILURE;
        }
    };
    plan.emit_hints();
    for refusal in &plan.refused {
        eprintln!("learn: refused {refusal}");
    }

    let mut wrote = false;
    let mut failure = None;
    // `--dry-run` writes nothing, so the proposal is the output: the very
    // string the write below hands to `write_atomic` (one editor, no second
    // renderer), and the same refusal sentence a real run reports
    // (`docs/ci.md`: "print the proposal, write nothing").
    let mut proposal = None;
    if options.dry_run && plan.would_write(false) {
        match plan.proposal(source) {
            Ok(text) => proposal = Some(text),
            Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
        }
    }
    if plan.would_write(options.dry_run) {
        match plan.proposal(source) {
            Ok(edited) if edited != source => {
                match blastit_manifest::edit::write_atomic(&config.manifest, &edited) {
                    Ok(()) => wrote = true,
                    Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
                }
            }
            Ok(_) => {}
            Err(error) => failure = Some(format!("{}: {error}", config.manifest.display())),
        }
    }

    let clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none();
    if options.json {
        // §1.3: the `--learn --json` document, intentionally without
        // `schema_version`, and never `wrote: true` for a write that failed.
        // [`LearnPlan::json`] is the one renderer of that schema; it derives
        // `wrote` from `would_write`, so it is handed a dry-run value that
        // reports what actually happened (a failed write is `wrote: false`).
        println!("{}", plan.json(options.dry_run || !wrote));
        if clean {
            eprintln!("{}", learn_line(&plan, &config.manifest, options.dry_run));
        }
    } else {
        println!("{}", learn_line(&plan, &config.manifest, options.dry_run));
        if let Some(text) = proposal {
            println!("learn: proposed Blastfile (nothing written):");
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
        }
    }
    if let Some(message) = failure {
        eprintln!("learn: {message}");
    }
    if clean { CLEAN } else { FINDINGS }
}

/// The §1.3 `learn:` line, with the absolute manifest path.
///
/// The two counts are written in their own number — `1 route`, `2 routes`,
/// `1 query`, `3 queries` — because this is a sentence a developer reads. The
/// `--json` document keeps `{"proposed": {"routes": N, "queries": M}}`: a
/// machine key never pluralises.
pub(super) fn learn_line(plan: &LearnPlan, manifest: &Path, dry_run: bool) -> String {
    let path = std::path::absolute(manifest)
        .unwrap_or_else(|_| manifest.to_path_buf())
        .display()
        .to_string();
    let routes = plural(plan.routes(), "route");
    let queries = plural(plan.queries(), "query");
    if dry_run {
        format!(
            "learn: {} {routes}, {} {queries} proposed (dry-run) → {path}",
            plan.routes(),
            plan.queries()
        )
    } else {
        format!(
            "learn: {} {routes}, {} {queries} added → {path}",
            plan.routes(),
            plan.queries()
        )
    }
}
