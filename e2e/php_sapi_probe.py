"""The real-load acceptance for `crates/blastit-php-sapi` (node MB.3.12).

    php_sapi_probe.py                 # build + readelf + three probe runs, exit 0/1
    php_sapi_probe.py --minor 8.4     # one minor is a build arg, not a rewrite
    php_sapi_probe.py --keep          # leave the image for debugging

The probe image's base distro and its four package names come from
`scripts/platform_matrix.py --leg shims --minor <m>`, never from a second list
here: no distro ships PHP 8.3, 8.4 *and* 8.5 (Ubuntu noble is 8.3, Debian
trixie is 8.4, Ubuntu 26.04 and Alpine 3.23 are 8.5), so `MB.3.18` runs this
driver once per leg.

Stdlib only, same shape as the other e2e drivers: build the image, run it,
parse the one JSON line the probe prints, and fail non-zero with the container
output on a mismatch. `make php-sapi-e2e` runs this; it is deliberately **not**
part of `make check` (Docker + a Debian PHP image), exactly like `e2e/`.

Four separate `docker run`s, because they answer different questions:

1. `readelf -d libblastit_php84-<arch>-<libc>.so` — the shim must carry **no**
   `DT_NEEDED` on `libphp`. Linking PHP into whatever loads the shim is the one
   thing that would silently link PHP into `init`, `http`, `scheduler` and
   `dbproxy`.
2. The probe — `dlopen(libphp, RTLD_NOW|RTLD_GLOBAL)`, then `dlopen(shim,
   RTLD_NOW|RTLD_LOCAL)`, six requests, one JSON line. It asserts the load
   order, the body, the head, the status, `cpu_ns`, the reserved-name drop, the
   pinned `allow_url_fopen`, and that `exit()` + a fatal leave the worker serving.
3. The same probe **under the real seccomp filter**: `run-probe.sh --sandbox`
   goes through `e2e/php-sapi/seccomp-exec`, which installs
   `SeccompRole::View { egress: false }` from `crates/blastit-sys` — the same
   `install()` a view worker calls (`MB.3.13`) — and execs the probe, which
   inherits it. This is the half the INI value cannot prove.
4. That filtered probe again with the INI control **removed**
   (`--egress allowed`, i.e. `allow_url_fopen=1`): the outbound attempt must
   still be refused by the kernel. A future INI refactor must not silently
   become the only control.

Which half proves what: `ini_get('allow_url_fopen')` proves the INI layer is
pinned; the *failure class* of each outbound attempt proves which layer stopped
it — `php_setting` (the wrapper was disabled), `network` (nothing listening, no
filter) or `sandbox` (`EPERM` from `socket(AF_INET)`, the kernel). `curl` and
`stream_socket_client` ignore `allow_url_fopen` entirely, which is why they are
the ones that separate the layers.
"""

import argparse
import json
import os
import platform
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
DOCKERFILE = os.path.join(HERE, "php-sapi", "Dockerfile")
DEFAULT_IMAGE = os.environ.get("PHP_SAPI_IMAGE", "blastit-php-sapi-e2e:local")
TIMEOUT = int(os.environ.get("PHP_SAPI_TIMEOUT", "1800"))

# The three runs of the same binary. `inet_socket_errno` is the layer that does
# not depend on PHP's wording: 0 = a plain `socket(AF_INET)` succeeded (no
# filter), 1 = `EPERM` (the real `View { egress: false }` filter is installed in
# the same process that then runs PHP). `classes` is what a given attempt's
# failure can actually prove; `must_fail` is the weaker claim every attempt owes.
EGRESS_CASES = (
    {
        "label": "no filter, egress denied (INI layer alone)",
        "args": (),
        "mode": "denied",
        "allow_url_fopen": "0",
        "inet_socket_errno": 0,
        "must_fail": ("fopen_http", "stream_socket", "curl"),
        "classes": {
            # The INI layer short-circuits the wrapper...
            "fopen_http": "php_setting",
            # ...and without a filter the other two reach `connect` and get
            # `ECONNREFUSED`: this is what makes the filtered run's difference
            # attributable to the filter rather than to port 9 being closed.
            "stream_socket": "network",
            "curl": "network",
        },
    },
    {
        "label": "real View{egress:false} filter, egress denied (both layers)",
        "args": ("--sandbox",),
        "mode": "denied",
        "allow_url_fopen": "0",
        "inet_socket_errno": 1,
        "must_fail": ("fopen_http", "stream_socket", "curl"),
        "classes": {
            # Still the INI layer: the wrapper is disabled before any socket.
            "fopen_http": "php_setting",
            # `stream_socket_client` reports "(Unknown error)" and curl reports
            # the same text as the unfiltered run when `socket()` itself fails,
            # so neither text can classify the refusal. The proof for these two
            # is `inet_socket_errno == 1` above, measured in this process.
        },
    },
    {
        "label": "real filter, INI control removed (allow_url_fopen=1)",
        "args": ("--sandbox", "--egress", "allowed"),
        "mode": "allowed",
        "allow_url_fopen": "1",
        "inet_socket_errno": 1,
        "must_fail": ("fopen_http", "stream_socket", "curl"),
        "classes": {},
    },
)


def docker(*args, **kwargs):
    """One `docker` call, text mode, never raising on a non-zero exit."""
    return subprocess.run(
        ["docker", *args], check=False, text=True, timeout=TIMEOUT, **kwargs
    )


def tail(text, lines=40):
    """The last `lines` lines of a command's output, for a failure report."""
    stripped = (text or "").strip().splitlines()
    if not stripped:
        return "  (no output)"
    return "\n".join(f"  {line}" for line in stripped[-lines:])


MATRIX = os.path.join(REPO, "scripts", "platform_matrix.py")


MACHINE = {
    "x86_64": "x86_64",
    "amd64": "x86_64",
    "aarch64": "aarch64",
    "arm64": "aarch64",
}


def leg(minor, libc="gnu", arch=None):
    """The one minor's build tuple, read from the matrix (MB.3.18).

    `scripts/platform_matrix.py` is the single place a leg is declared, and no
    single distro ships PHP 8.3, 8.4 *and* 8.5: the probe image for each minor is
    therefore a different base image with different package names. Reading them
    from the matrix is what keeps this driver from growing a second copy of that
    table — and from silently probing Debian trixie's 8.3, which does not exist.
    """
    result = subprocess.run(
        [
            sys.executable,
            MATRIX,
            "--leg",
            "shims",
            "--minor",
            minor,
            "--libc",
            libc,
            "--arch",
            arch or MACHINE.get(platform.machine(), platform.machine()),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise SystemExit(
            f"php-sapi-e2e: the matrix has no enabled shim leg for PHP {minor} on {libc}:\n"
            f"{tail(result.stdout + result.stderr)}"
        )
    fields = dict(
        line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
    )
    return fields


def build(image, minor, libc="gnu"):
    """Builds the image from the repo root; returns `(ok, report)`."""
    spec = leg(minor, libc)
    # `matrix.suites` decides whether this image compiles the two protected
    # suites at all (`MB.3.58` review): a leg whose pair the boot refuses would
    # only take their refusal branch, so building `blastit` there would prove
    # nothing. The musl legs carry the suites too since `MB.3.59`; the shim and
    # the probe are built either way.
    suites = "1" if spec.get("suites", "true") == "true" else "0"
    result = docker(
        "build",
        "-f",
        DOCKERFILE,
        "--build-arg",
        f"BASE={spec['base']}",
        "--build-arg",
        f"PHP_MINOR={minor}",
        "--build-arg",
        f"PHP_DEV={spec['dev']}",
        "--build-arg",
        f"PHP_EMBED={spec['embed']}",
        "--build-arg",
        f"PHP_CURL={spec.get('curl', spec['dev'].replace('-dev', '-curl'))}",
        "--build-arg",
        f"LIBPHP={spec['libphp']}",
        "--build-arg",
        f"PHP_SAPI_SUITES={suites}",
        "-t",
        image,
        REPO,
        capture_output=True,
    )
    if result.returncode != 0:
        return False, tail(result.stdout + result.stderr)
    return True, ""


def image_tuple(image):
    """The image's `<arch>-<libc>`, asked of `run-probe.sh --print-tuple`.

    Both halves come from the compiler that built the shim (`cc -dumpmachine`),
    exactly as `make php-sapi` derived them, so the `readelf` path and the probe
    cannot disagree and no `uname` guess is repeated here.
    """
    result = docker(
        "run",
        "--rm",
        image,
        "sh",
        "/src/e2e/php-sapi/run-probe.sh",
        "--print-tuple",
        capture_output=True,
    )
    tuple_ = (result.stdout or "").strip()
    if result.returncode != 0 or tuple_.count("-") != 1:
        return None, (
            f"cannot read <arch>-<libc> from {image}:\n"
            f"{tail(result.stdout + result.stderr)}"
        )
    return tuple_, ""


def shim_name(minor, tuple_):
    """`("8.4", "x86_64-gnu")` -> `libblastit_php84-x86_64-gnu.so`."""
    return f"libblastit_php{minor.replace('.', '')}-{tuple_}.so"


def check_linkage(image, minor, tuple_):
    """`readelf -d` on the shim: no `NEEDED` entry names libphp."""
    shim = f"target/php-sapi/{shim_name(minor, tuple_)}"
    result = docker("run", "--rm", image, "readelf", "-d", shim, capture_output=True)
    if result.returncode != 0:
        return (
            False,
            f"readelf -d {shim} failed:\n{tail(result.stdout + result.stderr)}",
        )
    needed = [line for line in result.stdout.splitlines() if "(NEEDED)" in line]
    offending = [line for line in needed if "libphp" in line]
    if offending:
        return False, (
            "the shim links libphp, which would link PHP into every binary that "
            "loads it:\n" + "\n".join(f"  {line.strip()}" for line in offending)
        )
    if not needed:
        return False, (
            f"{shim} has no DT_NEEDED at all; it must link libc (a static shim "
            "would not be loaded the way the worker loads it)"
        )
    return True, ", ".join(line.split("[")[-1].rstrip("]") for line in needed)


def run_probe(image, args):
    """Runs the probe (optionally under the seccomp harness); `(json, log)`.

    The script is named explicitly rather than appended to the image's `CMD`:
    `docker run <image> --sandbox` makes `--sandbox` the command, so the flag
    has to follow `sh /src/e2e/php-sapi/run-probe.sh`.
    """
    result = docker(
        "run",
        "--rm",
        image,
        "sh",
        "/src/e2e/php-sapi/run-probe.sh",
        *args,
        capture_output=True,
    )
    log = result.stdout + result.stderr
    if result.returncode != 0:
        return None, log
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        return (
            None,
            f"expected exactly one JSON line on stdout, got {len(lines)}:\n{log}",
        )
    try:
        return json.loads(lines[0]), log
    except json.JSONDecodeError as error:
        return None, f"stdout is not JSON ({error}):\n{log}"


def classify(attempt):
    """Which layer refused one outbound attempt.

    `errno`/`os_errno` are the strongest evidence: the seccomp filter makes
    `socket(AF_INET)` fail with `EPERM` (1), while an unsandboxed attempt reaches
    `connect` and gets `ECONNREFUSED` (111). The text checks are for the paths
    that lose the errno (`fopen` of a `http://` wrapper, and curl's message).
    """
    for key in ("os_errno", "errno"):
        if attempt.get(key) == 1:
            return "sandbox"
    text = str(attempt.get("error", "")).lower()
    if "allow_url_fopen" in text or "no suitable wrapper" in text:
        return "php_setting"
    if "operation not permitted" in text or "permission denied" in text:
        return "sandbox"
    for key in ("os_errno", "errno"):
        if attempt.get(key) == 111:
            return "network"
    if "connection refused" in text:
        return "network"
    if "timed out" in text or "timeout" in text:
        return "network"
    if "failed to connect" in text or "couldn't connect" in text:
        return "network"
    return "other"


def verify(report):
    """Every assertion the node promises, re-checked from one probe run."""
    problems = []

    def want(condition, message):
        if not condition:
            problems.append(message)

    want(report.get("abi") == 1, f"ABI version {report.get('abi')!r} is not 1")
    want(
        str(report.get("php_minor")) in {"803", "804", "805"},
        f"php_minor {report.get('php_minor')!r} is not a supported minor",
    )
    want(
        str(report.get("shim_before_libphp", "")).startswith("refused"),
        "loading the shim before libphp did not fail cleanly: "
        f"{report.get('shim_before_libphp')!r}",
    )
    want(
        report.get("cwd_preserved") is True,
        "a request moved the worker's cwd (php_execute_script must not chdir: the "
        "worker's cwd is its private proxy-socket directory)",
    )
    want(
        report.get("status") == 200,
        f"hello.php status {report.get('status')!r} is not 200",
    )
    heads = " ".join(str(head) for head in report.get("head", []))
    want("X-Probe: 1" in heads, f"the X-Probe head is missing (heads: {heads!r})")
    want(
        "hello from php" in str(report.get("body", "")),
        f"the body is {report.get('body')!r}",
    )
    cpu_ns = report.get("cpu_ns")
    wall_ns = report.get("wall_ns")
    want(isinstance(cpu_ns, int) and cpu_ns > 0, f"cpu_ns {cpu_ns!r} is not positive")
    want(
        isinstance(cpu_ns, int) and isinstance(wall_ns, int) and cpu_ns <= wall_ns,
        f"cpu_ns {cpu_ns!r} exceeds wall_ns {wall_ns!r}",
    )

    requests = report.get("requests") or []
    by_fixture = {}
    for request in requests:
        by_fixture.setdefault(request.get("fixture"), []).append(request)
    want(len(requests) == 6, f"expected 6 requests, got {len(requests)}")

    exit_calls = by_fixture.get("exit.php") or []
    want(
        bool(exit_calls) and exit_calls[0].get("contained") == 1,
        f"exit() was not contained: {exit_calls!r}",
    )
    want(
        bool(exit_calls) and exit_calls[0].get("status") == 200,
        f"exit() lost the response it had already sent: {exit_calls!r}",
    )
    fatal_calls = by_fixture.get("fatal.php") or []
    want(
        bool(fatal_calls) and fatal_calls[0].get("contained") == 1,
        f"the fatal was not contained: {fatal_calls!r}",
    )
    want(
        bool(fatal_calls) and fatal_calls[0].get("status") == 500,
        f"the contained fatal is not a 500: {fatal_calls!r}",
    )
    hellos = by_fixture.get("hello.php") or []
    want(
        len(hellos) == 2 and hellos[-1].get("status") == 200,
        f"the request after exit()+fatal was not served: {hellos!r}",
    )
    want(
        "hello from php" in str(hellos[-1].get("body", "")) if hellos else False,
        "the request after exit()+fatal returned no body",
    )

    vars_calls = by_fixture.get("vars.php") or []
    vars_body = str(vars_calls[0].get("body", "")) if vars_calls else ""
    for key in ("php_value", "php_admin_value", "redirect_status", "http_proxy"):
        want(
            f'"{key}":"absent"' in vars_body,
            f"{key} reached $_SERVER (vars.php said {vars_body!r})",
        )

    want("egress.php" in by_fixture, "the egress fixture did not run")
    return problems


def verify_egress(report, case):
    """The INI half and the per-run failure classes for one `EGRESS_CASES` row."""
    problems = []
    try:
        egress = json.loads(report.get("egress") or "{}")
    except json.JSONDecodeError as error:
        return [
            f"the egress fixture's body is not JSON ({error}): {report.get('egress')!r}"
        ]

    if report.get("egress_mode") != case["mode"]:
        problems.append(
            f"the probe ran with egress {report.get('egress_mode')!r}, "
            f"not {case['mode']!r}"
        )
    if egress.get("allow_url_fopen") != case["allow_url_fopen"]:
        problems.append(
            f"ini_get('allow_url_fopen') is {egress.get('allow_url_fopen')!r}, "
            f"not {case['allow_url_fopen']!r} (the INI layer is not pinned)"
        )
    disabled = str(egress.get("disable_functions", ""))
    for function in ("pcntl_signal", "posix_kill", "posix_getpgid", "posix_setsid"):
        if function not in disabled.split(","):
            problems.append(
                f"ini_get('disable_functions') does not disable {function}: {disabled!r}"
            )
    # The rest of the lockdown, read back effectively rather than trusted from
    # the rendered string: these are exactly the directives attempt 1 rendered
    # and never applied (`sapi_startup` nul'd them).
    view_tmp = str(report.get("view_tmp", ""))
    basedir = egress.get("open_basedir")
    if basedir != f"/fixtures:{view_tmp}":
        problems.append(
            f"ini_get('open_basedir') is {basedir!r}, expected "
            f"{f'/fixtures:{view_tmp}'!r}"
        )
    # `ffi.enable=0` / `phar.require_hash=1`, or the extension absent entirely:
    # Alpine's `php84-embed` ships neither, so `ini_get` answers `false`. That is
    # a stronger lockdown than the pinned value — no `ffi` to enable, no `phar`
    # whose hash could be skipped — but it is a *different* fact, so both halves
    # are checked: an absent extension passes, an extension that is loaded with a
    # value the INI pin did not hold still fails.
    for key, extension, expected in (
        ("ffi_enable", "ffi", "0"),
        ("phar_require_hash", "phar", "1"),
    ):
        got = egress.get(key)
        loaded = egress.get(f"{extension}_loaded")
        if got is False or got == "":
            if loaded is not False:
                problems.append(
                    f"extension {extension!r} is loaded but ini_get({key!r}) is "
                    f"{got!r}: the INI pin did not hold"
                )
        elif str(got) != expected:
            problems.append(f"ini_get({key!r}) is {got!r}, expected {expected!r}")
    attempts = egress.get("attempts") or {}
    for name in case["must_fail"]:
        attempt = attempts.get(name)
        if attempt is None:
            problems.append(f"the {name} attempt is missing")
        elif attempt.get("connected") is not False:
            problems.append(
                f"the {name} attempt connected; it must never reach the network"
            )
    for name, expected in case["classes"].items():
        attempt = attempts.get(name)
        if attempt is None:
            problems.append(f"the {name} attempt is missing")
            continue
        if attempt.get("connected") is not False:
            continue  # already reported by `must_fail`
        got = classify(attempt)
        if got != expected:
            problems.append(
                f"{name} was refused by {got!r}, expected {expected!r} "
                f"(error {attempt.get('error')!r}, errno {attempt.get('errno')!r}, "
                f"os_errno {attempt.get('os_errno')!r})"
            )

    # The layer-independent half: a plain `socket(AF_INET)` in this same process,
    # before PHP started and after the requests finished.
    inet = report.get("inet_socket_errno") or {}
    for when in ("before", "after"):
        got = inet.get(when)
        if got != case["inet_socket_errno"]:
            problems.append(
                f"socket(AF_INET) errno {when} the requests was {got!r}, expected "
                f"{case['inet_socket_errno']!r} (1 is the seccomp EPERM, 0 is no filter)"
            )
    return problems


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--minor",
        default=os.environ.get("PHP_MINOR", "8.4"),
        help="PHP minor to build and probe (default 8.4)",
    )
    parser.add_argument(
        "--libc",
        default=os.environ.get("PHP_SAPI_LIBC", "gnu"),
        choices=("gnu", "musl"),
        help="which libc leg to build (the matrix has one leg per libc)",
    )
    parser.add_argument("--image", default=DEFAULT_IMAGE, help="image tag to build")
    parser.add_argument(
        "--keep", action="store_true", help="do not remove the image afterwards"
    )
    args = parser.parse_args(argv)

    spec = leg(args.minor, args.libc)
    print(
        f"php-sapi-e2e: docker build --build-arg BASE={spec['base']} "
        f"--build-arg PHP_MINOR={args.minor} -t {args.image}"
    )
    ok, log = build(args.image, args.minor, args.libc)
    if not ok:
        print("php-sapi-e2e: FAIL the image did not build", file=sys.stderr)
        print(log, file=sys.stderr)
        return 1

    tuple_, error = image_tuple(args.image)
    if tuple_ is None:
        print("php-sapi-e2e: FAIL cannot read the image's target", file=sys.stderr)
        print(f"  {error}", file=sys.stderr)
        return 1

    print(f"php-sapi-e2e: readelf -d {shim_name(args.minor, tuple_)}")
    ok, detail = check_linkage(args.image, args.minor, tuple_)
    if not ok:
        print("php-sapi-e2e: FAIL the shim's dynamic linkage", file=sys.stderr)
        print(f"  {detail}", file=sys.stderr)
        return 1
    print(f"  ok  DT_NEEDED: {detail} (no libphp)")

    failed = False
    for case in EGRESS_CASES:
        print(f"php-sapi-e2e: probe — {case['label']}")
        report, log = run_probe(args.image, case["args"])
        if report is None:
            print(
                "php-sapi-e2e: FAIL the probe did not answer with one JSON line",
                file=sys.stderr,
            )
            print(tail(log), file=sys.stderr)
            return 1
        problems = verify(report) + verify_egress(report, case)
        if problems:
            failed = True
            print(f"  FAIL {case['label']}", file=sys.stderr)
            for problem in problems:
                print(f"  - {problem}", file=sys.stderr)
            print("  probe said: " + json.dumps(report), file=sys.stderr)
            print(tail(log), file=sys.stderr)
            continue
        egress = json.loads(report["egress"])
        attempts = egress["attempts"]
        inet = report["inet_socket_errno"]
        print(
            "  ok  php {version} abi {abi}: status {status}, cpu_ns {cpu_ns} <= "
            "wall_ns {wall_ns}, exit()+fatal contained, next request served".format(
                version=report.get("php_version"),
                abi=report.get("abi"),
                status=report.get("status"),
                cpu_ns=report.get("cpu_ns"),
                wall_ns=report.get("wall_ns"),
            )
        )
        print(
            "      allow_url_fopen={fopen}  socket(AF_INET) errno before/after="
            "{ib}/{ia}  fopen_http={f}  stream_socket={s} ({s_err})  "
            "curl={c} ({c_err})".format(
                fopen=egress.get("allow_url_fopen"),
                ib=inet.get("before"),
                ia=inet.get("after"),
                f=classify(attempts["fopen_http"]),
                s=classify(attempts["stream_socket"]),
                s_err=attempts["stream_socket"].get("error"),
                c=classify(attempts["curl"]),
                c_err=attempts["curl"].get("error"),
            )
        )
        print("      json: " + json.dumps(egress))

    if failed:
        return 1

    if not args.keep:
        docker("image", "rm", "-f", args.image, capture_output=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
