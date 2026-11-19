"""blastit: a sandboxing WSGI server.

The package is the console script (`blastit`, `python -m blastit`) and the
pure-stdlib bridge to the universal binary it ships (`blastit._blastit`).
Importing it starts nothing — no thread, no child, no file — which is what the
console script needs: `blastit.__main__.main()` is what hands the command line to
the binary.

It also imports no extension, which is what makes one `py3-none-*` wheel work on
every CPython `>=3.12`: `blastit._blastit` is Python that speaks NDJSON to
`blastit --internal-json`, and the pytest gate's ingress probe is its own
stdlib twin (`MB.42.2.2.2`).
"""

__all__: list[str] = []
