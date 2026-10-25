"""Write a PEP 503 simple index for the wheels attached to a GitHub release.

    simple_index.py ASSETS.jsonl OUT_DIR

ASSETS.jsonl holds one release asset per line, from
`gh api --paginate repos/<owner>/<repo>/releases/<id>/assets --jq '.[]'` (the
release object itself only embeds the first page). Each asset's SHA-256 comes
from GitHub's own `digest` field, so nothing is downloaded.
"""

import html
import json
import pathlib
import sys

assets = [json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]
out = pathlib.Path(sys.argv[2])

links = []
for asset in sorted(assets, key=lambda a: a["name"]):
    name = asset["name"]
    if not name.endswith(".whl"):
        continue
    digest = asset.get("digest") or ""
    fragment = "#" + digest.replace(":", "=", 1) if digest.startswith("sha256:") else ""
    url = html.escape(asset["browser_download_url"] + fragment)
    links.append(f'<a href="{url}" data-requires-python="&gt;=3.13">{html.escape(name)}</a><br/>')

if not links:
    sys.exit("the release has no wheels")

page = "<!DOCTYPE html>\n<html><head><title>{}</title></head><body>\n{}\n</body></html>\n"
(out / "simple" / "blastit").mkdir(parents=True, exist_ok=True)
(out / "simple" / "index.html").write_text(page.format("Simple index", '<a href="blastit/">blastit</a><br/>'))
(out / "simple" / "blastit" / "index.html").write_text(page.format("Links for blastit", "\n".join(links)))
(out / "index.html").write_text(
    page.format("blastit", '<a href="simple/">simple/</a> — the package index of '
                '<a href="https://github.com/OffworldNexus/blastit">blastit</a>')
)
print(f"{len(links)} wheel(s) listed")
