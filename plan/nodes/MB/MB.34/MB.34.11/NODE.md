---
id: MB.34.11
title: "Pin the reference page's 9:16 / 11:26 / 13:31 literals and a non-zero base (AUDIT-MB.34 G3)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/tests/validate.rs", docs/reference/blastfile.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Pin the reference page's own three `line:col` literals, and prove the `base` argument shifts no
line or column.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
```

One fixture places the page's entries on lines 9, 11 and 13 at 8-space indent and asserts
`Position::at(9,16)` (`query ''`), `at(11,26)` (`SELECT 1; SELECT 2`) and `at(13,31)`; the same
document parsed with `Manifest::parse_blastfile(source, 20_000)` yields the same three positions;
`docs/reference/blastfile.md` still prints `9:16`/`11:26`/`13:31`.

# Notes

- `Position::at(~, 26)` currently appears only on line 5 of the fixture, so a page edit can drift
  with every test green.
