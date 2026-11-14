# Log MB.3.33

## 2026-11-14T21:10 [planner]

created (task) Static root must never serve .php/.phar source (per-request 404, protected)

## 2026-11-14T21:10 [lead]

created (owner 2026-11-14, security-first order) because MB.9.2.2 is blocked behind the whole MB.9 static-port chain (MB.9.2.1.3<-1.2<-1.1<-MB.9.1.1<-MB.9.1.2, all ready). Fixes the source-disclosure behaviour in the CURRENT static path; MB.9.2.2 preserves it. Owner decision: per-request 404, not boot refusal, because entry = public/index.php lives under root = public.
