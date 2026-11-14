#!/bin/sh
# Serve one `[route.static]` fixture with `blastit dev` inside the e2e image.
#
# `MB.2.7` drives the commands of `docs/frameworks/static-and-spa.md` against a
# real process. The fixture is mounted read-only at `$1`; it is copied to the
# writable `$2` so the decoys the page's threat list names — `.env`, `.git/config`
# and a symlink that leaves `root` — can exist without being committed verbatim
# (git refuses a nested `.git`, and a real `.env` would be ignored).
#
# `$3` is `decoys` for the page-exact site and `plain` for the `spa = false` one.
# Any further arguments go to `blastit dev` (`--host 0.0.0.0` for the service
# that is reached by name over the compose network).
#
# `env -u`: the e2e image's own `BLASTIT_APP`/`BLASTIT_PRELOAD` name the Django
# demo. An all-static manifest refuses a reachable app (`R13`), so the dev
# processes start without them; `BLASTIT_MANIFEST`/`PORT` come from compose.
set -eu

src=$1
dest=$2
mode=$3
shift 3

mkdir -p "$dest"
cp -a "$src"/. "$dest"/

if [ "$mode" = decoys ]; then
    cp "$src/dotenv" "$dest/public/.env"
    mkdir -p "$dest/public/.git"
    cp "$src/git-config" "$dest/public/.git/config"
fi

cd "$dest"
exec env -u BLASTIT_APP -u BLASTIT_PRELOAD blastit dev "$@"
