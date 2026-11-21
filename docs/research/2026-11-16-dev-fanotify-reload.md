# `blastit dev` live reload: fanotify ABI, exact loaded-file tracking, and the reload protocol

- Node: `MB.23` (epic) — Author: researcher — Date: 2026-11-16
- Host all probes ran on: `Linux 6.12.117+deb13-amd64`, unprivileged `uid=1000`, xfs root, tmpfs `/tmp`, inside an LXC container.
- Hard constraint from the owner: **`fanotify` only, never `inotify`** — not as a fallback, not behind a flag.

## TL;DR (<= 8 lines)

1. Unprivileged `fanotify` works on 6.12 **only for a file-handle group**: `fanotify_init(FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME, O_RDONLY)` = `0x0c03`. The fd-based (non-FID) group is `EPERM` for a normal user, so there is no fd-based fallback.
2. `libc 0.2.191` already ships every syscall, struct and constant for `x86_64`/`aarch64` × `gnu`/`musl`, with two type traps (`mask` is `u64` on gnu but `c_ulonglong` on musl; event-mask consts are `u64`, init/mark consts are `c_uint`). `blastit-sys` needs no raw syscall numbers, only care with types and one aligned read buffer.
3. The event's `file_handle` is the **parent directory's** handle, not the file's, and it is **byte-identical** to `name_to_handle_at(dir, AT_HANDLE_FID)` — verified on xfs, tmpfs **and overlayfs**. `HashMap<(fsid, handle_type, handle_bytes) -> PathBuf>` therefore maps every event to a path with zero `open_by_handle_at` privilege. Key on the handle *bytes*, not just `handle_bytes`: `handle_type` is filesystem-dependent (`1` tmpfs, `129` xfs/overlay).
4. `AT_HANDLE_FID` (Linux 6.5+) is what makes this work on overlayfs, where plain `name_to_handle_at` returns `EOPNOTSUPP`. Without it, "app code COPY'd into a Docker image" silently has no reload.
5. `FAN_EVENT_ON_CHILD` is **immediate children only** — the tree must be marked recursively, and new directories must be marked from their `FAN_CREATE|FAN_ONDIR` event (the man page calls this racy; the fix is to treat a non-empty new directory as a change).
6. Unprivileged listeners get `pid == 0` for every event they did not cause, so reload decisions must be **path-based** (never "did my own worker write this?").
7. Python: `sys.modules` (`__spec__.origin`, never `__file__`) plus an `open`-only audit hook; a **warm** import opens only the `.pyc`, so `.pyc → .py` must be mapped rather than dropped, and Django caches templates unconditionally, so a template edit needs the restart. PHP: `EG(included_files)`, snapshotted between `php_execute_script` and `php_request_shutdown`, covers executed PHP plus compiled Twig/Blade caches; Twig/Blade revalidate their `.twig`/`.blade.php` sources in debug, so only `.php` needs a restart.
8. Reload scope: `Blastfile` change → re-exec the supervisor; code file known to route(s) → reload those routes; unknown code file → reload all routes. Debounce 50 ms quiet / 250 ms max, min 500 ms per route.

## Developer Experience Implications

**What the developer types.** Nothing new. `blastit dev` keeps reload ON by default, so the single sentence in `docs/local.md` today — *"The file is read once. Restart after editing a view or a grant."* — becomes: *"Edits reload the view they belong to. `--no-reload` turns that off."* Two lines change; no new config key, no new flag in the `Getting started` path, no `Blastfile` field.

| Situation | Out of the box | What the developer sees |
|---|---|---|
| Save a view (`app/views/cart.py`) | that route's views retire through the scheduler, the next request forks a fresh one | one pretty line naming the route and the file count |
| Save a shared module (`app/db.py`) | every route that loaded it reloads | one line per route, still one quiet window |
| Save the `Blastfile` | the whole `dev` supervisor re-execs | one `reload` line, then the same banner |
| Zed/Vim atomic save, `git checkout` | one reload, not N | one line per route — the 50 ms quiet window absorbs the burst |
| Old kernel / seccomp blocks fanotify | warning once at boot, server keeps serving | `reload disabled: fanotify is not available here (<errno>). Serving without live reload.` |
| App on a filesystem without file handles | warning once, server keeps serving | same line naming the filesystem type and the one-line fix (host bind mount) |
| `uv sync` / a dependency change | not watched (`.venv` is skipped: tens of thousands of inodes) | nothing — restart `blastit dev`, as today |
| `--no-reload` | zero fanotify fds, zero marks, zero audit hooks | nothing |

**Defaults that keep the 12-factor line count at zero:** the app root is the cwd the `Blastfile` was read from (no parent search today, so no ambiguity); the skip list is the one every framework already agrees on (`.git`, `node_modules`, `.venv`, `__pycache__`, framework cache dirs); the flags are dev-only (`--no-reload`, and `BLASTIT_DEV_RELOAD=poll` as a documented escape hatch for Docker Desktop/9p, never `inotify`).

**What must not leak into the other two environments.** Production `blastit` and `blastit check`/`ci` never open a fanotify fd and never install the Python audit hook; there is no env var that turns either on. The reload channel is a dev-only fd handed to the worker through the loader scope, like `--dev-obs-fd` — never an environment variable, because stage 0 strips every `BLASTIT_*` variable from a view.

## Technical Findings

### 1. fanotify, unprivileged, on this host

#### 1.1 What the kernel allows without `CAP_SYS_ADMIN`

`man 2 fanotify_init` (§ VERSIONS, man-pages 6.19) is explicit: since 5.13 an unprivileged caller may create a group, but must "create a group that identifies filesystem objects by file handles, for example, by providing the `FAN_REPORT_FID` flag", may mark **inodes only** (`FAN_MARK_MOUNT`/`FAN_MARK_FILESYSTEM` refused), may not use `FAN_CLASS_CONTENT`/`FAN_CLASS_PRE_CONTENT`, may not ask for unlimited queue/marks, and "will also not receive the pid that generated the event, unless the listening process itself generated the event".

Measured (all four facts reproduced, `/tmp/opencode/fani`, `/tmp/opencode/fanotify_depth_probe.c`, `/tmp/opencode/pidtest.c`):

```text
fanotify_init(FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME, O_RDONLY) -> fd 3    [flags 0x0c03]
fanotify_mark(fd, FAN_MARK_ADD, FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_EVENT_ON_CHILD, AT_FDCWD, dir) -> 0
fanotify_mark(fd, FAN_MARK_ADD|FAN_MARK_MOUNT, ...)                   -> EPERM (1)
fanotify_init(FAN_CLOEXEC|FAN_NONBLOCK, O_RDONLY)   /* no FID flags */ -> EPERM (1)
fanotify_init(...|FAN_CLASS_CONTENT, O_RDONLY)                        -> EPERM (1)
sysctl fs/fanotify/max_queued_events=16384  max_user_marks=539122  max_user_groups=128
```

Two of these are load-bearing:

- **No fd-based group.** A non-FID group is `EPERM` (checked before flag validation, `fanotify_user.c:1411-1425` for `FANOTIFY_UNPRIV`), so the classic `event->fd` + `readlink /proc/self/fd/N` path resolution (which *would* have worked on any filesystem) is not available to an unprivileged dev server. File handles are the only route. A consequence for testing: an unprivileged probe that expects `EINVAL` for a bogus init flag gets `EPERM` first.
- **One group for the whole app tree** is the simplest and the only one that needs no per-group bookkeeping: `max_user_groups` is 128 per real uid (`/proc/sys/fs/fanotify/max_user_groups`; the man page still spells it `max_user_group`), events are attributed by the handle they carry, not by which group caught them, and `max_user_marks` is 539 122 on this host — enough to mark every directory of a monorepo.

`FAN_REPORT_DFID_NAME` is a synonym for `FAN_REPORT_DIR_FID|FAN_REPORT_NAME` = `0x400|0x800` = `0x0c00` (`/usr/include/linux/fanotify.h:66`), so the init flags above are `0x1|0x2|0xc00 = 0x0c03`. Two traps: `0x400` alone silently produces name-less `DFID` records, and `FAN_REPORT_NAME` alone is `EINVAL`.

#### 1.2 Binary layout (verified with `sizeof`/`offsetof` + raw hex dump)

All from `/usr/include/linux/fanotify.h` (6.12) and measured by `/tmp/opencode/fanotify_handle_probe.c`:

| struct / constant | layout | value |
|---|---|---|
| `fanotify_event_metadata` | 24 B: `event_len` u32 @0, `vers` u8 @4, `reserved` u8 @5, `metadata_len` u16 @6, `mask` u64 @8 (`__aligned_u64`), `fd` s32 @16, `pid` s32 @20 | `FANOTIFY_METADATA_VERSION = 3` |
| `fanotify_event_info_header` | 4 B: `info_type` u8 @0, `pad` u8 @1, `len` u16 @2 | |
| `fanotify_event_info_fid` | 12 B: `hdr` @0, `fsid` (`__kernel_fsid_t`, 8 B) @4, `handle[]` @12; align 4 | |
| `file_handle` | 8 B: `handle_bytes` u32 @0, `handle_type` s32 @4, `f_handle[]` @8 | `MAX_HANDLE_SZ = 128` |
| init flags | `FAN_CLOEXEC 0x1`, `FAN_NONBLOCK 0x2`, `FAN_CLASS_NOTIF 0x0`, `FAN_REPORT_FID 0x200`, `FAN_REPORT_DIR_FID 0x400`, `FAN_REPORT_NAME 0x800`, `FAN_REPORT_DFID_NAME 0xc00`, `FAN_REPORT_TARGET_FID 0x1000` | |
| mark flags | `FAN_MARK_ADD 0x1`, `FAN_MARK_REMOVE 0x2`, `FAN_MARK_ONLYDIR 0x8`, `FAN_MARK_MOUNT 0x10`, `FAN_MARK_FILESYSTEM 0x100` | `FAN_MARK_INODE 0x0` |
| event masks | `FAN_CLOSE_WRITE 0x8`, `FAN_MOVED_FROM 0x40`, `FAN_MOVED_TO 0x80`, `FAN_CREATE 0x100`, `FAN_DELETE 0x200`, `FAN_RENAME 0x10000000`, `FAN_EVENT_ON_CHILD 0x08000000`, `FAN_ONDIR 0x40000000` | |
| info types | `FID 1`, `DFID_NAME 2`, `DFID 3`, `PIDFD 4`, `ERROR 5`, `OLD_DFID_NAME 10`, `NEW_DFID_NAME 12` | `FAN_NOFD = -1` |

Raw event for one `open(O_CREAT|O_WRONLY); write; close` of `app.py` in a marked directory (200 bytes read, 3 events):

```text
40 00 00 00 03 00 18 00 08 00 00 00 00 00 00 00 | ff ff ff ff 70 4c 36 00   event_len=64 vers=3 metadata_len=24 mask=0x8 fd=-1 pid=...
02 00 28 00                                     info_type=2 info_len=40
70 9e 7f d3 ce e9 b3 54                         fsid (8 B)
0c 00 00 00 01 00 00 00                         handle_bytes=12 handle_type=1
c6 dd 72 b4 a8 d9 09 00 00 00 00 00             f_handle (12 B) == the MARKED DIRECTORY's handle
61 70 70 2e 70 79 00                            name "app.py\0"
```

Four parse rules that follow from the dump:

1. `hdr.len` **includes a trailing NUL and is padded to 4 bytes** (`tmpl.html` arrives as `74 6d 70 6c 2e 68 74 6d 6c 00 00 00`, `len=44`). The name area is `hdr.len - 20 - handle_bytes` bytes (`20` = `offsetof(handle)` 12 + `sizeof(file_handle)` 8); stop at the first NUL, never at `len`. Kernel source: `fanotify_user.c:491-506`, `hdr.len = roundup4(12 + 8 + handle_bytes + strlen(name) + 1)`.
2. Info records are a chain from `metadata_len` to `event_len`; iterate on `hdr.len`, check `info_type`, and never index past `event_len`. With `FAN_REPORT_TARGET_FID` absent there is exactly one `DFID_NAME` record, but the chain is the ABI.
3. `event_len` may exceed `metadata_len + one record` only with stacked flags. A 16 KiB read buffer is enough for every `DFID_NAME` event (max ≈ `24 + 12 + 8 + 128 + 256`); `read(2)` returns `EINVAL` if the buffer is too small for the *next* event, which is a bug to fix, not to retry.
4. `read(2)` into `[u8; N]` is alignment 1 but `mask` is `__aligned_u64`: give the buffer `#[repr(C, align(8))]` or copy into an aligned struct. `libc` marks `fanotify_event_metadata` `#[repr(align(8))]` for exactly this reason.

#### 1.3 `libc 0.2.191`: everything needed already exists

Checked in `~/.cargo/registry/src/*/libc-0.2.191` (the version pinned in `Cargo.lock:873`), not from docs:

| item | defined at | `x86_64`/`aarch64` `gnu` | `musl` |
|---|---|---|---|
| `fanotify_init(flags: c_uint, event_f_flags: c_uint)` | `linux_like/linux/mod.rs:3781` | yes | yes |
| `fanotify_mark(fd, flags, mask, dirfd, path)` | `gnu/mod.rs:985`, `musl/mod.rs:710` | `mask: u64` | `mask: c_ulonglong` |
| `name_to_handle_at(dirfd, path, handle, mount_id, flags)` | `linux/mod.rs:3787` | yes | yes |
| `fanotify_event_metadata` | `linux/mod.rs:611` (`align(8)`), `musl/mod.rs:79` | `mask: u64` | `mask: c_ulonglong` |
| `fanotify_event_info_header`, `..._info_fid`, `file_handle` | `linux/mod.rs:278/285/983` | yes | yes |
| `__kernel_fsid_t` | `linux/mod.rs:33`, `237` | yes | yes |
| `FAN_*` constants (init, mark, masks, info types) | `linux/mod.rs:2930-3000` | yes | yes |
| `AT_HANDLE_FID`, `MAX_HANDLE_SZ` | `linux/mod.rs:1141-1142` | yes | yes |
| `FAN_REPORT_FD_ERROR` (0x2000), `fanotify_event_info_pidfd/_error` | — | **missing** | **missing** |

So `blastit-sys` defines **no** raw syscall numbers and **no** structs — only:

- type hygiene: `FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME` are `c_uint`, event masks are `u64`, `FAN_EVENT_INFO_TYPE_DFID_NAME` is `u8`, `FAN_MARK_*` are `c_uint`. Rust has no implicit widening; every site needs an explicit cast, and `mask as u64` is the portable way to read `metadata.mask` (gnu `u64`, musl `c_ulonglong`).
- a wrapper that hides the gnu/musl `fanotify_mark` signature difference behind one `fn mark(&self, flags: u32, mask: u64, path: &Path)` (one `as _` inside, no `cfg` in the public API).
- the alignment rule from 1.2.4.

The `[c_uchar; 0]` tails on `file_handle`/`fanotify_event_info_fid` are the C flexible-array idiom: `size_of` is 8/12, and the payload is read with `ptr::read_unaligned`/slices at `handle.as_ptr().add(8)`, bounded by `hdr.len` — never by a struct field.

#### 1.4 `(fsid, file_handle) -> PathBuf`, with no privilege at all

`open_by_handle_at(2)` needs `CAP_DAC_READ_SEARCH`; it is not needed and must not be used. The event already names the directory entry: `(parent dir handle, name)`.

The mapping is built once per marked directory with `name_to_handle_at(AT_FDCWD, dir, &handle, &mnt_id, AT_HANDLE_FID)` and verified against the event record byte for byte:

```text
# /tmp/opencode/fani /tmp/opencode/fani-root        (tmpfs)
name_to_handle_at(dir) OK mnt_id=194 handle_bytes=12 handle_type=1
dir handle  : c6 dd 72 b4 a8 d9 09 00 00 00 00 00
event handle: c6 dd 72 b4 a8 d9 09 00 00 00 00 00   compare=EQUAL    name=app.py
# /home/remy/.cache/fani-probe                      (xfs)
name_to_handle_at(dir) OK mnt_id=385 handle_bytes=12 handle_type=129
dir handle  : db fc 6d b3 00 00 00 00 e9 8e c8 51
event handle: db fc 6d b3 00 00 00 00 e9 8e c8 51   compare=EQUAL    name=app.py
# overlayfs (userns mount of lower/upper on tmpfs)
name_to_handle_at(merged, AT_HANDLE_FID) OK bytes=12 type=129        compare=EQUAL
```

Three corrections to the design sketched in the brief:

1. The key cannot be `(fsid, handle_bytes)`: `handle_bytes` is the *same number* on unrelated filesystems (12 on both tmpfs and xfs here) for completely different bytes, and the same directory can be reported under different `handle_type` values (`1` tmpfs, `129` xfs/overlayfs). Use `(fsid: [u32; 2], handle_type: i32, handle: Vec<u8>)`, or the concatenation of the three, as a `HashMap` key. Handles are opaque: compare bytes, never parse them.
2. `fsid` is `statfs(2).f_fsid` verbatim (man `fanotify(7)`: "contains the same value as f_fsid"), verified: tmpfs `statfs` `d37f9e70 54b3e9ce` = event bytes `70 9e 7f d3 ce e9 b3 54`. It is **not** `st_dev` and must not be derived from it (tmpfs `st_dev` is `71` here while its `fsid` is `0xd37f9e7054b3e9ce`; xfs `fsid = 0x0000fe03…`).
3. **`AT_HANDLE_FID` is required for overlayfs.** Plain `name_to_handle_at` on an overlay mount fails with `EOPNOTSUPP` (95) on every path, before and after copy-up, while `AT_HANDLE_FID` succeeds and matches the event handle exactly (`/tmp/opencode/fidflag`, `ovl_eq.c`). `AT_HANDLE_FID` is Linux 6.5+ (`include/uapi/linux/fcntl.h:152`); on older kernels fall back to `flags = 0` (works on xfs/ext4/btrfs/tmpfs) and disable reload where that fails.
4. Marking overlayfs *succeeds* and overlayfs *emits* `DFID_NAME` events (`info_type=2`, `handle_bytes=12`, `handle_type=129`), so a build without `AT_HANDLE_FID` fails **silently**: events arrive and no path can be resolved. Probe `name_to_handle_at` at startup and treat failure as "reload unavailable", never as "no changes yet".

One group covers several filesystems: marking a tmpfs directory and an xfs directory with the same fd delivered events with distinct `fsid` and `handle_type` (`/tmp/opencode/multifs.c`). Bind mounts of the same directory share the handle but not necessarily the path — mark the path the app is actually served from (the cwd), and one path per inode is enough.

#### 1.5 Recursion: mark every directory, and mark new ones from their own event

`man 7 fanotify` (NOTES, Limitations): "Fanotify monitoring of directories is not recursive… This approach is racy, because it can lose events that occurred inside the newly created subdirectory, before a mark is added." Measured:

```text
mark(root) with FAN_EVENT_ON_CHILD|FAN_ONDIR|FAN_CREATE|FAN_CLOSE_WRITE
write root/sub/deeper/deep.py     -> no event          (2 levels below the mark)
mkdir root/newdir                 -> mask=0x40000100 (FAN_CREATE|FAN_ONDIR) name='newdir'
mark(newdir) then write x.php     -> mask=0x108 (FAN_CREATE|FAN_CLOSE_WRITE) name='x.php'
duplicate FAN_MARK_ADD on root    -> 0 (no EEXIST; masks merge)
FAN_MARK_REMOVE                   -> 0, later writes produce no event
```

So the supervisor walks the app root once, marks every directory, and on every `FAN_CREATE|FAN_ONDIR` marks the new directory. The documented race is closed the cheap way: after marking, `read_dir` the new directory; if it is non-empty, treat it as a change (a directory that appears already populated is exactly a `git checkout`), and let the debounce coalesce. A walk must use `symlink_metadata` and never follow symlinks (an app tree can contain a symlink to `/`); directories that cannot be marked (`ENOENT` from a concurrent delete, `EACCES`) are skipped silently, not fatally.

Skip list for both the walk and the event filter (dev only, and it is the list every framework already uses): `.git`, `.hg`, `.svn`, `node_modules`, `.venv`, `venv`, `__pycache__`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.tox`, `target`. Framework caches (`var/cache`, `storage/framework/views`, `.next`, `dist`) must be **marked** (a stale cache is worth reporting) but **excluded from reload triggers** — otherwise a worker that writes a cache file reloads itself in a loop. Paths under a skipped directory can still appear in a route's loaded set (the Python hook sees `.venv/...`); they simply never produce an event, which is why a dependency change needs a restart rather than a reload.

#### 1.6 Event semantics that change the design

- **Coalescing is real**: one event carried `mask=0x108` (`FAN_CREATE|FAN_CLOSE_WRITE`) for a single create-write-close. Always test with `mask & bits`, never `==`.
- **`FAN_RENAME` duplicates**: with `FAN_RENAME` in the mask a rename produced `FAN_RENAME` (two records: `OLD_DFID_NAME` then `NEW_DFID_NAME`; a third, name-less `FID` record follows if `FAN_REPORT_FID` is set too) **plus** `FAN_MOVED_FROM` and `FAN_MOVED_TO`. Do not request it: `FAN_MOVED_FROM|FAN_MOVED_TO` alone gives the same information in single-record events.
- **Recommended mark mask**: `FAN_CLOSE_WRITE | FAN_MOVED_TO | FAN_MOVED_FROM | FAN_CREATE | FAN_DELETE | FAN_ONDIR | FAN_EVENT_ON_CHILD`. `FAN_MODIFY` is not requested: it fires on every write(2) and the close/move events already say "the file is stable enough to reload". `FAN_MOVED_FROM` is kept so a rename *out of* the tree still invalidates the route that had loaded the file; a plain unlink arrives as `FAN_DELETE` with the parent handle and the name.
- **`pid` is useless unprivileged**: an event caused by a forked child reported `pid=0`; the listener's own write reported its own pid. Never use `pid` to filter self-inflicted writes — filter by path (`__pycache__`, cache dirs) and by a per-route minimum interval instead.
- **Queue overflow** (`FAN_Q_OVERFLOW`, and `fd == FAN_NOFD` with no info record): events were dropped, so reload everything once, log one line, and continue. The limit is `fs/fanotify/max_queued_events` = 16384 here.
- **`read(2)` returns `EINVAL`** if the buffer is smaller than the next event, and `EAGAIN` when drained (the fd is `FAN_NONBLOCK`). Both are normal.

#### 1.7 The failure ladder (no `inotify`, ever)

| failure | errno | disposition |
|---|---|---|
| kernel without `CONFIG_FANOTIFY` / < 2.6.37 | `ENOSYS` | warn once, serve |
| seccomp profile blocks the call (Docker's **default** profile denies `fanotify_init`, moby `profiles/seccomp/default.json`: it sits under `CAP_SYS_ADMIN` while `fanotify_mark` is unconditionally allowed) | `EPERM` | warn once naming the three causes of unprivileged `EPERM` (capability, missing FID flag, container profile), serve |
| bad flags (a future constant) | `EINVAL` | warn once, serve |
| filesystem without file handles (9p, some FUSE; `f_fsid == 0` per man page) | `EOPNOTSUPP` from `name_to_handle_at` | warn once naming the fs type, serve |
| marks exhausted | `ENOSPC` (`fanotify_user.c:1245`) | warn once, stop marking, keep the marks already in place |
| groups exhausted | `EMFILE` (`fanotify_user.c:1493`) | cannot happen with one group |
| `FAN_MARK_ONLYDIR` on a non-directory | `ENOTDIR` | a bug in the walk, not a fallback |
| queue overflow while running | `FAN_Q_OVERFLOW` | reload all routes once, log |

The one warning must be a single line that names the cause and the fix, in the shape `dev` already uses (`reload disabled: <reason>; serving without live reload`). `BLASTIT_DEV_RELOAD=poll` (stat-based mtime scan, opt-in, default off, read by `dev` itself and never forwarded to a view) is the only alternative path worth documenting — for Docker Desktop's 9p/virtiofs bind mounts, where fanotify cannot work; it is not `inotify` and must never be the default. For a devcontainer the two honest options are a seccomp profile that allows `fanotify_init` (the mark needs no capability) or running `blastit dev` on the host; the warning says so in one clause.

`FAN_REPORT_FD_ERROR` (`0x2000`) is not used: upstream 6.13+, admin-only where present, absent from `libc` on every target, and it only refines the `fd` field that a file-handle group never sets.

### 2. Exact loaded-file tracking

The watcher says *which directory entry changed*. The loaded-file sets say *which route that file belongs to*. Both are dev-only.

#### 2.1 Python (`WsgiRuntime`)

Two sources merged, because neither alone works: a `sys.modules` walk (module closure) plus an `open`-only audit hook (data files). A **warm** import opens only the `.pyc`, and a **cached** template is opened only on its first render, so the tracker is *first-touch attribution* — freshness comes from the route restart, not from re-scanning.

**Modules.** `__spec__.origin` is authoritative: in 3.11+ `__file__` is *derived* from it (`importlib/_bootstrap.py:792`), and `__cached__` is a fabricated `__pycache__` path — never watch it. Resolve in this order, *before* any `isfile` (a zip module's `realpath` is unchanged and `isfile` is False, so an early `isfile` silently loses it):

| # | condition | action |
|---|---|---|
| 1 | `origin == "built-in"` | unwatchable |
| 2 | `origin == "frozen"` | unwatchable — **do not fall back to `__file__`**: `os.__file__` is a real `<stdlib>/os.py` (20 modules on 3.14) |
| 3 | `spec is None` | `module.__file__` (`__main__` from a script) |
| 4 | `origin is None` + `submodule_search_locations` | namespace package (Flask ships `flask/sansio` so) → watch the **directories** |
| 5 | `loader.archive` set | zipimport → report the **archive** |
| 6 | `.pyc`/`.pyo` suffix | sourceless deployment → watch the `.pyc` |
| 7 | else | `realpath(origin)` + `isfile` |

Measured over Flask 3.1 + Jinja 3.1 + Django 6.1 + `json` + `sqlite3` + an app module: 539 modules → 458 files, 1 namespace directory, 80 unwatchable (60 built-in, 20 frozen), no `.pyc` leaks; **21 ms cold, 33 µs** when re-entered against an unchanged `frozenset(sys.modules)` (memoise `id(mod)` and `origin → realpath`). PEP 660 editable installs are fine: `pkg.__file__` is the real project source.

**What is opened.** `sys.addaudithook`, `event == "open"` only. Measured on CPython 3.14.10 (`/tmp/opencode/audit_probe.py`):

```text
open ('/tmp/.../a.html', 'w', 524865)      # open(p, "w")
open ('/tmp/.../a.html', 'r', 524288)      # open(p) / pathlib.Path.read_text()
open ('/tmp/.../a.html', None, 524288)     # os.open(p, O_RDONLY)
open (3, 'w', 524865)                      # fd-based open(3, "w") -> path is an int
open ('.../json/__pycache__/tool.cpython-314.pyc.139739517369392', None, 524481)  # bytecode write
import ('json.tool', None, [sys.path...], [...])
```

One event name, `open`, for `open()`/`io.open`/`os.open`/`pathlib`/`_io.open_code`/importlib; `mode` is `None` for `os.open`; **`path` can be an `int`** (an open fd) and must be dropped unless `isinstance(path, str)`. `os.stat`/`lstat`/`access`/`exists`/`getmtime` raise **no** audit event at all in any CPython, which is why freshness cannot be observed by polling metadata from inside the worker.

**Cost** (min-of-N; the machine was running `cargo-mutants`, so treat as an upper bound): `sys.audit` with no hook 0.10 µs, noop hook 0.15 µs, `startswith + set.add` 0.24 µs (**+0.14**), `abspath + set.add` 0.98 µs (why `abspath`/`realpath` inside the hook is banned); a real `open()+close()` is 8–13 µs, so the hook disappears in the noise. A warm Flask request emits 2–6 events; 200 cached Jinja renders emit 10.

**Filter.** The predicate costs 0.69 µs inside the real `open()` path (0.24 µs of which is the audit dispatch), no syscall, no allocation:
```python
_ROOT = "/srv/app/"            # realpath WITH the trailing separator: "/app" matches "/app2"
_SUFFIXES = (".py", ".pyi", ".html", ".htm", ".jinja", ".jinja2", ".j2", ".json", ".yaml",
             ".yml", ".toml", ".ini", ".cfg", ".conf", ".env", ".txt", ".md", ".css", ".js", ".sql")
def wanted(p): return p.startswith(_ROOT) and p.endswith(_SUFFIXES)
```
Flush (1.40 µs, off the hot path): map `.pyc → .py` with `importlib.util.source_from_cache` (it does **no** existence check and raises `ValueError` for a legacy next-to-source `.pyc` — catch it), drop `_SKIP_DIRS = ("/__pycache__/", "/.git/", "/node_modules/")`, and drop `sysconfig.get_paths()["stdlib"]`. Keep site-packages and editable installs: a dependency edit is a real change.

**Safety.** The hook cannot be removed, fires on every thread, and fires *before* the syscall: a raise in it aborts the app's own `open` (verified: the file is never created), and re-entrancy is unbounded (a hook that logged recursed to `RecursionError`, aborting a real open). So: `try/except BaseException`, a `threading.local()` busy flag, no `getcwd`, no `stat`, no I/O, no import inside the hook. `sys.addaudithook` is itself audited, and a `RuntimeError` from an already-installed hook silently drops the new one — install exactly once, in the dev worker, before `import_callable` (`stage1.rs:1037`), and harvest after `BLASTIT_WARMUP` so the preloaded closure is reported once.

**Framework behaviour, and why the restart is still the answer.**
- **Django caches templates unconditionally**: `django/template/engine.py:37-41` wraps the loaders in `cached.Loader` whether or not `DEBUG`/`OPTIONS['debug']` is set, so after the first render it serves the stale template with **zero** syscalls (and no audit event). A template edit therefore *cannot* be noticed inside the process: the route restart is the mechanism.
- **Jinja/Flask** `auto_reload` re-`stat`s the file (no audit event, no fanotify event — the `stat` changes nothing on disk), then re-opens; the *first* open is the only signal either framework ever gives. The fanotify watcher sees the edit on disk regardless, so attribution from the first open is enough.
- Consequence: `blastit dev` restarts the route for the framework, and the framework's own reload is a bonus, never an assumption. Do not force `TEMPLATES` debug on: Django would need two coordinated overrides and re-parse every template per request, and Flask's `TEMPLATES_AUTO_RELOAD` still hides the `stat`.

**Report**: the boot harvest (once, after warmup — the same for every view) and the per-request delta. A `set[str]` in Python, a `HashMap<PathBuf, BTreeSet<u32>>` inverted index in Rust.

#### 2.2 PHP (`PhpEmbedRuntime` + `blastit_php_sapi.c`)

`EG(included_files)` is a per-request `HashTable` of absolute paths, initialized in `init_executor()` (`Zend/zend_execute_API.c:160`) on every `php_request_startup` and destroyed in `shutdown_executor()` (`:505`) from `zend_deactivate()` — step 9 of `php_request_shutdown`. The snapshot therefore goes **between `php_execute_script` returning and `php_request_shutdown`**, next to the existing `zend_end_try()`/`if (started)` in `blastit_php_request` (`c/blastit_php_sapi.c:594-610`). A `zend_bailout` does not invalidate the hash, so fatal/`exit()` requests still report their files. `fast_shutdown` skips the destroy in release builds (`:444-451`), so reading later dangles only in debug builds — which is how the bug would reach CI.

The hash's *values* are `IS_NULL` (so `ZEND_HASH_FOREACH_STR_KEY_VAL` buys nothing); iterate `ZEND_HASH_FOREACH_STR_KEY`, copy the bytes, filter `is_absolute()` in Rust (`opened_path` can be NULL, in which case the raw filename is the key), canonicalise and sort. Coverage:

| file kind | in `EG(included_files)`? |
|---|---|
| primary script, `auto_prepend_file`/`auto_append_file`, composer/vendor, app code, `include_once` even on a parse error | yes |
| compiled Twig (`var/cache/.../*.php`), compiled Blade (`storage/framework/views/*.php`), `bootstrap/cache/*.php`, Symfony `Container*.php` | yes |
| plain `include` of a parse-error file, missing/`open_basedir`-denied file | no |
| `.env`, YAML/JSON read with `file_get_contents`/`fopen` | no |
| raw `.twig`, raw `.blade.php` (warm cache never opens them) | no |

The missing row is handled without a stream hook: the compiled artifact embeds the absolute **source** path (Twig `ModuleNode`, Blade's `/**PATH … ENDPATH**/` header), so a compiled→source map can be built from the files the shim already reports, and a template edit is then attributed to its route. A `php_plain_files_wrapper` substitution is rejected outright: `php_resolve_path` compares wrapper identity, and `open_basedir` is enforced inside the plain opener — replacing it breaks include-path resolution and silently weakens the lockdown.

ABI: add an **optional callback sink** resolved with `dlsym` (`blastit_php_file_sink_install`, `int32_t (*included)(void *ctx, const char *path, size_t len)`), absent → no-op. No `ABI_VERSION` bump, no allocator ownership across the boundary (nothing to leak on the bailout path), no per-request allocation, and it mirrors the existing `sink.body/head/log` pattern. A `blastit_php_result_v1` extension with `malloc`ed paths would force a version bump or a second request signature.

Framework behaviour that makes this enough: Twig revalidates templates with `filemtime` when `auto_reload`/debug is on, and Blade recompiles when `isExpired` — both are the framework's *dev* defaults (`APP_DEBUG=1`, `APP_ENV=dev`/`local`). So a template edit needs **no** restart; a `.php` edit does, and `EG(included_files)` names the route.

#### 2.3 Worker → supervisor channel

A dedicated, dev-only `AF_UNIX SOCK_DGRAM` socketpair per view (a datagram is atomic, and a request must never block on reporting), the worker end added to the child's `keep` list in `child.rs` so `close_fds_except` leaves it alone. The fd number reaches the Python worker the way `--dev-obs-fd` already does — through the loader's `py.run` global scope, **not an environment variable**: stage 0 strips every `BLASTIT_*` variable from a view (`stage0.rs:2792`), and the app must not see one anyway. The supervisor attributes each datagram to its (route, pid) by the fd it arrived on; the payload repeats `route` only as a self-check (a mismatch is logged once and dropped). Binary, little-endian, one datagram per batch:

```text
batch  := u8 version (=1), u8 flags (bit0 boot, bit1 resync), u16 route, u32 seq,
          u16 count, count * { u16 len, len * u8 }        # len excludes the NUL
```

`seq` is monotonic per view: a gap means that view restarted, so its set is rebuilt. `boot` marks the chunk that replaces the route's set; `resync` (a `chdir`, a lost send, a full socket) means "treat every known path as changed for this route". Measured limits: `SO_SNDBUF` reads 212 992 B but only ~40 KB is actually in flight before `EAGAIN` (167 × 242 B, 49 × 2 KiB), and 262 142 B is `EMSGSIZE` — so chunk at **64 paths (~4 KiB)**, and on `EAGAIN` drop the batch and set `resync` rather than block. A datagram send of a 5-path batch is 1.17 µs.

The supervisor keeps `HashMap<PathBuf, BTreeSet<u32>>` (path → routes that loaded it); the channel is a new `Source` drained in `poll_once` before the log-pipe sources, and is only opened when reload is enabled.

Debounce, measured from the *last* event: 50 ms quiet window, hard 250 ms max wait, and a 500 ms per-route minimum interval. The supervisor's `poll` timeout becomes `Some(deadline)` while a window is armed (today `Shutdown::Serving` passes `None`), which is the only structural change to `supervise.rs`.

### 3. Reload semantics: what actually restarts

| trigger | action |
|---|---|
| `Blastfile` changed | re-exec the **stage-0** argv (stage 1 only knows its internal `--stage=1` argv, so stage 0 hands it over once, e.g. `BLASTIT_DEV_RESTART_ARGV`); a manifest change re-runs the boot, and pretending otherwise is a second configuration path |
| file in route *R*'s loaded set | `Ctl::Reload { route: R }` to the scheduler |
| file under a route's `root`/static tree | that route |
| `.py`/`.php` not in any loaded set | all routes (a new module can be imported by anyone) |
| template/config not in any loaded set | routes that loaded any file in the same directory; else all |
| `FAN_Q_OVERFLOW` | all routes |

`Ctl::Reload` (new type code `0x45`, payload `route u32`) is init → scheduler: the scheduler retires that route's *idle* views with the reason `reload` (a first-class `RetireReason` value, one word in `docs/reference/log-events.md`; `RetireReason::Other` already exists if a `MB.23` implementer prefers the string) and marks the route stale so the next request forks a fresh view from the immortal preloaded parent. init does not `SIGKILL` a view mid-request; the existing `RETIRE` path stays the scheduler's decision, which is where the request-integrity logic already lives.

The 10-second sanity check: `immortal` already keeps the preloaded heap, so a reload is a fork (low single-digit ms), not a re-import. That is what makes "reload all routes" acceptable as a fallback.

## Comparison Table

| option | mechanism | works unprivileged | overlayfs | `inotify`? | verdict |
|---|---|---|---|---|---|
| `fanotify` + `FAN_REPORT_DFID_NAME` + `AT_HANDLE_FID` | inode marks, dir handle + name | yes (5.13+, handles mandatory) | yes (6.5+ via `AT_HANDLE_FID`) | no | **chosen** |
| `fanotify` + non-FID group | `event->fd` + `/proc/self/fd` | **no** (`EPERM`) | n/a | no | impossible |
| `fanotify` + `FAN_MARK_MOUNT`/`FILESYSTEM` | whole-tree mark, race-free | no (`EPERM`) | n/a | no | impossible |
| `inotify` | per-directory watches | yes | yes | — | **banned by the owner** |
| stat/mtime polling | scan the tree every 500 ms | yes | yes | no | opt-in `BLASTIT_DEV_RELOAD=poll` only |

| license | crate | use |
|---|---|---|
| `MIT OR Apache-2.0` | `libc 0.2.191` (already in `Cargo.lock`) | syscall and struct definitions |
| `MIT OR Apache-2.0` | `rustix 1.1.5` (already) | `poll` deadline while a window is armed |
| — | no new dependency | the whole feature is `libc` + std |

## Concrete Recommendations for Blastit

1. **`blastit-sys`** gets one module, `fanotify.rs`, behind the existing crate rules (`unsafe` confined, `// SAFETY:` comments, `io::Result`, no `unwrap`):
   ```rust
   pub struct Watcher { /* OwnedFd + HashMap<HandleKey, PathBuf> */ }
   impl Watcher {
       pub fn new() -> io::Result<Self>;                     // 0x0c03, O_RDONLY
       pub fn mark_dir(&mut self, dir: &Path) -> io::Result<()>;   // name_to_handle_at(AT_HANDLE_FID) then FAN_MARK_ADD
       pub fn read(&mut self, out: &mut Vec<Event>) -> io::Result<()>; // non-blocking drain
   }
   pub struct Event { pub mask: u64, pub dir: PathBuf, pub name: Vec<u8> }
   ```
   `Event` keeps `(dir, name)` rather than a joined `PathBuf`: the caller decides whether a non-UTF-8 name is filtered or reload-worthy. Errors are typed (`Unavailable { errno }`, `NoFileHandle { fs_type }`) so `dev` can print one sentence per cause.
2. **`crates/blastit/src/dev_reload.rs`** owns policy: the startup walk + mark, the skip/cache lists, the `(fsid, type, bytes)` table, the debounce state machine, the route-selection rules, and the one-line warning. `supervise.rs` gains one `Source` and a `Some(deadline)` poll timeout; `cli.rs` gains `--no-reload` (dev only, default on) alongside `--learn`.
3. **Never request `FAN_MODIFY`, `FAN_ACCESS`, `FAN_RENAME`, `FAN_REPORT_TARGET_FID`, `FAN_REPORT_FD_ERROR` or `FAN_CLASS_CONTENT`**: noise, duplicates, `EPERM`, or a constant that is not in `libc`.
4. **Probe before promising**: `fanotify_init` + `name_to_handle_at` on the app root happen before the banner, so the boot line can say `reload: on (N directories)` or the one warning — never a later silent no-op.
5. **Python**: install the audit hook once in the dev worker (before `import_callable`, after the fork), guard it with `try/except BaseException` + a `threading.local()` busy flag, harvest `sys.modules` after warmup, map `.pyc → .py` with `source_from_cache`, and report one `boot` batch plus per-request deltas on the datagram fd.
6. **PHP**: add the `dlsym`-resolved file sink, snapshot `EG(included_files)` between `php_execute_script` and `php_request_shutdown`, build the compiled→source map for Twig/Blade, and leave the stream wrappers alone.
7. **Docs**: `docs/local.md` loses the "restart after editing" sentence; `docs/reference/cli.md` gains one row for `--no-reload`; `docs/troubleshooting.md` gains one row (`reload disabled` → which of the three `EPERM` causes and what filesystem to look at, plus `BLASTIT_DEV_RELOAD=poll`); `docs/reference/log-events.md` gains the word `reload` in the `process.retired` reason list. No other page changes.
8. **Tests** (added to `MB.23`, not this node): a `critical_` test for "a reload is never triggered by the reload itself" (worker writes a cache file → exactly zero further reloads), one for "atomic save = one reload", one for "`fanotify` unavailable → server still answers 200 and logs once", one for "an audit hook that raises never aborts the app's `open`", and one for overlay-mapped handles (`AT_HANDLE_FID`) so a regression to `flags = 0` fails.

## Unverified / Open Questions

- **9p / virtiofs / NFS**: `AT_HANDLE_FID` needs the filesystem to implement `encode_fh`; 9p does not, and the man page warns FUSE may report `fsid == 0`. Not testable on this host — verify on a Docker Desktop / WSL2 box before documenting `poll` as the fix.
- **Kernels 5.13–6.4**: `AT_HANDLE_FID` is 6.5+; the fallback `flags = 0` works on local filesystems but not overlay. Decide the minimum kernel for `blastit dev` reload (suggest: works or warns, never refuses to boot).
- **`AT_HANDLE_FID` + `AT_SYMLINK_FOLLOW`**: not probed; a symlinked app root (`/home/me/src → /data/src`) may need `AT_SYMLINK_FOLLOW` to match the event handle of the resolved path.
- **Python datagram edge cases**: `EAGAIN` on a full `SO_SNDBUF` was reproduced by filling it; the exact in-flight ceiling (~40 KB) is host-dependent, so the 64-path chunk is an estimate, not a guarantee.
- **`FAN_DELETE` of a directory** (`FAN_ONDIR`) was not isolated in the probes; it should behave like the file case (parent handle + name).
- **Blastit's own `RETIRE`/`SPAWN` writes** (log files, runtime root) are outside the app root, so they cannot loop; confirm no `$XDG_RUNTIME_DIR` path is ever marked.
- **Rust-side cost of the attribute index** is unmeasured (the design assumes a `HashMap<PathBuf, BTreeSet<u32>>` under a few thousand paths).

## Sources

- `man 2 fanotify_init`, `man 2 fanotify_mark`, `man 7 fanotify`, `man 2 name_to_handle_at` (man-pages 6.19, fetched 2026-11-16): unprivileged requirements, `FAN_REPORT_*` semantics, non-recursive caveat and the documented race, `fsid` = `f_fsid`, `FAN_Q_OVERFLOW`, info-record chain.
- `/usr/include/linux/fanotify.h` (6.12): every constant and struct in §1.2 (`FAN_REPORT_DFID_NAME` :66, info types :139-149, metadata :133, `FAN_NOFD` :231).
- `/usr/include/linux/fcntl.h:152` (`AT_HANDLE_FID = 0x200`), `/usr/include/x86_64-linux-gnu/bits/fcntl-linux.h:368` (`MAX_HANDLE_SZ`).
- `libc 0.2.191` sources at the paths in §1.3 (pinned by `Cargo.lock:873`).
- Probes in `/tmp/opencode/`: `fanotify_handle_probe.c`, `fanotify_depth_probe.c`, `handle_only.c`, `fidflag.c`, `ovl_eq.c`, `multifs.c`, `pidtest.c`, `dele.c`, `mark_only.c`, `fdmodel.c`, `audit_probe.py`, `audit_cost.py`, `harvest_check.py` (outputs quoted inline).
- Python: `importlib/_bootstrap.py:792` (`__file__` derived from `origin`), `_io/fileio.c:392` and `posixmodule.c:11247` (the two `open` audit shapes), `posixmodule.c:3260-3331` (no audit on `stat`/`access`), `django/template/engine.py:37-41` (`cached.Loader` unconditional), PEP 578; measured on CPython 3.14.10.
- PHP: `php-src` `Zend/zend_globals.h`, `Zend/zend.c:1316-1323/1351`, `Zend/zend_execute_API.c:160/444-451/505`, `Zend/zend_execute.c:5143`, `Zend/zend_language_scanner.l:704-710`, `ext/standard/fopen_wrappers.c:525/546`, `main/main.c:1868/1993/2603-2626`; Twig `ModuleNode`/`FilesystemLoader`, Blade `BladeCompiler::isExpired` and the `/**PATH … ENDPATH**/` header.
- In-repo: `crates/blastit-sys/src/lib.rs`, `crates/blastit/src/{supervise,dev_learn,child,cli}.rs`, `crates/blastit-frame/src/ctl.rs`, `crates/blastit-view/src/{runtime,php}.rs`, `crates/blastit-php-sapi/c/blastit_php_sapi.c`, `docs/research/php-embed-sapi.md`.
