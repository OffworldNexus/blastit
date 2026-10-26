"""What a real blastit serves, from the outside (D3c).

Every test here drives a *process*: `blastit_live_server` starts the real
`blastit` binary on a free port, with this project's manifest and the test
database, and the requests below go over HTTP to it. Nothing in this file talks
to Django directly — which is the point: the traffic is what a browser, a
`requests` call or Playwright produces, and what the plugin cannot see from
inside its own process.

Three things are checked, and each one is a separate server, because the fixture
is per test:

* the granted route is served (the manifest is enough for real traffic);
* the ungranted route is refused, and its denial lands in the report as a
  missing grant;
* a path with no route is blastit's own 404, and lands in the report as
  `unrouted`.

The last test uses `live_server` rather than `blastit_live_server`: it only runs
when the outer session passed `--blastit-live-server`, which is what the plugin
does with that name.
"""

import pytest
from urllib.error import HTTPError
from urllib.request import urlopen


def test_the_granted_route_is_served(blastit_live_server) -> None:
    """The manifest grants `SELECT 1`, so the request works end to end."""
    with urlopen(f"{blastit_live_server.url}/live/") as response:  # noqa: S310 - 127.0.0.1
        assert response.status == 200
        assert response.read() == b"ok"


def test_the_ungranted_route_is_refused(blastit_live_server) -> None:
    """`SELECT 'denied'` is not in the manifest: dbproxy refuses it, the view fails."""
    with pytest.raises(HTTPError) as refused:
        urlopen(f"{blastit_live_server.url}/live/denied/")  # noqa: S310 - 127.0.0.1
    assert refused.value.code == 500


def test_a_path_with_no_route_is_blastits_own_404(blastit_live_server) -> None:
    """`/live/oops/` exists in Django; blastit answers 404 before Django sees it."""
    with pytest.raises(HTTPError) as refused:
        urlopen(f"{blastit_live_server.url}/live/oops/")  # noqa: S310 - 127.0.0.1
    assert refused.value.code == 404


def test_the_live_server_override_serves_live_server(live_server) -> None:
    """`--blastit-live-server`: pytest-django's name, blastit's policy.

    The proof is the refusal. pytest-django's own `live_server` runs the app in a
    thread of this process, where nothing checks the SQL, so `/live/denied/`
    would answer 200; blastit refuses the statement the manifest does not grant.
    """
    with pytest.raises(HTTPError) as refused:
        urlopen(f"{live_server.url}/live/denied/")  # noqa: S310 - 127.0.0.1
    assert refused.value.code == 500
