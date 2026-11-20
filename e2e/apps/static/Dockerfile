# The static leg's runtime: Debian trixie and nothing else.
#
# `blastit` is mounted from the artifact under test (no wheel, no rebuild), so
# this image is the proof the static half needs *no language runtime at all*:
# there is no CPython and no PHP here to fall back to, and the build fails if a
# base image ever grows one.
FROM debian:trixie-slim

RUN ! command -v python3 \
 && ! command -v python \
 && ! command -v php \
 && ! command -v php-cgi

CMD ["/bin/sh"]
