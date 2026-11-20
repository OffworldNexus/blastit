//! `missing_ingress`, `ingress_mismatch` and `raw_ingress` (`R36`).
//!
//! What a request body was is the gate's sample, never the bytes: a MIME
//! essence and the `valid_json`/`valid_form` verdicts are all these findings
//! read, so a report can say which codec to declare without carrying a body.

use std::collections::BTreeSet;

use blastit_ingress::{BodyCodec, BodySample, IngressPolicy, suggest_fix};
use blastit_manifest::Manifest;

use super::diff::body_locator;
use super::findings::{Finding, Fix, Severity};
/// `missing_ingress` and `ingress_mismatch` (`R36`).
pub(crate) fn ingress_findings(
    manifest: &Manifest,
    bodies_per_route: &[Vec<BodySample>],
    findings: &mut Vec<Finding>,
) {
    for (index, samples) in bodies_per_route.iter().enumerate() {
        if samples.is_empty() {
            continue;
        }
        let Some(route) = manifest.route(index) else {
            continue;
        };
        let Some(policy) = route.ingress() else {
            let (codecs, allow_raw) = suggest_fix(samples);
            findings.push(Finding::MissingIngress {
                severity: Severity::Error,
                route: route.name().to_string(),
                observed: distinct_mimes(samples),
                fix: Fix::declare_ingress(codecs, allow_raw),
            });
            continue;
        };
        if policy.body == BodyCodec::Raw {
            // `raw` accepts any bytes; whether it was acknowledged is static
            // (`raw_ingress_findings`).
            continue;
        }
        let mismatching: Vec<BodySample> = samples
            .iter()
            .filter(|sample| !codec_accepts(policy, sample))
            .cloned()
            .collect();
        if mismatching.is_empty() {
            continue;
        }
        let (codecs, allow_raw) = suggest_fix(&mismatching);
        findings.push(Finding::IngressMismatch {
            severity: Severity::Error,
            route: route.name().to_string(),
            declared: policy.body,
            observed: distinct_mimes(&mismatching),
            fix: Fix::fix_ingress(codecs, allow_raw),
        });
    }
}

/// The static `raw_ingress` finding (`R36`).
pub(crate) fn raw_ingress_findings(manifest: &Manifest, findings: &mut Vec<Finding>) {
    for (index, _) in manifest.routes().iter().enumerate() {
        let Some(route) = manifest.route(index) else {
            continue;
        };
        if let Some(policy) = route.ingress()
            && policy.body == BodyCodec::Raw
            && !policy.allow_raw
        {
            findings.push(Finding::RawIngress {
                severity: Severity::Error,
                route: route.name().to_string(),
                fix: Fix::fix_raw_ingress(body_locator(route)),
            });
        }
    }
}

/// Whether a declared codec accepts one observed sample.
fn codec_accepts(policy: IngressPolicy, sample: &BodySample) -> bool {
    match policy.body {
        BodyCodec::Json => sample.valid_json,
        BodyCodec::Form => sample.valid_form,
        BodyCodec::None => false,
        BodyCodec::Raw => true,
    }
}

/// The distinct MIME essences of a set of samples, sorted.
fn distinct_mimes(samples: &[BodySample]) -> Vec<String> {
    samples
        .iter()
        .map(|sample| sample.mime.clone())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}
