# Log MB.47.1

## 2026-11-20T07:50 [planner]

created (task) Rootless Docker e2e blastit-init alignment, venv-smoke py3-none glob & root-safe critical tests

## 2026-11-20T07:50 [lead]

status ready -> active

## 2026-11-20T07:50 [lead]

attempt 1/3 started

## 2026-11-20T07:50 [lead]

base 9066d280ecb80f98b14c898895dffb66d1273bcd

## 2026-11-20T07:59 [lead]

status active -> review

## 2026-11-20T08:01 [lead]

REVIEW FIX (in scope, e2e/**): (a) rootless/README.md standalone example copies blastit-init into /usr/local/bin, but check.rs resolves the package as <exe_dir>/../python — must copy+setcap inside $lib=/usr/local/share/blastit/lib and run BLASTIT_EXEC=$lib/blastit $lib/blastit-init; (b) README:258 + run.py:773 stale 'base image is the wheel's interpreter tag' rationale vs py3-none universal wheel; (c) README:61 + run.py:592 'shared interpreter' -> shared binary _bin/blastit; (d) run.py:572 FAIL text must name file+expected+got; (e) run.py:525 'Permission denied not in runnable' is a false green, also require blastit's own --version output; (f) venv-smoke.sh:36 error must name dir+pattern. REJECTED as noise: venv-smoke.sh:11 manylinux_2_28->_39 tag churn (tag is build-image dependent).

## 2026-11-20T08:02 [lead]

REVIEW ROUND 2 (review-arch, all verified by lead): [BLOCKER] getcap on symlink command -v python3 prints nothing/rc=0 so 'cap_ not in python' can never fail (regression vs the readlink -f it replaced) + same bug in the setcap -r remediation; [MAJOR] usable_php_runtime reads BLASTIT_LIBPHP while dev_command env_clears it (asymmetric), (Some,None,_) swallows Ok(answer) so 500/404 unchecked, refusal arm log.contains(php) is vacuous, python3 -c from WORKDIR /app lets the app tree choose $lib/BLASTIT_EXEC as init uid (needs -I), runnable check fail-open on rc. Also rejected 859-line gratuitous reformat of run.py (py-lint covers only python tools).

## 2026-11-20T08:15 [lead]

status review -> done
