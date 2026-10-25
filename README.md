# blastit

A WSGI server for Python that sandboxes views: each route of the app runs in its
own process under its own uid, and only gets the capabilities (for now: SQL
statements, through a PostgreSQL proxy) that its manifest grants. Built to run
as the only process in a gVisor container, such as DigitalOcean App Platform.

* [`ARCHITECTURE.md`](ARCHITECTURE.md): the design.
* [`ROADMAP.md`](ROADMAP.md): how it gets built, step by step.
* [`docs/spikes/`](docs/spikes/): what was measured on the target platform.
* [`docs/archive/`](docs/archive/): the original feasibility experiments.

Status: under construction. Nothing here is usable yet.

## Development

```console
$ cargo fmt --all -- --check
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo test --workspace
```

`develop` is the only long-lived branch. Each roadmap step lands as one pull
request from a `feature/<step>-<slug>` branch; CI must be green before merging.
