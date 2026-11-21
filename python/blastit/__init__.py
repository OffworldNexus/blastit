"""blastit: a sandboxing WSGI server.

The package is the console scripts (`blastit`, `blastit-lsp`, `python -m
blastit`) and a pure-stdlib launcher for the universal Rust binary it ships
(`blastit._blastit`). Importing it starts nothing — no thread, no child, no
file — which is what the console script needs: `blastit.__main__.main()` hands
the command line to that binary with one `execvpe`.

It imports no extension, which is what makes one `py3-none-*` wheel work on every
CPython `>=3.12`.
"""

__all__: list[str] = []
