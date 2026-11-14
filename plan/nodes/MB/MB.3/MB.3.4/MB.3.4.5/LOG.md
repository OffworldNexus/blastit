# Log MB.3.4.5

## 2026-11-14T12:36 [test-curator]

created (task) Apply the 8192-byte value rule inside lockdown::resolve so no public function returns an over-long CONTENT_TYPE

## 2026-11-14T13:00 [lead]

status ready -> active

## 2026-11-14T13:00 [lead]

attempt 1/3 started

## 2026-11-14T13:00 [lead]

base 612ecad

## 2026-11-14T13:02 [implementer]

move the 8192-byte CONTENT_TYPE cap into resolve, record the drop in build, add the boundary unit test (44/12/30/1); mutants 3 tolerated missed, 18 unviable

## 2026-11-14T13:04 [lead]

status active -> review

## 2026-11-14T13:09 [lead]

status review -> done

## 2026-11-14T13:09 [lead]

commit cad3b72
