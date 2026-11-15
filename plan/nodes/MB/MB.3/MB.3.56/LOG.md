# Log MB.3.56

## 2026-11-15T10:05 [lead]

created (task) Pin [route.php] entry by (st_dev, st_ino) at boot: the ADR wins over the gap (H-007)

## 2026-11-15T10:11 [lead]

CITATION CORRECTION (the decision does not change): MB.3.7's reviewer checked where the entry-pin claim actually lives. ADR-0010 §3 claims `(path, st_dev, st_ino)` recording + stage-1 equality for `libphp` and the shim only — it makes NO pin claim for `[route.php] entry`. The claim comes from `crates/blastit/src/php_view.rs:89` ("Absolute `SCRIPT_FILENAME`, pinned at boot (`[route.php] entry`)") plus MB.3.7's node §7.3. So the gap is a code doc comment that is false today, and the pin is now the way to make it true. The owner's reasoning stands on its own merits: consistency with the pair, and a process able to write `manifest_dir` swapping the executed script between requests is a different trust boundary. H-006 was a duplicate of H-007 and is closed.
