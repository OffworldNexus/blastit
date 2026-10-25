"""The plugin's own tests run the *inner* project, which pytest must not collect."""

# `python/tests/project` is a Django project, not part of this suite: it is
# copied into a temporary directory and run in a subprocess by
# `test_pytest_plugin.py`.
collect_ignore = ["project"]
