# Log MB.3.6.1.2

## 2026-11-14T10:51 [planner]

created (task) Workspace Cargo edges, Cargo.lock and the fastcgi-codec conformance driver - design MB.3.6a1 part 2

## 2026-11-14T15:14 [lead]

SUPERSEDED by owner decision 2026-11-14 ('No fallback please'): there is NO external FastCGI path. [route.fastcgi] stops existing as a key (unknown-key refusal, not a default-off switch); the BLASTIT_ALLOW_EXTERNAL_FASTCGI switch and every doc sentence sanctioning the fallback are deleted. Removal is MB.3.17 (depends MB.3.12). Kept: crates/blastit-php-sapi, PhpEmbedRuntime, view.chan framing, the Upstream/WorkerRuntime contract and MB.9.3 -- this removes one backend implementation, not the abstraction.

## 2026-11-14T15:14 [lead]

status ready -> superseded
