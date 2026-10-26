"""The plugin's own tests run the *inner* projects, which pytest must not collect."""

# `python/tests/project` is a Django project, not part of this suite: it is
# copied into a temporary directory and run in a subprocess by
# `test_pytest_plugin.py`. `python/tests/live_project` is the same thing for the
# live server (D3c), run by `test_live_server.py`.
collect_ignore = ["project", "live_project"]
