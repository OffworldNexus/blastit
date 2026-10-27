"""Create and fill the table the `/db` route reads.

Runs once, before any server, with the real credentials — the same way the e2e
suite runs its migrations outside blastit. The table is small enough to sit in
shared buffers, so the scenario measures the round trip and the proxy, not disk.
"""

from __future__ import annotations

import os

import psycopg

ROWS = int(os.environ.get("BENCH_SEED_ROWS", "1000"))

DDL = """
DROP TABLE IF EXISTS bench_item;
CREATE TABLE bench_item (
    id      serial PRIMARY KEY,
    name    text NOT NULL,
    payload text NOT NULL
);
"""


def main() -> None:
    """Create `bench_item` and fill it, on the direct (unpooled) endpoint."""
    with psycopg.connect(os.environ["DATABASE_URL"], autocommit=True) as conn:
        with conn.cursor() as cur:
            cur.execute(DDL)
            cur.executemany(
                "INSERT INTO bench_item (name, payload) VALUES (%s, %s)",
                [(f"item-{i}", "x" * 64) for i in range(ROWS)],
            )
    print(f"seeded {ROWS} rows")


if __name__ == "__main__":
    main()
