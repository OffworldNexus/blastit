# Log MB.2.5.1

## 2026-11-13T20:52 [planner]

created (task) Rewrite precedence and the static method/body gate (decide) - design MB.2.5a

## 2026-11-13T22:01 [lead]

status ready -> active

## 2026-11-13T22:01 [lead]

attempt 1/3 started

## 2026-11-13T22:01 [lead]

base 7a79a95

## 2026-11-13T22:06 [implementer]

rewrite::decide (total §4.3/§4.4 order, all four statuses, Allow: GET, HEAD), rewrite::declares_body, RouteInfo::with_static/static_forward; 13-test protected pure half critical_rewrite.rs; test/lib/clippy/doc/fmt/guard(7a79a95) green

## 2026-11-13T22:06 [lead]

status active -> review

## 2026-11-13T22:13 [lead]

REVIEW PASS attempt 1 (3 MINOR, all design-doc sync: add declares_body to §4.2, add the refusal-terminality row to §4.4, record that critical_rewrite.rs carries both leaves so its budget is raised).

## 2026-11-13T22:13 [lead]

status review -> done
