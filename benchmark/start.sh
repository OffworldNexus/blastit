#!/bin/sh
# Start the server named by BENCH_SERVER. The concurrency shape is
# BENCH_WORKERS x BENCH_THREADS (x BENCH_WORKER_CLASS for gunicorn), so the grid
# search can address any point without a new flag per shape. The image's
# ENTRYPOINT.
#
# `BENCH_WORKERS=1 BENCH_THREADS=4` is one process with four request threads.
# `BENCH_WORKERS=4 BENCH_THREADS=1` is four processes of one thread. blastit
# serves a route from a single process, so it always runs one process and is
# given BENCH_WORKERS*BENCH_THREADS request threads; every result records the
# shape each server actually ran.
set -eu

: "${BENCH_SERVER:?BENCH_SERVER is required}"
BENCH_WORKERS="${BENCH_WORKERS:-1}"
BENCH_THREADS="${BENCH_THREADS:-1}"
BENCH_WORKER_CLASS="${BENCH_WORKER_CLASS:-gthread}"
PORT="${PORT:-8080}"
APP="${BENCH_APP:-app:application}"
CONCURRENCY=$((BENCH_WORKERS * BENCH_THREADS))

# The app's connection pool is *per process*, so it is sized to the concurrency
# one process can offer, not to the total: granian and gunicorn have one pool per
# worker, blastit has a single process holding every request thread. `app.py`
# reads BENCH_POOL_MAX.
case "$BENCH_SERVER" in
  blastit) PER_PROC=$CONCURRENCY ;;
  granian) PER_PROC="$BENCH_THREADS" ;;
  gunicorn)
    if [ "$BENCH_WORKER_CLASS" = "gthread" ]; then PER_PROC="$BENCH_THREADS"; else PER_PROC=1; fi
    ;;
  *) PER_PROC=$CONCURRENCY ;;
esac
export BENCH_POOL_MAX="${BENCH_POOL_MAX:-$((PER_PROC + 2))}"
export DATABASE_URL="${DATABASE_URL:?DATABASE_URL is required}"

case "$BENCH_SERVER" in
  blastit)
    export BLASTIT_APP="$APP"
    export BLASTIT_MANIFEST="${BLASTIT_MANIFEST:-/app/blastit.toml}"
    export BLASTIT_MAX_WORKERS="$CONCURRENCY"
    export BLASTIT_VIEW_MAX_WORKERS="$CONCURRENCY"
    # The admission queue is bounded (256 by default). With hundreds of client
    # connections blastit would answer 503 rather than queue, and a shed 503 is
    # cheap, so load-shedding would masquerade as throughput and score better
    # than serving. Size it so blastit queues like the multi-process servers.
    export BLASTIT_QUEUE_SIZE="${BLASTIT_QUEUE_SIZE:-65536}"
    export BLASTIT_QUEUE_TIMEOUT="${BLASTIT_QUEUE_TIMEOUT:-120s}"
    export PORT
    exec blastit
    ;;
  granian)
    exec granian --interface wsgi --http 1 --no-ws \
      --host 0.0.0.0 --port "$PORT" \
      --workers "$BENCH_WORKERS" --blocking-threads "$BENCH_THREADS" "$APP"
    ;;
  gunicorn)
    if [ "$BENCH_WORKER_CLASS" = "gthread" ]; then
      exec gunicorn --bind "0.0.0.0:$PORT" --workers "$BENCH_WORKERS" \
        --threads "$BENCH_THREADS" --worker-class gthread "$APP"
    else
      exec gunicorn --bind "0.0.0.0:$PORT" --workers "$BENCH_WORKERS" \
        --worker-class "$BENCH_WORKER_CLASS" "$APP"
    fi
    ;;
  *)
    echo "unknown BENCH_SERVER: $BENCH_SERVER" >&2
    exit 2
    ;;
esac
