# MB.61 — review-arch (truth pass on the public "why Linux" claim)

Scope: `git diff 76b1bcb -- docs/local.md docs/index.md`. Every mechanism claim was checked
against the tree, not against the node brief. Base `docs/local.md` = 139 lines, head = 147;
`docs/index.md` = 125.

**VERDICT: FAIL (CHANGES)** — one public mechanism claim is false, two framings overstate what
the code does, and the page breaks its own `<130` line budget.

## Per-claim verdicts

| Claim (`docs/local.md:24-30`) | Source of truth | Verdict |
|---|---|---|
| per-`view` uid split (`setresuid`, `CAP_SETUID`/`capset`) | `crates/blastit-sys/src/privs.rs:65-86`, `caps.rs:64-96` (`Capability::NEEDED`) | TRUE, production-only, correctly phrased |
| `openat2`-confined static reads (`RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`, verified-walk fallback) | `crates/blastit-view-static/src/static_asset.rs:68-71`, `174-183`, `273-279`, `556-615` | TRUE, but the flag list and the fallback's name are imprecise |
| seccomp "for forged outgoing data" | `crates/blastit-sys/src/seccomp.rs:108-145`, `695-770`; `ARCHITECTURE.md:1437-1503` | **FALSE** |
| `SCM_CREDENTIALS` between roles | `crates/blastit-sys/src/sock.rs:340-425`; `crates/blastit-dbproxy/src/state.rs:508-535`; `server.rs:467-490` | TRUE (one direction: view → dbproxy) |
| "no portable equivalent" (all four) | see finding 2 | OVERSTATED for 2 of 4 |
| "which production enforces" | `crates/blastit/src/supervise.rs:212-213`, `stage0/dev.rs:382`, `dev.rs:531-533` | IMPRECISE (3 of 4 also run in `dev`) |
| "Not packaging" | `ADR-0010…:36` (same line cites Homebrew PHP and the Windows embed ABI) | **FALSE as an absolute** |
| "design project, not a recompile" | defensible for the four mechanisms; not for the PHP/toolchain half | DEFENSIBLE, needs scoping |

## Findings

### [BLOCKER] 1. `docs/local.md:28` — seccomp does not act on outgoing *data*

`for forged outgoing data` describes a payload-inspection mechanism that does not exist. The
filter is a **per-role syscall refusal list with default allow** (`seccomp.rs:14-30`,
`build_for` at `seccomp.rs:641-790`); it never reads a byte of user data. What it actually
denies:

- every role: `ptrace`, `process_vm_readv/writev`, `pidfd_getfd`, `kcmp`, `bpf`, `keyctl`,
  `perf_event_open`, `io_uring_setup/enter/register`, `init_module`/`finit_module`,
  `mount`/`umount2`/`open_tree`/`move_mount`, `setns`, `unshare`, `reboot` (`seccomp.rs:108-118`);
- `socket()` by address family: a view keeps `AF_UNIX` only, and gains `AF_INET`/`AF_INET6`
  only when its route declares `egress` (`seccomp.rs:748-762`, table at `:136-145`);
  http and scheduler get no `socket()` at all; `socketpair()` is `AF_UNIX`-only everywhere;
- `EmbeddedView` (PHP) also loses `execve`/`execveat`/`fork`/`vfork`/process-creating `clone`
  (`seccomp.rs:383-400`).

`ARCHITECTURE.md:1503` states the intent exactly: "This is our only lever for **egress
control**, and it is what `egress` in the manifest buys."

Exact minimal replacement (keeps the one-sentence shape, stays true, keeps `egress any`
honest):

> "seccomp, which refuses a view its own `AF_INET`/`AF_INET6` sockets unless its route declares `egress`, so egress leaves only through a stage-0 proxy"

### [MAJOR] 2. `docs/local.md:24` — "no portable equivalent" overstates two of the four

- `setresuid`/`setresgid`/`setgroups` are POSIX and exist on macOS/BSD
  (`privs.rs:67-81`); what is Linux-only is doing the uid split **rootless via capability
  sets** (`caps.rs:64-96`).
- `SCM_CREDENTIALS` peer authentication has a portable counterpart (`getpeereid` /
  `LOCAL_PEERCRED` on macOS/BSD). It is chosen here because `SO_PEERCRED` lies under gVisor
  (`sock.rs:340-348`), which is a *reason for this implementation*, not proof that the
  boundary cannot be built elsewhere.

`openat2` itself has no portable equivalent (`static_asset.rs:174-183`), and `seccomp` has
none either — so the conclusion survives, but the blanket phrase does not. Fix by dropping
the phrase and letting the next sentence carry it:

> "Not just packaging: serving is built on Linux kernel mechanisms — …"

### [MAJOR] 3. `docs/local.md:25` — "which production enforces" contradicts the page and the code

Attributing all four to production contradicts `docs/local.md:15-16` on the same page
("The credential barrier and the grants are real" in dev) and the code: `blastit dev` runs
the same scheduler (so `openat2` static confinement), passes `config.seccomp` into the same
spawn path (`stage0/dev.rs:382`, `stage1/boot.rs:193,446,495`, `spawn.rs:444-445`), and binds
its dbproxy listeners with `SO_PASSCRED` (`stage0/dev.rs:531-533`). Only the uid split and the
capability clearing are production-only (`supervise.rs:212-213`).

Exact minimal replacements (the first keeps the sentence grammatical; the second keeps the
dev/production distinction without attributing all four to production):

> "which the sandbox is built on:"

> "a per-`view` uid split … (production; `blastit dev` runs one uid)"

The existing `??? warning` above already scopes dev, so the paragraph must not imply dev has no
seccomp, `openat2` confinement or `SCM_CREDENTIALS`.

### [MAJOR] 4. `docs/local.md:24` — "Not packaging:" contradicts ADR-0010 §4

`ADR-0010-php-embedded-via-libphp-embed.md:36` lists three independent reasons in one
sentence: the four kernel mechanisms, **"Homebrew PHP omits `--enable-embed`"** (a packaging
gap that a local `--enable-embed` build does close) and **"Windows uses a distinct embed/ZTS
ABI"**. Saying the reason is "not packaging" therefore contradicts the ADR this node was told
not to contradict, and it is the one place where "not a recompile" is wrong: a rebuilt
`libphp` is exactly a recompile.

Exact minimal replacement: `Not packaging:` → `Not just packaging:` (one word). That keeps
the strong claim for the kernel half and stays consistent with §4 and §3.

### [MAJOR] 5. `docs/local.md` is 147 lines — the node's `<130` hard constraint fails

`wc -l docs/local.md` = **147** (base 139, +8 from this diff); `docs/index.md` = 125. The node's
own acceptance says both must be `<130`, and AGENTS.md §4 says docs pages stay `<130`. Nine of
the 17 excess lines predate this change, but the acceptance item cannot pass at 147, so `PASS`
must not be claimed.

Do both: compress the new paragraph to the **6 lines** below (this also drops the flag triple
that finding 6 flags), and either trim the remaining 17 lines from the untouched body of the
page or escalate the `<130` criterion — do not quietly report green. Recommended final
paragraph:

```markdown
    Not just packaging: serving is built on Linux kernel mechanisms — a per-`view` uid split,
    `openat2`-confined static reads, seccomp (which refuses a view its own `AF_INET`/
    `AF_INET6` sockets unless its route declares `egress`, so egress leaves only through a
    stage-0 proxy), and `SCM_CREDENTIALS` uids on the database sockets. Porting means
    replacing them — a design project, not a recompile — so serving stays Linux **for now**
    while the editor ships native everywhere.
```

This is also more DX-appropriate than the current text: AGENTS.md §4 keeps kernel-internals
detail out of user pages, and every mechanism still names its mechanism.

### [MINOR] 6. `docs/local.md:26-27` — the `RESOLVE_*` list is incomplete

`STATIC_RESOLVE_FLAGS` (`static_asset.rs:68-71`) is `BENEATH | NO_SYMLINKS | NO_MAGICLINKS`,
and `ARCHITECTURE.md:384` spells all three. Either list all three
(`RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`) or, preferred per finding 5,
drop the flag list from public prose.

### [MINOR] 7. `docs/local.md:27` — "verified-walk fallback" is not the code's name for it

`Backend::Walk` (`static_asset.rs:174-183`, `556-615`) is "a per-segment `O_NOFOLLOW`
`openat` walk"; the type checks (`fstat` per intermediate, `ELOOP` on a symlink) and the
`(st_dev, st_ino)` probe→read comparison (`same_inode`, `static_asset.rs:632-638`) are what
make it as strong as the `openat2` path, but "verified walk" is not a term the code or
`ARCHITECTURE.md` uses, and it reads as identity-verifying every prefix (it does not).
Replacement: "with a per-segment `O_NOFOLLOW` walk where the kernel has no `openat2`".
Naming the fallback is accurate — it is the same confinement, not a weaker one.

### [MINOR] 8. `docs/local.md:25-26` — `capset` does not split uids

`setresuid`/`setresgid` split (`privs.rs:67-81`); `capset` (`caps.rs:clear_after_uid_change`,
called at `privs.rs:86`) empties the child's permitted/effective sets so the split cannot be
undone, and `CAP_SETUID` is what lets a **rootless** init call `setresuid` at all
(`caps.rs:64-96`). Suggested wording: "(`setresuid`, `CAP_SETUID` for the rootless init,
`capset` to empty each child)".

### [NIT] 9. `docs/index.md:37-38` — accurate and correctly a link, not a second list

`because serving rests on Linux kernel facilities ([why](local.md))` is true and keeps one
canonical explanation (conformance with the node's "one explanation" rule). No change needed
beyond inheriting finding 4's "not packaging" nuance in the target page.

### [NIT] 10. Others true-by-code that the paragraph omits, and should keep omitting

`fanotify` reload (`docs/local.md` mermaid), `O_PATH`, `/proc` reads and
`PR_SET_NO_NEW_PRIVS` are also Linux-only, but the paragraph says "rests on" four load-bearing
mechanisms — the same four as `ADR-0010:36` — so it is not misleading. Do not grow the list.

## Conclusion defensibility

"Not a recompile" is defensible **for the four kernel mechanisms**: each would be replaced by
a different host facility (uid split → a different privilege model; `openat2` → another
confinement primitive; seccomp → a different syscall-sandbox API; `SCM_CREDENTIALS` → peer
credentials), which is design work. It is not defensible as a blanket "not packaging" claim
(finding 4), which is the only conclusion-level wording change this review requires.
