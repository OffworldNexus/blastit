# Log MB.9.2.2

## 2026-11-14T03:37 [planner]

created (task) StaticUpstream adapter and deletion of the static dispatch branches - design MB.9.2b

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T21:04 [lead]

RELEASE GATE (2026-11-14, from MB.3.10 review): the / EntryScript 404 gate in this node's  is now load-bearing documentation. The static responder currently serves a .php file under a [route.static] root as source (no gate in static_asset.rs/rewrite.rs), so MB.3.10 cannot claim 'entry is never served'; the PHP page says only 'only entry is executed' and keeps sources out of root. This node MUST land before 0.2.0 ships; MB.3.13.3 makes a php route servable and exposes it. Also: its Context cites docs/design/php-fastcgi.md, which MB.3.32 deletes — MB.3.31 repoints live briefs to backend-runtimes.md §4.2/ADR-0009.

## 2026-11-14T21:04 [lead]

Correction to the line above (shell ate the backticked names): the `.php`/`.phar` EntryScript 404 gate in this node`s `plan` is the release-gated claim. Yes.

## 2026-11-14T21:10 [lead]

SUPERSEDED IN PART (2026-11-14): the EntryScript behaviour lands EARLY as MB.3.33 (surgical, in the current static_asset/rewrite path, with protected critical_static_entry) because this node is blocked behind MB.9.2.1.3<-1.2<-1.1<-MB.9.1.1<-MB.9.1.2. When you land, port/preserve that gate in plan and keep the cases (or move them to conformance with identical assertions). Do not regress to serving source.
