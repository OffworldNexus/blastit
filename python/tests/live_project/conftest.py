"""A project that defines its own `live_server` keeps it (D3c).

`--blastit-live-server` registers blastit's server under the name pytest-django
uses, but it is registered at `pytest_configure`, and a fixture from a project's
own `conftest.py` is collected later: the project wins. That is the behaviour
worth pinning down — the option is a convenience for a suite that already uses
`live_server`, never something that takes a project's own fixture away from it.

The fixture is only defined when `BLASTIT_TEST_OWN_LIVE_SERVER` is set, because
the suite that tests the option needs `live_server` to be blastit's; one test
of the two behaviours then asks for this one by setting it.
"""

import os

import pytest


if os.environ.get("BLASTIT_TEST_OWN_LIVE_SERVER"):

    @pytest.fixture
    def live_server() -> object:
        """A project's own live server: a sentinel, not blastit, not a server."""

        class _OwnLiveServer:
            url = "http://127.0.0.1:1/"
            host = "127.0.0.1"
            port = 1

            # pytest-django's autouse `_live_server_helper` enables and disables
            # an `ALLOWED_HOSTS` override on whatever `live_server` yields, so
            # anything standing in for it has to carry one.
            from django.test.utils import modify_settings

            _live_server_modified_settings = modify_settings(
                ALLOWED_HOSTS={"append": "localhost"}
            )

        return _OwnLiveServer()
