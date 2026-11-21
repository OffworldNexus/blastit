# Log MB.31

## 2026-11-17T12:33 [planner]

created (epic) One pattern grammar for static immutable: bare immutable, view-syntax patterns and regex '...' (kill the six-glob staircase)

## 2026-11-18T00:37 [lead]

gate passed: 31.1 docs (dx+ai-ergonomics accept), 31.2 lowering through pattern.rs with the 32-cap at the 33rd node and Spelling so a regex is never read as a glob, 31.3 one matcher (ImmutableGlob/glob/256-byte guard deleted; cache header == admission matcher; red both ways), 31.4 hover + regenerated grammar with 18/18 corpus. 523 manifest + 288 scheduler tests, make check EXIT=0, docs-test 48 fences 0 failed, guards clean. Remaining, filed: the Zed grammar rev re-pin at release (no commit to point at; never guessed) and tree-sitter corpus policing in make check (MB.26.9).

## 2026-11-18T00:37 [lead]

status ready -> done
