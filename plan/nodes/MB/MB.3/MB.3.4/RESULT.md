# Result MB.3.4

## Summary

The pure `blastit-fastcgi` crate ships: record framing, the PARAMS name-value codec, the CGI
head parser, and the canonicalise/allowlist/refuse wall applied before a byte leaves the
process. Six children `done`: `.1` crate + records/param/cgi (38→44 unit tests), `.2` param
wall + 25-row protected §4.7 matrix, `.3` protected lockdown hardening (30 tests),
`.4` codec hardening (the `Status: 2:0`→300 / `20:`→210 digit hole) + the 12-test protected
`critical_fastcgi_codec` suite, `.5` `resolve` owning the 8 192-byte `CONTENT_TYPE` rule,
`.6` the last two unprotected branches (`head_end` at exactly `MAX_HEAD_BYTES`, the
digit-leading header-name drop). Zero normal dependencies, no I/O, no `unsafe`, so `MB.3.6.2`
sockets and `MB.3.7` CVE proofs rest on a codec that cannot touch network or disk.

## Evidence

`CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked
test result: ok. 44 passed; 0 failed; 0 ignored   (unittests)
test result: ok. 12 passed; 0 failed; 0 ignored   (critical_fastcgi_codec)
test result: ok. 30 passed; 0 failed; 0 ignored   (critical_fastcgi_lockdown)
test result: ok. 1 passed; 0 failed; 0 ignored    (doc-tests)
```

`CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked`

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test result: ok. 30 passed; 0 failed; 0 ignored
```

```console
$ test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none | tail -n +2)" && echo "zero dependencies"
zero dependencies

$ ! grep -rn 'tokio\|UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-fastcgi/src && echo "pure codec, no I/O"
pure codec, no I/O
```

```console
$ make check
56 fences: 29 executed, 27 accounted, 0 failed
plan OK
EXIT=0
```

No proof on trust (`AUDIT-MB.3.4-gate.md`): every owned §4.7 row is a test case; every codec/
record/param refusal carries its accepted twin in `critical_fastcgi_codec.rs`;
`documented_index_request_serialises_to_the_exact_param_stream` pins every name, value and the
full encoded byte string of the documented `public/index.php` request. Mutation: **3 missed /
331 caught / 18 unviable of 352** (331/334 viable = 99.1 %); survivors are the two `|`→`^`
equivalences (`lockdown.rs:589`, `param.rs:60`) and `Params::is_empty` (no production caller),
all classified non-gaps. No test written to pass: 0 fluff, 0 misplaced, 0 broken-oracle, no
public item without an oracle.

## Files

- `crates/blastit-fastcgi/Cargo.toml`, `Cargo.lock`
- `crates/blastit-fastcgi/src/{lib,record,param,cgi,lockdown}.rs`
- `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` (30, protected)
- `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs` (12, protected)
- `plan/nodes/MB/MB.3/MB.3.4/RESULT.md`

## Follow-ups

- **Design drift:** `docs/design/php-fastcgi.md` §7.1's critical-file list omits
  `critical_fastcgi_codec.rs` (it names `critical_fastcgi_lockdown.rs` and the
  scheduler-level suites). The file still runs in `make test-critical` via the
  `crates/*/tests/critical_*.rs` glob; update §7.1 when `MB.3.6.2.2` lands.
- **Scope:** fallback-only if `MB.12` lands the libphp embed model (no FastCGI hop). Committed
  and correct for an external-fpm fallback either way.
- **Line budget:** `MB.3.4.1` **1 575** changed lines and `MB.3.4.2` **1 322**, both against
  the ≤550 guide; waivers recorded in each leaf, no cap/record type/refusal row dropped.
- `NODE.md` Context still describes a two-leaf split; six shipped (`.3`–`.6` came from
  reviews/audits). Reword at milestone close.
