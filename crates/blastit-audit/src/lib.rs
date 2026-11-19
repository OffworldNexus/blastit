//! The one audit and grant diff (`ARCHITECTURE.md` §6, `docs/ci.md`).
//!
//! Two entry points used to compute the same thing twice: the pytest bridge's
//! [`audit`], which diffs the observations document a plugin writes, and
//! `blastit check`'s [`check_audit`]/[`evaluate`], which diff the two JSONL
//! streams a gate writes. Both answer one question — *what did this view ask
//! for, and what was it granted?* — so the pass that answers it lives here, once:
//!
//! * [`Ledger`] records, per `(view, proxy)`, which statements matched a grant
//!   and which were executed and denied — the grant diff itself;
//! * [`GrantCheck`] is the grant a pair is checked against: the same allowlist
//!   dbproxy enforces, with the statements it holds and whether it allows
//!   transaction control;
//! * the two presenters ([`audit`] and [`evaluate`]) group that diff into the
//!   documents their callers already read. They differ in *grouping* — the
//!   legacy report files a named group's dead grant under each route, `check`
//!   reports it once — never in the answer: the same fixture yields the same
//!   missing and dead statements through both.
//!
//! Nothing here reads a credential: every report and every `stderr` line names
//! a normalised statement, and [`blastit_sqlpolicy::redact_literals`] removes
//! the literals before a statement can reach one.
//!
//! # Invariants that are properties, not style
//!
//! * [`audit`] and [`covering`] return **JSON text**, not values: `fold.py`
//!   `json.loads`es what they return.
//! * every untrusted document — manifest, observations, SQL — is parsed, never
//!   indexed with `unwrap()`/`expect()`; a refusal names the place it happened.
//! * [`blastfile_apply_edits`] is the one writer `fold`, `check --learn` and
//!   `dev --learn` share, and it writes atomically with the file's mode kept.
//! * the `E'…'`/`U&'…'` literal handling of `MB.2.3.1` is
//!   [`blastit_sqlpolicy`]'s: this crate adds no second lexer.

/// How many distinct test node ids and paths one report entry keeps.
pub(crate) const EXAMPLES: usize = 5;

/// Version of the observations document and of the report this build reads.
pub(crate) const VERSION: u64 = 1;

/// The label a request made outside any test is reported under.
pub(crate) const NO_TEST: &str = "(no test)";

mod analyze;
mod assemble;
mod dead;
mod diff;
mod error;
mod evaluate;
mod findings;
mod human;
/// Route inference for `--learn`: the views an unrouted request proposes.
pub mod infer_routes;
mod ingress;
mod learn;
mod ledger;
mod model;
mod normalize;
mod obs;
mod observe;
mod patterns;
mod pytest;
mod sections;
mod static_roots;

pub use analyze::{LOCK_NEEDS_A_COLUMN, analyze, classify, covering};
pub use diff::{GrantCheck, TcProbe, grant_locator, grant_of, resolve_proxy, trait_locator};
pub use error::{AuditError, ObsError};
pub use evaluate::{check_audit, evaluate, parse_manifest};
pub use findings::{Finding, Fix, ProxyCounts, Report, RouteCounts, Severity};
pub use human::{human, plural, probe_ingress_json};
pub use learn::{LearnPlan, plan_learn, plan_learn_json};
pub use ledger::{Ledger, Occurrence, ProxyLedger, RouteLedger};
pub use model::{blastfile_apply_edits, blastfile_model};
pub use normalize::{has_quoted_literal, normalize, redact_literals};
pub use obs::{CheckObsV1, HttpObsLine, SCHEMA_VERSION, parse_http_obs, parse_sql_obs};
pub use patterns::pattern_is_writable;
pub use pytest::audit;
pub use static_roots::{unrequested_static_root_line, unrequested_static_roots};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod check_tests;
#[cfg(test)]
mod tests;
