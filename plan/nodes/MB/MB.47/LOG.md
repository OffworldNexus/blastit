# Log MB.47

## 2026-11-20T07:49 [conductor]

created (task) Align rootless e2e, ci.yml py-sapi/wheel shims, venv-smoke py3-none glob, root-safe critical tests & standalone archive Python shims

## 2026-11-20T07:50 [planner]

edited fields

## 2026-11-20T07:50 [planner]

edited fields

## 2026-11-20T08:01 [lead]

OUT OF SCOPE for every MB.47 slice (docs/** not in paths) — needs a docs node before the 0.2.0 deploy guide ships: docs/deploy/compose-traefik.md §4 uses lib=/usr/share/blastit/lib but docs/install.sh default prefix is /usr/local (=> /usr/local/share/blastit/lib), and §5 starts blastit-init with NO BLASTIT_EXEC so Host::placement refuses a capable /proc/self/exe (a child --role=dbproxy cannot execve a capable file). Every reader copying §4/§5 gets exit 1. Proven by MB.47.1's now-correct e2e, which must export BLASTIT_EXEC.

## 2026-11-20T08:02 [lead]

TWO OUT-OF-SCOPE FOLLOW-UPS for a crates/blastit node (host.rs not in any MB.47 slice's paths): (1) Host::binary() should name <dir of /proc/self/exe>/blastit when it exists beside a *-init, which would delete BLASTIT_EXEC from all three e2e start commands and remove the python3 -c resolution (and its cwd input) entirely; (2) refuse a stale capable init fail-closed: after 'pip install --force-reinstall blastit' the old _bin/blastit-init stays behind, silently pinning a pre-upgrade binary as init (check by device+inode against _bin/blastit).

## 2026-11-20T08:40 [lead]

status ready -> done
