# Log MB.21

## 2026-11-16T00:38 [planner]

created (epic) Replace blastit.toml with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)

## 2026-11-16T00:38 [planner]

Decomposed into 5 children (2 tasks + 3 epics, 19 nodes total): DX docs MB.21.1 -> design+ADR MB.21.2 -> toolchain MB.21.3.{1..6} -> cutover MB.21.4.{1..4} -> docs/proof MB.21.5.{1..4}. Deviations: epic owner omitted because plan.py's OWNERS has no 'lead' (gate stays with lead); the Blastfile ADR is ADR-0012, since ADR-0011 is already accepted (raw-ingress boot refusal, MB.3.53); MB.20.depends_on extended with MB.21.5.1 so docs/frameworks is rewritten once.

## 2026-11-16T00:40 [conductor]

Owner-settled decisions for Blastfile (mandatory for 0.2.0): (1) NO workers knob in Blastfile — scheduler owns parallelism, [route.php] workers is removed completely; (2) NO global top-level groups — trait <name> attaches only via explicit 'use <name>' inside a view; (3) NO pooler/cloud/infra knobs in Blastfile.
