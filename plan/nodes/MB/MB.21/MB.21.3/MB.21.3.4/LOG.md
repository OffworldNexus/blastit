# Log MB.21.3.4

## 2026-11-16T00:38 [planner]

created (task) Lossless Blastfile CST, canonical printer and blastit fmt [--check] [PATH]

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T05:23 [lead]

edited fields

## 2026-11-16T05:23 [lead]

depends_on re-pointed to MB.21.3.8: the design created the fixtures in 3.2, this plan creates them in 3.8 (lowering), and fmt's P1-P3 loop runs over that corpus and compares models -- so the printer cannot be green before lowering exists. Also blastfile/** overlaps 3.8, so the two are serialised anyway.

## 2026-11-16T06:04 [lead]

status ready -> active

## 2026-11-16T06:04 [lead]

attempt 1/3 started

## 2026-11-16T06:04 [lead]

base 1d0d17b29ac852174a7da7b79f2166bfd236dfd1

## 2026-11-16T06:24 [lead]

status active -> review

## 2026-11-16T06:24 [lead]

attempt 1 done with 8 disclosed deviations (RESULT.md); goldfish review on base 1d0d17b.

## 2026-11-16T06:28 [lead]

edited fields

## 2026-11-16T06:28 [lead]

REVIEW FAIL (attempt 1): 2 MAJOR + 5 MINOR. MAJOR1: split_lines splits on every Newline, so a list continued after a comma (select a,/b) is emitted wrapped -- §14.2 says lists are never wrapped and --check exits 0 on it. MAJOR2: fixtures 06/07/15 are not canonical, so §21's "fmt --check clean on the fixtures" fails; fixtures added to this leaf's paths (reformatting them is canonical-layout work, and the corpus test lives here), parity content unchanged. MINOR: deviation (b) is wrong for a grant list; cli one_line/positioned duplication; TooLarge sentence for the formatted length (needs error.rs -> follow-up); fmt discovery is local until MB.21.4.1.

## 2026-11-16T06:28 [lead]

status review -> ready

## 2026-11-16T06:28 [lead]

status ready -> active

## 2026-11-16T06:28 [lead]

attempt 2/3 started

## 2026-11-16T06:28 [lead]

base 1d0d17b29ac852174a7da7b79f2166bfd236dfd1 (attempt 2)

## 2026-11-16T06:40 [lead]

status active -> review

## 2026-11-16T06:40 [lead]

attempt 2 reported done: both MAJORs fixed (split_lines honours §6.6; fixtures 06/07/15 reformatted + a corpus-canonicity test), deviation (b) corrected. Fresh review on base 1d0d17b.

## 2026-11-16T06:45 [lead]

reviewer verdict PASS (attempt 2), both MAJORs verified closed with non-tautological tests. 6 MINOR recorded: (1) fmt does not re-parse its output at runtime while §22/S11 claims it does (R25's cap check is present) -> MB.21.4.1; (2) cli positioned() infers the position from Display -> needs ManifestError::position(); (3) the write-failure test silently returns under uid 0 -> needs an injectable failure seam; (4) a comment inside an unwrapped list is hoisted, so §14.3's owner oracle is not literally satisfied there -> design sentence + owner test; (5) one_line duplicates error::clipped; (6) fmt discovery is local until MB.21.4.1.

## 2026-11-16T06:45 [lead]

status review -> done
