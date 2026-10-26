# The rootless end-to-end image (D6). Build context: the repository root.
#
# The install is the shipped one — the wheel, so the app's dependencies and the
# console script are the ones a project gets from `pip install blastit` — and the
# capability is given to a **dedicated copy of the interpreter** (B1):
#
#   * the shared interpreter stays capability-free, because *every* process of
#     the service uid can run it — an operator's `python manage.py`, a worker, a
#     second shell — and a file with file capabilities turns any of them into
#     init (`setuid(0)` and all);
#   * the copy is owned by that uid, mode `0500`, so nothing else can run it, and
#     it sits next to the original: CPython finds its stdlib and its venv from
#     the executable's own path;
#   * the children cannot use it either: their bounding set is empty and they set
#     `no_new_privs`, and the kernel refuses such an `execve` outright.
#
# Why not `--cap-add` on a `user:` container: Docker hands a non-root `--user`
# process **no** capabilities at all — `--cap-add`/`cap_drop` only set the
# container's bounding set, which is what the file capability may fill. Verified
# with `docker compose run` on this file's image: `CapPrm`, `CapEff` and `CapAmb`
# are all zero for `user:` with `cap_add`.
#
# The uid the server runs as (`BLASTIT_UID`, 10000 by default) must stay below
# `BLASTIT_UID_BASE` (20000, §2): the boot refuses a uid the layout hands to
# another process.
FROM python:3.13-slim-trixie AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates gcc libc6-dev clang libclang-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH \
    PYO3_PYTHON=/usr/local/bin/python3.13 \
    MATURIN=maturin
RUN pip install --no-cache-dir maturin
WORKDIR /src
COPY Cargo.toml Cargo.lock pyproject.toml README.md LICENSE ./
COPY crates crates
COPY python python
COPY scripts scripts
# Only the registry is cached, for the reason `e2e/Dockerfile` gives: cargo's
# fingerprints are mtime-based, and a `target/` shared between checkouts can be
# considered fresh while it is not.
RUN --mount=type=cache,target=/root/.cargo/registry \
    scripts/build-wheel.sh

FROM python:3.13-slim-trixie
COPY e2e/demo/requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt
# `setcap` is a build-time need; `getcap` is also what the driver prints with.
RUN apt-get update && apt-get install -y --no-install-recommends libcap2-bin \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/dist/*.whl /tmp/
RUN pip install --no-cache-dir /tmp/*.whl && rm -rf /tmp/*.whl
ARG BLASTIT_UID=10000
RUN useradd --uid "$BLASTIT_UID" --user-group --create-home --shell /usr/sbin/nologin blastit
# The dedicated copy of the interpreter, with the capabilities (B1). Order
# matters: `chown` clears a file capability, so nothing re-owns the file after
# the `setcap`, and the copy stays next to the original so CPython still finds
# its stdlib and site-packages (`sys.prefix` follows the executable's path).
RUN interpreter="$(readlink -f /usr/local/bin/python3.13)" \
    && cp "$interpreter" "${interpreter%/*}/python3.13-init" \
    && chown "$BLASTIT_UID:$BLASTIT_UID" "${interpreter%/*}/python3.13-init" \
    && chmod 0500 "${interpreter%/*}/python3.13-init" \
    && setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
         "${interpreter%/*}/python3.13-init" \
    && getcap "${interpreter%/*}/python3.13-init" && getcap "$interpreter" || true
COPY e2e/demo /app
WORKDIR /app
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONPATH=/app \
    BLASTIT_APP=demo.wsgi:application \
    BLASTIT_PRELOAD=demo.urls,notes.views \
    BLASTIT_MANIFEST=/app/blastit.toml \
    PORT=8080
EXPOSE 8080
# No `USER`: the container's user is the `user:` field of the compose service,
# which is where the non-root identity of this deployment is declared. The
# command is the *copy*: the shared interpreter stays capability-free.
CMD ["/usr/local/bin/python3.13-init", "-m", "blastit"]
