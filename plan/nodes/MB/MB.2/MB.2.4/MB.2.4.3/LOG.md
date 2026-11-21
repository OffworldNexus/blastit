# Log MB.2.4.3

## 2026-11-13T23:09 [test-curator]

created (task) critical_static_resolve: pin the open contract and four surviving mutants

## 2026-11-13T23:21 [lead]

status ready -> active

## 2026-11-13T23:21 [lead]

attempt 1/3 started

## 2026-11-13T23:21 [lead]

base cb7a1e4

## 2026-11-13T23:53 [implementer]

G1-G4 closed in the protected resolve suite (18 tests, was 14): recorded_backend from StaticRoot's Debug asserted per fixture + StaticRoot::open; sparse cap.bin at STATIC_MAX_FILE_BYTES vs big.bin +1 (+ same-inode grow past the ceiling for the '>'->'==' variant); space 0x20 legal next to 0x1f Control (matrix row + segments + head_segments + glob); symlink-declared root opens while in-tree symlinks stay Hidden. Campaign at cb7a1e4: 96 mutants, 63 caught / 3 timeout / 20 missed / 10 unviable = 66/86 killed 76.7% (was 59/86 68.6%); all 7 filed mutants now CAUGHT, 541:5->true is an equivalence on a kernel with openat2. Evidence in RESULT.md.

## 2026-11-13T23:56 [lead]

status active -> review

## 2026-11-14T00:02 [lead]

REVIEW PASS attempt 1 (1 MINOR: RESULT says 'nothing was reclassified' while 40:42 moved CAUGHT->TIMEOUT and 541:5->true joined the equivalences, so 66/66 rests on 20 equivalences). GATE: make check exit 0 (104s).

## 2026-11-14T00:02 [lead]

status review -> done
