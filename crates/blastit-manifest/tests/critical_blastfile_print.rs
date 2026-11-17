//! Protected suite for the canonical printer (`R5`–`R7`, `R25`;
//! `docs/design/blastfile-dsl.md` §14.2, §14.3).
//!
//! Four properties, each over §20's **round-trip corpus** — the fixtures in
//! `tests/fixtures/` **plus every document the pages publish** — and over a
//! hand-written document that uses every production, so a layout rule cannot be
//! right for one shape and wrong for the next:
//!
//! * **P1**: no token and no comment of the source is dropped, duplicated,
//!   reordered or rewritten (the byte-level `print_lossless` oracle is a module
//!   test in `syntax.rs`, §14.3.1, because it is `pub(crate)`);
//! * **P2**: `fmt(fmt(t)) == fmt(t)`, and a canonical file is returned byte for
//!   byte;
//! * **P2b**: the ordered list of comment texts is identical before and after,
//!   and every comment keeps the node it was written against;
//! * **P3**: `lower(fmt(t))` equals `lower(t)`, warnings included.
//!
//! The corpus is **derived**, never a literal: `common::round_trip_corpus`
//! enumerates the fixture directory and reads every `text title="Blastfile"`
//! fence off `README.md` and `docs/**`, and asserts §20's floor of 20, so a
//! page whose example does not round-trip fails this gate instead of shrinking
//! the corpus silently (`MB.21.5.7`). The documented **fragments** — the
//! keyword-led plain `text` fences that are statement lists, not files — are
//! classified and round-tripped as fragments rather than skipped.
//!
//! The comment oracle is a scanner over the text, independent of the lexer: a
//! formatter that drops every comment cannot pass by agreeing with itself.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_manifest::{Manifest, UidLayout, blastfile};

/// One significant thing in a document: a comment, or a token.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// A comment, from its `#` to the end of its line.
    Comment(String),
    /// One token, as written.
    Token(String),
}

/// The comments and tokens of a document, in source order.
///
/// A test-local scanner: it knows the two string forms and `#`, and nothing
/// else. It never looks at the crate's lexer, so the two cannot share a bug.
fn items(text: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let width = character.len_utf8();
        match character {
            '#' => {
                let end = rest.find('\n').unwrap_or(rest.len());
                let comment = rest.get(..end).unwrap_or("").trim_end_matches('\r');
                items.push(Item::Comment(comment.to_owned()));
                rest = rest.get(end..).unwrap_or("");
            }
            '"' => {
                let end = escaped_string_end(rest);
                items.push(Item::Token(rest.get(..end).unwrap_or("").to_owned()));
                rest = rest.get(end..).unwrap_or("");
            }
            '\'' => {
                let end = raw_string_end(rest);
                items.push(Item::Token(rest.get(..end).unwrap_or("").to_owned()));
                rest = rest.get(end..).unwrap_or("");
            }
            ';' => rest = rest.get(width..).unwrap_or(""),
            character if character.is_whitespace() => rest = rest.get(width..).unwrap_or(""),
            '{' | '}' | ',' | '(' | ')' | '.' => {
                items.push(Item::Token(character.to_string()));
                rest = rest.get(width..).unwrap_or("");
            }
            _ => {
                let end = rest
                    .char_indices()
                    .find(|(_, character)| {
                        character.is_whitespace()
                            || matches!(
                                character,
                                '#' | '"' | '\'' | ';' | '{' | '}' | ',' | '(' | ')' | '.'
                            )
                    })
                    .map(|(at, _)| at)
                    .unwrap_or(rest.len());
                items.push(Item::Token(rest.get(..end).unwrap_or("").to_owned()));
                rest = rest.get(end..).unwrap_or("");
            }
        }
    }
    items
}

/// The byte just past the `"…"` string that starts `text`.
fn escaped_string_end(text: &str) -> usize {
    let mut characters = text.char_indices();
    characters.next();
    while let Some((at, character)) = characters.next() {
        match character {
            '\\' => {
                characters.next();
            }
            '"' => return at.saturating_add(1),
            _ => {}
        }
    }
    text.len()
}

/// The byte just past the `'…'` string that starts `text`.
fn raw_string_end(text: &str) -> usize {
    text.char_indices()
        .skip(1)
        .find(|(_, character)| *character == '\'')
        .map(|(at, _)| at.saturating_add(1))
        .unwrap_or(text.len())
}

/// Every token of a document, in source order (P1's half of the scan).
fn tokens(text: &str) -> Vec<String> {
    items(text)
        .into_iter()
        .filter_map(|item| match item {
            Item::Token(token) => Some(token),
            Item::Comment(_) => None,
        })
        .collect()
}

/// Every comment of a document, in source order (P2b's first half).
fn comment_texts(text: &str) -> Vec<String> {
    items(text)
        .into_iter()
        .filter_map(|item| match item {
            Item::Comment(comment) => Some(comment),
            Item::Token(_) => None,
        })
        .collect()
}

/// The next token after every comment, in source order (P2b's second half).
///
/// A comment that keeps its place keeps the directive it was written against,
/// so the token after it is the same before and after `fmt`.
fn comment_anchors(text: &str) -> Vec<Option<String>> {
    let mut anchors: Vec<Option<String>> = Vec::new();
    let mut waiting = 0;
    for item in items(text) {
        match item {
            Item::Comment(_) => {
                anchors.push(None);
                waiting += 1;
            }
            Item::Token(token) => {
                for anchor in anchors.iter_mut().rev().take(waiting) {
                    *anchor = Some(token.clone());
                }
                waiting = 0;
            }
        }
    }
    anchors
}

/// A document that uses every production, in canonical layout.
const EVERY_PRODUCTION: &str = concat!(
    "# the whole grammar, in one file\n",
    "app \"shop\"\n",
    "\n",
    "entrypoint python \"shop.wsgi:application\" {\n",
    "    preload \"shop.urls\", \"shop.signals\"\n",
    "}\n",
    "\n",
    "postgres $DATABASE_URL\n",
    "\n",
    "postgres analytics $ANALYTICS_URL\n",
    "\n",
    "postgres reporting {\n",
    "    host $DB_HOST\n",
    "    port $DB_PORT\n",
    "    user $DB_USERNAME\n",
    "    password $DB_PASSWORD\n",
    "    database $DB_DATABASE\n",
    "    sslmode $DB_SSLMODE\n",
    "    ca_cert $DB_CA_CERT\n",
    "}\n",
    "\n",
    "trait session {\n",
    "    postgres {\n",
    "        transactions\n",
    "        functions st_distance, pg_catalog.unaccent\n",
    "        select products(id, name), categories\n",
    "        insert orders(id, total)\n",
    "        update products(price)\n",
    "        delete archived_products\n",
    "        query 'SELECT count(*) FROM products WHERE price > $?'\n",
    "        query \"SET TIME ZONE 'UTC'\"\n",
    "    }\n",
    "}\n",
    "\n",
    "view products GET, POST \"/products\", \"/products/{id:int}\" {\n",
    "    body json\n",
    "    use session\n",
    "    egress any\n",
    "    prefork\n",
    "    postgres {\n",
    "        select products(id, name)\n",
    "    }\n",
    "}\n",
    "\n",
    "view cart {\n",
    "    GET \"/cart\"\n",
    "    POST \"/cart/items\" {\n",
    "        body form\n",
    "    }\n",
    "    DELETE, OPTIONS \"/cart/items/{id:int}\"\n",
    "    postgres analytics unrestricted\n",
    "}\n",
    "\n",
    "view items regex '/v[0-9]+/items' {\n",
    "    body none\n",
    "}\n",
    "\n",
    "view uploads PUT \"/uploads\" {\n",
    "    body raw unvalidated\n",
    "}\n",
    "\n",
    "view admin \"/admin\" {\n",
    "    postgres unrestricted\n",
    "}\n",
    "\n",
    "static \"/assets\" {\n",
    "    root \"public/assets\"\n",
    "    index \"index.html\"\n",
    "    fallback \"index.html\"\n",
    "    immutable \"/assets/*\", \"/fonts/*\"\n",
    "}\n",
);

/// The same document with `;` separators, no indentation, blank lines in a
/// block and a one-line endpoint block: every rule of §14.2 is exercised.
const MESSY: &str = concat!(
    "# the whole grammar, in one file\n",
    ";app \"shop\"\n",
    "\n\n\n",
    "entrypoint python \"shop.wsgi:application\" { preload \"shop.urls\",\"shop.signals\" }\n",
    "\n",
    "postgres   $DATABASE_URL\n",
    "\n",
    "postgres analytics $ANALYTICS_URL\n",
    "\n",
    "postgres reporting {\n",
    "host $DB_HOST;port $DB_PORT\n",
    "user $DB_USERNAME\n",
    "\n",
    "password $DB_PASSWORD\n",
    "database $DB_DATABASE\n",
    "sslmode $DB_SSLMODE\n",
    "ca_cert $DB_CA_CERT\n",
    "}\n",
    "\n",
    "trait session {\n",
    "postgres {\n",
    "transactions\n",
    "functions st_distance,pg_catalog.unaccent\n",
    "select products(id,name),\n",
    "categories\n",
    "insert orders(id,total)\n",
    "update products(price)\n",
    "delete archived_products\n",
    "query 'SELECT count(*) FROM products WHERE price > $?'\n",
    "query \"SET TIME ZONE 'UTC'\"\n",
    "}\n",
    "}\n",
    "\n",
    "view products GET,POST \"/products\",\"/products/{id:int}\" {\n",
    "body json\n",
    "use session\n",
    "egress any\n",
    "prefork\n",
    "postgres { select products(id,name) }\n",
    "}\n",
    "\n",
    "view cart {\n",
    "GET \"/cart\"\n",
    "POST \"/cart/items\" { body form }\n",
    "DELETE,OPTIONS \"/cart/items/{id:int}\"\n",
    "postgres analytics unrestricted\n",
    "}\n",
    "\n",
    "view items regex '/v[0-9]+/items' { body none }\n",
    "\n",
    "view uploads PUT \"/uploads\" { body raw unvalidated }\n",
    "\n",
    "view admin \"/admin\" { postgres unrestricted }\n",
    "\n",
    "static \"/assets\" {\n",
    "root \"public/assets\"\n",
    "index \"index.html\"\n",
    "fallback \"index.html\"\n",
    "immutable \"/assets/*\",\"/fonts/*\"\n",
    "}\n",
);

/// A document with a comment in every position §14.2 names: above a section,
/// above a directive, trailing a directive, trailing a block's brace, and the
/// last line of the file without a newline.
const COMMENTED: &str = concat!(
    "# above the view\n",
    "view v \"/x\" { # on the header line\n",
    "    body json # trailing the directive\n",
    "    # above the grant\n",
    "    postgres { # on the block's header\n",
    "        select t\n",
    "    } # after the block\n",
    "}\n",
    "# the last line, no newline\n",
);

/// The same document, written the way a developer leaves it.
const COMMENTED_MESSY: &str = concat!(
    "# above the view\n",
    "view v \"/x\" {   # on the header line\n",
    "  body json   # trailing the directive\n",
    "\n",
    "  # above the grant\n",
    "  postgres {   # on the block's header\n",
    "        select t\n",
    "  }   # after the block\n",
    "}\n",
    "# the last line, no newline",
);

/// P1 (lossless) over the corpus, at the public boundary: the canonical text
/// carries **every** token and **every** comment of the source, in the same
/// order — formatting moves layout and rewrites nothing.
///
/// §14.3.1 states P1 as `print_lossless(parse(text)?) == text`, which is a
/// module test inside `syntax.rs` because `print_lossless` is `pub(crate)` and
/// an integration suite cannot call it. What a caller can observe, and what
/// this test pins over the whole corpus, is that nothing significant is lost:
/// a token or a comment the source had is a token or a comment the canonical
/// text still has, once, in the same place in the sequence.
#[test]
fn p1_no_token_or_comment_is_dropped() {
    let corpus = common::round_trip_corpus();
    for (name, source) in &corpus {
        let formatted = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            tokens(&formatted),
            tokens(source),
            "{name} lost, duplicated or reordered a token"
        );
        assert_eq!(
            comment_texts(&formatted),
            comment_texts(source),
            "{name} lost, duplicated or reordered a comment"
        );
    }
}

/// P2 over the corpus: formatting twice is formatting once, and the canonical
/// text ends with exactly one newline.
#[test]
fn p2_fixed_point_over_the_corpus() {
    // The fixture set keeps its own pinned size (§20's 15 rows); the
    // round-trip corpus is that set plus the documented documents.
    assert_eq!(common::CORPUS_SIZE, 15, "the corpus contract");
    assert_eq!(
        common::corpus().len(),
        common::CORPUS_SIZE,
        "the fixture count"
    );
    let corpus = common::round_trip_corpus();
    assert!(
        corpus.len() >= 20,
        "§20's floor is 20 documents; the corpus holds {}",
        corpus.len()
    );
    for (name, source) in &corpus {
        let once = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let twice = blastfile::fmt(&once).unwrap_or_else(|error| panic!("{name} twice: {error}"));
        assert_eq!(twice, once, "{name} is not a fixed point");
        assert!(
            once.ends_with('\n') && !once.ends_with("\n\n"),
            "{name} does not end with exactly one newline: {once:?}"
        );
    }
}

/// P2 over the whole grammar: a canonical document is returned byte for byte,
/// and the messy spelling of it formats to exactly that document.
#[test]
fn p2_fixed_point_over_every_production() {
    assert_eq!(
        blastfile::fmt(EVERY_PRODUCTION).expect("the canonical document parses"),
        EVERY_PRODUCTION
    );
    assert_eq!(
        blastfile::fmt(MESSY).expect("the messy document parses"),
        EVERY_PRODUCTION
    );
    assert_eq!(
        blastfile::fmt(&blastfile::fmt(MESSY).expect("formats")).expect("formats twice"),
        EVERY_PRODUCTION
    );
}

/// §21: every fixture is already in the canonical layout, so `blastit fmt
/// --check` is clean on the corpus and a fixture that drifts is caught here
/// rather than by a later leaf's gate.
#[test]
fn the_corpus_is_canonical() {
    let corpus = common::corpus();
    assert_eq!(common::CORPUS_SIZE, 15, "the corpus contract");
    assert_eq!(corpus.len(), common::CORPUS_SIZE, "the fixture count");
    for (name, source) in &corpus {
        let formatted = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(formatted, *source, "{name} is not in the canonical layout");
    }
}

/// P2b: the ordered comment texts are identical before and after `fmt`, over
/// the corpus, over the documented fragments, and over a document with a
/// comment in every position.
#[test]
fn p2b_comments_survive() {
    let mut corpus = common::round_trip_corpus();
    corpus.extend(
        common::documented_fragments()
            .into_iter()
            .map(|fence| (fence.label(), fence.source)),
    );
    for (name, source) in &corpus {
        let formatted = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            comment_texts(&formatted),
            comment_texts(source),
            "{name} lost or reordered a comment"
        );
        assert_eq!(
            comment_anchors(&formatted),
            comment_anchors(source),
            "{name} moved a comment away from the directive it describes"
        );
    }
    assert_eq!(
        blastfile::fmt(COMMENTED_MESSY).expect("the commented document parses"),
        COMMENTED
    );
    assert_eq!(comment_texts(COMMENTED), comment_texts(COMMENTED_MESSY));
    assert_eq!(comment_anchors(COMMENTED), comment_anchors(COMMENTED_MESSY));
    // Every one of the seven comments is still there, once.
    assert_eq!(comment_texts(COMMENTED).len(), 7);
}

/// A comment that has no line of its own — one written inside a list the
/// canonical layout never wraps — is kept, above the statement it belongs to,
/// and stays there on the second pass.
#[test]
fn a_comment_inside_a_list_is_kept_above_its_statement() {
    // A view header list and a grant list: both are unwrapped, so a comment
    // written inside either moves above the statement it belongs to.
    for (source, canonical) in [
        (
            "view v GET,\n    # the second method\n    POST \"/x\" {\n    body none\n}\n",
            concat!(
                "# the second method\n",
                "view v GET, POST \"/x\" {\n",
                "    body none\n",
                "}\n",
            ),
        ),
        (
            "view v \"/x\" {\n    postgres {\n        select products(id),\n            # and the categories\n            categories\n    }\n}\n",
            concat!(
                "view v \"/x\" {\n",
                "    postgres {\n",
                "        # and the categories\n",
                "        select products(id), categories\n",
                "    }\n",
                "}\n",
            ),
        ),
    ] {
        let formatted = blastfile::fmt(source).expect("parses");
        assert_eq!(formatted, canonical);
        assert_eq!(comment_texts(&formatted), comment_texts(source));
        assert_eq!(
            blastfile::fmt(&formatted).expect("formats twice"),
            canonical,
            "the hoisted comment is not a fixed point"
        );
    }
}

/// P3 over the corpus: the canonical text lowers to the same manifest, and the
/// same warnings.
#[test]
fn p3_over_the_corpus() {
    let corpus = common::round_trip_corpus();
    for (name, source) in &corpus {
        let (before, before_warnings) = Manifest::parse_blastfile(source, UidLayout::DEFAULT_BASE)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let formatted = blastfile::fmt(source).unwrap_or_else(|error| panic!("{name}: {error}"));
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

// ---- the documented fragments, classified rather than skipped -----------

/// Every fence on every published page is classified (§16.1), and the ones
/// that are **fragments** — a keyword-led plain `text` fence, i.e. a statement
/// list that is not a complete file — are named, classified and checked rather
/// than silently skipped.
///
/// This is the "no silent skip" rule for §20's corpus. The corpus itself is
/// complete documents only (a fragment binds no `app` and has no listener to
/// describe), so a page whose example is a statement list has to be accounted
/// for here or the gate quietly shrinks.
///
/// A statement list is not always a file: `docs/reference/blastfile.md` shows
/// the three store spellings as one fence, and two unnamed stores are a
/// refusal on purpose. So the fragment class splits into the ones that lower
/// (`from_blastfile_str` succeeds) and the ones that show alternatives
/// (lowering refuses), and **the split is derived per fragment**, never a
/// table; the second group is still checked — formatting never turns a refused
/// statement list into an accepted file, or the other way round — and printed
/// by page and line so a reader sees what the branch covers.
#[test]
fn the_documented_fragments_are_classified_and_checked() {
    let all = common::documented_fences();
    let documents = common::documented_documents();
    let fragments = common::documented_fragments();
    assert!(
        !fragments.is_empty(),
        "the pages carry statement lists; none was classified as a fragment"
    );
    // The three filters are exhaustive over the scanned fences: every fence the
    // reader returns is counted by exactly one of the three independent passes,
    // so a fence that fell out of all three would show up as a short sum.
    let illustrations = all
        .iter()
        .filter(|fence| fence.class == common::FenceClass::Illustration)
        .count();
    assert_eq!(
        documents.len() + fragments.len() + illustrations,
        all.len(),
        "the three filters are exhaustive over the scanned fences: {} document(s) + \
         {} fragment(s) + {} illustration(s) != {} fence(s)",
        documents.len(),
        fragments.len(),
        illustrations,
        all.len()
    );
    assert!(
        illustrations > 0,
        "the illustration class has no subject: the classification is not doing work"
    );
    let mut alternatives = Vec::new();
    for fence in &fragments {
        let name = fence.label();
        let formatted = blastfile::fmt(&fence.source)
            .unwrap_or_else(|error| panic!("{name} is a statement list: {error}"));
        assert_eq!(
            blastfile::fmt(&formatted).unwrap_or_else(|error| panic!("{name} twice: {error}")),
            formatted,
            "{name} is not a fixed point"
        );
        assert_eq!(
            tokens(&formatted),
            tokens(&fence.source),
            "{name} lost, duplicated or reordered a token"
        );
        assert_eq!(
            comment_texts(&formatted),
            comment_texts(&fence.source),
            "{name} lost, duplicated or reordered a comment"
        );
        // A fragment is a statement list of top-level sections, so it lowers
        // to the model like a document — unless it is a menu of alternative
        // spellings, which a page may show in one fence. Either way formatting
        // cannot change which of the two it is.
        let before = Manifest::from_blastfile_str(&fence.source);
        let after = Manifest::from_blastfile_str(&formatted);
        match (before, after) {
            (Ok(before), Ok(after)) => assert!(
                after.equivalent(&before),
                "{name} changed meaning when formatted"
            ),
            (Err(_), Err(_)) => alternatives.push(name),
            (before, after) => panic!(
                "{name} is {} before formatting and {} after: the layout changed whether \
                 the statement list is a file",
                outcome(&before),
                outcome(&after)
            ),
        }
    }
    // Reported by page and line, not counted: a reader can see which fragments
    // are illustrations of alternative spellings without a second table.
    for name in &alternatives {
        println!("statement list of alternatives: {name}");
    }
}

/// `Ok`/`Err`, for a panic message about a fragment.
fn outcome<T, E>(result: &Result<T, E>) -> &'static str {
    if result.is_ok() {
        "a file"
    } else {
        "alternatives"
    }
}

/// P3 over the whole grammar, and over the commented document: the layout is
/// the only thing `fmt` changes.
#[test]
fn p3_over_every_production() {
    for source in [EVERY_PRODUCTION, MESSY, COMMENTED_MESSY] {
        let before = Manifest::from_blastfile_str(source).expect("the document lowers");
        let formatted = blastfile::fmt(source).expect("the document formats");
        let after = Manifest::from_blastfile_str(&formatted).expect("the canonical text lowers");
        assert!(after.equivalent(&before), "formatting changed the manifest");
    }
}

/// R25: a `query` that holds a `'` or a `"` is written back byte for byte, and
/// the output parses — the formatter never emits a file its own reader refuses.
#[test]
fn a_query_with_a_quote() {
    let source = concat!(
        "view v \"/x\" {\n",
        "    postgres {\n",
        "        query \"SELECT \\\"a\\\", 'b' FROM t\"\n",
        "        query 'SELECT \\d+ FROM t'\n",
        "    }\n",
        "}\n",
    );
    let formatted = blastfile::fmt(source).expect("the document parses");
    assert_eq!(formatted, source);
    Manifest::from_blastfile_str(&formatted).expect("the canonical text parses");
}

/// R25: a file that formatting would grow past `MAX_SOURCE_BYTES` is refused
/// rather than written.
#[test]
fn fmt_refuses_a_file_it_would_grow_past_the_cap() {
    let mut source = String::from("view v \"/x\" {\npostgres {\n");
    for _ in 0..1_000_000 {
        source.push_str("delete t\n");
    }
    source.push_str("}\n}\n");
    assert!(source.len() < 16_777_216, "{} bytes", source.len());
    let error = blastfile::fmt(&source).expect_err("the canonical form is over the cap");
    let first = error.errors().first().expect("one refusal");
    assert!(
        first.to_string().contains("16777216"),
        "the refusal names the cap: {first}"
    );
}
