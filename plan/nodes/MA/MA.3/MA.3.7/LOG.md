# Log MA.3.7

## 2026-11-12T23:41 [lead]

created (task) Product fixes: dev watermark boot contract, queries=[] denying listener, one-pass dev --learn

## 2026-11-12T23:41 [lead]

status ready -> active

## 2026-11-12T23:41 [lead]

attempt 1/3 started

## 2026-11-12T23:41 [lead]

base 5596dc29793945570b5836b149c5ba1e43918c37

## 2026-11-13T00:09 [lead]

attempt 1 blocked: (a)(b)(c) fixed and pinned, but the mandated boot arm makes every dev fence fail because the watermark reads cgroup memory.current which counts ~40 GiB of reclaimable page cache on this host. Attempt 2: measure usage as memory.current - inactive_file (cache is reclaimable), keep the 85% default incl. dev, add a dev boot WARN naming the knob; then docs-test can be green honestly.

## 2026-11-13T00:09 [lead]

attempt 2/3 started

## 2026-11-13T00:09 [lead]

retry base 5596dc29793945570b5836b149c5ba1e43918c37

## 2026-11-13T00:38 [lead]

status active -> review

## 2026-11-13T00:50 [lead]

status review -> done
