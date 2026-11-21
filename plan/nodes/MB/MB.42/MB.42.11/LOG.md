# Log MB.42.11

## 2026-11-20T01:51 [unknown]

created (task) blastit check from a package/bundle: bundled_python() demands _bin, which no bundle ships (check exits 3)

## 2026-11-20T01:51 [lead]

status ready -> active

## 2026-11-20T01:51 [lead]

attempt 1/3 started

## 2026-11-20T01:51 [lead]

base 2176a531f079a6696bfbe29068d0719686a7969b

## 2026-11-20T01:59 [lead]

status active -> done

## 2026-11-20T01:59 [lead]

commit next; note for MB.42.3.3: check.rs's exe_dir.join("../../..") candidate is dead - it resolves one level ABOVE a wheel's site-packages/blastit/_bin, so it can never match (../.. is what a wheel root would need). Either make it load-bearing for the repacked py3-none wheel or delete it as a dead fallback.
