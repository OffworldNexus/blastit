# The benchmark image: the app and the three servers, on one interpreter.
#
# It installs a blastit wheel built beforehand (`scripts/build-wheel.sh`, which
# puts the dbproxy binary in the wheel too) exactly the way a project would, then
# granian and gunicorn next to it. One image for all three servers, so the only
# difference between a blastit run and a gunicorn run is the server.
#
# Build context: the repository root.
#   scripts/build-wheel.sh && docker compose -f benchmark/compose.yaml build
FROM python:3.13-slim-trixie

COPY benchmark/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt

COPY dist/blastit-*-cp313-*.whl /tmp/dist/
RUN set -e; \
    wheels="$(ls /tmp/dist/*.whl)"; \
    count="$(printf '%s\n' "$wheels" | wc -l)"; \
    if [ "$count" != 1 ]; then \
        echo "benchmark/Dockerfile wants exactly one cp313 wheel in dist/, found $count:" >&2; \
        printf '%s\n' "$wheels" >&2; \
        exit 1; \
    fi; \
    pip install --no-cache-dir "$wheels"; \
    rm -rf /tmp/dist

COPY benchmark/app.py benchmark/seed.py benchmark/blastit.toml benchmark/start.sh /app/
RUN chmod +x /app/start.sh

WORKDIR /app
ENV PYTHONUNBUFFERED=1 \
    BLASTIT_LOG_FORMAT=json \
    PORT=8080
EXPOSE 8080
ENTRYPOINT ["/app/start.sh"]
