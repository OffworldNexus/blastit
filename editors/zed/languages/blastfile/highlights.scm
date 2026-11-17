; Blastfile highlights, written against Zed's verified theme-capture set:
; fourteen captures that every bundled theme styles, plus variable.special as
; the first half of the right-to-left fallback pair on `$IDENT` (Ayu has no
; variable.special). Do not add a builtin- or method-suffixed capture here:
; those are not universal across the bundled themes.

; --- sections: literal words, which are unambiguous only at section level
["app" "entrypoint" "postgres" "trait" "view" "static"] @keyword

; --- directives and grants, captured by parent so a view named `view` stays a name
(directive (keyword) @keyword)
(use_directive "use" @keyword)
(store_decl "postgres" @keyword)
(grant "postgres" @keyword)
(mode (keyword) @constant)
(grant (keyword) @constant)
(table_grant (keyword) @function)	; select, insert, update, delete

; --- literals
(codec) @constant
(language) @type
(method) @attribute
(string) @string
(string (escape) @string.escape)
(raw_string) @string.special
(regex_raw_string) @string.regex
(env_var) @variable.special @variable

; --- names
(view_decl (name) @function)
(trait_decl (name) @type)
(store_decl (name) @type)
(grant (name) @type)
(use_directive (name) @type)
(table) @property
(column) @property
(dotted_name) @property

; --- punctuation and comments
["{" "}" "(" ")"] @punctuation.bracket
["," ";"] @punctuation.delimiter
(comment) @comment
