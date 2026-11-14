/*
 * blastit_php_abi_v1.h — the frozen C ABI between a Blastit view worker and one
 * per-minor PHP SAPI shim (`libblastit_php8{2,3,4}-<arch>-<libc>.so`, e.g.
 * `libblastit_php84-x86_64-gnu.so`; the file is named by ADR-0010 §4.1 and its
 * `DT_SONAME` is that same string).
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Why a C shim exists at all (ADR-0010 §1, `docs/research/php-embed-sapi.md` §2):
 * `sapi_globals_struct` and `EG(bailout)` move between PHP minors, so Rust code
 * cannot declare them by hand and survive an `exit()`. Each shim is compiled
 * against exactly one minor's headers and exposes *this* struct-and-function
 * surface, which never changes: the Rust side is versioned by
 * `BLASTIT_PHP_ABI_VERSION` and refuses a shim that reports another value.
 *
 * Linkage rule: a shim carries no `DT_NEEDED` on `libphp`. Its Zend/SAPI symbols
 * stay undefined until the view worker `dlopen`s the host's `libphp.so` with
 * `RTLD_NOW | RTLD_GLOBAL` and then the shim with `RTLD_NOW | RTLD_LOCAL`. A
 * `dlopen` of the shim *before* `libphp` must fail with `undefined symbol`, not
 * crash; the probe in `src/probe/` proves both halves.
 *
 * The Rust mirror of every type below lives in `crates/blastit-php-sapi/src/abi.rs`;
 * `tests/critical_shim_contract.rs` keeps the two in step.
 */
#ifndef BLASTIT_PHP_ABI_V1_H
#define BLASTIT_PHP_ABI_V1_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** The ABI this header describes. Bumped only by an incompatible change. */
#define BLASTIT_PHP_ABI_VERSION 1u
/** The PHP major line every shim speaks: 8. */
#define BLASTIT_PHP_ABI_MAJOR 8

/** `blastit_php_request` returned and the script ran to completion. */
#define BLASTIT_PHP_OUTCOME_OK 0
/**
 * `blastit_php_request` returned and a `zend_bailout()` was trapped:
 * `exit()`, `die()`, an uncaught fatal, or `memory_limit` exhaustion. The
 * worker owns a status code, not a longjmp.
 */
#define BLASTIT_PHP_OUTCOME_BAILOUT 1

/**
 * Response sink, implemented by the worker. Every call happens on the worker's
 * own thread, inside `blastit_php_request`.
 *
 * `head` is called once per response header and then once more with
 * `name == NULL && name_len == 0` as the end-of-head marker, so a status is
 * always reported even when the script sent no header at all. The `status`
 * argument is the same on every call.
 */
typedef struct blastit_php_sink_v1 {
    void *ctx;
    /** A response body chunk. Returns 0 on success. */
    int32_t (*body)(void *ctx, const unsigned char *data, size_t len);
    /** One header, or the end-of-head marker described above. */
    int32_t (*head)(void *ctx, int32_t status, const char *name, size_t name_len,
                    const char *value, size_t value_len);
    /** One bounded log line (already truncated to 2048 bytes). */
    void (*log)(void *ctx, int32_t level, const char *line, size_t len);
} blastit_php_sink_v1;

/** One already-validated `$_SERVER` entry. */
typedef struct blastit_php_server_var_v1 {
    const char *name;
    size_t name_len;
    const char *value;
    size_t value_len;
} blastit_php_server_var_v1;

/**
 * One request. Every pointer is valid for the duration of the call; the shim
 * never takes ownership and never frees one. `NULL` means absent.
 */
typedef struct blastit_php_request_v1 {
    /** Absolute `SCRIPT_FILENAME`; pinned by the worker, never request-derived. */
    const char *entry;
    const char *document_root;
    const char *script_name;
    const char *request_uri;
    /** `NULL` when the request has no query string. */
    const char *query_string;
    const char *request_method;
    /** `NULL` when the request has no body. */
    const char *content_type;
    /** 0 when the request has no body. */
    int64_t content_length;
    /** The `Cookie` header value, or `NULL`. Parsed by Zend into `$_COOKIE`. */
    const char *cookie;
    /** Validated server variables. Reserved names are dropped by the shim too. */
    const blastit_php_server_var_v1 *server_vars;
    size_t server_var_count;
    /** The already-read request body, or `NULL`. */
    const unsigned char *body;
    size_t body_len;
} blastit_php_request_v1;

/**
 * Worker configuration, fixed for the process lifetime.
 *
 * `ini` is a NUL-terminated, newline-separated `key=value` blob parsed at the
 * INI *startup* stage, after `php.ini` and after every scanned `.ini`. That is
 * why a request cannot relax `open_basedir`, `allow_url_include` or
 * `user_ini.filename`: this shim has no request-derived INI surface at all
 * (unlike FastCGI's `PHP_VALUE` parser, which is deleted).
 */
typedef struct blastit_php_config_v1 {
    const char *ini;
    /** Informational: the view's private temp directory. */
    const char *view_tmp;
} blastit_php_config_v1;

/** What one request cost and produced. */
typedef struct blastit_php_result_v1 {
    /** `BLASTIT_PHP_OUTCOME_OK` or `BLASTIT_PHP_OUTCOME_BAILOUT`. */
    int32_t outcome;
    /** The response status: what the script sent, else 200, else 500 on bailout. */
    int32_t status;
    /** 1 when a `zend_bailout()` was trapped (always 0 for `OUTCOME_OK`). */
    int32_t contained;
    int32_t reserved;
    /** `CLOCK_PROCESS_CPUTIME_ID` delta across startup..shutdown, in ns. */
    uint64_t cpu_ns;
    /** Body bytes handed to the sink. */
    uint64_t body_bytes;
} blastit_php_result_v1;

/** The ABI this shim was compiled for. */
uint32_t blastit_php_abi_version(void);

/** The PHP minor this shim was compiled against: 802, 803 or 804. */
uint32_t blastit_php_minor(void);

/** `sapi_startup` + `php_module_startup`. 0 on success. */
int32_t blastit_php_worker_init(const blastit_php_config_v1 *cfg);

/** `php_module_shutdown` + `sapi_shutdown`. Idempotent. */
void blastit_php_worker_shutdown(void);

/** One request, with its own `zend_first_try` guard. 0 on success. */
int32_t blastit_php_request(const blastit_php_request_v1 *req,
                            const blastit_php_sink_v1 *sink,
                            blastit_php_result_v1 *out);

#ifdef __cplusplus
}
#endif

#endif /* BLASTIT_PHP_ABI_V1_H */
