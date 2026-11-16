//! Protected suite for the canonical printer (`R5`–`R7`, `R25`;
//! `docs/design/blastfile-dsl.md` §14.2, §14.3).
//!
//! Three properties, each over **every** fixture in `tests/fixtures/` and over
//! a hand-written document that uses every production, so a layout rule cannot
//! be right for one shape and wrong for the next:
//!
//! * **P2**: `fmt(fmt(t)) == fmt(t)`, and a canonical file is returned byte for
//!   byte;
//! * **P2b**: the ordered list of comment texts is identical before and after,
//!   and every comment keeps the directive or block it was written against;
//! * **P3**: `lower(fmt(t))` equals `lower(t)`, warnings included.
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

/// P2 over the corpus: formatting twice is formatting once, and the canonical
/// text ends with exactly one newline.
#[test]
fn p2_fixed_point_over_the_corpus() {
    let corpus = common::corpus();
    assert_eq!(common::CORPUS_SIZE, 15, "the corpus contract");
    assert_eq!(corpus.len(), common::CORPUS_SIZE, "the fixture count");
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
/// the corpus and over a document with a comment in every position.
#[test]
fn p2b_comments_survive() {
    for (name, source) in common::corpus() {
        let formatted = blastfile::fmt(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            comment_texts(&formatted),
            comment_texts(&source),
            "{name} lost or reordered a comment"
        );
        assert_eq!(
            comment_anchors(&formatted),
            comment_anchors(&source),
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
    for _ in 0..80_000 {
        source.push_str("delete t\n");
    }
    source.push_str("}\n}\n");
    assert!(source.len() < 1_048_576, "{} bytes", source.len());
    let error = blastfile::fmt(&source).expect_err("the canonical form is over the cap");
    let first = error.errors().first().expect("one refusal");
    assert!(
        first.to_string().contains("1048576"),
        "the refusal names the cap: {first}"
    );
}
