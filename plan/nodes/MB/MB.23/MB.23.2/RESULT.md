# MB.23.2 — design + ADR + four goldfish reviews

## Delivered

| Artefact | What |
|---|---|
| `docs/design/dev-fanotify-reload.md` | `status: In review`, 9 sections: DX target, goals/non-goals, R1–R16, the two seams and the frozen interfaces, the union and its datagram codec, the debounce machine, the two transactions (all-views vs the reload cycle with the dbproxy policy refresh and the stage-1 re-exec), the failure ladder, both encodings, the budgets, a 35-row test table, the four-leaf plan, and the full `## Review Resolution` including the reviewer's nine-`MINOR` sweep. |
| `docs/decisions/ADR-0014-dev-fanotify-live-reload.md` | 8 decisions, the kernel/filesystem floor (`5.13`, `6.5`, `flags = 0`), 7 rejected alternatives (one row is the banned watcher), consequences. |
| `docs/design/dev-fanotify-reload.review-{security,implementability,testability,consistency}.md` | four fresh-context lens reviews; each re-ran once after the first pass and verified the later amendments in its own `## Resolution`. All four end `VERDICT: PASS`. |
| `docs/decisions/README.md`, `docs/design/README.md` | one row each, `Node: MB.23`; the decisions README was re-read immediately before the edit and only the ADR-0014 row was added. |

## Decisions this document freezes (for MB.23.3–MB.23.6)

- **Two seams, no more**: `trait ReloadWatcher { mark_dir; read }` (real `blastit_sys::fanotify::Watcher`, one debug/test fake) and `trait Clock`. `INIT_FLAGS`/`MARK_MASK`/`probe_flags` and `parse_records` are the pure oracles; the policy takes `Instant` and never sleeps.
- **One reload unit**: the union of the `preload` phase's and every view worker's reported files (`ReportTable` owns the per-view report fds with `register`/`revoke`; a non-absolute, non-UTF-8 or missing path is dropped); one `Ctl::Reload` (`0x45`, empty payload); every idle view retires together with `RetireReason::Reload`; mid-request views go stale and finish.
- **Two actions, two words**: `Entry.pre_fork` chooses the action (all-views transaction vs the cycle), the class chooses `reason` (`loaded` for an ordinary file the app loaded, `preload` only for a file of a manifest `preload` module, `blastfile`, `overflow`). Cache dirs are **marked but never trigger**.
- **`:8080` never moves**: `http` keeps the listening socket for the whole run. A `Blastfile`/`preload`/boot-imported file save validates, emits the event, refreshes dbproxy's authoritative allowlist over the dev policy socket (non-blocking write + ack, 100 ms, little-endian length prefix), retires all views, stops only the scheduler, and re-execs the `/proc/self/cmdline` stage-1 prefix with `http`, `http.chan`, dbproxy, the `--learn` fds (`Learn::adopt`), the policy socket, the root and `--dev-reload-since` adopted; the new image closes the no-watcher gap with one `reconcile`.
- **One honest limit**: a change to the proxy *listener set* waits for a restart and says so; a grant edit (and a removal) applies on save; a connection admitted before the swap keeps its grants until its view retires at `DONE`. `--learn` skips the manifest cycle so its tally survives.
- **Dev only**: `--no-reload` means zero fds, marks, hooks and events; production, `check` and `test` never construct it; `seccomp.rs` unchanged; no new dependency.

## Acceptance — commands and their tails

```console
$ test -s docs/design/dev-fanotify-reload.md && test -s docs/decisions/ADR-0014-dev-fanotify-live-reload.md
test -s: ok

$ for lens in security implementability testability consistency; do tail -2 "docs/design/dev-fanotify-reload.review-$lens.md"; done

VERDICT: PASS

VERDICT: PASS

VERDICT: PASS

VERDICT: PASS

$ grep -c "^## " docs/design/dev-fanotify-reload.md
9

$ grep -nE "0x0c03|AT_HANDLE_FID|50 ms|250 ms|500 ms|64-path|16 KiB" docs/design/dev-fanotify-reload.md
(29 matching lines; first and last)
98:   directory, `AT_HANDLE_FID` handles). Exactly one group per dev boot, created only in
1113:| F1 no test could see `0x0c03` or the mark mask | MAJOR | accepted → §4.2 freezes `INIT_FLAGS`/`MARK_MASK`/`init_flags`/`mark_mask`; T1 asserts equality plus one behavioural twin per bit |

$ grep -nE "Ctl::Reload|all (active )?view|preload cycle|listener" docs/design/dev-fanotify-reload.md
(29 matching lines; first and last)
31:every view worker reported is the watched set; any tracked change sends one `Ctl::Reload`
1159:| S-F4 a boot listener omitted from the policy document | MINOR | accepted, then corrected in the resolution check: the document is built from the new manifest's own `(route, proxy)` pairs and is authoritative (whole-map replace, absent pair = deny), so neither an emptied grant nor a dropped block can fail open |

$ grep -rniE "per-route (floor|dependency|selection)" docs/design/dev-fanotify-reload.md   # only the rejected-alternative row
882:| A per-route dependency map with selective single-view restarts | Mixed module versions across views; `preload` files belong to no route; extra bookkeeping for pre-fork and lazily-loaded files; rejected by the epic and recorded here and in `ADR-0014`. |

$ grep -rn "inotify" docs/decisions/ADR-0014-dev-fanotify-live-reload.md   # only the "rejected" row
85:| `inotify` | Banned by the owner: one watcher, no second one, not behind a flag and not as a fallback. |

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s

$ uv run --no-project tools/guard.py
guard: OK (40 staged/untracked file(s) checked)

$ uv run --no-project tools/plan.py validate
plan OK
```

## Post-review MINOR sweep (the reviewer's nine)

All nine are applied in the design's `## Review Resolution` (a cache dirs are marked but never trigger; b relative `open()` paths become absolute in the sender's flush and a non-absolute datagram path is dropped; c the `pid == 0` path-only attribution rule is stated once; d `dev_learn.rs` is unchanged and `Learn::adopt` re-adopts the descriptors; e every ladder row has its warning clause, `EINVAL` and `EMFILE` included; f a malformed record is skipped and parsing continues, `ShortBuffer` only for an unusable buffer; g nine `0x45` sites (five compiler-caught, four catch-alls) and four commits, everywhere; h the pre-swap connection wording is qualified and F9/F11 are resolved (`ARCHITECTURE.md` in §8, little-endian on every frame and length prefix); i `reason=preload` is only a manifest `preload` module's file, with `Entry.pre_fork` carrying the action). None needed a re-review; no security, budget or seam content was weakened.

## Notes / Follow-ups

- **Plan path additions** (bookkeeping, not a design change; §8 lists them): `MB.23.4` needs `crates/blastit-view/src/{config,request}.rs`, `crates/blastit-php-sapi/src/loader.rs`, `Makefile`, `e2e/php-sapi/Dockerfile`; `MB.23.5` needs `crates/blastit/src/{stage1,zygote,stage0}.rs`, `crates/blastit-frame/src/frame.rs`, `crates/blastit-events/src/{json,pretty}.rs`, `crates/blastit-dbproxy/src/{learn,state,server}.rs`, `ARCHITECTURE.md`, `Makefile`, `scripts/check-no-test-seam.sh`. `crates/blastit/src/dev_learn.rs` stays unchanged.
- **MB.23.5 is the largest leaf**: ordered as 5a/5b-i/5b-ii/5c; `MB.23.5.1` (dbproxy policy refresh) and `MB.23.4.1` (PHP half) are pre-authorized splits.
- The design contains exactly one occurrence of the banned watcher word — the required guard-script path in §8; the ADR contains exactly one — the rejected row.
