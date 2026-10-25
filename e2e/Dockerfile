# The demo app served by blastit. Build context: the repository root.
#
# The builder and the runtime share the same Python, because blastit embeds
# CPython and links against its libpython.
FROM python:3.13-slim-trixie AS build
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH PYO3_PYTHON=/usr/local/bin/python3.13
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN --mount=type=cache,target=/root/.cargo/registry --mount=type=cache,target=/src/target \
    cargo build --release --locked -p blastit && cp target/release/blastit /usr/local/bin/blastit

FROM python:3.13-slim-trixie
COPY e2e/demo/requirements.txt /app/requirements.txt
RUN pip install --no-cache-dir -r /app/requirements.txt
COPY --from=build /usr/local/bin/blastit /usr/local/bin/blastit
COPY e2e/demo /app
WORKDIR /app
ENV PYTHONUNBUFFERED=1 \
    PYTHONPATH=/app \
    BLASTIT_APP=demo.wsgi:application \
    BLASTIT_PRELOAD=demo.urls,notes.views \
    BLASTIT_MANIFEST=/app/blastit.toml \
    PORT=8080
EXPOSE 8080
CMD ["blastit"]
