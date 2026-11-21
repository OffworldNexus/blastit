# Log MB.50

## 2026-11-20T10:28 [conductor]

created (task) Fix final 5 PR #64 CI items (wheel-smoke entry_points spacing, host.rs symlink-independent test, empty-PATH POSIX fallback, hostedtoolcache chown & py_sapi_leg gnu/musl)

## 2026-11-20T10:28 [lead]

status ready -> active

## 2026-11-20T10:28 [lead]

attempt 1/3 started

## 2026-11-20T10:28 [lead]

base 3f0325bd2adaec97d0fc97d419337a65939e5a3e

## 2026-11-20T10:38 [lead]

REVIEW FIX (review-ux): py_sapi_leg.sh libpython guard still uses 'readelf | grep -q' under pipefail (SIGPIPE 141 -> guard silently skipped, message lies); same for the NEEDED printf|grep -q checks. Replace with 'grep -E ... >/dev/null' / case match. Minor: docker-run bash-bootstrap failure needs an 'error: py-sapi-leg:' line; drop incident-diary comment wording.

## 2026-11-20T10:42 [lead]

REVIEW FIX (review-arch): (1) BLOCKER search() POSIX fallback finds /usr/bin/python3=3.12 -> UnsupportedMinor; prefer python3.<minor> from SUPPORTED_MINORS per directory; (2) MAJOR chown -R root:root $HOME/.local breaks pipx/setup-uv; narrow to ${UV_PYTHON_INSTALL_DIR:-$HOME/.local/share/uv/python}; (3) MAJOR py_sapi_leg libpython/NEEDED grep -q SIGPIPE guards; (4) MINOR build-wheel dnf clean after success; (5) MINOR wheel-smoke regex scoped to [pytest11] + CRLF.

## 2026-11-20T10:50 [lead]

status active -> done

## 2026-11-20T10:50 [lead]

commit a60d6c4
