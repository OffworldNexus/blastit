/*
 * blastit_php_sapi.c — the per-minor Blastit SAPI shim (Apache-2.0).
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * One source file, compiled once per supported PHP minor against that minor's
 * own headers (`php-config8X --include-dir`). It links against nothing: every
 * Zend/SAPI symbol below stays undefined until the view worker `dlopen`s the
 * host's NTS `libphp.so` with `RTLD_NOW | RTLD_GLOBAL` and then this shim with
 * `RTLD_NOW | RTLD_LOCAL`. Never link against libphp, never add a `DT_NEEDED`.
 *
 * The request path, in order:
 *
 *   1. `zend_first_try` around `php_request_startup` -> `php_execute_script` ->
 *      `php_request_shutdown`. `exit()`, `die()`, an uncaught fatal and
 *      `memory_limit` exhaustion all reach `zend_bailout()`, which is a
 *      `longjmp`; letting it cross a Rust frame is undefined behaviour, so the
 *      shim traps it here and returns `BLASTIT_PHP_OUTCOME_BAILOUT` with a
 *      status. Rust never sees a jump.
 *   2. `ub_write` -> sink body frames. `send_headers` -> one sink head frame per
 *      header plus an end-of-head marker that always carries the status.
 *   3. `register_server_variables` imports only the worker's already-validated
 *      list, and re-refuses the four reserved names (`PHP_VALUE`,
 *      `PHP_ADMIN_VALUE`, `REDIRECT_STATUS`, `HTTP_PROXY`) so a bug on the Rust
 *      side cannot become an INI/host-proxy escalation here.
 *   4. `log_message` truncates to `BLASTIT_LOG_LINE_MAX` and forwards one line.
 *   5. `CLOCK_PROCESS_CPUTIME_ID` is read around the whole request, outside
 *      PHP userland, so no script can forge `cpu_ns`.
 *
 * The INI lockdown is *not* here: it arrives as `cfg->ini`, rendered by
 * `blastit_php_sapi::IniPolicy`, and is parsed by `php_init_config` at the
 * startup stage after `php.ini` and every scanned `.ini`. There is no
 * request-derived INI surface for a request to reach.
 */

#include "blastit_php_abi_v1.h"

#include <php.h>
#include <php_main.h>
#include <php_variables.h>
#include <SAPI.h>
#include <Zend/zend_stream.h>
#include <main/php_output.h>

#include <stdlib.h>
#include <string.h>
#include <time.h>

/* Layout drift between this file and `src/abi.rs` is a segfault at `exit()`
 * time, not a compile error, so pin the numbers here. The Rust side asserts the
 * same ones in `src/abi.rs` and `tests/critical_shim_contract.rs`. */
_Static_assert(sizeof(blastit_php_result_v1) == 32, "blastit_php_result_v1 size");
_Static_assert(offsetof(blastit_php_result_v1, cpu_ns) == 16, "result.cpu_ns offset");
_Static_assert(offsetof(blastit_php_result_v1, body_bytes) == 24, "result.body_bytes offset");
_Static_assert(sizeof(blastit_php_sink_v1) == 4 * sizeof(void *), "blastit_php_sink_v1 size");
_Static_assert(offsetof(blastit_php_sink_v1, head) == 2 * sizeof(void *), "sink.head offset");
_Static_assert(offsetof(blastit_php_request_v1, content_length) == 7 * 8,
               "request.content_length offset");
_Static_assert(offsetof(blastit_php_request_v1, server_vars) == 9 * 8,
               "request.server_vars offset");
_Static_assert(offsetof(blastit_php_request_v1, body_len) == 12 * 8, "request.body_len offset");

/* One log line, then truncation: a log frame is never unbounded. */
#define BLASTIT_LOG_LINE_MAX 2048
/* A `$_SERVER` name longer than this is refused, not truncated. */
#define BLASTIT_SERVER_VAR_NAME_MAX 127

/* php_request_startup() sets SG(server_context); a non-NULL value is also what
 * makes Zend call read_cookies() at all, so it points at the live context. */
typedef struct blastit_request_ctx {
    const blastit_php_sink_v1 *sink;
    const blastit_php_request_v1 *req;
    char *cookie; /* owned: read_cookies() must return a mutable buffer */
    size_t body_pos;
    int32_t status;
    int32_t headers_sent;
    uint64_t body_bytes;
} blastit_request_ctx;

/* A view worker is single-threaded and runs one request at a time (NTS), and
 * the SAPI callbacks below receive no context argument, so the active request
 * is a file-scope pointer for the duration of `blastit_php_request`. */
static blastit_request_ctx *bt_current;
static sapi_module_struct bt_sapi;
static int bt_started;

/* ------------------------------------------------------------------------- */
/* Helpers                                                                   */
/* ------------------------------------------------------------------------- */

static int bt_ascii_eq_ci(const char *a, size_t alen, const char *b)
{
    size_t i;

    for (i = 0; i < alen; i++) {
        char ca = a[i];
        char cb = b[i];

        if (cb == '\0') {
            return 0;
        }
        if (ca >= 'a' && ca <= 'z') {
            ca = (char)(ca - 'a' + 'A');
        }
        if (cb >= 'a' && cb <= 'z') {
            cb = (char)(cb - 'a' + 'A');
        }
        if (ca != cb) {
            return 0;
        }
    }

    return b[alen] == '\0';
}

/* The four names a request may never bring, whatever the Rust side decided. */
static int bt_reserved_name(const char *name, size_t len)
{
    static const char *const reserved[] = {
        "PHP_VALUE", "PHP_ADMIN_VALUE", "REDIRECT_STATUS", "HTTP_PROXY"
    };
    size_t i;

    for (i = 0; i < sizeof(reserved) / sizeof(reserved[0]); i++) {
        if (bt_ascii_eq_ci(name, len, reserved[i])) {
            return 1;
        }
    }

    return 0;
}

/* `$_SERVER` names are `[A-Z0-9_]`; anything else is refused, never escaped. */
static int bt_valid_server_var_name(const char *name, size_t len)
{
    size_t i;

    if (len == 0 || len > BLASTIT_SERVER_VAR_NAME_MAX) {
        return 0;
    }

    for (i = 0; i < len; i++) {
        char c = name[i];

        if (!((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_')) {
            return 0;
        }
    }

    return 1;
}

/* Whether PHP's output buffer holds nothing that would still reach the sink.
 * Used to decide one thing only: whether a contained fatal may still change an
 * unstarted response's status. Anything unknown counts as "holding output", so
 * the script's own status is never overwritten on a guess. */
static int bt_output_empty(void)
{
    zval length;
    int empty;

    if (php_output_get_level() == 0) {
        return 1;
    }
    ZVAL_UNDEF(&length);
    if (php_output_get_length(&length) != SUCCESS) {
        zval_ptr_dtor(&length);

        return 0;
    }
    empty = !(Z_TYPE(length) == IS_LONG && Z_LVAL(length) > 0);
    zval_ptr_dtor(&length);

    return empty;
}

/* ------------------------------------------------------------------------- */
/* SAPI callbacks                                                            */
/* ------------------------------------------------------------------------- */

static size_t bt_ub_write(const char *str, size_t len)
{
    const blastit_php_sink_v1 *sink;

    if (bt_current == NULL || len == 0) {
        return 0;
    }
    sink = bt_current->sink;
    if (sink == NULL || sink->body == NULL) {
        return 0;
    }
    if (sink->body(sink->ctx, (const unsigned char *)str, len) != 0) {
        return 0;
    }
    bt_current->body_bytes += len;

    return len;
}

static void bt_flush(void *server_context)
{
    (void)server_context;
}

static int32_t bt_emit_head(int32_t status, const char *name, size_t name_len,
                            const char *value, size_t value_len)
{
    const blastit_php_sink_v1 *sink;

    if (bt_current == NULL) {
        return 1;
    }
    sink = bt_current->sink;
    if (sink == NULL || sink->head == NULL) {
        return 1;
    }
    bt_current->status = status;

    return sink->head(sink->ctx, status, name, name_len, value, value_len);
}

static void bt_send_header(sapi_header_struct *sapi_header, void *server_context)
{
    const char *colon;
    const char *value;
    size_t name_len;
    size_t value_len;

    (void)server_context;

    if (bt_current == NULL || sapi_header == NULL || sapi_header->header == NULL) {
        return;
    }
    colon = memchr(sapi_header->header, ':', sapi_header->header_len);
    if (colon == NULL || colon == sapi_header->header) {
        return;
    }
    name_len = (size_t)(colon - sapi_header->header);
    value = colon + 1;
    value_len = sapi_header->header_len - name_len - 1;
    while (value_len > 0 && (*value == ' ' || *value == '\t')) {
        value++;
        value_len--;
    }
    while (value_len > 0 && (value[value_len - 1] == ' ' || value[value_len - 1] == '\t')) {
        value_len--;
    }
    if (bt_emit_head(bt_current->status != 0 ? bt_current->status : 200,
                     sapi_header->header, name_len, value, value_len) == 0) {
        bt_current->headers_sent = 1;
    }
}

static int bt_send_headers(sapi_headers_struct *sapi_headers)
{
    zend_llist_position pos;
    sapi_header_struct *header;
    int32_t status;

    if (bt_current == NULL || sapi_headers == NULL) {
        return SAPI_HEADER_SEND_FAILED;
    }

    status = sapi_headers->http_response_code;
    if (status <= 0) {
        status = 200;
    }
    if (status < 100 || status > 599) {
        status = 500;
    }
    bt_current->status = status;
    /* The default 200 exists before this call, and PHP only calls us with
     * headers_sent == 0, so a per-header callback here would double-report. */
    bt_current->headers_sent = 1;

    for (header = zend_llist_get_first_ex(&sapi_headers->headers, &pos);
         header != NULL;
         header = zend_llist_get_next_ex(&sapi_headers->headers, &pos)) {
        const char *colon;
        const char *value;
        size_t name_len;
        size_t value_len;

        if (header->header == NULL || header->header_len == 0) {
            continue;
        }
        colon = memchr(header->header, ':', header->header_len);
        if (colon == NULL || colon == header->header) {
            continue;
        }
        name_len = (size_t)(colon - header->header);
        value = colon + 1;
        value_len = header->header_len - name_len - 1;
        while (value_len > 0 && (*value == ' ' || *value == '\t')) {
            value++;
            value_len--;
        }
        while (value_len > 0 && (value[value_len - 1] == ' ' || value[value_len - 1] == '\t')) {
            value_len--;
        }
        if (bt_emit_head(status, header->header, name_len, value, value_len) != 0) {
            return SAPI_HEADER_SEND_FAILED;
        }
    }

    /* End-of-head marker: always sent, so a status of 204 or 500 is reported
     * even when the script emitted no header at all. */
    if (bt_emit_head(status, NULL, 0, NULL, 0) != 0) {
        return SAPI_HEADER_SEND_FAILED;
    }

    return SAPI_HEADER_SENT_SUCCESSFULLY;
}

static size_t bt_read_post(char *buffer, size_t count_bytes)
{
    const blastit_php_request_v1 *req;
    size_t left;
    size_t n;

    if (bt_current == NULL || buffer == NULL || count_bytes == 0) {
        return 0;
    }
    req = bt_current->req;
    if (req == NULL || req->body == NULL || bt_current->body_pos >= req->body_len) {
        return 0;
    }
    left = req->body_len - bt_current->body_pos;
    n = left < count_bytes ? left : count_bytes;
    memcpy(buffer, req->body + bt_current->body_pos, n);
    bt_current->body_pos += n;

    return n;
}

static char *bt_read_cookies(void)
{
    return bt_current != NULL ? bt_current->cookie : NULL;
}

static void bt_register_server_variables(zval *track_vars_array)
{
    const blastit_php_request_v1 *req;
    size_t i;

    if (bt_current == NULL || track_vars_array == NULL) {
        return;
    }
    req = bt_current->req;
    if (req == NULL || req->server_vars == NULL) {
        return;
    }

    for (i = 0; i < req->server_var_count; i++) {
        const blastit_php_server_var_v1 *var = &req->server_vars[i];
        char name[BLASTIT_SERVER_VAR_NAME_MAX + 1];

        if (var->name == NULL || var->value == NULL) {
            continue;
        }
        if (!bt_valid_server_var_name(var->name, var->name_len)) {
            continue;
        }
        if (bt_reserved_name(var->name, var->name_len)) {
            continue;
        }
        if (memchr(var->name, '\0', var->name_len) != NULL) {
            continue;
        }
        memcpy(name, var->name, var->name_len);
        name[var->name_len] = '\0';
        php_register_variable_safe(name, var->value, var->value_len, track_vars_array);
    }
}

static void bt_log_message(const char *message, int syslog_type)
{
    const blastit_php_sink_v1 *sink;
    size_t len;

    if (bt_current == NULL || message == NULL) {
        return;
    }
    sink = bt_current->sink;
    if (sink == NULL || sink->log == NULL) {
        return;
    }
    len = strnlen(message, BLASTIT_LOG_LINE_MAX);
    sink->log(sink->ctx, (int32_t)syslog_type, message, len);
}

/* The worker's environment is stripped by design; PHP's `getenv()` sees none. */
static char *bt_getenv(const char *name, size_t name_len)
{
    (void)name;
    (void)name_len;

    return NULL;
}

/* ------------------------------------------------------------------------- */
/* The frozen ABI                                                            */
/* ------------------------------------------------------------------------- */

uint32_t blastit_php_abi_version(void)
{
    return BLASTIT_PHP_ABI_VERSION;
}

uint32_t blastit_php_minor(void)
{
    return (uint32_t)(PHP_MAJOR_VERSION * 100 + PHP_MINOR_VERSION);
}

int32_t blastit_php_worker_init(const blastit_php_config_v1 *cfg)
{
    if (bt_started) {
        return -1;
    }
    if (cfg == NULL) {
        return -1;
    }
    /* `cfg->ini` is the ABI's only lockdown channel. A NULL there would leave
     * the worker on the host's `php.ini` — `open_basedir` unset,
     * `allow_url_fopen=On`, `disable_functions` empty — which is the exact
     * silent failure this shim was fixed for. Refuse the worker; never default
     * it. */
    if (cfg->ini == NULL) {
        return -1;
    }

    memset(&bt_sapi, 0, sizeof(bt_sapi));
    bt_sapi.name = "blastit";
    bt_sapi.pretty_name = "Blastit PHP Worker";
    bt_sapi.ub_write = bt_ub_write;
    bt_sapi.flush = bt_flush;
    bt_sapi.getenv = bt_getenv;
    bt_sapi.send_headers = bt_send_headers;
    bt_sapi.send_header = bt_send_header;
    bt_sapi.read_post = bt_read_post;
    bt_sapi.read_cookies = bt_read_cookies;
    bt_sapi.register_server_variables = bt_register_server_variables;
    bt_sapi.log_message = bt_log_message;
    /* Never look for a php.ini in the CWD: the worker's cwd holds only granted
     * proxy sockets, and a `.user.ini` is refused by the lockdown below. */
    bt_sapi.php_ini_ignore = 0;
    bt_sapi.php_ini_ignore_cwd = 1;
    bt_sapi.phpinfo_as_text = 0;

    bt_sapi.ini_entries = NULL;

    sapi_startup(&bt_sapi);

    /* `sapi_startup` copies the struct into `sapi_module` **and nuls
     * `ini_entries` on the copy it was given** (`main/SAPI.c` line 70), so the
     * lockdown must be assigned *after* it, on the struct handed to
     * `php_module_startup` — which copies it into `sapi_module` again, and
     * `php_init_config` is what finally reads it. Assigning it before
     * `sapi_startup` (the obvious order) leaves every directive NULL, i.e. the
     * worker silently runs the host's `php.ini`: `open_basedir` unset,
     * `allow_url_fopen=On`, `disable_functions` empty. `php_embed.c` documents
     * the same rule. */
#if PHP_VERSION_ID >= 80300
    bt_sapi.ini_entries = cfg->ini;
#else
    /* PHP 8.2 declares `char *ini_entries`; 8.3 changed it to `const char *`.
     * This cast is the whole per-minor difference in this file, and is why one
     * source is compiled once per minor instead of once for all of them. */
    bt_sapi.ini_entries = (char *)cfg->ini;
#endif

    /* `php_module_startup` runs every MINIT and can `zend_bailout` (a module's
     * MINIT failing, a `memory_limit` hit during startup). Not attacker-
     * reachable today, but "not reachable" is a claim that decays: trap it here
     * the same way a request is trapped, and refuse the worker with -2. */
    {
        volatile int started_ok = 0;

        zend_first_try {
            started_ok = (php_module_startup(&bt_sapi, NULL) == SUCCESS) ? 1 : 0;
        } zend_catch {
            started_ok = 0;
        } zend_end_try();

        if (!started_ok) {
            sapi_shutdown();

            return -2;
        }
    }

    bt_started = 1;

    return 0;
}

void blastit_php_worker_shutdown(void)
{
    if (!bt_started) {
        return;
    }
    bt_started = 0;
    bt_current = NULL;
    /* RSHUTDOWN/`mshutdown` of every module runs here and can `zend_bailout`
     * too. `sapi_shutdown` must still run (it frees the SAPI globals), so the
     * trap is around the Zend half only. */
    zend_try {
        php_module_shutdown();
    } zend_catch {
        /* A module bailed out during shutdown: nothing left to unwind. */
    } zend_end_try();
    sapi_shutdown();
}

int32_t blastit_php_request(const blastit_php_request_v1 *req,
                            const blastit_php_sink_v1 *sink,
                            blastit_php_result_v1 *out)
{
    blastit_request_ctx ctx;
    struct timespec cpu0;
    struct timespec cpu1;
    /* `volatile`: both are written inside `zend_first_try` and read after a
     * `longjmp`, which is exactly the case -Wclobbered warns about. */
    volatile int started = 0;
    volatile int bailed = 0;
    volatile int executed = 1;
    int cpu_ok = 1;

    if (!bt_started || req == NULL || sink == NULL || out == NULL || req->entry == NULL) {
        return -1;
    }
    /* NTS, one request per process: a nested call would clobber bt_current. */
    if (bt_current != NULL) {
        return -1;
    }

    memset(out, 0, sizeof(*out));
    out->outcome = BLASTIT_PHP_OUTCOME_OK;
    out->status = 500;

    memset(&ctx, 0, sizeof(ctx));
    ctx.sink = sink;
    ctx.req = req;
    if (req->cookie != NULL) {
        size_t len = strlen(req->cookie);

        ctx.cookie = malloc(len + 1);
        if (ctx.cookie == NULL) {
            return -1;
        }
        memcpy(ctx.cookie, req->cookie, len + 1);
    }

    bt_current = &ctx;
    SG(server_context) = &ctx;
    SG(request_info).request_method = req->request_method;
    SG(request_info).request_uri = (char *)req->request_uri;
    SG(request_info).query_string = (char *)req->query_string;
    SG(request_info).path_translated = (char *)req->entry;
    SG(request_info).content_type = req->content_type;
    SG(request_info).content_length = (zend_long)req->content_length;
    SG(request_info).cookie_data = ctx.cookie;
    SG(request_info).proto_num = 1001;
    SG(request_info).no_headers = 0;
    SG(headers_sent) = 0;
    SG(sapi_headers).http_response_code = 0;
    /* ADR-0010 §1: the worker's cwd is its private directory holding only the
     * proxy sockets it was granted. `php_execute_script` chdirs to the script
     * by default; that would move `/proc/self/cwd` into the application tree
     * and break every cwd-relative proxy socket. Refuse the chdir instead. */
    SG(options) |= SAPI_OPTION_NO_CHDIR;

    /* `clock_gettime` can fail (an emulated clock, a seccomp profile that
     * refuses the call). `cpu_ns` is a cost report, so a failed read is "no
     * report" (0), never garbage: the scheduler already treats 0 that way. */
    cpu_ok = (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu0) == 0) ? 1 : 0;
    if (!cpu_ok) {
        memset(&cpu0, 0, sizeof(cpu0));
    }

    zend_first_try {
        if (php_request_startup() == SUCCESS) {
            zend_file_handle handle;

            started = 1;
            memset(&handle, 0, sizeof(handle));
            zend_stream_init_filename(&handle, req->entry);
            handle.primary_script = 1;
            /* `php_execute_script` has its own `zend_try … zend_catch`, so an
             * `exit()` or a fatal inside the script is trapped *there* and comes
             * back as `false` — it never reaches this handler. That is why the
             * return value, not only the catch below, marks a contained
             * bailout (`main/main.c` `php_execute_script_ex`). */
            executed = php_execute_script(&handle) ? 1 : 0;
            zend_destroy_file_handle(&handle);
        } else {
            bailed = 1;
        }
    } zend_catch {
        bailed = 1;
    } zend_end_try();

    if (!executed) {
        bailed = 1;
    }
    /* A contained fatal on a response that never started is a 500, not PHP's
     * implicit 200. Three conditions must all hold: no head was sent, no body
     * byte reached the sink, and PHP's output buffer is empty (`exit()` after
     * an `echo` fails the last one). Only then has nothing been promised to the
     * client, so only then can the status still change. */
    if (bailed && !ctx.headers_sent && ctx.body_bytes == 0
        && bt_output_empty()
        && SG(sapi_headers).http_response_code == 0) {
        SG(sapi_headers).http_response_code = 500;
    }

    if (started) {
        /* php_request_shutdown is written for a bailout: it ends observers and
         * runs every step under its own zend_try. Skipping it would leave the
         * executor active and the next php_request_startup would see a dirty
         * request, so run it even when the script bailed out. */
        zend_try {
            php_request_shutdown(NULL);
        } zend_catch {
            bailed = 1;
        } zend_end_try();
    }

    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu1) != 0) {
        cpu_ok = 0;
        memset(&cpu1, 0, sizeof(cpu1));
    }

    out->cpu_ns = cpu_ok
        ? (uint64_t)((int64_t)(cpu1.tv_sec - cpu0.tv_sec) * 1000000000LL
                     + (int64_t)(cpu1.tv_nsec - cpu0.tv_nsec))
        : 0;
    out->body_bytes = ctx.body_bytes;
    out->contained = bailed ? 1 : 0;
    out->outcome = bailed ? BLASTIT_PHP_OUTCOME_BAILOUT : BLASTIT_PHP_OUTCOME_OK;
    if (ctx.status > 0) {
        out->status = ctx.status;
    } else if (!bailed) {
        out->status = 200;
    } else {
        out->status = ctx.headers_sent ? 200 : 500;
    }

    SG(request_info).request_method = NULL;
    SG(request_info).request_uri = NULL;
    SG(request_info).query_string = NULL;
    SG(request_info).path_translated = NULL;
    SG(request_info).content_type = NULL;
    SG(request_info).content_length = 0;
    SG(request_info).cookie_data = NULL;
    SG(server_context) = NULL;
    SG(sapi_headers).http_response_code = 0;

    free(ctx.cookie);
    bt_current = NULL;

    return 0;
}
