"""Check blastit's stdout after the e2e scenario (ARCHITECTURE.md §8.1). Stdlib only.

    check_logs.py APP_LOG      # E2E_SECRET_VAR names the variable holding the secret

* every line is one JSON event of at most 2048 bytes;
* the ungranted statement of /notes/peek/ was denied and logged, for route notes;
* the raw-SQL probe (`/notes/sql/`, `/debug/sql/`) was refused everywhere except
  on the route granted that exact statement, and nothing destructive reached
  the database;
* the scenario's requests were routed to the expected routes;
* the database password appears nowhere.
"""

import json
import os
import sys

FAILURES = []


def check(cond, what):
    print(f"  {'ok  ' if cond else 'FAIL'} {what}", flush=True)
    if not cond:
        FAILURES.append(what)


def main():
    raw = open(sys.argv[1], encoding="utf-8", errors="replace").read()
    secret = os.environ.get(os.environ.get("E2E_SECRET_VAR", ""), "")
    lines = [line for line in raw.splitlines() if line.strip()]
    events, bad = [], []
    for line in lines:
        try:
            events.append(json.loads(line))
        except ValueError:
            bad.append(line[:120])
    print("logs")
    check(bool(lines), "blastit printed events")
    check(not bad, f"every line is JSON ({len(bad)} not: {bad[:3]})")
    check(all(len(line.encode()) <= 2048 for line in lines), "every line is at most 2048 bytes")
    check(bool(secret) and secret not in raw, "the database password appears nowhere in the logs")

    kinds = {e.get("event") for e in events}
    check({"boot", "ready"} <= kinds, "boot and ready events")
    denied = [e for e in events if e.get("event") == "sql.denied"]
    sqls = [e.get("sql", "") for e in denied]
    check(any(e.get("route") == "notes" and '"auth_user"."password"' in e.get("sql", "") for e in denied),
          "the password-hash read from the notes route was denied and logged")
    check(any(e.get("route") == "notes" and sql.strip().startswith("DROP TABLE") for sql in sqls),
          "the destructive statement was refused and logged")
    check(any("auth_user" in sql and "count" in sql.lower() for sql in sqls),
          "the users table couldn't be counted from the route that wasn't granted it")
    check(not any(sql.strip() == "SELECT count(*) FROM notes_note" for sql in sqls),
          "the statement the sql route was granted was never refused")
    routes = {e.get("route") for e in denied}
    check(routes <= {"notes", "debug", "sql"}, f"every refusal names its own route ({routes})")

    routed = {(e.get("path"), e.get("route")) for e in events if e.get("event") == "request"}
    for path, route in [("/login/", "login"), ("/notes/", "notes"), ("/logout/", "logout"),
                        ("/debug/env/", "debug"), ("/", "fallback"), ("/notes/peek/", "notes"),
                        ("/notes/sql/", "notes"), ("/debug/sql/", "sql")]:
        check((path, route) in routed, f"{path} was routed to {route}")
    print(f"\n{'PASS' if not FAILURES else 'FAIL'}: {len(FAILURES)} failure(s)")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
