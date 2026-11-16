//! §18's rendering contract: one line, escaped, cut, and position-less where
//! there is no position (`R2`, `R26`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use blastit_manifest::blastfile;
use blastit_manifest::{Manifest, ManifestError, Position};

/// A unique directory under `$TMPDIR`, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "blastit-errors-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The one refusal `source` produces, rendered as the user sees it.
fn refusal(source: &str) -> String {
    let errors = Manifest::from_blastfile_str(source).expect_err("the document is refused");
    errors
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_else(|| "no error".to_owned())
}

/// Every refusal is `<line>:<col>: <message>`, on one line (§18).
#[test]
fn every_refusal_is_positioned_and_one_line() {
    for source in [
        "View products \"/x\" { }\n",
        "view v \"/x/{id:int\" {\n}\n",
        "view v \"/x\" {\n  use ghost\n}\n",
        "entrypoint php \"../index.php\"\n",
        "static \"/x\" {\n}\n",
        "postgres $A\npostgres db $B\n",
    ] {
        let rendered = refusal(source);
        assert!(!rendered.contains('\n'), "{source:?}: {rendered:?}");
        let (position, message) = rendered.split_once(": ").expect("a position and a message");
        let mut parts = position.split(':');
        for part in [parts.next(), parts.next()] {
            let part = part.expect("two numbers");
            assert!(
                part.parse::<u32>().is_ok_and(|value| value > 0),
                "{source:?}: {rendered:?}"
            );
        }
        assert!(parts.next().is_none(), "{source:?}: {rendered:?}");
        assert!(!message.is_empty(), "{source:?}: {rendered:?}");
    }
}

/// A value longer than 120 `char`s is cut with `…`, and a control byte is
/// escaped (§18, R26).
#[test]
fn a_value_is_escaped_and_cut() {
    let name = "a".repeat(121);
    let rendered = refusal(&format!(
        "view {name} \"/x\" {{\n}}\nview {name} \"/y\" {{\n}}\n"
    ));
    assert!(
        rendered.contains(&format!("\"{}\u{2026}\"", "a".repeat(120))),
        "{rendered}"
    );

    let rendered = refusal("static \"/x\" {\n  root \"a\u{1}b\"\n}\n");
    assert!(rendered.contains("\\u{1}"), "{rendered:?}");
    assert!(!rendered.contains('\u{1}'), "{rendered:?}");
}

/// §8.3's `regex` row is escaped **and cut**: the crate's own message goes
/// through the same 120-`char` rule as a value (§8.3, §18).
#[test]
fn the_regex_message_is_escaped_and_cut() {
    let error = ManifestError::PatternRegex {
        at: Position::at(1, 14),
        pattern: "/a/".to_owned(),
        message: "x".repeat(200),
    };
    let rendered = error.to_string();
    assert_eq!(
        rendered,
        format!("1:14: regex `/a/` does not compile: {}…", "x".repeat(120))
    );
    // A control byte in the crate's message is escaped, not printed.
    let error = ManifestError::PatternRegex {
        at: Position::at(1, 14),
        pattern: "/a/".to_owned(),
        message: "a\nb".to_owned(),
    };
    assert!(!error.to_string().contains('\n'), "{error}");
}

/// A position-less refusal renders bare, so the caller prefixes the path
/// exactly once (§18).
#[test]
fn a_positionless_refusal_renders_bare() {
    let directory = TempDir::new("too-large");
    let path = directory.path().join("Blastfile");
    std::fs::write(&path, vec![b'#'; 16_777_217]).expect("a file over the cap");
    let error = blastfile::read(&path).expect_err("refused");
    let rendered = error
        .errors()
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(
        rendered.starts_with("the file is 16777217 bytes;"),
        "{rendered}"
    );
    assert!(!rendered.contains("0:0"), "{rendered}");
    assert!(!rendered.contains('\n'), "{rendered}");
}

/// The list is one line per problem, so a boot's output is N lines (§18).
#[test]
fn the_list_is_one_line_per_problem() {
    let source = "view a \"/a\" {\n  postgres x { select t }\n}\n\
                  view b \"/b\" {\n  postgres y { select t }\n}\n\
                  view c \"/c\" {\n  postgres z { select t }\n}\n";
    let errors = Manifest::parse_blastfile(source, 20_000).expect_err("refused");
    assert_eq!(errors.len(), 3);
    let rendered = errors.to_string();
    assert_eq!(rendered.lines().count(), errors.len() + 1, "{rendered}");
    for error in errors.errors() {
        assert!(!error.to_string().contains('\n'), "{error}");
    }
}

/// A refusal never carries a value that could be a credential (§18, §17.1).
///
/// `InvalidApp` still quotes its value: `mutation_gaps.rs` pins that rendering
/// and it is outside this leaf's paths, so the re-spelling §18 asks for is a
/// follow-up (`MB.21.3.0`/`MB.21.4.3`).
#[test]
fn a_refusal_never_carries_a_secret() {
    let secrets = ["hunter2", "s3cr3t", "postgres://"];
    for source in [
        "postgres \"postgres://user:hunter2@db/shop\"\n",
        "postgres {\n  password \"s3cr3t\"\n}\n",
        "entrypoint php \"../postgres://user:hunter2@db/index.php\"\n",
        "view v \"/x\" {\n  postgres \"postgres://user:hunter2@db\"\n}\n",
    ] {
        let errors = Manifest::parse_blastfile(source, 20_000).expect_err("refused");
        let rendered = errors.to_string();
        for secret in secrets {
            assert!(!rendered.contains(secret), "{source:?}: {rendered}");
        }
    }
}
