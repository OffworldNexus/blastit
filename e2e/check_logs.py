"""Check blastit's stdout after the e2e scenario (ARCHITECTURE.md §8.1). Stdlib only.

    check_logs.py APP_LOG      # E2E_SECRET_VAR names the variable holding the secret

* every line is one JSON event of at most 2048 bytes;
* the ungranted statement of /notes/peek/ was denied and logged, for route notes;
* the raw-SQL probe (`/notes/sql/`, `/debug/sql/`) was refused everywhere except
  on the route granted that exact statement, and nothing destructive reached
  the database;
* the scenario's requests were routed to the expected routes, including the
  egress probe, and the boot event says the seccomp filters are on;
* `E2E_EXPECT_PRESET` names the preset the deployment runs under, when it has
  one (E1), and the boot event names it and the settings it supplied;
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
    check(any(e.get("event") == "boot" and e.get("seccomp") is True for e in events),
          "the boot event says the seccomp filters are on")

    # D6: the same boot event says which mode init runs in, because a rootless
    # server and a root one are indistinguishable from the outside — and the
    # rootless one is the deployment whose capabilities have to be right.
    stage0 = [e for e in events if e.get("event") == "boot" and e.get("stage") == "stage0"]
    check(len(stage0) == 1, f"one stage 0 boot event ({len(stage0)})")
    if stage0:
        privilege = stage0[0].get("privilege")
        check(privilege in ("root", "capabilities"),
              f"the boot event says the mode init runs in ({privilege})")
        capabilities = stage0[0].get("capabilities") or []
        check(bool(capabilities) == (privilege == "capabilities"),
              f"the capability set is listed exactly in rootless mode "
              f"({privilege}: {capabilities})")
        check(all(str(c).startswith("cap_") for c in capabilities),
              f"every capability it names is a cap_* name ({capabilities})")

    # §9.2 #7 and §8 Sizing: the preload `boot` event reports the
    # immortalisation and the view process cap the memory bought.
    preload = [e for e in events if e.get("event") == "boot" and "immortalized" in e]
    check(len(preload) == 1, f"one preload boot event ({len(preload)})")
    if preload:
        immortalize = preload[0].get("immortalize")
        check(immortalize in ("on", "off"),
              f"the boot event says whether the heap was immortalised ({immortalize})")
        immortalized = preload[0].get("immortalized")
        check(isinstance(immortalized, int) and not isinstance(immortalized, bool),
              f"the immortalised count is an integer ({immortalized})")
        if immortalize == "on":
            check(immortalized > 0,
                  f"the preloaded heap was immortalised ({immortalized} objects)")
            check(isinstance(preload[0].get("immortalize_ms"), (int, float)),
                  f"the pass was timed ({preload[0].get('immortalize_ms')}ms)")
        else:
            # Skipped: nothing was marked, and the event carries the reason
            # (CPython 3.14, an unsupported build, BLASTIT_IMMORTALIZE=0).
            check(immortalized == 0, f"nothing was marked ({immortalized})")
            check(bool(preload[0].get("immortalize_reason")),
                  f"the boot event says why it was skipped "
                  f"({preload[0].get('immortalize_reason')})")
        cap = preload[0].get("max_view_processes")
        check(isinstance(cap, int) and not isinstance(cap, bool) and cap >= 1,
              f"the view process cap was derived from memory ({cap})")
        check(isinstance(preload[0].get("mem_total"), int) and preload[0]["mem_total"] > 0,
              f"mem_total is a byte count ({preload[0].get('mem_total')})")
        check(isinstance(preload[0].get("mem_used"), int),
              f"mem_used is a byte count ({preload[0].get('mem_used')})")
        # A skipped pass says so once, at warn, before the boot event.
        if immortalize == "off":
            warnings = [e for e in events
                        if e.get("event") == "app.output" and e.get("level") == "warn"
                        and "immortalisation is off" in e.get("line", "")]
            check(len(warnings) == 1,
                  f"one warn line says immortalisation is off ({len(warnings)})")

    denied = [e for e in events if e.get("event") == "sql.denied"]
    sqls = [e.get("sql", "") for e in denied]
    check(any(e.get("route") == "notes" and '"auth_user"."password"' in e.get("sql", "") for e in denied),
          "the password-hash read from the notes route was denied and logged")
    check(any(e.get("route") == "notes" and sql.strip().startswith("DROP TABLE") for e, sql in zip(denied, sqls)),
          "the destructive statement was refused and logged")
    check(any("auth_user" in sql and "count" in sql.lower() for sql in sqls),
          "the users table couldn't be counted from the route that wasn't granted it")
    refusals = {(e.get("route"), sql.strip()) for e, sql in zip(denied, sqls)}
    granted_text = "SELECT count(*) FROM notes_note"
    check(("notes", granted_text) in refusals,
          "the very statement the other route was granted is refused here")
    check(not any(route == "sql" and sql == granted_text for route, sql in refusals),
          "the route that was granted the statement was never refused it")
    routes = {e.get("route") for e in denied}
    check(routes <= {"notes", "debug", "sql"}, f"every refusal names its own route ({routes})")

    routed = {(e.get("path"), e.get("route")) for e in events if e.get("event") == "request"}
    for path, route in [("/login/", "login"), ("/notes/", "notes"), ("/logout/", "logout"),
                        ("/debug/env/", "debug"), ("/debug/request/", "debug"),
                        ("/debug/caps/", "debug"),
                        ("/", "fallback"), ("/notes/peek/", "notes"),
                        ("/notes/sql/", "notes"), ("/debug/sql/", "sql"),
                        ("/debug/egress/", "debug"), ("/egress/probe/", "egress")]:
        check((path, route) in routed, f"{path} was routed to {route}")

    # E1: the trust rules of this deployment are a preset, and the scenario has
    # just checked what they decide (the client address, the scheme). The log
    # says which preset it was, and which settings it supplied.
    expected_preset = os.environ.get("E2E_EXPECT_PRESET", "")
    if expected_preset:
        boots = [e for e in events if e.get("event") == "boot"]
        named = [e for e in boots if e.get("preset") == expected_preset]
        check(bool(named), f"the boot event names the preset ({expected_preset})")
        if named:
            supplied = named[-1].get("preset_settings") or {}
            check(bool(supplied),
                  f"and the settings it supplied ({sorted(supplied)})")
            check(all(str(name).startswith("BLASTIT_") for name in supplied),
                  f"every one of them is a BLASTIT_* variable ({sorted(supplied)})")

    print(f"\n{'PASS' if not FAILURES else 'FAIL'}: {len(FAILURES)} failure(s)")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
