# Security review — Blastit 0.1.1 user documentation

Lens: **`security` (zero-trust, fail-closed, CVE bar)**. Subject: the public docs for the Python WSGI +
PostgreSQL 0.1.1 scope, written docs-first. Read-only review; this file is the only artefact.

Sources read: the 14 pages in the brief, `docs/VISION.md`, `plan/nodes/MA/MA.1/NODE.md`,
`.opencode/skills/security-invariants/SKILL.md`, `docs/knowledge/LESSONS.md`, and
`docs/reference/log-events.md` where the reviewed pages refer to it. Nothing here judges code that
does not exist; it judges whether the documented guarantees are honest, fail-closed and consistent.
The two earlier reviews (dx, ai-ergonomics) already own functional defects; this one does not repeat them.

## Direct answers to the brief

| Question | Answer |
|---|---|
| Is `blastit dev` clearly a single-UID simulation? | **Mostly.** Banner (`README.md:41`, `getting-started.md:42-44`), `"privilege": "unprivileged-dev"` (`security.md:101`, `cli.md:30`), loopback default and an `--host` flag exist. But the `--host` line is a convenience note, not a warning (S10), and the dev credential barrier is overclaimed (S3). |
| Does production genuinely fail closed? | **For uids/capabilities, yes** (`deploy/index.md:3-5`, `cli.md:65-67`, `settings.md:50-53`). Not for seccomp, which a single env var can remove with no documented warning (S6). |
| Do credentials stay behind the stage-0 barrier in every documented flow? | **No.** The dev flow tells you to keep the real URL in `.env` inside the project while the app runs as your uid (S3), `--learn` can persist literal-bearing statements into a committed manifest (S1), and the migration jobs run app import-time code with the owner URL (S8). |
| Does `[route.ingress]` overclaim, or leave an unvalidated body looking safe? | **Yes, in effect.** 0.1.1 has no WASM/CDR wall; the only enforcement is an opt-in CI check, and `raw` passes it with one boot warning (S4). |
| Do `*` presets state the deployment obligation loudly enough? | **The two presets do; the Kubernetes recipe contradicts itself and claims a false safety property** (S2). |
| Do copy-paste snippets create capability/uid/secret footguns? | **Yes:** LAN-exposed superuser Postgres (S12), owner DB URL (S7), migration secrets (S8), command-line secrets (S14), an omitted `SETPCAP` (S15). |

## BLOCKER

### S1 — The "no secrets" promise is false: `--learn` and `sql.denied` carry quoted literals into git and CI logs
`docs/reference/manifest.md:117`: "`blastit.toml` holds no secrets and is safe to commit."
`docs/reference/manifest.md:223`: matching is "on normalised text: trimmed, a trailing `;` removed,
whitespace collapsed **outside quotes**." `docs/local.md:102`: "`sql` is the statement as normalised —
exactly what a grant has to contain." `docs/local.md:86-88`: learn mode "adds what was missing to the
manifest"; `docs/ci.md:84-91` tells you to commit the resulting `git diff`.

A statement with an inline value is kept verbatim, quoted text included. An app that inlines
`WHERE email = 'a@b.c'` or `WHERE token = '…'` (the docs already know inlining happens:
`troubleshooting.md:61`) produces:

* a `sql.denied` line whose `sql` field contains the value (`local.md:99`, `log-events.md:28`), shipped
  to DigitalOcean/CI stdout, contradicting `log-events.md:65` "SQL parameter values" are never logged;
* a `--learn` grant containing the value, then committed to `blastit.toml` and to `git` forever — PII,
  password-reset tokens, customer data, whatever the query embedded.

This is a documented path that exfiltrates request data into logs and source control, and it makes the
one claim that lets teams commit the manifest safely false.

**Fix:** (a) redact every quoted literal in `sql.denied`'s `sql` field and in `--json` findings
(`$?`/`<literal>`), and state it in `log-events.md` and `ci.md`; (b) make `--learn`/`blastit-manifest
fold` **refuse to persist** a statement containing a literal and print "this statement inlines a value;
parameterise it (see frameworks/index.md#bind-parameters-server-side)"; (c) keep the "no secrets" claim
only after (a) and (b), or qualify it. Add a critical test that a literal never reaches stdout or the
written manifest.

### S2 — The Kubernetes recipe trusts every peer on a false premise and has no NetworkPolicy
`docs/deploy/anything-else.md:138-141`:

```yaml
# The ingress is the only peer that can reach the pod: nothing
# untrusted is published, as with the presets.
- name: BLASTIT_TRUSTED_PROXIES
  value: "*"
```

This is false. Kubernetes has a flat pod network by default: any pod in any namespace — a compromised
sidecar, an unrelated workload, a debug pod — can reach `podIP:8080` directly, bypassing the ingress,
and with `*` its `X-Forwarded-For`/`X-Forwarded-Proto` are believed. Consequences: forged
`REMOTE_ADDR`, forged `request.is_secure()`, attacker-chosen fair-queue flow keys, poisoned audit
logs, and a request path that skips TLS and every ingress control. The page's own table
(`:11`) says the opposite ("the addresses your ingress connects *from*"), so a reader following the
prose and copying the YAML gets two different answers.

**Fix:** add a `NetworkPolicy` that admits only the ingress-controller namespace/labels to `8080`, or
set the exact ingress pod address (`/32>`), and delete the false comment. Copy the preset warning
boxes (`deploy/index.md:24-28`, `compose-traefik.md:22-25`) verbatim into the Kubernetes section.

### S3 — Dev's "credential barrier is real" is defeated by the `.env` the docs tell you to create
`docs/local.md:15-17`: "dev mode has the process topology, **the credential barrier** and the policy
checks, but it has one uid." `docs/local.md:57`: "Keep the URL in `.env` if you like and run
`uv run --env-file .env blastit dev`." `docs/local.md:60-62`: "The password never enters the app's
environment."

The app does not need the real URL; a compromise does not need the app either. It runs as the
developer's uid with the project directory readable, so a request-time RCE reads `./.env` (or `/proc/<any-same-uid-process>/environ`) and gets the real
`DATABASE_URL`, then connects to PostgreSQL **directly**, bypassing dbproxy and every `[route.db]`
grant. The same one uid can read `~/.ssh`, cloud credentials and the shell's environment. So the
promise at `local.md:4-5` — "a mistake still fails on your machine instead of in production" — is
false for the exact class of mistake Blastit exists to catch: a grant error silently succeeds outside
the proxy, and the developer learns nothing. `security.md:96-103` says dev is not a sandbox, but the
same page and `local.md` still call the barrier "real"; the page never says the barrier is only about
`environ`/`argv` and that same-uid file access nullifies it.

**Fix:** one explicit paragraph in `local.md` and `security.md`: "In dev the barrier only keeps the
secret out of the app's `environ`/`argv`. With one uid the app can read `.env` and `/proc`, so it can
bypass the proxy. Never put a production or shared database URL in a dev `.env`; use a throwaway local
database, and treat any dev run against real data as an unsandboxed run." Consider a boot line that
names the resolved upstream host so a non-loopback database in dev is obvious.

## MAJOR

### S4 — The ingress contract is enforced only by the app's own test run; `raw` passes CI
`docs/reference/manifest.md:67`: "`[route.ingress]` — **required** for a route that takes one."
`docs/security.md:83-86`: "`json` decodes and re-encodes it before the app sees it, `none` rejects a
body, and `raw` — the loud opt-out — is flagged at boot. `blastit check` fails a route that took a
body without a declaration."

Boot cannot know a route "takes" a body; only `blastit check` can, and only for bodies the suite
actually sends. The production default for an undeclared route is never stated. `raw` is a boot warning
only; `blastit check --json` has no finding kind for it, so the machine path can be green with
`raw` on every upload route. `docs/ci.md:54` even emits `"fix": "[route.ingress] body = \"json\""` for
a route named `upload` — a `json` codec on a multipart upload returns `400`, so the only working move
is `raw`, i.e. the security control is opt-out and nothing fails.

0.1.1 has no media/PDF CDR (VISION Milestone C, `0.3.x`), so `raw` forwards polyglots and parser
attacks untouched. The docs never say that.

**Fix:** make the runtime default deny (`413`) for a body on a route without `[route.ingress]`, or
abort the boot in production; make `raw` a CI **finding** unless the manifest carries an explicit
`allow_raw = true` acknowledgement; add `form`/`multipart` so the honest path exists; add one line to
`security.md`: "0.1.1 does not disarm media/PDF; `raw` means the app's parser sees the original bytes."

### S5 — `egress = true` is documented as a one-word toggle, but it is an unrestricted SSRF primitive
`docs/reference/manifest.md:65`: "`egress` | no | `true` lets the process open outbound TCP
connections." `docs/security.md:74-77`: views may use `AF_UNIX` only, "plus `AF_INET`/`AF_INET6` with
`egress = true`".

There is no host, port or address allowlist in 0.1.1 (the HTTP proxy is Milestone D, `VISION.md:65`).
`egress = true` therefore permits `169.254.169.254`, RFC1918 services, the Docker host, and any
attacker-chosen `host:port`. That is the exact SSRF class the vision promises to stop. A developer
reading the one-line table entry will enable it for "my API" and get a much larger capability.

**Fix:** state the truth inline in the manifest table and in `security.md`: "`egress = true` is any
IPv4/IPv6 address and port, including cloud metadata; it is not an allowlist until 0.4." Add a boot
warning (like `unrestricted`) and mention it on the deploy page's threat list.

### S6 — `BLASTIT_SECCOMP=0` removes the only egress/syscall wall, and fail-closed is defined without seccomp
`docs/reference/settings.md:47`: "`BLASTIT_SECCOMP` | `1` | the per-role seccomp filters; the only
lever for egress control." Nothing says what `0` costs, or that production should refuse it.
`docs/security.md:102` and `docs/reference/cli.md:65-67` define fail-closed as "without root or the
capability set", omitting the seccomp requirement that `security-invariants` makes mandatory
("plus `seccomp`"). One plain env var (or a platform that does not support seccomp) disables the
per-role refusal list: `ptrace`, `bpf`, `keyctl`, `io_uring`, mount, namespace moves, and the
`socket()` restriction that is the only egress lever.

**Fix:** include seccomp in the fail-closed sentence; state that `0` is honoured only with a loud
`boot` warning and is refused by both presets (or is dev-only); add a `troubleshooting.md` row.

### S7 — Nothing tells you to give the upstream database a least-privilege role
`docs/deploy/digitalocean-paas.md:81-82` hands the web service `${db.DATABASE_URL}` (DigitalOcean's
default, owner-level user), and `docs/deploy/compose-traefik.md:73-75` creates PostgreSQL with
`POSTGRES_USER: shop` — also the owner. `docs/security.md:33` is honest that dbproxy holds "the real
database credentials" and cannot be prevented from using them. So the per-route grant is the *only*
wall: any dbproxy bug, parser divergence or operator `unrestricted` gives the whole database, not one
route, contradicting the "worth that page, not the app" promise (`README.md:11-14`).

**Fix:** require a non-owner role scoped to the union of grants (no `CREATEROLE`, no `SUPERUSER`, no
`pg_read_file`/`COPY … PROGRAM`), show its creation in the DO and Compose recipes, and add one sentence
to `security.md`: "the proxy's role is the backstop; never connect it as the database owner."

### S8 — Migration jobs run app import-time code with the owner URL, and the docs do not mark that boundary
`docs/deploy/digitalocean-paas.md:62-71`: a `PRE_DEPLOY` job runs `python manage.py migrate` in the app
image with `${db.DATABASE_URL}` and `DJANGO_SECRET_KEY`. `docs/deploy/compose-traefik.md:80-85` does
the same as uid `10001` with `environment: *app-env` (URL, secret key). `docs/security.md:14-16` puts
"code the app runs at import time" explicitly out of scope because it runs before any view exists;
the migration job is precisely that code, with owner credentials and no sandbox. A malicious or
compromised dependency owns the database and the app secret at deploy time. There is no migration
recipe at all for systemd/Kubernetes, so users will run it by hand in the same privileged container.

**Fix:** use a DDL-only migration role (or the owner only for migrations, never for the web service),
pass only the variables `manage.py` needs, and add one sentence: "migrations are a trusted-code
boundary: they run unsandboxed with real credentials; pin and review the image." Cover systemd/K8s
with a separate oneshot/Job.

### S9 — `BLASTIT_MAX_BODY_BYTES=100 MiB` with no per-view memory cap is a one-line OOM DoS
`docs/reference/settings.md:91`: `100 MiB`. `docs/security.md:30,36`: a view has "all the CPU and
memory the container has" and "no CPU or memory limit of its own". Both presets use a 1 GiB container
(`anything-else.md:153`, `digitalocean-paas.md:78`). Sixteen workers buffering a 100 MiB body, or a
few slow uploads on a 1 GiB instance, exhaust memory; the kernel kills init and `PDEATHSIG` takes the
whole service down. Nothing in the docs multiplies body size by worker count.

**Fix:** lower the default (10 MiB), make the cap per route (`[route.ingress] max_body`), reject an
oversized `Content-Length` before buffering, and document `max_body × max_workers` against container
memory in `deploy/anything-else.md`.

### S10 — `blastit dev --host` is presented as a phone/VM convenience, with no consequence and no documented boot warning
`docs/local.md:24` and `docs/reference/cli.md:24`: "bind another address, to reach it from a VM or a
phone; unauthenticated, so loopback stays the default." `docs/troubleshooting.md:43`: "add `--host
0.0.0.0` only for a trusted network."

Dev has no authentication and no CSRF protection; any browser tab can issue a simple cross-origin
`POST`/form request to `http://<laptop>:8080/<route>` (and any LAN peer can call it directly). The app
runs as the developer's uid, so an RCE there gets the home directory, SSH keys, cloud credentials and
the `.env` from S3. "Unauthenticated" understates what is exposed. The banner shown in
`getting-started.md:42-44` is the loopback banner; the docs never say it changes for `--host`.

**Fix:** require an explicit second acknowledgement (e.g. `BLASTIT_DEV_EXPOSE=1`) and print a distinct
boot line: "exposed on <addr>: no auth, app runs as <user>; an RCE gets your files". Add "never point
this at a production database or an `unrestricted` route" to `local.md`.

### S11 — An exact `queries` grant is not analysed, and the denial→paste workflow invites a `pg_read_file` grant
`docs/reference/manifest.md:169-170`: "An exact `queries` entry is trusted as it is: the text is
compared and run, **without looking at what it calls**." `docs/local.md:106-111`: "It belongs there.
Replace the empty list with the normalised statement:" — copy the denial text into `queries`.

A compromised app can provoke a denial with `SELECT pg_read_file('/etc/passwd')`,
`COPY … FROM PROGRAM`, `dblink`, or `pg_sleep`. The developer pastes it because the guide says a
denial means "it belongs there"; the exact grant skips the function analysis that would have rejected
it. `blastit-manifest fold --allow-dangerous` (`cli.md:85`) exists but the docs never say what
"dangerous" means, so it cannot help.

**Fix:** add a warning box at the paste step: "an exact grant is not analysed — check every function
call; `pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file/network capabilities." Make
`--learn`/`fold` refuse or flag those functions by default and document the flag's exact rule.

### S12 — The local PostgreSQL snippet publishes a superuser on `0.0.0.0` with a known password
`docs/local.md:33`:

```console
$ docker run --rm -d -p 5432:5432 -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=shop postgres:18
```

`-p 5432:5432` binds all interfaces (and Docker's iptables rules bypass host firewalls). `POSTGRES_USER`
is unset, so the account is the `postgres` **superuser** with password `secret`; the URL at
`local.md:53` (`shop:secret`) names a role the command never creates, so the flow fails for the wrong
reason — but if anyone fixes the user, they get a LAN-reachable superuser. Any host on the café/office
network can own the database and, via `COPY … FROM PROGRAM`, the laptop.

**Fix:** `-p 127.0.0.1:5432:5432`, a random password, `-e POSTGRES_USER=shop` (and fix the URL), and
one line: "never publish a database port beyond loopback."

## MINOR

### S13 — "No process ever holds a real secret in its `environ`" is false for stage 0
`docs/security.md:62` and `docs/how-it-works.md:104` say "No process ever holds a real secret in its
`environ` or `argv`". Stage 0 reads the secret from its own environment and holds it until its own
`execve`; the next sentence even says "the one that does is `execve`'d". Under the sandboxed kernel
another uid can read `/proc/<pid>/environ` (`security.md:51-54`, `LESSONS.md`), so the absolute wording
is wrong in exactly the window the docs describe. Reword to "no process after the barrier; stage 0's
own environment is replaced by `execve` before any app or broker process exists."

### S14 — Secrets on the command line
`docs/deploy/anything-else.md:63` passes `-e DATABASE_URL='postgresql://shop:secret@…'` in `docker run`;
`docs/deploy/compose-traefik.md:158` prefixes `DB_PASSWORD=… DJANGO_SECRET_KEY=… docker compose up`.
Both land in shell history. The container environment must hold the URL for stage 0, so the delta is
history/process listing, but the fix is free: use `--env-file`/`docker compose --env-file`.

### S15 — The Kubernetes snippet omits `SETPCAP`, so the documented bounding-set drop cannot happen
`docs/deploy/anything-else.md:149-151` drops `ALL` and adds six capabilities; `SETPCAP` is missing,
while `compose-traefik.md:166` calls it "wanted" and both systemd (`:91-92`) and Compose include it.
The boot event will report `"bounding_set": "kept"` (`troubleshooting.md:79`) and the page never says
what that means. Add `SETPCAP` (the root process already has it in Docker's default set; K8s does not
unless added) or explain why kept is acceptable.

### S16 — `sslmode=disable` upstream is shown as the preset default without a note
`docs/deploy/compose-traefik.md:64` uses `?sslmode=disable` between app and `db` on the compose
network. Fine for a single host, but it makes the password and query traffic plaintext on that bridge;
the page should say so and show the `[proxy.db] ca_cert` variant when the database is not on the same
host (as `digitalocean-paas.md:99` does).

### S17 — Completeness claims are stronger than the mechanism
`docs/ci.md:11` "A clean run means no path, grant or route was forgotten." The check only sees what
the suite sent. An undeclared route that production traffic reaches later, or a body route the suite
never POSTs to, is invisible. This is the same overclaim that makes S4 dangerous. Scope it: "every
path, grant and route your suite exercised."

## Verdict

Production is genuinely fail-closed for uids/capabilities and the dev banner is honest, but three
load-bearing security promises — "`blastit.toml` holds no secrets", "dev's credential barrier is
real", and "trusted proxies are safe in Kubernetes" — are false as written; fix S1–S3 (and S4, S6,
S7) before MA.5 treats these pages as the design source of truth.
