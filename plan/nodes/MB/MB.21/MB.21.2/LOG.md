# Log MB.21.2

## 2026-11-16T00:38 [planner]

created (task) Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics

## 2026-11-16T00:40 [conductor]

Settled up front (not open): [route.php] workers is removed (scheduler owns parallelism; no workers directive in Blastfile), global top-level groups are removed (only explicit per-view 'use <trait>'), and pooler/vendor knobs are removed from the manifest.
