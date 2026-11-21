# Log MB.21.3.6

## 2026-11-16T00:38 [planner]

created (task) Parse blastfile doc fences in docs_fences, reporting both openings until the strict flip

## 2026-11-16T07:38 [lead]

status ready -> active

## 2026-11-16T07:38 [lead]

attempt 1/3 started

## 2026-11-16T07:38 [lead]

base d652117ab658884d08e85e04d094333d2d62f20c

## 2026-11-16T07:52 [lead]

status active -> review

## 2026-11-16T07:52 [lead]

attempt 1 done: both openers read, pinned PAGES table with per-opening counts, one flip constant for 5.2, three deviations disclosed. Review on base d652117.

## 2026-11-16T07:57 [lead]

reviewer verdict PASS. 3 MINOR for MB.21.5.2: (1) no self-test parses a VALID blastfile fence through parse_fence (the shared branch implies it); (2) the "missing document" drift direction is not self-tested (only extra/equal); (3) the retired-opening arm tests only toml title="blastit.toml", so a plain toml fragment on a pinned page would slip through after the strict flip -- exclude the pinned fragment page or count FRAGMENT_OPENING on every other row.

## 2026-11-16T07:57 [lead]

status review -> done
