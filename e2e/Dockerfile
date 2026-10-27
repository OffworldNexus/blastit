# The demo app served by blastit. Build context: the repository root.
#
# The builder and the runtime share the same Python, because the wheel is built
# for the interpreter it will be installed into: the extension is not abi3, and
# it links no `libpython` — the interpreter that loads it provides the symbols.
# `scripts/build-wheel.sh` builds the Python-free `blastit-dbproxy` binary first
# and copies it into the wheel at `blastit/_bin/blastit-dbproxy` (§4).
FROM python:3.13-slim-trixie AS build
# `libclang` is a build dependency of `pg_query`: it generates the Rust bindings
# for PostgreSQL's own parser with bindgen (D7, §6). The manylinux images the
# published wheels are built in ship it; this one has to install it.
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates gcc libc6-dev clang libclang-dev \
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
# Only the registry is cached: the builder's `target` cache is shared between
# checkouts (and between agents working in parallel worktrees), and cargo's
# fingerprints are mtime-based, so an artifact compiled from *another* checkout
# can be considered fresh and the build then fails on a stale dependency. CI
# builds on a fresh builder anyway.
RUN --mount=type=cache,target=/root/.cargo/registry \
    scripts/build-wheel.sh

FROM python:3.13-slim-trixie
COPY e2e/demo/requirements.txt /app/requirements.txt
COPY e2e/sqlalchemy/requirements.txt /sqlalchemy/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt -r /sqlalchemy/requirements.txt
COPY --from=build /src/dist/*.whl /tmp/
RUN pip install --no-cache-dir /tmp/*.whl && rm -rf /tmp/*.whl
COPY e2e/demo /app
# The second app of the end-to-end test (E2b): Flask + SQLAlchemy, served from
# this directory so its own manifest and its own grants are what run it.
COPY e2e/sqlalchemy /sqlalchemy
WORKDIR /app
ENV PYTHONUNBUFFERED=1 \
    PYTHONPATH=/app \
    BLASTIT_APP=demo.wsgi:application \
    BLASTIT_PRELOAD=demo.urls,notes.views \
    BLASTIT_MANIFEST=/app/blastit.toml \
    PORT=8080
EXPOSE 8080
# The console script: it runs on the interpreter above, and stage 1 re-executes
# that same interpreter (§4).
CMD ["blastit"]
