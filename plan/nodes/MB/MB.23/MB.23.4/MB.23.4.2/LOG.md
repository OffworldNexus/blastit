# Log MB.23.4.2

## 2026-11-17T11:17 [planner]

created (task) The PHP loaded-file sink: EG(included_files) between execute and request_shutdown, optional dlsym sink with no ABI bump

## 2026-11-17T11:53 [lead]

status ready -> active

## 2026-11-17T11:53 [lead]

attempt 1/3 started

## 2026-11-17T11:53 [lead]

base 95b0db4

## 2026-11-17T12:14 [lead]

implementer done: optional dlsym sink (never RTLD_DEFAULT), EG(included_files) snapshotted before request_shutdown so bailouts report, bounded FileSink + compiled->source map, 6-leg protected critical_file_sink, Makefile/Dockerfile third suite. make check 175 ok; rs-test-php 6+9+27+6.

## 2026-11-17T12:14 [lead]

status active -> review

## 2026-11-17T12:35 [lead]

polish: sink moved into a loader-owned Box before C sees its address (NoSink::NotInitialised/Absent/Refused replaces the conflating bool), memo cap now real, vacuous assert replaced, Makefile wording. make check 175 ok; rs-test-php 6/9/27/8. Committing.

## 2026-11-17T12:35 [lead]

status review -> done
