//! Standard-tier coverage for the surgical `Blastfile` editor: the shapes a
//! hand-written grant block can have, where a new `postgres` block lands, which
//! comment belongs to the catch-all, and the refusals that happen before
//! anything is written.
//!
//! `critical_manifest_edit.rs` proves the promises (surgical, idempotent, every
//! output boots). This file walks the branches of the editor that decides them,
//! because every one of those branches either edits a file a person wrote or
//! refuses to.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use blastit_manifest::blastfile::{BlastfileEdits, ProposedGrant, ProposedView, apply_edits};
use blastit_manifest::edit::{EditError, write_atomic};
use blastit_manifest::{Grant, Manifest, ProxyName};

/// The declaration every fixture shares: the default store, `db` (§7.4).
const HEAD: &str = "postgres $DATABASE_URL\n";

/// The statement a view already grants in the fixtures below.
const KEPT: &str = r#"SELECT "t"."a" FROM "t""#;
/// The statement the edits add.
const ADDED: &str = r#"SELECT "t"."b" FROM "t""#;

fn grant(view: &str, queries: &[&str]) -> ProposedGrant {
    ProposedGrant {
        view: view.to_string(),
        store: Some("db".to_string()),
        queries: queries.iter().map(|query| query.to_string()).collect(),
    }
}

fn grant_on(view: &str, store: &str, queries: &[&str]) -> ProposedGrant {
    ProposedGrant {
        view: view.to_string(),
        store: Some(store.to_string()),
        queries: queries.iter().map(|query| query.to_string()).collect(),
    }
}

fn new_view(name: &str, pattern: &str) -> ProposedView {
    ProposedView {
        name: name.to_string(),
        methods: Vec::new(),
        patterns: vec![pattern.to_string()],
    }
}

/// The view names of a manifest, in document order.
fn view_names(manifest: &Manifest) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for route in manifest.routes() {
        if names.last().copied() != Some(route.name()) {
            names.push(route.name());
        }
    }
    names
}

/// The `queries` one view grants on `store`, as written.
fn queries_in(manifest: &Manifest, name: &str, store: &str) -> Vec<String> {
    manifest
        .routes()
        .iter()
        .find(|route| route.name() == name)
        .and_then(|route| route.grant(&ProxyName::from(store)))
        .and_then(Grant::as_postgresql)
        .map(|grant| grant.queries().to_vec())
        .unwrap_or_default()
}

/// The edited text, parsed strictly: every branch below has to produce a
/// document that still reads back.
fn edited_and_parsed(source: &str, edits: &BlastfileEdits) -> (String, Manifest) {
    let edited = apply_edits(source, edits).expect("the edit applies");
    let manifest = Manifest::from_blastfile_str(&edited).expect("the edit parses");
    (edited, manifest)
}

//
// The shapes of an existing grant block.
//

/// A block the author wrote on one line holds a statement, so it is not empty:
/// the editor has to put a `;` between the author's `query` and the new one.
/// Reading the block as empty spliced `query 'a' query 'b'`, which is not a
/// document, and the postcondition then refused the edit.
#[test]
fn a_one_line_grant_block_gains_a_separator_before_the_new_statement() {
    let source = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{ query '{KEPT}' }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains(&format!("postgres {{ query '{KEPT}'; query '{ADDED}' }}")),
        "the one-line block was extended without a separator:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [KEPT.to_string(), ADDED.to_string()]
    );

    // A clause is a bare token, not a node: `transactions` is content too, so
    // the statement after it needs the separator.
    let clause = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{ transactions }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &clause,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );
    assert!(
        edited.contains(&format!("postgres {{ transactions; query '{ADDED}' }}")),
        "the clause was treated as an empty block:\n{edited}"
    );
    assert_eq!(queries_in(&manifest, "products", "db"), [ADDED.to_string()]);
}

/// A view's own header is not content: an empty one-line view gets its new
/// block directly inside the braces, and a one-line view that already holds a
/// statement gets the `;` the shape needs.
#[test]
fn a_one_line_view_gets_its_new_block_inside_the_braces() {
    let empty = format!("{HEAD}\nview products GET \"/products/\" {{ }}\n");
    let (edited, manifest) = edited_and_parsed(
        &empty,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );
    assert!(
        edited.contains(&format!(
            "view products GET \"/products/\" {{ postgres {{ query '{ADDED}' }} }}"
        )),
        "the block did not land inside the one-line view:\n{edited}"
    );
    assert_eq!(queries_in(&manifest, "products", "db"), [ADDED.to_string()]);

    let declaring = format!("{HEAD}\nview products GET \"/products/\" {{ body json }}\n");
    let (edited, manifest) = edited_and_parsed(
        &declaring,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );
    assert!(
        edited.contains(&format!(
            "view products GET \"/products/\" {{ body json; postgres {{ query '{ADDED}' }} }}"
        )),
        "the block was not separated from the view's own statement:\n{edited}"
    );
    assert_eq!(queries_in(&manifest, "products", "db"), [ADDED.to_string()]);
    assert_eq!(
        edited.matches("body json").count(),
        1,
        "the author's directive was duplicated:\n{edited}"
    );
}

#[test]
fn a_block_with_several_clauses_is_extended_after_them() {
    let source = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{\n\
         \x20       transactions\n\
         \x20       select products(id, name)\n\
         \x20       query '{KEPT}'\n\
         \x20   }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    // The clauses the author wrote are byte-identical, and the new statement is
    // the block's last line: nothing was reflowed or reordered.
    assert!(
        edited.contains(&format!(
            "        select products(id, name)\n        query '{KEPT}'\n        query '{ADDED}'\n"
        )),
        "the block was not extended after its clauses:\n{edited}"
    );
    assert_eq!(edited.matches("postgres {").count(), 1, "{edited}");
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [KEPT.to_string(), ADDED.to_string()]
    );
    let products = manifest
        .routes()
        .iter()
        .find(|route| route.name() == "products")
        .and_then(|route| route.grant(&ProxyName::from("db")))
        .and_then(Grant::as_postgresql)
        .expect("the grant");
    assert!(products.transaction_control(), "`transactions` was lost");
    assert_eq!(
        products.tables().tables().len(),
        1,
        "the table grant was lost: {:?}",
        products.tables()
    );
}

#[test]
fn a_comment_inside_the_block_stays_above_the_inserted_query() {
    let source = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{\n\
         \x20       query '{KEPT}'\n\
         \x20       # learned by --learn, edited by hand\n\
         \x20   }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains(&format!(
            "        # learned by --learn, edited by hand\n        query '{ADDED}'\n"
        )),
        "the author's note moved or the query landed above it:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [KEPT.to_string(), ADDED.to_string()]
    );
}

#[test]
fn a_block_without_a_query_clause_gets_one() {
    let source = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{\n\
         \x20       transactions\n\
         \x20   }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert!(
        edited.contains(&format!("        transactions\n        query '{ADDED}'\n")),
        "the statement does not follow the block's last clause:\n{edited}"
    );
    assert_eq!(queries_in(&manifest, "products", "db"), [ADDED.to_string()]);
}

//
// Where a new block lands.
//

#[test]
fn a_view_without_a_block_for_that_store_gets_one_at_the_end() {
    let source = format!(
        "{HEAD}\npostgres analytics $ANALYTICS_URL\n\n\
         view products GET \"/products/\" {{\n\
         \x20   postgres {{\n\
         \x20       query '{KEPT}'\n\
         \x20   }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant_on("products", "analytics", &[ADDED])],
        },
    );

    assert!(
        edited.contains(&format!(
            "    postgres {{\n        query '{KEPT}'\n    }}\n\
             \x20   postgres analytics {{\n        query '{ADDED}'\n    }}\n}}\n"
        )),
        "the new block was not written at the end of the view:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [KEPT.to_string()],
        "the existing block was rewritten"
    );
    assert_eq!(
        queries_in(&manifest, "products", "analytics"),
        [ADDED.to_string()]
    );
}

#[test]
fn a_new_block_is_separated_from_the_views_own_fields() {
    let source = format!(
        "{HEAD}\npostgres analytics $ANALYTICS_URL\n\n\
         view products GET \"/products/\" {{\n\
         \x20   use catalogue\n\
         }}\n\
         \ntrait catalogue {{\n\
         \x20   postgres {{\n\
         \x20       query '{KEPT}'\n\
         \x20   }}\n\
         }}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant_on("products", "analytics", &[ADDED])],
        },
    );

    assert!(
        edited.contains(&format!(
            "    use catalogue\n    postgres analytics {{\n        query '{ADDED}'\n    }}\n}}\n"
        )),
        "the new block is not the view's last statement:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products", "analytics"),
        [ADDED.to_string()]
    );
    assert!(
        edited.contains(&format!("        query '{KEPT}'\n")),
        "the trait was rewritten:\n{edited}"
    );
}

//
// Where a new view lands.
//

#[test]
fn a_new_view_goes_above_the_comment_run_that_documents_the_catch_all() {
    // No blank line above the comments: the run documents the catch-all, so the
    // new block goes above it and the whole insertion site is re-spaced.
    let source = format!(
        "{HEAD}\nview about GET \"/about/\" {{\n}}\n\
         # The first comment.\n# The second comment.\nview fallback \"/*\" {{\n}}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: Vec::new(),
        },
    );

    let inserted = edited.find("view orders").expect("the new view");
    for kept in ["# The first comment.", "# The second comment."] {
        let at = edited.find(kept).expect("the comment");
        assert!(
            inserted < at,
            "{kept} is no longer attached to the catch-all"
        );
    }
    // One blank line separates the block from the view above it and from the
    // comment run below it: no more, no fewer.
    assert!(
        edited.contains("}\n\nview orders \"/orders/\" {\n}\n\n# The first comment.\n"),
        "the insertion site is not separated by exactly one blank line:\n{edited}"
    );
    assert!(
        !edited.contains("\n\n\n"),
        "a doubled blank line:\n{edited}"
    );
    assert_eq!(view_names(&manifest), ["about", "orders", "fallback"]);
}

#[test]
fn a_comment_run_that_already_has_a_blank_line_above_it_keeps_exactly_one() {
    let source = format!(
        "{HEAD}\nview about GET \"/about/\" {{\n}}\n\n\
         # The comment for the catch-all.\nview fallback \"/*\" {{\n}}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: Vec::new(),
        },
    );

    assert!(
        edited.contains("}\n\nview orders \"/orders/\" {\n}\n"),
        "the block is not separated from the view above it:\n{edited}"
    );
    assert!(
        !edited.contains("\n\n\n"),
        "a doubled blank line:\n{edited}"
    );
    assert_eq!(view_names(&manifest), ["about", "orders", "fallback"]);
}

#[test]
fn a_query_that_reads_like_a_directive_is_never_edited_as_one() {
    // The statement holds the text of a `view` header and of a `postgres`
    // block. The editor reads the tree, so neither is structure: the real block
    // is extended and the statement comes back byte for byte.
    let trap = "SELECT 'view fake /* {'";
    let source = format!(
        "{HEAD}\nview products GET \"/products/\" {{\n\
         \x20   postgres {{\n\
         \x20       query \"{trap}\"\n\
         \x20   }}\n\
         }}\n\nview fallback \"/*\" {{\n}}\n"
    );
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: vec![grant("products", &[ADDED])],
        },
    );

    assert_eq!(
        view_names(&manifest),
        ["products", "orders", "fallback"],
        "the statement was read as a view:\n{edited}"
    );
    assert_eq!(edited.matches("postgres {").count(), 1, "{edited}");
    assert!(
        edited.contains(&format!("query \"{trap}\"")),
        "the author's statement changed:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [trap.to_string(), ADDED.to_string()]
    );
}

#[test]
fn two_proposed_views_are_written_in_order_with_a_blank_line_between_them() {
    let source = format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("carts", "/carts/"), new_view("orders", "/orders/")],
            grants: vec![grant("carts", &[ADDED])],
        },
    );

    assert_eq!(view_names(&manifest), ["about", "carts", "orders"]);
    assert!(
        edited.contains("view about GET \"/about/\" {\n}\n\nview carts \"/carts/\" {\n"),
        "the append re-spaced the existing view:\n{edited}"
    );
    assert!(
        edited.contains(&format!(
            "view carts \"/carts/\" {{\n    postgres {{\n        query '{ADDED}'\n    }}\n}}\n\n\
             view orders \"/orders/\" {{\n}}\n"
        )),
        "the proposed views and their grant block are not laid out as documented:\n{edited}"
    );
    assert_eq!(queries_in(&manifest, "carts", "db"), [ADDED.to_string()]);
}

#[test]
fn a_missing_final_newline_still_separates_the_appended_view() {
    // No trailing newline: the append still separates whole lines.
    let source = format!("{HEAD}\nview about GET \"/about/\" {{\n}}");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: Vec::new(),
        },
    );

    assert!(
        edited.ends_with("view orders \"/orders/\" {\n}\n"),
        "the appended view does not end the file:\n{edited:?}"
    );
    assert!(
        edited.contains("view about GET \"/about/\" {\n}\n\nview orders"),
        "the append did not separate the two views:\n{edited:?}"
    );
    assert_eq!(view_names(&manifest), ["about", "orders"]);
}

#[test]
fn a_crlf_document_keeps_its_line_endings() {
    let source = format!(
        "postgres $DATABASE_URL\r\n\r\nview products GET \"/products/\" {{\r\n\
         \x20   postgres {{\r\n        query '{KEPT}'\r\n    }}\r\n}}\r\n"
    );
    let edited = apply_edits(
        &source,
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: vec![grant("products", &[ADDED])],
        },
    )
    .expect("the edit applies");

    assert!(
        !edited.replace("\r\n", "").contains('\n'),
        "the edit mixed line endings into a CRLF document:\n{edited:?}"
    );
    let manifest = Manifest::from_blastfile_str(&edited).expect("the edit parses");
    assert_eq!(view_names(&manifest), ["products", "orders"]);
    assert_eq!(
        queries_in(&manifest, "products", "db"),
        [KEPT.to_string(), ADDED.to_string()]
    );
}

//
// Refusals that never reach the document.
//

#[test]
fn duplicate_proposed_view_names_are_refused() {
    let error = apply_edits(
        &format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n"),
        &BlastfileEdits {
            views: vec![
                new_view("orders", "/orders/"),
                new_view("orders", "/sales/"),
            ],
            grants: Vec::new(),
        },
    )
    .expect_err("the two proposals collide");

    assert!(
        matches!(error, EditError::DuplicateProposedRoute { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("orders"), "{error}");
}

#[test]
fn an_empty_pattern_is_refused_by_its_rule_index() {
    let error = apply_edits(
        &format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n"),
        &BlastfileEdits {
            views: vec![ProposedView {
                name: "orders".to_string(),
                methods: Vec::new(),
                patterns: vec!["/orders/".to_string(), String::new()],
            }],
            grants: Vec::new(),
        },
    )
    .expect_err("an empty pattern matches nothing");

    assert!(
        matches!(error, EditError::EmptyPattern { rule: 1, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("patterns[1]"), "{error}");
}

#[test]
fn a_method_that_is_not_a_method_is_refused_before_the_document() {
    let error = apply_edits(
        &format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n"),
        &BlastfileEdits {
            views: vec![ProposedView {
                name: "orders".to_string(),
                methods: vec!["get".to_string()],
                patterns: vec!["/orders/".to_string()],
            }],
            grants: Vec::new(),
        },
    )
    .expect_err("`get` is not a method");

    assert!(
        matches!(error, EditError::InvalidMethod { .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("upper-case"), "{error}");
}

#[test]
fn an_empty_statement_is_refused_without_quoting_it() {
    let source =
        format!("{HEAD}\nview products GET \"/products/\" {{\n    postgres {{\n    }}\n}}\n");
    let error = apply_edits(
        &source,
        &BlastfileEdits {
            views: vec![],
            grants: vec![grant("products", &["   "])],
        },
    )
    .expect_err("a blank statement is not grantable");

    assert!(
        matches!(error, EditError::EmptyQuery { index: 0, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("queries[0]"), "{error}");
}

#[test]
fn a_path_without_a_file_name_is_refused_before_anything_is_written() {
    let error = write_atomic(Path::new("/"), "app \"a:b\"\n").expect_err("a root has no name");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("file name"), "{error}");
}

#[test]
fn two_proposals_for_one_new_view_share_a_single_block() {
    let source = format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n");
    let (edited, manifest) = edited_and_parsed(
        &source,
        &BlastfileEdits {
            views: vec![new_view("carts", "/carts/")],
            // The same statement twice, then a different one: one block, two
            // entries, in the order they were proposed.
            grants: vec![grant("carts", &[ADDED]), grant("carts", &[ADDED, KEPT])],
        },
    );

    assert_eq!(
        edited.matches("postgres {").count(),
        1,
        "the grant was written as more than one block:\n{edited}"
    );
    assert_eq!(
        edited.matches(ADDED).count(),
        1,
        "the statement was repeated:\n{edited}"
    );
    assert_eq!(
        queries_in(&manifest, "carts", "db"),
        [ADDED.to_string(), KEPT.to_string()]
    );
}

/// A proposal with no method list writes the header the reference documents:
/// `view <name> "<pattern>" { … }`, which answers every method (§8.4).
#[test]
fn a_view_with_no_methods_is_written_without_a_method_list() {
    let (edited, manifest) = edited_and_parsed(
        &format!("{HEAD}\nview about GET \"/about/\" {{\n}}\n"),
        &BlastfileEdits {
            views: vec![new_view("orders", "/orders/")],
            grants: Vec::new(),
        },
    );

    assert!(
        edited.contains("view orders \"/orders/\" {\n}\n"),
        "the method-less header is not the documented shape:\n{edited}"
    );
    assert_eq!(view_names(&manifest), ["about", "orders"]);
    let orders = manifest
        .routes()
        .iter()
        .find(|route| route.name() == "orders")
        .expect("the new view");
    assert_eq!(
        orders.methods(),
        None,
        "a view with no methods answers every method"
    );
}

//
// The atomic write's two path branches: a bare file name, and a directory the
// process cannot write. Both are reachable through `BLASTIT_MANIFEST` (the
// value the operator types), so both are pinned here rather than left to the
// shape of `Path::parent`.
//

/// Set in the child this module re-executes for the bare-name case.
const BARE_NAME_CHILD: &str = "BLASTIT_MANIFEST_EDIT_BARE_NAME_CHILD";

/// A unique directory under `$TMPDIR`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "blastit-edit-branches-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The entries of a directory, by name.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the directory reads")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// What the bare-name write puts in the file.
fn bare_name_content() -> String {
    format!("{HEAD}# written through a bare file name\n")
}

/// `write_atomic("Blastfile", …)`: the shape `BLASTIT_MANIFEST=Blastfile`
/// produces. `Path::new("Blastfile").parent()` is `Some("")`, not `None`, so
/// the parent has to be replaced with `.`. Taken verbatim, the temp file is
/// written and renamed into place and then the directory `fsync` opens `""`,
/// which fails `ENOENT` — a completed write reported as a failed one.
///
/// The branch needs a process-wide working directory, so the case runs in a
/// child re-execution of this test binary whose `cwd` is its own temporary
/// directory: no other test in this binary can observe a changed directory, and
/// this test does not depend on `--test-threads=1`. The parent asserts on the
/// exit status, on the bytes the child wrote and on the directory it left
/// behind, so a child that never ran its body fails the test instead of passing
/// it.
#[test]
fn a_bare_file_name_is_written_into_the_process_directory() {
    if std::env::var_os(BARE_NAME_CHILD).is_some() {
        write_atomic(Path::new("Blastfile"), &bare_name_content())
            .expect("a bare file name is a path in the process's own directory");
        return;
    }

    let dir = TempDir::new("bare-name");
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "a_bare_file_name_is_written_into_the_process_directory",
            "--nocapture",
        ])
        .env(BARE_NAME_CHILD, "1")
        .current_dir(dir.path())
        .output()
        .expect("the child test binary runs");

    assert!(
        output.status.success(),
        "the write through a bare file name failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Blastfile"))
            .expect("the manifest the child wrote"),
        bare_name_content(),
        "the child did not write the manifest into its own directory"
    );
    assert_eq!(
        entries(dir.path()),
        ["Blastfile"],
        "the write left a temporary file behind"
    );
}

/// `write_atomic` into a directory the process cannot write. The temp file is
/// created with `O_CREAT | O_EXCL`, so the failure is `EACCES` from `open` — an
/// error that says nothing about a stale temp file. Retrying past it unlinks a
/// name that was never created and reports `ENOENT` instead: a permission
/// problem described as a missing file, and the temporary-file branch taken for
/// the wrong reason.
///
/// Cargo-mutants runs unprivileged, so the mode is enforced there. Under uid 0
/// the kernel bypasses the permission check, the write simply succeeds, and
/// this test says so and asserts nothing: `smoke (root)` cannot observe this
/// branch, and nothing here relies on it doing so.
#[cfg(unix)]
#[test]
fn a_write_into_an_unwritable_directory_reports_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    /// Puts the directory's mode back so [`TempDir`] can remove it.
    struct RestoreMode(PathBuf);

    impl Drop for RestoreMode {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    let dir = TempDir::new("unwritable");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
        .expect("chmod the directory read-only");
    let _restore = RestoreMode(dir.path().to_path_buf());

    // The check above is only meaningful when the kernel applies it: probe with
    // the same `O_CREAT | O_EXCL` a temp file uses, and stop when even that
    // succeeds.
    let probe = dir.path().join("probe");
    let privileged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if privileged {
        let _ = std::fs::remove_file(&probe);
        eprintln!("skipped: this process writes into a read-only directory (uid 0)");
        return;
    }

    let path = dir.path().join("Blastfile");
    let error = write_atomic(&path, "app \"a:b\"\n").expect_err("the directory is not writable");

    assert_eq!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied,
        "a refused create was reported as {error:?} instead of the permission problem"
    );
    assert!(!path.exists(), "the manifest was written anyway");
    assert!(
        entries(dir.path()).is_empty(),
        "a temporary file was left behind: {:?}",
        entries(dir.path())
    );
}
