# Log MB.23.4.1

## 2026-11-17T11:17 [planner]

created (task) The one loaded-file codec and the Python reporter: sys.modules closure + open audit hook, boot/preload harvest, per-request delta

## 2026-11-17T11:17 [lead]

status ready -> active

## 2026-11-17T11:17 [lead]

attempt 1/3 started

## 2026-11-17T11:17 [lead]

base 9e23dc5 (work already on disk from MB.23.4 attempt 1)

## 2026-11-17T11:33 [lead]

verified on disk; codec 21 legs, critical_loaded_files 2 legs, make check 173 targets ok (only the MB.23.5 fence). Added the missing EAGAIN/resync, no-I-O-in-hook, namespace/stdlib and stage1-order witnesses, all mutation-checked; fixed a READY-ordering flake in the critical test.

## 2026-11-17T11:33 [lead]

status active -> review

## 2026-11-17T11:39 [lead]

REVIEW FAIL (1 MAJOR, 3 MINOR): a boot set larger than MAX_PATHS goes out as several datagrams each flagged boot with seq 0,1,2, so a design-following consumer drops all but the last chunk and most of the watched set is never marked; Sequence::accept also admits boot with seq != 0. MINORs: 64 KiB batch cap vs the design's 32 KiB, two string-grep tests overclaiming behavioural proof, and a tautological assert plus a 60 s timeout on the real regression leg.

## 2026-11-17T11:39 [lead]

status review -> ready

## 2026-11-17T11:39 [lead]

status ready -> active

## 2026-11-17T11:39 [lead]

attempt 2/3 started

## 2026-11-17T11:46 [lead]

attempt 2: is_boot only on the first datagram (65-path boot -> 2 datagrams, one boot; the measured 458-file case -> 8, one boot, union complete), BootNotFirst refused on encode/decode, 32 KiB batch cap restored, both source scans re-documented as token/ordering witnesses with a widened ban list, EAGAIN leg de-tautologised. Mutation-checked.

## 2026-11-17T11:46 [lead]

status active -> review

## 2026-11-17T11:53 [lead]

reviewer attempt 2 VERDICT PASS (mutation-checked: split-boot, 32 KiB cap, nonblocking, hook ban list). Two MINORs carried to MB.23.5 (its adopting leaf): a debug_assert for a re-boot on a live reporter and the stale attempt-1 summary line. Committing.

## 2026-11-17T11:53 [lead]

status review -> done
