# The rootless end-to-end test (D6)

The same scenario as `e2e/run.py` — the demo Django app, PostgreSQL, the SQL
denials and the log checks — with one difference: **blastit is not root**. It
runs as an unprivileged uid holding only the capabilities the boot needs, and
the check is that everything else is unchanged: the uid layout, the secret
barrier, the seccomp filters, and a view process that holds no capability at
all.

```console
$ uv run e2e/rootless/run.py --local       # Docker Compose, on this machine
$ uv run e2e/rootless/run.py --droplet     # a DigitalOcean Droplet in ams3
```

Both modes build `e2e/rootless/Dockerfile` (the wheel install, plus `setcap`),
run `e2e/rootless/compose.yml`, drive `e2e/scenario.py` over HTTP and check
`e2e/check_logs.py` plus D6's own assertions on the event stream:

* the `boot` event of stage 0 **and** of the preload phase say
  `"privilege": "capabilities"` and list the capabilities init holds;
* every `process.spawned` has a uid of its own, above `BLASTIT_UID_BASE`;
* a view's `/debug/caps/` probe — `/proc/self/status`, read in the view —
  reports `CapInh`/`CapPrm`/`CapEff`/`CapAmb` all zero and `NoNewPrivs: 1`.

## The capabilities, and why each one is there

`crates/blastit-sys/src/caps.rs` is the source of truth; the image's `setcap`
line, the compose `cap_add` list and the driver's `CAPABILITIES` list are the
same set:

| Capability | Needed for |
|---|---|
| `cap_setuid` | `setresuid` to a view's uid |
| `cap_setgid` | `setgroups(0, NULL)` and `setresgid` |
| `cap_chown` | the view directories and the pg sockets, chowned to the view uid (§2.2) |
| `cap_fowner` | the *mode* of those files, applied after the `chown` that made them the view's |
| `cap_dac_override` | opening a view's directory to bind its socket — the directory is already the view's, `0700` |
| `cap_kill` | `kill(2)` on a view process, whose uid is never init's |
| `cap_setpcap` | dropping every child's **bounding** set, so nothing can hand a capability back |

`cap_sys_resource` is **not** needed: blastit only ever *lowers* a `setrlimit`.

## How the capabilities are granted

Two mechanisms exist; this test uses both, one per context:

* **File capabilities on the binary** (`setcap`) — what the image does. In a
  wheel install the "binary" is the **interpreter**: `blastit` is a console
  script, its shebang names the image's Python, and the file capability is
  granted at *that* `execve`.
* **Ambient capabilities from a launcher** — `setpriv --reuid … --inh-caps +…
  --ambient-caps +…` from an entrypoint that starts as root. This is the way to
  do it on a platform that sets `no_new_privs` on the container (App Platform,
  §10), where *file* capabilities are ignored.

What does **not** work is the obvious spelling the roadmap mentions:
`docker run --user 1000 --cap-add SETUID …`. Verified on this machine
(Docker 29.5, compose v5) and again from the droplet: a non-root `user:`
process ends up with `CapPrm`, `CapEff` and `CapAmb` **all zero** — Docker's
`--cap-add` only limits the container's *bounding* set. The compose file here
therefore keeps `user:` (the process really is non-root), uses `cap_drop: [ALL]`
and `cap_add:` for exactly the bounding set a file capability may fill, and
relies on `setcap` for permitted/effective.

Two consequences worth knowing when deploying this:

* a child that has cleared its **bounding** set cannot `execve` a file with file
  capabilities at all: the kernel refuses with `EPERM` ("insufficient to execute
  correctly"), which is why the wheel's separate `blastit-dbproxy` binary — no
  file capabilities — is what the capability-holding wheel install `execve`s for
  that role. It also means a view cannot use the interpreter's capabilities
  even if it manages to `execve` it: the attempt fails.
* `/run/blastit` must be writable by the uid init runs as. `/run` belongs to
  root, so the compose file mounts a `tmpfs` there owned by that uid:
  `tmpfs: ["/run/blastit:mode=0700,uid=10000,gid=10000"]`.

A trailing note on the uid: `init`'s uid must not be one the layout hands out
(`BLASTIT_UID_BASE + 1…3`, and one per route from `BLASTIT_UID_BASE + 1000`).
The boot refuses a collision by name.

## On a Droplet

`run.py --droplet` creates a droplet (tag `blastit-d6`, region `ams3`, default
size `s-2vcpu-4gb`) with a **freshly generated SSH key** imported under a
`blastit-d6-…` name, installs Docker through cloud-init, ships `git archive
HEAD`, builds and starts the compose file there, tunnels the app's loopback port
back over ssh (nothing is exposed to the internet), runs the scenario and the
log checks here, and then — whatever happened — deletes the droplet and the key
and verifies they are gone.

```console
$ uv run e2e/rootless/run.py --droplet --name blastit-d6-manual
[ rootless … ] verified: neither the droplet nor the key named blastit-d6-manual exists
```

`--keep` leaves both in place for debugging; `--cleanup --name …` deletes them
afterwards. The janitor deletes what an interrupted run left behind:

```console
$ uv run e2e/rootless/run.py --janitor --older-than-minutes 120
```

Droplets tagged `blastit-d6` older than the cutoff go. An SSH key named
`blastit-d6-…` goes too, unless a *surviving* droplet named after it is still
there — `doctl` does not report when a key was created, so the rule is the one
that cannot delete a key still in use.

## In CI

`e2e/rootless/Dockerfile.prebuilt` installs a wheel CI has already built, so a
job needs no Rust toolchain. The proposed job (for the orchestrator, since
`.github/` is out of scope for this step) is the D6 analogue of `e2e`:

```yaml
  rootless:
    name: e2e rootless (capabilities, Docker)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with: { python-version: "3.13" }
      - run: echo "PYO3_PYTHON=${{ steps.python.outputs.python-path }}" >> "$GITHUB_ENV"
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: pipx install maturin && scripts/build-wheel.sh
      - name: The rootless scenario
        run: python e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
```

The droplet run is not a CI job: it needs `DIGITALOCEAN_ACCESS_TOKEN` and about
twenty minutes, so it stays a step run by hand (§10, the D6 roadmap row), with
`--janitor` available for the same `live.yml` schedule as the App Platform test.
