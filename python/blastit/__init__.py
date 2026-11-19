"""blastit: a sandboxing WSGI server.

The package is the console script (`blastit`, `python -m blastit`) and the
compiled extension behind it (`blastit._blastit`). Importing it starts nothing —
no thread, no interpreter, no file — which is what the console script needs:
`blastit.__main__.main()` is what starts the server, on the interpreter the
wheel was installed into.

It also imports no extension eagerly, which is what makes `import
blastit._check_pytest` safe on an interpreter whose CPython minor does not match
the staged `_blastit` binary: the pytest gate falls back to its pure-stdlib
ingress probe instead of the import raising (`MB.40.8`).
"""

__all__: list[str] = []
