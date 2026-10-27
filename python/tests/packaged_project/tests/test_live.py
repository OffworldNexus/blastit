"""What a real blastit serves, in the layout `startproject` writes (D3c).

The project is *not* installed and the working directory is the only thing that
makes `myshop` importable, which is what a project this shape has to be able to
rely on: the server runs in the project root and blastit puts that root first on
`sys.path` (§5.3).

One request is enough, and it is the whole point: if the fixture runs the server
in the settings module's own directory, or hands `BLASTIT_APP` the dotted
`WSGI_APPLICATION` Django writes, this test is the one that fails.
"""

from urllib.request import urlopen


def test_the_packaged_project_is_served(blastit_live_server) -> None:
    """The route the manifest grants answers, through a real blastit."""
    with urlopen(f"{blastit_live_server.url}/packaged/") as response:  # noqa: S310 - 127.0.0.1
        assert response.status == 200
        assert response.read() == b"ok"
