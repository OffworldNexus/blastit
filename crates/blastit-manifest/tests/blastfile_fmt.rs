//! `blastfile::fmt` over the fixture corpus: idempotence, meaning, and the cap
//! (`R6`, `R7`, `R25`; `docs/design/blastfile-dsl.md` §14.3).
//!
//! Every case runs over **every** fixture in `tests/fixtures/`, not one
//! hand-picked file, and the corpus size is asserted so a later leaf cannot
//! shrink the loop without saying so.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, UidLayout, blastfile};

/// The corpus is the 15 rows of §7.1/§7.2, and every one of them formats.
#[test]
fn the_corpus_is_the_fifteen_fixtures() {
    let corpus = common::corpus();
    assert_eq!(common::CORPUS_SIZE, 15, "the corpus contract");
    assert_eq!(corpus.len(), common::CORPUS_SIZE, "the fixture count");
    for (name, source) in &corpus {
        let formatted = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!formatted.is_empty(), "{name} formatted to nothing");
    }
}

/// P2: formatting twice is formatting once, for every fixture.
#[test]
fn p2_idempotent_over_the_corpus() {
    for (name, source) in common::corpus() {
        let once = blastfile::fmt(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let twice = blastfile::fmt(&once).unwrap_or_else(|error| panic!("{name} twice: {error}"));
        assert_eq!(twice, once, "{name} is not a fixed point");
        assert!(
            once.ends_with('\n') && !once.ends_with("\n\n"),
            "{name} does not end with exactly one newline: {once:?}"
        );
    }
}

/// P3: the canonical text lowers to the same manifest, warnings included, for
/// every fixture.
#[test]
fn p3_meaning_preserved_over_the_corpus() {
    for (name, source) in common::corpus() {
        let (before, before_warnings) = Manifest::parse_blastfile(&source, UidLayout::DEFAULT_BASE)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let formatted = blastfile::fmt(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let (after, after_warnings) =
            Manifest::parse_blastfile(&formatted, UidLayout::DEFAULT_BASE)
                .unwrap_or_else(|error| panic!("{name} formatted: {error}"));
        assert!(
            after.equivalent(&before),
            "{name} changed meaning when formatted"
        );
        assert_eq!(after_warnings, before_warnings, "{name} changed warnings");
    }
}

/// The formatter's output parses: `fmt` never writes a file its own reader
/// refuses (`R25`), and the re-parsed document is the one it just wrote.
#[test]
fn output_reparses() {
    for (name, source) in common::corpus() {
        let formatted = blastfile::fmt(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        Manifest::from_blastfile_str(&formatted)
            .unwrap_or_else(|error| panic!("{name} formatted does not parse: {error}"));
        assert_eq!(
            blastfile::fmt(&formatted).unwrap_or_else(|error| panic!("{name} again: {error}")),
            formatted,
            "{name} is not the document the reader saw"
        );
    }
}

/// A file that is already canonical comes back byte for byte (`R6`).
#[test]
fn a_canonical_file_is_returned_byte_for_byte() {
    let canonical = concat!(
        "app \"shop\"\n",
        "\n",
        "postgres $DATABASE_URL\n",
        "\n",
        "view products GET \"/products\" {\n",
        "    body json\n",
        "    postgres {\n",
        "        select products(id, name)\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(
        blastfile::fmt(canonical).expect("canonical parses"),
        canonical
    );
}

/// A file that does not parse is refused with the ordinary `line:col`, and no
/// canonical text is produced (`R21`).
#[test]
fn a_broken_file_is_refused() {
    let error = blastfile::fmt("view products GET \"/products\" {\n    body xml\n}\n")
        .expect_err("`xml` is not a codec");
    let first = error.errors().first().expect("one refusal");
    let message = first.to_string();
    assert!(
        message.starts_with("2:10: ") && message.contains("not a codec"),
        "the refusal is at the codec: {message}"
    );
}

/// R25: a file that formatting would grow past `MAX_SOURCE_BYTES` is refused
/// rather than written.
#[test]
fn over_cap_is_refused() {
    let mut source = String::from("view v \"/x\" {\npostgres {\n");
    for _ in 0..1_000_000 {
        source.push_str("delete t\n");
    }
    source.push_str("}\n}\n");
    assert!(source.len() < 16_777_216, "{} bytes", source.len());
    let error = blastfile::fmt(&source).expect_err("the canonical form is over the cap");
    assert!(
        error.to_string().contains("16777216"),
        "the refusal names the cap: {error}"
    );
}
