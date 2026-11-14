# Log MB.3.5.2

## 2026-11-14T10:51 [planner]

created (task) SocketProbe, boot probes and the manifest reference entry - design MB.3.5 part 2

## 2026-11-14T13:19 [lead]

DO NOT START until MB.3.9.2 and MB.3.9.3 land: this node's brief derives from the old grammar (user-authored listen, DevProbe::check = 'socket must already exist/is S_ISSOCK/connectable'). Under the 2026-11-14 owner decision blastit owns the socket, so check_fastcgi_entries' rule set and DevProbe::check become 'the run dir is blastit's to create; the leaf is absent or stale'. implementer: read MB.3.9.2's amended 4.1 + MB.3.9.3's amended 4.8.1 first; if they are not yet on disk, report blocked rather than implementing the stale rules.
