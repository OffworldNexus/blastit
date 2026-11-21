# Log MB.42.8

## 2026-11-19T23:19 [unknown]

created (task) Finish async-signal-safe fork cleanup: critical_child_sequence.rs forks then child::enter (allocates)

## 2026-11-20T06:22 [lead]

status proposed -> superseded

## 2026-11-20T06:22 [lead]

superseded -> MB.43 (proposed): deferred out of MB.42 so the release gate can close; the seccomp fork-in-libtest class is already fixed (MB.42.7), this is the last known site and passes today.
