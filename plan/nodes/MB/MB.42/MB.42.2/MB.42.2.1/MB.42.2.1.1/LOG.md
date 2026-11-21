# Log MB.42.2.1.1

## 2026-11-19T17:21 [planner]

created (task) blastit_sys::pin + blastit-py-sapi (abi/discovery/loader), rewired blastit-view-php

## 2026-11-19T17:21 [lead]

status ready -> active

## 2026-11-19T17:21 [lead]

attempt 1/3 started

## 2026-11-19T17:21 [lead]

base 5b3ce8df52aed5b4954eefd7b5543e16e4bc6ec6

## 2026-11-19T17:47 [lead]

status active -> review

## 2026-11-19T17:50 [lead]

REVIEW FIX (filtered): MAJOR loader.rs duplicates blastit_sys::elf c_str/le_u32/le_u64 + 3rd .dynamic/.dynsym walk (extend elf with dynamic_strings/dynsym_names, delete copies in loader/host/view-php); dynsym entsize==0 -> 64M String DoS; st_shndx ignored so imports count as exports; hooks() outlives dlclose (UB); minor picked from dir names not pyvenv.cfg; pin_object still hand-formats /proc/self/fd; entry_identity dead+stat/lstat conflict; 19 refusal-text defects (NoShim tells apt users to run make; python3.14 hardcoded; relative-path misdiagnosis; packed 313 printed as CPython 313; two free-threaded wordings; /proc/self/fd/N in messages; Debug tuple)

## 2026-11-19T18:01 [lead]

status review -> done
