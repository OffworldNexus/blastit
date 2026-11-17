# `frameworks-restructure` — review (`ai-ergonomics`), resolution check

Provenance: goldfish `ai-ergonomics` critic, fresh context. This file records the **resolution
check** of the nine third-round findings against `docs/design/frameworks-restructure.md` as it
stands (md5 `2dd9ca1432a2e951435b16d5eb5d4ae2`, 490 lines, `release/0.2.0`). No new findings and no
MINOR round; the earlier detailed findings are superseded by the verdicts below. M1 was re-read
after its fix landed (the earlier snapshot quoted a cell that has since been rewritten).

## Resolution check

| # | Finding | Verdict | Evidence |
|---|---|---|---|
| B1 | `## The admin` pointed at a `table_grants` key `check --json` never prints | **RESOLVED** | `outline:107` now enumerates the admin's grant lines by hand (`auth_user`, `auth_group`, `auth_permission`, `django_content_type`, `django_admin_log`, `django_session`), and `outline:116-119` states the key "is a JSON key of the pytest plugin's own report … so `check --json` never carries it". The section no longer depends on a report. |
| B2 | The SQLAlchemy probe grant was uppercase while the wire is lowercase | **RESOLVED** | `outline:133`: "the lowercase `select pg_catalog.version()` probe and the `transactions` that `engine.begin()` needs belong in the one `connection` trait every view `use`s". |
| B3 | Flask lost `transactions`, so `engine.begin()` is denied with no actionable finding | **RESOLVED** | `outline:130`: "a `connection` trait carrying `transactions` and the setup statements"; `outline:133` says why `engine.begin()` needs it. |
| M1 | Django's `SECURE_PROXY_SSL_HEADER` advice trusted the header in the untrusted case | **RESOLVED** | `outline:108`: "Do not set `SECURE_PROXY_SSL_HEADER`: Blastit sets `wsgi.url_scheme` from a trusted peer and the raw `X-Forwarded-Proto` still reaches the app, so trusting it again is the bypass"; `outline:134`: "Nothing to configure: Blastit sets `REMOTE_ADDR` and `wsgi.url_scheme` from a trusted peer, so the app must not re-read `X-Forwarded-*` (`ProxyFix` would let a client choose both)". |
| M2 | Flask's `ProxyFix` re-read the untrusted `X-Forwarded-*` headers | **RESOLVED** | `outline:134`: "Nothing to configure: … the app must not re-read `X-Forwarded-*` (`ProxyFix` would let a client choose both)". |
| M3 | PHP's learn pass had no driver, and no page said the grants stay unproven | **RESOLVED** | `outline:155` drives the sweep ("start `blastit dev --learn`, send every route … stop it with `Ctrl-C`, read `git diff`, and run `blastit fmt`"); `outline:156` says "nothing proves a PHP grant is complete in 0.2.0". |
| M4 | A view with no `postgres {}` block has no socket, so learn sees nothing | **RESOLVED** | `outline:88`: "a view with no `postgres` block has no socket so give every database view an empty `postgres {}` before the run"; `outline:130` labels Flask's catch-all a demo, "not the shape for the app's other routes". |
| M5 | The static refusal list's new home contradicted the public `/.well-known` path | **RESOLVED** | `outline:247-248` makes the Home require "the `.well-known` exception to the dotfile `404`" in `#static-mounts` **before** the page's list dies. |
| M6 | The new `fmt --check` fence is a command kind `e2e/php2shell.py` has never classified | **RESOLVED** | `outline:424-428` adds `fmt --check` to `SKIPS`/`command_kind` as an explained command, counts the new kind, and rewrites the docstring's "three `$ ` commands" and "runs pytest" sentences. |

## Verdict

All nine third-round findings are resolved on the page contracts. The admin no longer names a report
key that does not exist; the SQLAlchemy recipe is lowercase and carries `transactions` in one trait;
the PHP learn loop is scriptable and honest about unproven grants; the listener rule is back on the
connector; `.well-known` has a named home; the `fmt --check` fence is classified for the e2e; and
Django's `## Behind TLS` now says "Do not set `SECURE_PROXY_SSL_HEADER`", with Flask's
`## Proxy headers` saying the app must not re-read `X-Forwarded-*` or add `ProxyFix`. On the shared
question — "Can I get from 'I use Django/Laravel' to the sentence that answers me in two clicks, and
does any sentence appear twice?" — routing is a yes (chooser to the framework page in one click, to
the connector in two), and the split-store and PDO answers are single-homed (outline:154, 372-373),
so the pages no longer restate one another's answers.

VERDICT: PASS
