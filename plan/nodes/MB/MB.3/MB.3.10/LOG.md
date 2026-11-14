# Log MB.3.10

## 2026-11-14T14:39 [lead]

created (task) Retarget the PHP page and reference onto [route.php] + libphp-embed + BLASTIT_LIBPHP (ADR-0010)

## 2026-11-14T15:14 [lead]

RETARGETED (owner 2026-11-14, 'No fallback please'): the docs retarget teaches [route.php] + libphp-embed + BLASTIT_LIBPHP ONLY. The fail-closed switch (BLASTIT_ALLOW_EXTERNAL_FASTCGI) and the [route.fastcgi] fallback note I added to this body earlier are SUPERSEDED by that decision -- do not document a switch whose removal is MB.3.17's. Still HELD: docs/frameworks/** is the architect's path while its lenses run.
