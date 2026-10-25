"""blastit: a sandboxing WSGI server.

The package is the console script (`blastit`, `python -m blastit`) and the
compiled extension behind it (`blastit._blastit`). Importing it starts nothing —
no thread, no interpreter, no file — which is what the console script needs:
`blastit.__main__.main()` is what starts the server, on the interpreter the
wheel was installed into.
"""

__all__: list[str] = []
