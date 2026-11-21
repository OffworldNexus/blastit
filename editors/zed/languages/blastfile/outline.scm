; `@item` + `@name` are Zed's required outline captures: cmd-shift-o becomes a
; section navigator. A named declaration contributes its name; an unnamed
; `postgres $DATABASE_URL` renders the declaration's own text.
(app_decl) @item
(entrypoint_decl) @item
(entrypoint_decl (string) @name)
(store_decl) @item
(store_decl (name) @name)
(trait_decl) @item
(trait_decl (name) @name)
(view_decl) @item
(view_decl (name) @name)
(static_decl) @item
(static_decl (string) @name)
