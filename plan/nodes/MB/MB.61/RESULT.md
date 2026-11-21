# MB.61 — Explain why serving is Linux-only (kernel facilities, not packaging)

## What changed

- `docs/local.md`: kept the first two sentences of the collapsed
  `??? note "Developing on macOS or Windows?"` verbatim, then appended **one**
  paragraph of evidence. Every mechanism named is verified in this tree
  (`blastit-sys/src/caps.rs`, `blastit-view-static/src/static_asset.rs`,
  `blastit-sys/src/seccomp.rs`, `blastit-sys/src/sock.rs`).
- `docs/index.md`: one clause on the existing `!!! note`'s last sentence
  pointing at the explanation; no mechanism list repeated there.
- Nothing else touched: no `ARCHITECTURE.md`, no `docs/decisions/`, no
  `!!! warning` before command 1, no roadmap promise, note stays collapsed
  below command 1.

## Truth fixes applied in the second pass

- **D1 — seccomp does not inspect data.** `blastit-sys/src/seccomp.rs` is a
  per-role syscall refusal list; `socket()` is the sole network path and
  `role.sockets()` decides whether one exists, with `AF_INET`/`AF_INET6`
  allowed only under `role.egress()`. The paragraph now says exactly that and
  no longer claims a filter over outgoing bytes.
- **D2 — only the uid split is production-only.** `blastit dev` also installs
  `SeccompRole::View { egress }` (`crates/blastit/src/stage1/spawn.rs:439-447`)
  and gets `openat2` static confinement and `SO_PASSCRED`; the split is
  `Privileges::DropTo` vs `Privileges::Shared` (same file 430-437). Production
  is now named only for the uid split, matching this page's warning that the
  credential barrier and grants are real in dev.
- **D3 — not an absolute packaging gap.** ADR-0010:36 also cites Homebrew PHP
  omitting `--enable-embed` and the Windows embed/ZTS ABI, so the paragraph
  opens with "Not just packaging", drops "no portable equivalent"
  (`setresuid`/`getpeereid` exist elsewhere) and lets "Porting means replacing
  those mechanisms" carry the portability point. The trailing "while the editor
  ships native everywhere" was dropped because lines 21-22 already say it.

## Exact final paragraph (docs/local.md, inside the collapsed note)

```text
    Not just packaging: serving is built on Linux kernel facilities. Production's
    per-`view` isolation is a real uid split (`setresuid`, `CAP_SETUID`/`capset`);
    static reads are confined by `openat2` (`RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`,
    with an `O_NOFOLLOW` walk fallback on older kernels); a seccomp filter refuses a
    view its own `AF_INET`/`AF_INET6` sockets unless its route declares `egress`;
    roles identify each other with `SCM_CREDENTIALS` on Unix sockets. Porting means
    replacing those mechanisms — a design project, not a recompile — so serving
    stays Linux **for now**.
```

`docs/index.md`: the pointer now reads `([why Linux](local.md))`.

Consistency with `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md:36`
(Linux UIDs, `openat2`, `SCM_CREDENTIALS`, `seccomp`): same four mechanisms, no
contradiction, no fifth invented.

## Length (known >130 deviation, accepted)

```console
$ wc -l docs/local.md docs/index.md
 148 docs/local.md
 125 docs/index.md
```

Base was **139** lines in HEAD before this node, so the <130 guideline was
already exceeded by the file this node edits; the owner accepted ~146 and asked
for no informational trim. The paragraph adds 8 lines (blank separator + 8
wrapped lines, within the <=8 line budget). Nothing was deleted to chase 130.

## Verification

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.33s
```

`grep -rn "kernel facilit" docs/` → `docs/index.md:38` (the pointer) and
`docs/local.md:24` (the only mechanism list).

## STATUS

done
