# Log MB.3.53

## 2026-11-15T08:05 [lead]

created (task) body = raw without allow_raw must refuse at boot in production (stage 0 and stage 1), not just warn in check

## 2026-11-15T08:05 [lead]

status ready -> active

## 2026-11-15T08:05 [lead]

attempt 1/3 started

## 2026-11-15T08:05 [lead]

base b2d0fa2

## 2026-11-15T08:22 [lead]

status active -> review

## 2026-11-15T08:30 [lead]

status review -> done

## 2026-11-15T08:32 [lead]

reviewer PASS (attempt 1); 4 MINORs filed as MB.3.54 (two assertions that cannot fail, the stage-0 call site, docs/frameworks/index.md's contradiction). Committed 86f885b.

## 2026-11-15T08:32 [lead]

RECORDED LIMITATION, kept in this node's history deliberately: no unprivileged suite witnesses stage 0's CALL to `raw_ingress_gate`, nor its position before `bind_view_sockets` — that half is proved only by a manual root/container run (RESULT follow-up 4 will add an e2e boot-refusal scenario as MB.3.54). This is acceptable ONLY because production stage 1 re-reads the manifest from disk and refuses there, and that refusal IS process-tested. Coverage honesty, not a hole in the gate: a manifest cannot reach a served request with `body = "raw"` and no `allow_raw` in any configuration this branch ships.
