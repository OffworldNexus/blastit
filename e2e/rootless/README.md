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

Both modes build `e2e/rootless/Dockerfile` (the wheel install, plus `setcap` on
a dedicated copy of the interpreter), run `e2e/rootless/compose.yml`, drive
`e2e/scenario.py` over HTTP and check `e2e/check_logs.py` plus D6's own
assertions:

* on the event stream: the `boot` event of stage 0 **and** of the preload phase
  say `"privilege": "capabilities"`, list the capabilities init holds, and say
  `"bounding_set": "dropped"`; every `process.spawned` has a uid of its own,
  above `BLASTIT_UID_BASE`; a view's `/debug/caps/` probe — `/proc/self/status`,
  read in the view — reports `CapInh`/`CapPrm`/`CapEff`/`CapBnd`/`CapAmb` all
  zero and `NoNewPrivs: 1`;
* in `/proc` of the container: **every** process running under a layout uid —
  http (20001), the scheduler (20002), dbproxy (20003) and every view — holds
  nothing, bounding set included, and has `no_new_privs` set;
* on the capable file itself: it is mode `0500`, owned by the uid init runs as,
  that uid can run it, and **every other uid is refused by the kernel**
  (`EACCES`) — the worker and the migrations run as a uid of their own in the
  compose file, and that is the uid the check uses;
* as a worker's uid: a plain `python3`, and a `manage.py`, hold nothing and
  cannot `setuid(0)`: the *shared* interpreter grants nothing, and the uid that
  can run the copy is a dedicated one (B1).

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
`cap_setpcap` is *wanted* rather than required (M1): without it the boot runs, a
child still holds nothing (its permitted, effective, inheritable and ambient sets
are cleared, and it sets `no_new_privs`, which is what stops an `execve` from
granting what the bounding set allows), and the `boot` event says
`"bounding_set": "kept"` with a warning. A container that cannot give
`cap_setpcap` — an old `--cap-drop`, a runtime without `PR_CAPBSET_DROP` — keeps
working. Missing one of the six *required* ones is still refused at boot, with
the names and the recipe.

## How the capabilities are granted

Two mechanisms exist; this test uses the first, and the second is the spelling
for a platform that sets `no_new_privs`:

* **File capabilities on a dedicated copy of the file init starts from**
  (`setcap`). In a wheel install that file is the **interpreter**: `blastit` is a
  console script whose shebang names the image's Python. The *shared* interpreter
  never carries them — see below.
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

### The capable file is init's alone (B1, B2)

A file with file capabilities turns **any** process of its owner's uid that can
run it into init: `CAP_SETUID`, `CAP_SETPCAP`, `setuid(0)`. So the capability
never goes on a file anybody else runs:

```
/usr/local/bin/python3.13        root:root  0755   the interpreter a project's `python` is
/usr/local/bin/python3.13-init   10000:10000 0500  the copy init is started from, with
                                                  cap_chown,cap_dac_override,cap_fowner,
                                                  cap_kill,cap_setgid,cap_setuid,cap_setpcap
```

* the copy sits **next to** the original, because CPython finds its stdlib
  (`sys.prefix`) and the venv's `site-packages` from the executable's own path;
* `0500`, owned by the uid init runs as — and **that uid is dedicated**: it is
  the uid that can run the copy, so a worker, a migration or an operator's shell
  must use another one. The compose file gives them `E2E_WORKER_UID` (10001 by
  default), and the e2e checks that uid is refused the copy (`EACCES`) and that
  it holds nothing when it runs the *shared* interpreter;
* the boot refuses, with the recipe, a deployment where **any name** of the
  shared interpreter carries the capabilities (`python`, `python3`, `python3.13`
  and `sys._base_executable` are all checked, and compared with init's copy by
  device and inode rather than by name), and refuses a wheel that cannot name the
  capability-free interpreter at all;
* a child cannot use the copy even if it could run it: its bounding set is empty
  and it sets `no_new_privs`, and the kernel refuses such an `execve` outright
  (`EPERM`, "insufficient to execute correctly");
* `BLASTIT_EXEC`'s target must be owned by root or by init's uid and writable by
  neither group nor others, in a directory that is not either: a uid that could
  replace that binary would decide what dbproxy — the process holding the real
  credentials — runs next. The boot refuses anything else, `/tmp` included.

**Standalone install.** The same principle one level up: `setcap` on the binary
itself breaks `--role=dbproxy`, which re-executes `/proc/self/exe` from a child
whose bounding set is empty. A rootless standalone deployment therefore keeps two
copies and names the capability-free one:

```console
$ cp /usr/local/bin/blastit /usr/local/bin/blastit-init
$ chown 10000:10000 /usr/local/bin/blastit-init && chmod 0500 /usr/local/bin/blastit-init
$ sudo setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
      /usr/local/bin/blastit-init
$ BLASTIT_EXEC=/usr/local/bin/blastit /usr/local/bin/blastit-init
```

Two more consequences worth knowing when deploying this:

* a child that has cleared its **bounding** set cannot `execve` a file with file
  capabilities at all, which is why the wheel's separate `blastit-dbproxy`
  binary — no file capabilities — is what the capability-holding wheel install
  `execve`s for that role;
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

`--keep` leaves both in place for debugging (it prints the path of the private
key, which is otherwise removed with the droplet); `--cleanup --name …` deletes
them afterwards. The janitor deletes what an interrupted run left behind:

```console
$ uv run e2e/rootless/run.py --janitor --older-than-minutes 120
[rootless …] stale droplet blastit-d6-stale (1, created 2020-01-01T00:00:00Z)
[rootless …] the SSH key of the droplet that went: blastit-d6-stale (11)
```

Two conservative rules, both about what the API does not say:

* a droplet goes when its age is **known** to be past the cutoff. A `created_at`
  that cannot be read is a lower bound of zero, not a licence to delete: there is
  no way to tell a crashed run from one that is starting, so the droplet stays;
* an SSH key goes **with the droplet of the same name**, and only then — `doctl
  compute ssh-key list` reports no creation time, so a key whose droplet is being
  created at this very moment (the driver imports the key first) looks exactly
  like an orphan. Any other key is left for `--cleanup --name`.

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
