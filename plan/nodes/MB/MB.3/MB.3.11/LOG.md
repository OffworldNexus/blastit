# Log MB.3.11

## 2026-11-14T14:39 [lead]

created (task) Feature-gate pyo3 (feature=python); decouple RawFrame transport and forking from the GIL

## 2026-11-14T14:39 [lead]

status ready -> active

## 2026-11-14T14:39 [lead]

attempt 1/3 started

## 2026-11-14T14:39 [lead]

base 4cd727b; ADR-0010 §4: standalone archive must ship php with python OFF (zero libpython). This leaf unblocks the shim (MB.3.12) and the Python-free binary; it owns Cargo.toml/Cargo.lock for the whole PHP subtree.
