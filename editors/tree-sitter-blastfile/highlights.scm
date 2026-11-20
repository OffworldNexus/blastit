; Blastfile pattern highlights: the two spellings `immutable` and a `view`
; take. One grammar serves both surfaces, so `immutable "/assets/*"`,
; `immutable regex '…'` and `view v GET "/x", regex '/v[0-9]+/items'` all
; colour as patterns instead of falling back to a name and a loose string.
;
; Zed installs its own copy at editors/zed/languages/blastfile/highlights.scm;
; this one travels with the grammar package so any other consumer of
; src/node-types.json sees the same captures. Every capture below is inside
; Zed's fifteen-name theme set (docs/design/blastfile-lsp-zed.md §10.1), and
; every pattern below also appears in Zed's copy, so the two render identically
; -- `make zed-check` asserts that containment (a subset, not an equal copy:
; Zed's file styles more than the grammar package needs), because a second copy
; is exactly how the two drift apart.

; `"..."` -- a quoted pattern (and every other double-quoted value).
(string) @string
(string (escape) @string.escape)

; `regex '...'` -- the raw escape hatch, never confused with a plain raw string.
(regex_raw_string) @string.regex

; `'...'` -- byte-verbatim: every value except a `query`'s, plus a regex body
; (which has its own node above). A `query '...'` is `sql_string` below, because
; `query` left the shared `keyword` rule and took its value with it.
(raw_string) @string.special

; `query` is a rule of its own now, so its word is captured by parent node.
(query_decl "query" @keyword)

; The SQL inside `query '...'`: the wrapper first, then the two tokens the
; reference page explains. `sql_word`, `sql_number`, `sql_operator` and
; `sql_other` are deliberately uncaptured (docs/design/blastfile-lsp-zed.md
; §10.1): `number` and `operator` are not among the fifteen names the bundled
; themes style. They inherit `@string.special` from the wrapper, which is the
; right colour for a word. `$?`/`$n`/`...` take `$IDENT`'s right-to-left
; fallback pair, in that order (Ayu has no `variable.special`).
(sql_string) @string.special
(sql_keyword) @keyword
(sql_parameter) @variable.special @variable
(sql_ellipsis) @variable.special @variable
