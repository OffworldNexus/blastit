# Log MB.56.4.3

## 2026-11-20T21:07 [lead]

created (task) e2e/run_packaging.py: deb/rpm/install.sh/wheel/composer

## 2026-11-20T22:27 [lead]

status ready -> active

## 2026-11-20T22:27 [lead]

attempt 1/3 started

## 2026-11-20T22:27 [lead]

base 8384cfd36e5a9b1ad600cb2edfcf3c98b6df1a65

## 2026-11-20T22:45 [lead]

status active -> done

## 2026-11-20T22:45 [lead]

commit c90b03d

## 2026-11-20T22:56 [lead]

REGRESSION FOUND AT THE GATE: e2e/universal_binary.py still points at e2e/php/fixture (deleted in MB.56.4.1) -> make check fails at e2e-universal with FileNotFoundError. Repoint the php leg at e2e/apps/php or drop the leg (run_apps.py covers it).

## 2026-11-20T22:56 [lead]

status done -> active

## 2026-11-20T22:57 [implementer]

status active -> done

## 2026-11-20T22:57 [lead]

status done -> done

## 2026-11-20T22:57 [lead]

GATE FIX: universal_binary php leg repointed at e2e/apps/php (BLASTIT_MANIFEST=Blastfile.php2shell, store-less); make e2e-universal 6/6; make check EXIT=0
