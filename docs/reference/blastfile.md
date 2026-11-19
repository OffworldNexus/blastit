# Blastfile reference

`Blastfile` is Blastit's routing table and permission model. It is read from
`./Blastfile`; `BLASTIT_MANIFEST` names another path, and there is no upward
search. No header, no version, no `=`.

## The whole file

`postgres` is the store keyword, and a `Blastfile` declares one application backend: a Python WSGI
callable, a PHP front controller, or static files. `static` mounts may sit beside the backend, or
stand alone. A view's `body` is one of `json`, `form`, `none` or `raw unvalidated`, and that is the
whole set of codecs.

## Layout and tokens

```text
file        → app? entrypoint? section*
section     → app | entrypoint | store | trait | view | static
block       → '{' statement* '}'   # a statement ends at a newline, ';', or this '}'
app         → 'app' STRING
entrypoint  → 'entrypoint' ( 'python' STRING block? | 'php' STRING )
store       → 'postgres' ( ENV | block | NAME ( ENV | block ) )
trait       → 'trait' NAME block
view        → 'view' NAME ( METHOD ( ',' METHOD )* )? ( PATTERN ( ',' PATTERN )* ) block
            | 'view' NAME block    # multi-endpoint: METHOD[, …] PATTERN block? inside
static      → 'static' STRING block
```

| Token | Written | Notes |
|---|---|---|
| Comment | `# …` | to the end of the line, anywhere outside a string |
| Terminator | newline or `;` | both end one statement, and so does the `}` that closes the block; a newline inside `(…)`, a string, or right after a `,` does not |
| String | `"…"` | `\"`, `\\`, `\n`, `\t`; use it when the text holds a `'` |
| Raw string | `'…'` | no escapes; SQL and regex go here |
| Environment variable | `$IDENT` | `[A-Z_][A-Z0-9_]*` |
| Bind placeholder | `$?` | one per value, any number per statement; only inside `query` |
| Name | `shop_db` | `[a-z0-9][a-z0-9_-]*`, for a view, trait or store |
| Method | `GET` | an upper-case HTTP token — `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`…; `HEAD` is answered wherever `GET` is, and `CONNECT` and `TRACE` are refused |

A quoted literal where `$IDENT` is required is a syntax error at `line:col`, so a
variable name pasted as a string never reaches a connection. Indentation and
blank lines are free; [`blastit fmt`](#formatting) has the canonical layout.
Commas separate the items of one list: `use session, catalog`,
`immutable "/assets/*", "/fonts/*"`, and a list has no trailing comma. A `;` or a
newline separates one directive from the next. Names are unique per kind, and a
trait or a store may be referenced before it is written.

## Sections

| Section | How many | Example |
|---|---|---|
| `app` | one, optional | `app "shop"` |
| `entrypoint` | one, optional | `entrypoint python "shop.wsgi:application"` |
| `postgres` | one per store | `postgres $DATABASE_URL` |
| `trait` | any | `trait session { … }` |
| `view` | any | `view products GET "/products" { … }` |
| `static` | any | `static "/" { root "public" }` |

`app` names the application; `entrypoint python` names the callable behind it.
`entrypoint` declares the one program behind every view — a Python callable or a
PHP front controller — and a static-only file omits both and is one mount:

```text
static "/" {
    root "public"
    fallback "index.html"
}
```

```text
app "shop"

entrypoint python "shop.wsgi:application" {
    preload "shop.urls", "shop.signals"
}
```

`preload` imports each module once, before the worker forks, so every request
shares one copy. The block holds `preload` only.

```text
entrypoint php "public/index.php"
```

The PHP path is relative to the `Blastfile`, ends in `.php`, and holds no `..`.
Every view in the file reaches it, so a PHP app is at least one catch-all view:

```text
app "shop"
entrypoint php "public/index.php"

static "/" { root "public" }

view app {
    GET, DELETE, OPTIONS "/*"
    POST "/*" { body form }
    PUT, PATCH "/*" { body json }
}
```

`"/*"` matches every path, and a regular file under `public` is served first.
Declare only the methods the app answers: an undeclared one is a `405`. A JSON
API beside the app is a second view with `body json` and its own grant, written
above the catch-all because the first match wins. A `Blastfile` declares one
entrypoint, so a file is either a Python app, a PHP app, or static only.

## Stores

The protocol is the keyword. Without a name a store is the default one, and its
name in `check` output is `db`; a name declares a second store.

```text
postgres $DATABASE_URL               # the default store, one libpq URL

postgres analytics $ANALYTICS_URL    # a named second store

postgres {                           # the default store, split slots
    host $DB_HOST
    port $DB_PORT
    user $DB_USERNAME
    password $DB_PASSWORD
    database $DB_DATABASE
}
```

| Directive | Value |
|---|---|
| `host`, `port`, `user`, `password`, `database` | `$IDENT` |
| `sslmode` | `$IDENT` holding `disable`, `require`, `verify-ca` or `verify-full` |
| `ca_cert` | `$IDENT` holding the PEM for the upstream TLS leg |

Every input variable keeps its name and gets the local value; a slot's variable is
the `$IDENT` written beside it:

| Slot | The app gets |
|---|---|
| `url` | `postgresql://blastit@/shop?host=<socket dir>&port=<port>` |
| `host` | the view's own socket directory |
| `port` | the local listener port |
| `user` | `blastit` |
| `database` | unchanged |
| `sslmode` | `disable`, because the local leg is a Unix socket |
| `password`, `ca_cert` | removed: they have no local value |

* A URL or the split slots, never both.
* The real values stay in the environment. A `Blastfile` holds no credential and
  is safe to commit.
* A URL's query parameters are allowlisted: `application_name`,
  `channel_binding`, `connect_timeout`, `sslmode`, `sslcompression`,
  `target_session_attrs` and `keepalives*`. Anything else refuses the boot, so a
  URL cannot smuggle a session setting past the broker.

## Views

A view is one sandbox: one worker, one set of permissions. Views are tried in the
order written, and the first match wins.

### Headers

```text
view products GET "/products", "/products/{id:int}" {
    body none
    postgres { select products(id, name, price) }
}
```

`view <name> [METHOD[, …]] <pattern[, …]> { … }`: methods first, then patterns. A
view with no method answers every method, including ones you did not intend, and
`check` cannot flag it: declare methods wherever you can. Views match on method
and pattern, so a request whose path a view covers but whose method none declares
gets a `405`, unless a static `fallback` answers it on `GET` or `HEAD`; `OPTIONS`
is one of those methods, so add it when a browser sends a preflight.

```text
view cart {
    GET "/cart"
    POST "/cart/items" { body json }
    DELETE "/cart/items/{id:int}"

    postgres { select carts(id); insert cart_items(product_id, quantity) }
}
```

The multi-endpoint form is one sandbox for several methods. A directive outside an
endpoint applies to all of them; `body` inside an endpoint applies to that method.
Grants are per view, so an `insert` granted once is allowed to every endpoint of
that sandbox: split the view when one method must not have it. A view with no
endpoint matches nothing, so declare at least one `METHOD PATTERN`.

### Path patterns

| Pattern | Matches |
|---|---|
| `"/products"` | that path; `.` is a literal dot |
| `"/products/{id}"` | one segment, `[^/]+` |
| `"/products/{id:int}"` | `[0-9]+` |
| `"/orders/{id:uuid}"` | a UUID |
| `"/assets/*"` | the prefix and everything after it; `*` is only valid at the end |
| `"/*"` | every path — the catch-all |
| `regex '/v[0-9]+/items'` | a full regular expression |

`regex` matches the whole path in linear time and has no lookaround or
backreferences. A quoted pattern matches the whole path, case-sensitively,
without the query string; `/products` does not match `/products/`, so declare
both when the framework answers both.

### View directives

| Directive | Effect |
|---|---|
| `body json` | only a valid JSON body, re-encoded canonically; anything else is a `400` |
| `body form` | only `application/x-www-form-urlencoded`, re-encoded; anything else is a `400` |
| `body none` | no body; a request with one is a `413` — the default |
| `body raw` | the bytes unchanged — the loud opt-out; `--learn` never writes it; `blastit dev` serves it with a warning, `check` reports `raw_ingress`, and a production boot refuses the file |
| `body raw unvalidated` | the same bytes, acknowledged: production serves it with a boot warning, and the app's parser receives them unchanged |
| `use session, catalog` | attach traits to this view |
| `egress any` | open any IPv4/IPv6 address and port, with no allowlist; warns at boot |
| `prefork` | fork the worker at startup instead of on the first request |
| `postgres { … }` | a grant on the default store |
| `postgres analytics { … }` | a grant on a named store |
| `postgres unrestricted` | every statement on the default store |
| `postgres analytics unrestricted` | every statement on the named store |

`raw` is the only codec that passes bytes through unvalidated, and the
acknowledgement is the whole of the decision. An unacknowledged `raw` view is
served by `blastit dev` with a warning and reported by `check`, and a production
boot refuses the file; add `unvalidated` and the file deploys, with a boot
warning. The two legitimate uses are a file upload and a raw-signature webhook
(HMAC verification). `--learn` writes neither form. The codec follows the essence
the suite sent: `application/json` is `json`, `application/x-www-form-urlencoded`
is `form`, and anything else is `raw`. A body sent to a view that declares no
`body` is a `413` in `dev` and production alike.

### Grants

```text
view catalog GET "/products/{id:int}" {
    postgres {
        transactions
        functions st_distance
        select products(id, name), categories
        update products(price)
        delete archived_products
        query 'SELECT count(*) FROM products WHERE price > $?'
    }
}
```

| Directive | Allows |
|---|---|
| `transactions` | `BEGIN`, `COMMIT`, `ROLLBACK`, savepoints |
| `functions st_distance, unaccent` | function calls beyond the built-in safe set below |
| `select products(id, name)` | reading those columns; a bare `products` reads any column |
| `insert orders(id, total)` | inserting those columns |
| `update orders(status)` | updating those columns, and row locks |
| `delete orders` | deleting rows |
| `query 'SELECT id FROM products WHERE price > $?'` | one exact PostgreSQL statement, matched as a template (three shapes below); repeat the directive for more |

* A store with no grant in a view gets no listener: the view cannot reach it.
* A grant is the union of the view's own blocks and every attached trait;
  `transactions` is on when any contributor turns it on.

Matching reads the string as a template. Three shapes, and the statement
PostgreSQL receives:

| Template | PostgreSQL receives |
|---|---|
| `$?` | the next `$n`: `$1`, `$2`, … in statement order |
| `IN ($?)`, `IN ($?, ...)` | `IN ($1, $2, $n)` — one `$n` per value, at any length; `NOT IN` too: one grant covers every list |
| `($?, $?), ...` | two or more identical `VALUES` rows, any count — one row needs its own `query`; every copy still reaches PostgreSQL |

* `$1` to `$n` written by hand is accepted as it stands: PostgreSQL parses it, and
  matching drops the numbering, so `$1` and `$?` are one grant.
* One `query` is one PostgreSQL statement, parsed after those expansions: a single
  trailing `;` is allowed; a typo, a missing `FROM` or a stray `)` is a
  `Blastfile:line:col` diagnostic at the statement's first token, from `blastit
  check`, the editor and the production boot — never a runtime `sql.denied`.
* An exact `query` is matched on the text you wrote, with comments, whitespace and
  `$n` folded, and gets no table or column analysis. Read it before pasting:
  `pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file or network
  capabilities, not ordinary statements.

In the editor, hovering either token inside a `query` says exactly this:

> `$?` is one bind placeholder: the app sends one value for it, and the wire sees
> `$1, $2, …` in the order the placeholders appear. `IN ($?, ...)` binds any number
> of values with one grant — the list length is not part of the grant.

> `...` is Blastit's variadic marker, not SQL: it is legal only as `IN ($?, ...)`
> (any number of values) or after a `VALUES` row, `($?, $?), ...` (any number of
> rows). `blastit check` refuses it anywhere else.

Each refusal prints its site in front of the sentence:

| Written | Site | The CLI prints |
|---|---|---|
| `query ''` | `9:16` | ``this `query` is empty; write the one statement it grants`` |
| `query 'SELECT 1; SELECT 2'` | `11:26` | ``a `query` is one statement; this one starts a second`` |
| `query 'SELECT id FROM ...'` | `13:31` | `` `...` is only valid in `IN`/`NOT IN ($?, ...)` and after a `VALUES` row `` |
| `query 'SELCT id FROM t'` | `9:16` | ``the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` `` |
| a `query` of 65 537 characters | its first character | ``a `query` is 65536 characters at most; split it, or grant the tables and columns instead`` |
| a 1 025th `$?` | that `$?` | ``a `query` holds 1024 bind placeholders at most; grant the tables and columns instead`` |
| the 4 097th distinct `query` in one file | its own `line:col` | ``a `Blastfile` validates 4096 `query` entries at most; fold the shared statements into a `trait` that many views `use` `` |

* The parse's own message is not printed: it can quote the word it stopped at,
  which can be a value.
* Only two statements may keep a quoted literal: `SET TIME ZONE 'UTC'` and
  `SELECT set_config('TimeZone', $?, false)` ([traits](#traits), [connection
  setup](#connection-setup)). Any other quoted string — `'x'`, `E'x'`, a
  dollar-quoted block — is refused at its own `line:col`, because a value cannot be
  learned: bind it (`$?`) in the query and in the code that sends it. A number,
  `true`, `NULL` and a quoted identifier are ordinary text.
* A `…` in a `query` is refused: it is what redaction writes, and the value it
  replaced is gone. Grant the exact statement, with `$?` where the values bind.
* Table grants allow a statement no exact `query` lists when every table, column
  and function it touches is granted. Reading a column anywhere — a `WHERE`, an
  `ORDER BY`, a join — is reading it. `count(*)` reads no column, and a table
  grant takes at least one: a statement that only names a table gets advice to
  name a column (its primary key, say), never a `select <table>` that would
  widen the grant.
* `pg_catalog` and `information_schema` are never covered by table grants; write
  those statements as `query`.
* `RETURNING` reads the columns it names, so grant them with `select` as well.
* A statement containing a `'` goes in a `"…"` string, as in the trait example;
  `'…'` has no escapes.
* `unrestricted` grants every statement on that store, so it also makes `check`
  pass: the audit can no longer tell you what the view needed. It is not a fix
  for a missing grant, and it warns at boot. The sandbox, the uid check and the
  other stores' grants still apply.

### Connection setup

`SET`, `SHOW` and `set_config(…)` run on whichever view opens the connection
first, so keep them in one trait and attach it to every view that reaches the
store:

```text
trait connection {
    postgres {
        query "SET TIME ZONE 'UTC'"
    }
}

view cart GET "/cart" {
    use connection
}
```

`check` reports such a statement as `add_group`; the trait is where it belongs,
not one view's block.

### The built-in safe function set

A table or column grant allows these functions; any other needs an entry in
`functions`.

| Category | Functions |
|---|---|
| Aggregates | `array_agg`, `avg`, `bool_and`, `bool_or`, `count`, `json_agg`, `json_build_object`, `jsonb_agg`, `jsonb_build_object`, `max`, `min`, `string_agg`, `sum` |
| Conditions | `coalesce`, `greatest`, `least`, `nullif` |
| Strings | `btrim`, `concat`, `left`, `length`, `lower`, `ltrim`, `overlay`, `position`, `replace`, `right`, `rtrim`, `similar_to_escape`, `substr`, `substring`, `trim`, `upper` |
| Numbers | `abs`, `ceil`, `floor`, `round` |
| Dates and times | `age`, `date_trunc`, `extract`, `make_interval`, `now`, `timezone`, `to_char` |
| Sets and arrays | `array_length`, `cardinality`, `generate_series`, `unnest` |
| Window | `dense_rank`, `rank`, `row_number`, `cume_dist`, `first_value`, `lag`, `last_value`, `lead`, `mode`, `nth_value`, `ntile`, `percent_rank`, `percentile_cont`, `percentile_disc`, `stddev`, `stddev_pop`, `stddev_samp`, `var_pop`, `var_samp`, `variance` |
| JSON and full text | `json_extract_path_text`, `jsonb_array_length`, `jsonb_extract_path_text`, `plainto_tsquery`, `to_tsvector`, `ts_rank`, `websearch_to_tsquery` |

`pg_catalog.` is dropped before the lookup. `unaccent` reads a rule file, so a
grant names it in `functions`.

## Traits

```text
trait session {
    postgres {
        transactions
        query "SET TIME ZONE 'UTC'"
    }
}
```

| Rule | Effect |
|---|---|
| A trait holds store grants only (`postgres [name] { … }`) | `unrestricted` and `egress any` inside one are refused at boot |
| A trait grants nothing until a view writes `use <name>` | a trait used nowhere warns at boot |

```text
view products GET "/products" {
    use session
    postgres { select products(id) }
}
```

## What `--learn` writes

`blastit dev --learn` records what the app does and rewrites the file on exit:

| It writes | As |
|---|---|
| a view it saw | named after the first path segment; two paths that share one become a single view carrying the paths and methods it saw |
| its pattern | the path it saw, with a numeric or UUID segment as `{id:int}` or `{id:uuid}` |
| a statement it ran | `query '…'`, normalised, with `$?` for every value |
| a `body` codec | never: declare `json`, `form` or `none` yourself |
| `body raw`, `unrestricted`, `use` | never |
| a transaction-control statement | never: declare `transactions` |

`/users/42` becomes `view users GET "/users/{id:int}"`. It refuses a statement
that inlines a value, so bind the value and learn again. Views are
first-match-wins: review the diff, narrow any pattern it guessed too widely, and
split a view by hand when two paths under one segment need different grants. Then
run `blastit fmt`. `check --learn --json` prints a proposal instead of findings:
`{"proposed": …,"wrote": …}`; add `--dry-run` and it writes nothing (`"wrote":
false`), which is the safe first step.

## Static mounts

```text
static "/" {
    root "public"
    index "index.html"
    immutable "/assets/*", "/fonts/*"
    fallback "index.html"
}
```

| Directive | Default | Effect |
|---|---|---|
| `root` | required | the directory served, relative to the `Blastfile`; no leading `/`, no `..` segment, no `\`, no control byte |
| `index` | `index.html` | the file a directory answers with, relative to `root` |
| `immutable` | none | the files that never change: written alone it caches every exact file the mount serves, or it takes a pattern list; a match gets `Cache-Control: public, max-age=31536000, immutable`; an `index` or `fallback` answer is always `no-cache` |
| `fallback` | none | the file a path that is no file answers with, relative to `root` |

Several mounts may match; the longest prefix answers. A request under a prefix is
served from `root` with the prefix removed: `static "/assets" { root
"public/assets" }` answers `/assets/app.js` from `public/assets/app.js`. The mount
serves regular files only: `..`, NUL, absolute and backslash paths are `400`; a
dotfile, any symlink, and a `.php`, `.phtml` or `.phar` file are `404`. A dotfile is
public only when its first segment is exactly `.well-known`:
`/.well-known/acme-challenge/<token>` is served, `/.well-known/.env` is not. A missing
or unreadable `root` stops the boot before a listener binds.

### What `immutable` caches

`immutable` takes the same [patterns](#path-patterns) a `view` takes — `regex '…'`
included — matched whole against the request path, mount prefix included: under
`static "/static"`, write `immutable "/static/admin/*"`, not `"/admin/*"`.

| Written | Cached for a year |
|---|---|
| `immutable` | every exact file the mount serves — only when each name changes with its bytes; a tree with unhashed originals uses a pattern or `regex` |
| `immutable "/assets/*", "/fonts/*"` | each file whose request path matches a pattern; a trailing `/*` crosses `/`, so `"/assets/*"` reaches `/assets/chunks/x.js` |
| `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` | each file the expression matches, when only hashed names may be cached |

A mount takes at most 32 patterns, each at most 4096 bytes, quoted or
`regex '…'`; a misplaced `*` or an expression that does not compile is refused
with the same sentence a `view` pattern gets.

### Request precedence

1. A `static` mount whose prefix matches, where a regular file exists there: serve
   the file.
2. The first `view` whose method and pattern match.
3. The matching mount's `fallback` file on `GET` or `HEAD`, when the mount
   declares one.
4. `405` when a pattern matched but no view answers the method, and `404` when no
   pattern matched at all.

## `blastit check`

`check` runs the suite, audits every request and statement, and exits `0` clean,
`1` findings, `2` it could not start, `3` the suite failed. On `2` and `3` stdout
is empty: there is no document to parse.

| Finding (`kind`) | Meaning | Also carries |
|---|---|---|
| `missing_grant` | a statement no grant allows | `route`, `proxy`, `sql` |
| `dead_grant` | a grant the suite never used | `route`, `proxy`, `sql`, and `group` when it is a trait's |
| `unexercised_route` | a view the suite never reached; not reported when the suite reached nothing at all | `route` |
| `missing_ingress` | a view took a body and declares no codec | `route`, `observed` |
| `ingress_mismatch` | the declared codec is not what the suite sent | `route`, `declared`, `observed` |
| `raw_ingress` | a `raw` view without `unvalidated` | `route` |
| `unrouted_request` | a request no view matched | `method`, `path` |
| `no_observations` | the suite reached no view or store | — |
| `proxy_unused` | a store saw no statement | `proxy` |

Every finding also has `severity` (`error` in 0.2.0; a `warning` would leave the
exit code alone) and `fix`. The keys stay `route` for a view and `proxy` for a
store until the cutover re-spells them. A view the suite never reached has no
opt-out: add a test, or delete the view.

| `fix.action` | The Blastfile edit |
|---|---|
| `add_query` | add `query '<sql>'` inside that view's `postgres { }`; `sql` is normalised, `$?` per value |
| `add_group` | connection setup: write it once in `trait connection { postgres { query '<sql>' } }` and `use connection` in every view that opens the store |
| `remove_query` | delete that `query` line, or the trait's entry when the finding names a trait |
| `declare_ingress`, `fix_ingress` | set `body json` or `form` from `fix.codecs`; when `fix.allow_raw` is true the bytes are neither, so write `body raw unvalidated` after reading why |
| `add_route` | add `view <name> <method> "<path>"` from the finding's `method` and `path` |
| `add_test` | add a request to the suite, or delete the view |
| `observe` | not an edit: a transaction label is `transactions`, and a redacted `sql` is a value to bind and learn again |

`sql` is pasteable only when `fix.action` is `add_query`. `remove_route` is
reserved: 0.2.0 proposes none.

```json
{
  "schema_version": 1, "ok": false,
  "requests": 2, "statements": 1,
  "routes": {"total": 2, "exercised": 1},
  "proxies": {"db": {"statements": 1, "denied": 1}},
  "findings": [
    {"kind": "missing_grant", "severity": "error", "route": "catalog",
     "proxy": "db", "sql": "SELECT 2",
     "fix": {"action": "add_query", "to": "view catalog postgres"}},
    {"kind": "unrouted_request", "severity": "error", "method": "GET",
     "path": "/robots.txt", "fix": {"action": "add_route"}},
    {"kind": "unexercised_route", "severity": "error", "route": "checkout",
     "fix": {"action": "add_test"}}
  ]
}
```

`fix.action` is one of `add_query`, `add_group`, `remove_query`, `add_route`,
`remove_route`, `add_test`, `declare_ingress`, `fix_ingress` and `observe`.
`fix.to` names the Blastfile node to edit. It appears on the grant actions — the
view's store block for an `add_query` or `remove_query`, the trait a view attaches
for `add_group` — and on `raw_ingress`, where it names the endpoint's `body`
directive, so applying it means writing `body raw unvalidated`. `check` never
rewrites a file: apply the finding by editing the node it names, or run `--learn`
or `fmt` where that is the writer.

`fix.codecs` and `fix.allow_raw` appear on the body findings: `json` for a JSON
body, `form` for a form body, and `raw` only when a sample validated as neither.
`remove_query` and `remove_route` carry `"unsafe": true`. `--learn` writes `json`,
`form` or `none`, and never `raw unvalidated`.

The audit report also carries one `table_grants` object per view and store:
`tables` (the operations and columns its statements touched), `functions`,
`unanalysable`, and `blastfile` — those same directives under a
`# view <name> postgres` header, ready to paste into that view's `postgres { }`
block. `--prefer-tables` prints the identical lines. A table its statements only
named has no directive: `blastfile` carries advice to name a column instead,
because a table grant takes at least one.

`--json` prints that document on stdout and nothing else: the suite's own output
goes to stderr. `--learn --json` prints the proposal instead:
`{"proposed": {"routes": N, "queries": N}, "wrote": true}` — `wrote` is `false`
under `--dry-run` or when a statement was refused, and the document has neither
`schema_version` nor `findings`. On an existing view `check --learn` proposes no
statement grant; that is the 0.2.x gap.

An all-static file runs no suite: `check` audits every `root` and exits `0`, or `2`
naming a root it cannot read. For a PHP app the gate is `blastit fmt --check`: the
manifest, `root` and `libphp` preflight runs, but 0.2.0 has no request audit, so
`check` exits `2` without pytest and `3` when pytest collects no tests. With a
passing Python suite it reports `no_observations` — not one `unexercised_route`
per view — plus whatever the run really produced. A static
root the suite never requested prints an advisory line on stderr and changes no
finding. A Python file needs `pytest` (add it with `uv add --dev pytest`), which
`check` runs as `pytest -q`.

## A complete file

```text
app "shop"

entrypoint python "shop.wsgi:application" {
    preload "shop.urls"
}

postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL

trait session {
    postgres {
        transactions
        query "SET TIME ZONE 'UTC'"
    }
}

static "/assets" {
    root "public/assets"
    immutable "/assets/*"
}

view products GET "/products", "/products/{id:int}" {
    use session
    postgres {
        select products(id, name, price)
    }
    postgres analytics {
        select events(id)
    }
}

view cart {
    GET "/cart"
    POST "/cart/items" {
        body json
    }
    DELETE "/cart/items/{id:int}"
    use session
    postgres {
        select carts(id)
        insert cart_items(product_id, quantity)
    }
}
```

## Formatting

| Command | Effect |
|---|---|
| `blastit fmt` | rewrite `./Blastfile` in the canonical layout |
| `blastit fmt PATH` | rewrite that file |
| `blastit fmt --check` | change nothing; name every file that is not canonical and exit non-zero |
| `blastit fmt --check PATH` | the same for one file |

From a `uv add` install, prefix every command with `uv run`; from the release archive or
`make install`, the same command is plain `blastit`.

Canonical layout is one directive per line, four spaces per level, and one blank
line between sections:

```text
view products GET "/products/{id:int}" {
    body none
    postgres {
        select products(id, name, price)
    }
}
```

It is lossless: every comment survives, and a `--learn` edit goes through the
same tree, so a comment above a view stays above it. `blastit check` never
rewrites the file: `blastit fmt --check` is its own CI step.

## Limits

A `Blastfile` is policy, not input, so these budgets exist to bound a generated
file, never to refuse a real one. Each refusal is one line and names the budget; a
`query`'s three caps are under [Grants](#grants).

| Budget | Value | Refused with |
|---|---|---|
| file size | 16 MiB (16 777 216 bytes) | ``<file>: the file is <n> bytes; Blastfile is limited to 16777216 bytes — split the grants into a `trait`, or delete a generated block`` |
| tokens | 4 194 304 | ``<file>: <n> tokens; Blastfile is limited to 4194304 — the file is too large: replace repeated `query '…'` lines with table or column grants (`select`, `insert`, `update`, `delete`), or fold the shared grants into a `trait` that many views `use` `` |
| trivia runs (whitespace and comments) | 4 194 304 | ``<file>: <n> trivia runs; Blastfile is limited to 4194304 — the file is too large: delete the generated comments and blank lines, or fold the shared grants into a `trait` that many views `use` `` |
| list items (methods, patterns, columns, preload, grants) | 4 096 | ``<file>:<line>:<col>: a list holds at most 4096 items`` |
| pattern length | 4 096 bytes | ``<file>:<line>:<col>: a pattern holds at most 4096 bytes`` |
| `app` name length | 256 characters | ``<file>:<line>:<col>: an `app` name is 1 to 256 characters with no control byte`` |

Blocks nest two deep at most (`view { postgres { … } }`), and a value is printed
at 120 characters in an error, cut with `…`. A 4 KiB pattern is a longer
*pattern*, not a bigger *regex*: the program `regex '…'` compiles is still bounded
by `REGEX_SIZE_LIMIT` (2^20 bytes) and the nest limit, so the source cap never
widens what the scheduler will compile.

## Errors

Every syntax or whole-file check error is one line,
`<file>:<line>:<col>: <what is wrong and how to write it>`. A parse error names
the token; a whole-file check names the view and the directive — except a `query`
refusal, which prints its site and its sentence ([grants](#grants)). A refusal
with no position — an unset `$IDENT`, a missing `root`, a refused `body raw` —
names the view and the fix instead. A file, token stream, list or pattern over a
[limit above](#limits) is refused by that budget's sentence, and the advice it
carries is an edit this one file can take.

| Written | The message names |
|---|---|
| an unknown keyword | the keyword, and the section it opened |
| `=` after a keyword | the `=`, and how that keyword is written |
| a quoted string where `$IDENT` is required | the string, and `$IDENT` |
| an unknown directive | the directive, and the block it sits in |
| an unclosed `{` | the line the block opened on |
| a grant on a store that is never declared | the grant, and the store it names |
| a duplicate view, trait or store name | the second declaration |

```text
Blastfile:3:9: `=` is not Blastfile syntax; write the value after the keyword
```
