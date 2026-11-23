#!/usr/bin/env bash
# One bounded retry for every `dnf install` that installs build tooling (`MB.74`).
#
#   source scripts/dnf-retry.sh
#   dnf_install_retry clang binutils tar gzip git
#
#   docker run ... bash -euc ". /usr/local/lib/blastit/dnf-retry.sh; dnf_install_retry rpm-build"
#
# A distro/manylinux mirror can serve a repository index whose own metadata has
# not finished publishing yet, so a single `dnf install` fails with
# `Errors during downloading metadata for repository 'appstream'`. That is the
# AlmaLinux mid-publish window that killed the `v0.2.1` `Packages` section 3 deb
# aarch64 leg (and, earlier, a `standalone` leg); three attempts with
# `dnf clean all` between them ride the window out.
#
# Only transport is retried. A genuinely missing package, or a repository that
# stays broken, still fails: the function returns dnf's status after the last
# attempt and prints its last output, so a typo is never turned into a pass. No
# failure is swallowed -- there is no `|| true` on the install.
#
# Safe under `set -euo pipefail` and under `bash -euc` (the shapes the callers
# use): each dnf run's status is captured explicitly so `set -e` cannot exit
# between attempts, no pipeline is relied on, and no unset variable is read.

dnf_install_retry() {
    if [ "$#" -eq 0 ]; then
        echo "dnf_install_retry: needs at least one package name" >&2
        return 2
    fi
    if ! command -v dnf >/dev/null 2>&1; then
        echo "error: dnf_install_retry needs a builder with dnf (manylinux is AlmaLinux)" >&2
        return 1
    fi

    local attempt=1 status output
    while [ "$attempt" -le 3 ]; do
        status=0
        output=$(dnf install -y --setopt=install_weak_deps=False "$@" 2>&1) || status=$?
        if [ "$status" -eq 0 ]; then
            [ -z "$output" ] || printf '%s\n' "$output"
            return 0
        fi
        if [ "$attempt" -lt 3 ]; then
            printf 'dnf_install_retry: attempt %s/3 failed (status %s) for: %s\n' \
                "$attempt" "$status" "$*" >&2
            printf 'dnf_install_retry: clearing metadata and retrying\n' >&2
            if ! dnf clean all >/dev/null 2>&1; then
                printf 'dnf_install_retry: warning: `dnf clean all` failed; retrying anyway\n' >&2
            fi
            sleep 2
        fi
        attempt=$((attempt + 1))
    done

    printf 'dnf_install_retry: dnf install failed after 3 attempts for: %s\n' "$*" >&2
    printf '  last dnf output:\n' >&2
    printf '%s\n' "$output" >&2
    printf '  If the output names `Failed to download metadata for repo ...`, the mirror was\n' >&2
    printf '  mid-publish and a re-run should pass. Otherwise the package name or the\n' >&2
    printf '  repository is genuinely wrong and this failure is the answer.\n' >&2
    return "$status"
}

# Run directly (`scripts/dnf-retry.sh <pkg>...`) as well as sourced, which is how
# a one-off proof needs no container helper of its own.
#
# A sourced library must not change the shell options of its caller, so
# `set -u` and friends live only on the direct-run path.
[ -n "${BASH_VERSION:-}" ] || {
    echo "dnf-retry.sh must run under bash" >&2
    exit 1
}
if [ "${BASH_SOURCE[0]:-}" = "${0:-}" ]; then
    set -euo pipefail
    dnf_install_retry "$@"
fi
