; Blastfile pattern highlights: the two spellings `immutable` and a `view`
; take. One grammar serves both surfaces, so `immutable "/assets/*"`,
; `immutable regex '…'` and `view v GET "/x", regex '/v[0-9]+/items'` all
; colour as patterns instead of falling back to a name and a loose string.
;
; Zed installs its own copy at editors/zed/languages/blastfile/highlights.scm;
; this one travels with the grammar package so any other consumer of
; src/node-types.json sees the same captures. Every capture below is inside
; Zed's fifteen-name theme set (docs/design/blastfile-lsp-zed.md §10.1), so the
; two copies render identically.

; `"..."` -- a quoted pattern (and every other double-quoted value).
(string) @string
(string (escape) @string.escape)

; `regex '...'` -- the raw escape hatch, never confused with a plain raw string.
(regex_raw_string) @string.regex

; `'...'` -- byte-verbatim: a regex body, or `query 'SELECT ...'`.
(raw_string) @string.special
