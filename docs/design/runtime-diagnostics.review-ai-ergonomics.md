# ai-ergonomics review — `runtime` diagnostics — node MB.25.1 — round 2 — files: `docs/reference/log-events.md` (`runtime` row + `## The `runtime` line`), `docs/local.md` (`## Start` table + its one sentence), against `docs/VISION.md`

**Lens:** `ai-ergonomics`. **Round:** 2, re-review after the round-1 fixes. **Test unchanged:** an LLM agent implements MB.25.2 from these two pages alone, with no Rust/JSON encoder to read, and must emit the three JSON shapes and the three pretty lines, prove the `info`/`warn` split, and say when a second `runtime` line appears. A sentence survives only if the agent cannot produce a defensible different output. Frozen names are not re-litigated; no new page, fence or flag is requested.

## Round 1 findings and their resolution

- **MAJOR — absent vs `null`** — **resolved.** `docs/reference/log-events.md:100` now says ``A field a row does not list is absent from the object, never `null`.``, `:37` says "only the fields its row lists", and `:98` gives `static` its own `entrypoint`-only row. All three shapes are decidable.
- **MAJOR — `php.ini` label and `null` rendering** — **resolved.** `:102–103` names all three pretty labels and `php.ini none`; `:96` keeps `php_ini` as the field, so label and JSON key can no longer merge.
- **MAJOR — `warn` paragraph** — **still open (narrowed to one item).** `:105` now includes `unrestricted`, `:106` names "a `postgres` store no view or trait grants", and the criterion is softened to "something needs attention", so three of the four defects are gone; the `raw` item is still wrong — see Round 2 #1.
- **MINOR — "the one `warn` at boot"** — **resolved.** `:46` reads "`dev.reload.unavailable` is a `warn` at boot".
- **MINOR — "Boot names what it will run"** — **resolved.** `docs/local.md:18`: "Startup prints one `runtime` line naming what it will run:".
- **MINOR — `static` row "neither"** — **resolved.** `:98` now says "`entrypoint` only: no language runtime is loaded", which answers both the field and the "loaded" question.
- **MINOR — duplicated `php.ini` rule on `docs/local.md`** — **resolved.** `docs/local.md:26` keeps one sentence, and the `PHPRC`/`PHP_INI_SCAN_DIR` rule now exists only at `:100–101` with one verb ("strips").
- **MINOR — `component` value unnamed** — **resolved.** `:89` names `(component: "init")` explicitly.

## Round 2 findings

### MAJOR — the `raw` warn names the shape that is a production refusal and omits the shape that warns

`docs/reference/log-events.md:105–107`

> `warn` means something needs attention: `manifest.warning` (`unrestricted`, `egress any`, a `postgres` store no view or trait grants, an unacknowledged `raw` body, an unused `trait`, an `entrypoint php` that no view reaches) and `dev.reload.unavailable`.

Why it confuses an agent: this page opens with "In server mode stdout is the only output" (`:3`), and on that path an unacknowledged `body raw` never becomes a `warn` — `crates/blastit-manifest/src/validate.rs:1048–1058` makes it a **boot refusal**, and `docs/reference/blastfile.md:215` says the production boot "refuses the file". The shape that does warn at boot is the acknowledged one: `docs/reference/blastfile.md:216` — "`body raw unvalidated` … production serves it with a **boot warning**" — while `blastit dev` warns for both shapes (`validate.rs:1553–1568`, "`dev` keeps the warning for both raw shapes (acknowledged or not)"). An agent building the info/warn oracle from this reference therefore asserts "`body raw unvalidated` boots clean", the exact inverse of the neighbouring page's contract, and its only escape is to weaken the test or demote the warning — the reclassification trap round 1 flagged. It also plants the wrong fixture: a manifest that refuses to boot is used as a "warning" fixture.

Concrete replacement (one sentence, same list, one item corrected):

> `warn` means something needs attention: `manifest.warning` (`unrestricted`, `egress any`, a `postgres` store that no view or trait grants, a `raw` body — the acknowledged `body raw unvalidated` warns, the unacknowledged one refuses a production boot — an unused `trait`, an `entrypoint php` that no view reaches) and `dev.reload.unavailable`.

### MINOR — the second `runtime` line is not scoped to `blastit dev`

`docs/reference/log-events.md:89–92`

> one line each time the entrypoint resolves: at boot in `blastit dev` and in server mode, and again after a `Blastfile` change that alters it, so the latest line names what is serving.

Why it confuses an agent: the same page says "production has no reload at all" (`:50`), so the only mode that can re-resolve after a `Blastfile` edit is dev, yet the unqualified "again after a `Blastfile` change" sits immediately after "and in server mode". An agent choosing a cadence test has two defensible readings — second line in dev only (correct), or a second line expected during a production run where no watcher exists and the test hangs or fails. The governing clause ("each time the entrypoint resolves") lets a careful agent recover, hence MINOR, but the stated audience is the cheap implementer, and this reviewer's brief is exactly "know when a second `runtime` line appears".

Concrete replacement:

> one line each time the entrypoint resolves: at boot in `blastit dev` and in server mode, and in `blastit dev` again after a `Blastfile` change that alters it, so the latest line names what is serving.

### MINOR — the reference pins the labels but not the unlabeled `version`, the separator, or the `static` summary

`docs/reference/log-events.md:102–103`

> `pretty` labels the fields `libphp`, `php.ini` and `environment`, and prints `php.ini none` when `php_ini` is `null`.

Why it confuses an agent: the sentence enumerates the *labeled* fields and stops. It never says `version` is printed at all, nor where, nor that segments are separated by two spaces. `php 8.5.4` and `static  serving static assets only` exist only as cells on the other page (`docs/local.md:22–24`). An implementer working from the reference alone can defensibly emit `php  libphp …` (no version) or `runtime     static` (no summary), both of which fail the `docs/local.md` golden line — the same class as round-1 MAJOR #2, one field further along: the one field with no label is the one left unpinned.

Concrete replacement:

> `pretty` prints `<entrypoint> <version>`, then the labels `libphp`, `php.ini` and `environment`; it prints `php.ini none` when `php_ini` is `null`, and `static` as `static  serving static assets only`.

### MINOR — the page's only `warn` definition sits under the `runtime` heading and never says `runtime` is never `warn`

`docs/reference/log-events.md:89–92` and `:105–108`

> `runtime` is `info` (`component: "init"`) … A resolved runtime is not a problem.
>
> `warn` means something needs attention: `manifest.warning` (… an `entrypoint php` that no view reaches) …

Why it confuses an agent: the sole definition of `warn` on the page is inside `## The `runtime` line`, and one of its items is a condition about the entrypoint itself. An agent can hang the "`entrypoint php` that no view reaches" warning on the `runtime` event at level `warn` ("something needs attention"), contradicting the frozen `level: info`. The named events in the colon list (`manifest.warning`, `dev.reload.unavailable`) and "is not a problem" make `info` recoverable, so this is MINOR, but two words would remove the guess.

Concrete replacement:

> `runtime` is always `info`; the separate `warn` events are `manifest.warning` (`unrestricted`, `egress any`, a `postgres` store that no view or trait grants, a `raw` body — the acknowledged `body raw unvalidated` warns, the unacknowledged one refuses a production boot — an unused `trait`, an `entrypoint php` that no view reaches) and `dev.reload.unavailable`.

### Verified with no finding

- `:37` + `:94–100`: present vs absent vs `null` is decidable for all three entrypoints; `php_ini` is the only nullable field and it is listed only on the `php` row.
- `:89` pins `component: "init"`, level `info` and the `runtime` event name; `:37` enumerates `php`/`python`/`static`; the JSON envelope shape is available from the example at `:22` and the rule at `:15`.
- `:96` (`php_ini`) vs `:102` (`php.ini`): no reading makes `php.ini` a JSON key, because the sentence explicitly scopes it to `pretty`.
- The remaining warns map to real `ManifestWarning` variants in `crates/blastit-manifest/src/validate.rs:961–1043`: `unrestricted` (`UnrestrictedRoute`), `egress any` (`EgressRoute`), an ungranted store (`UngrantedProxy`), an unused `trait` (`UnattachedGroup`), an unreached PHP entrypoint (`EntrypointNotReached`).
- `:46` now says "a `warn` at boot", so no test learns "exactly one warn at boot"; `:49–50` still keeps `dev.reload` info-only and production reload-free.
- Pretty lines at `docs/local.md:22–24` are 150/116/89 characters including the table-cell wrappers, `INFO` is followed by two spaces, and `runtime` occupies 12 columns before the summary — no format defect.
- Neither page asks for a new page, nav entry, fence, flag, `shim`, retired filename, second PHP transport, or `inotify`/`fanotify`.

## Round 2 findings and their resolution

- **MAJOR — `raw` warn named the refused shape** — **resolved.** `docs/reference/log-events.md:107–110` now reads "a `raw` body — both shapes in `blastit dev`, only the acknowledged `body raw unvalidated` in production", matching `crates/blastit-manifest/src/validate.rs:1553–1568` (`dev` warns for both shapes, production for the acknowledged one only) and `docs/reference/blastfile.md:215–216`. The inverse assertion and the "`body raw unvalidated` boots clean" trap are gone; the sentence does not restate the production refusal for the unacknowledged shape, but it makes no false claim and the info/warn oracle is now correct.
- **MINOR — second `runtime` line unscoped** — **resolved.** `:89–92`: "…and in `blastit dev` again after a `Blastfile` change that alters it, so the latest line names what is serving."
- **MINOR — pretty contract (`version`, separator, `static`)** — **resolved.** `:102–105` now prints the entrypoint and `version`, names the three labels "two spaces apart", keeps `php.ini none`, and pins `static` to `serving static assets only`, matching the golden lines at `docs/local.md:22–24`. One residual MINOR, accepted as non-blocking: "two spaces apart" attaches to the labelled fields, so the single space in `php 8.5.4` (`docs/local.md:22`) remains implicit in the reference.
- **MINOR — `warn` ownership** — **resolved.** `:107` opens "`runtime` is never `warn`: `warn` is a separate event and means something needs attention — …", so the "`entrypoint php` that no view reaches" item cannot be hung on the `runtime` event at `warn` any more.

VERDICT: PASS
