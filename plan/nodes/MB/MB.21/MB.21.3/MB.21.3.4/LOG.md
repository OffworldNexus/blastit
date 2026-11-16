# Log MB.21.3.4

## 2026-11-16T00:38 [planner]

created (task) Lossless Blastfile CST, canonical printer and blastit fmt [--check] [PATH]

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T05:23 [lead]

edited fields

## 2026-11-16T05:23 [lead]

depends_on re-pointed to MB.21.3.8: the design created the fixtures in 3.2, this plan creates them in 3.8 (lowering), and fmt's P1-P3 loop runs over that corpus and compares models -- so the printer cannot be green before lowering exists. Also blastfile/** overlaps 3.8, so the two are serialised anyway.
