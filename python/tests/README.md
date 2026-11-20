# Python package tests

| File | What it proves |
|---|---|
| `test_launcher.py` | the console scripts are one `execvpe` of `blastit/_bin/blastit`: argv and exit status preserved, a missing binary named instead of a traceback |
| `test_check_pytest_gate.py` | `blastit check`'s WSGI gate over real socketpairs: the observation file, `push_unrouted`/`pop`, and a dead channel raising `ControlError` (exit `3`) |
| `critical/test_stdlib_only.py` | the published package installs no dependency and imports nothing but the stdlib |

`make docs-unit` runs the first two offline; `make test-critical` runs the
critical tier against the staged binary.
