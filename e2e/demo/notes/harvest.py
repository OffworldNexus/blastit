"""Opt-in SQL harvesting (DEMO_HARVEST=<file>), used to write the manifest's grants.

Outside blastit only. Records each statement with the request's method and
path, as psycopg sends it with server-side binding (``%s`` → ``$n``).
"""

import json
import os
import re
import threading

from django.db import connection

_lock = threading.Lock()


def _to_wire(sql):
    n = 0

    def repl(m):
        nonlocal n
        if m.group(0) == "%%":
            return "%"
        n += 1
        return f"${n}"

    return re.sub(r"%%|%s", repl, sql)


class HarvestMiddleware:
    def __init__(self, get_response):
        self.get_response = get_response
        self.path = os.environ.get("DEMO_HARVEST")

    def __call__(self, request):
        if not self.path:
            return self.get_response(request)
        seen = []

        def wrapper(execute, sql, params, many, context):
            seen.append(_to_wire(sql) if params is not None else sql)
            return execute(sql, params, many, context)

        with connection.execute_wrapper(wrapper):
            response = self.get_response(request)
        with _lock, open(self.path, "a") as f:
            for sql in seen:
                f.write(json.dumps({"method": request.method, "path": request.path_info, "sql": sql}) + "\n")
        return response
