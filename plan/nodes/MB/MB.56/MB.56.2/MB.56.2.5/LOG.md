# Log MB.56.2.5

## 2026-11-20T17:15 [lead]

created (task) Consolidate blastit-lsp e2e_lsp.rs (<=600 LOC) over the real binary + keep zed-check

## 2026-11-20T18:27 [lead]

status ready -> active

## 2026-11-20T18:27 [lead]

attempt 1/3 started

## 2026-11-20T18:27 [lead]

base a7db6b3a1cc38f5417e7420420f196841c8fc815

## 2026-11-20T18:33 [lead]

REVIEW FIX: (a) Session::spawn env_clear drops LLVM_PROFILE_FILE -> litters worktree with default_*.profraw and breaks make coverage (-p blastit-lsp) ; re-inject it after env_clear + delete strays; (b) port 4 legs before 56.3 deletes critical_lsp.rs: multi-error didOpen (2 diagnostics), didClose publishes [] w/o version, formatting a broken buffer -> null, non-BMP UTF-16 position; (c) hover directives (view/postgres) + $NAME; (d) quick-fix single-quoted query fixture + caret outside every query -> []; (e) trim argv/prose to stay <=600 LOC

## 2026-11-20T18:33 [lead]

edited fields

## 2026-11-20T18:33 [lead]

REVIEW FIX 2 (ux): (a) codeAction request omits required context + only:["quickfix"]; (b) add the documented query-parse squiggle ('SELCT id FROM t' -> one diagnostic at the first token with the docs/troubleshooting.md:43 sentence); (c) docs/local.md:119 wrongly points Quick Fix at a line:col that sql.denied never prints - rewrite; (d) hover.rs:74 references a deleted mod tests

## 2026-11-20T18:39 [lead]

edited fields

## 2026-11-20T18:42 [lead]

status active -> done
