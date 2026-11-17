# Log MB.25.2

## 2026-11-17T01:30 [planner]

created (task) Emit the INFO runtime event: PHP version + php_ini_opened_path via the C SAPI, Python version + environment, static-only summary; retire the WARN line

## 2026-11-17T21:09 [lead]

status ready -> active

## 2026-11-17T21:09 [lead]

attempt 1/3 started

## 2026-11-17T21:09 [lead]

base 4ee740d

## 2026-11-17T21:47 [lead]

implementer done: closed Runtime enum (php/python/static) on all four surfaces, the WARN abuse and php_runtime_line deleted, php_ini_opened_path surfaced through one additive shim accessor with ABI still 1, three real blastit dev tails (php 8.4.27 + /etc/php/8.4/embed/php.ini, python 3.14.10 + venv, static). make check EXIT=0 180 ok. Open: no python line in a cargo build (stage1 emit_preload, MB.24.x) and a reload re-emits the php line.

## 2026-11-17T21:47 [lead]

status active -> review

## 2026-11-17T22:11 [lead]

polish: hang-probe legs widened (250->2000 ms, 20->30 s wait, 20 s refusal bound) with every containment assertion kept -- that flake reproduced at the base commit and has been noisy the whole chain; the RTLD_NOW claim narrowed to a pre-addition shim; the forbid(unsafe_code) claim corrected; the 8.5 acceptance scoped to PHP-free hosts with the direct fail-closed probe as evidence. make check EXIT=0, hang leg ran twice.

## 2026-11-17T22:11 [lead]

status review -> done
