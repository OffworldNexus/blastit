# Log MB.3.33

## 2026-11-14T21:10 [planner]

created (task) Static root must never serve .php/.phar source (per-request 404, protected)

## 2026-11-14T21:10 [lead]

created (owner 2026-11-14, security-first order) because MB.9.2.2 is blocked behind the whole MB.9 static-port chain (MB.9.2.1.3<-1.2<-1.1<-MB.9.1.1<-MB.9.1.2, all ready). Fixes the source-disclosure behaviour in the CURRENT static path; MB.9.2.2 preserves it. Owner decision: per-request 404, not boot refusal, because entry = public/index.php lives under root = public.

## 2026-11-14T21:25 [lead]

status ready -> active

## 2026-11-14T21:25 [lead]

attempt 1/3 started

## 2026-11-14T21:25 [lead]

base f4bfcbe; .10 landed 578b2bd (PHP page truthful, no source-served claim). This node makes the claim true.

## 2026-11-14T22:12 [lead]

status active -> done
