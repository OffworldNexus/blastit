//! The two security properties of the lowering (`R20`, `R21`, `R24`).
//!
//! * A `$IDENT` is a **name**, never a value: lowering never reads the
//!   environment, so a document cannot smuggle a secret into a manifest by
//!   naming the variable that holds it (§17.2).
//! * A refusal never yields a manifest: there is no partial model a caller could
//!   boot from, and no value that could be a credential is ever quoted (§18).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use blastit_manifest::{Manifest, ManifestError, ProxyName};

/// A `$IDENT` becomes the string of its own name, whatever the environment
/// holds (§7.4, §17.2).
#[test]
fn parse_never_reads_env() {
    // `HOME` is set on every host this runs on, so a lowering that resolved the
    // name would put the home directory in the input map.
    let home = std::env::var("HOME").expect("HOME is set");
    assert!(!home.is_empty());

    let source = "postgres $HOME\n\nview v \"/v\" {\n  postgres { select products }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    let store = manifest.proxy("db").expect("the default store");
    assert_eq!(
        store.input().get("url").map(String::as_str),
        Some("HOME"),
        "the value is the variable's name"
    );
    assert!(
        !store.input().values().any(|value| value.contains(&home)),
        "the environment's value never reaches the model"
    );

    // The same for a slot of the block form, and for a name nothing declares.
    let source = "postgres {\n  host $BLASTIT_TEST_UNSET_VARIABLE\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    assert_eq!(
        manifest
            .proxy("db")
            .and_then(|store| store.input().get("host"))
            .map(String::as_str),
        Some("BLASTIT_TEST_UNSET_VARIABLE")
    );
}

/// No `$IDENT` value is ever printed, and a quoted store is refused without
/// echoing it (§5.3, §18).
#[test]
fn a_value_is_never_echoed() {
    for source in [
        "postgres \"postgres://user:hunter2@db.internal/shop\"\n",
        "postgres {\n  host \"postgres://user:hunter2@db.internal/shop\"\n}\n",
    ] {
        let errors = Manifest::from_blastfile_str(source).expect_err("refused");
        let rendered = errors.to_string();
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(!rendered.contains("db.internal"), "{rendered}");
        assert!(
            rendered.contains("a quoted string is not a variable name"),
            "{rendered}"
        );
    }
}

/// A refusal never yields a manifest, at either entry point (`R21`).
#[test]
fn an_error_never_yields_a_manifest() {
    let documents = [
        "view v \"/v\" {\n  body\n}\n",
        "view v \"/v/{id:int\" {\n}\n",
        "view v \"/v\" {\n  use ghost\n}\n",
        "entrypoint php \"../index.php\"\n",
        "static \"/x\" {\n}\n",
        "postgres $A\npostgres db $B\n",
    ];
    for source in documents {
        assert!(
            Manifest::from_blastfile_str(source).is_err(),
            "lowering accepted {source:?}"
        );
        assert!(
            Manifest::parse_blastfile(source, 20_000).is_err(),
            "validation accepted {source:?}"
        );
    }
    // A grant on an undeclared store lowers (step 3 records it) and is refused
    // by `validate`, which is the whole difference between the two entry points.
    let undeclared = "view v \"/v\" {\n  postgres analytics { select products }\n}\n";
    assert!(Manifest::from_blastfile_str(undeclared).is_ok());
    assert!(Manifest::parse_blastfile(undeclared, 20_000).is_err());
}

/// A grant on an undeclared store is reported against the view, and the model
/// that reaches `validate` still carries no grant for it (§7.3, §13.1).
#[test]
fn an_undeclared_store_grants_nothing() {
    let source = "view v \"/v\" {\n  postgres analytics { select products }\n}\n";
    let manifest = Manifest::from_blastfile_str(source).expect("lowers");
    let route = manifest.route(0).expect("one route");
    assert!(
        route.grant(&ProxyName::from("analytics")).is_none(),
        "an undeclared store contributes no grant"
    );
    assert!(manifest.effective_grants(0).is_empty());
    // The refusal is `GrantOnUnknownProxy`, and it names the view.
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("refused");
    let first = errors.errors().first().expect("one error");
    assert!(
        matches!(first, ManifestError::GrantOnUnknownProxy { .. }),
        "{first:?}"
    );
    assert!(first.to_string().contains("view \"v\""), "{first}");
}
