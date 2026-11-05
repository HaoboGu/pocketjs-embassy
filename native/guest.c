#include "pocket_guest_quickjs.h"
#include <stdlib.h>

struct experiment_guest {
    JSRuntime *runtime;
    JSContext *context;
    pocket_guest_quickjs *binding;
    unsigned remaining;
    char bridge_error[256];
};

static int interrupt(JSRuntime *runtime, void *opaque) {
    (void)runtime;
    struct experiment_guest *guest = opaque;
    if (guest->remaining == 0) return 1;
    guest->remaining--;
    return 0;
}

void *experiment_guest_create(size_t heap_limit, size_t stack_limit) {
    struct experiment_guest *guest = calloc(1, sizeof *guest);
    if (!guest) return NULL;
    guest->runtime = JS_NewRuntime();
    if (!guest->runtime) { free(guest); return NULL; }
    JS_SetMemoryLimit(guest->runtime, heap_limit);
    JS_SetMaxStackSize(guest->runtime, stack_limit);
    guest->context = JS_NewContext(guest->runtime);
    if (!guest->context) { JS_FreeRuntime(guest->runtime); free(guest); return NULL; }
    guest->binding = pocket_guest_quickjs_attach(guest->context, POCKET_QJS_CANONICAL);
    if (!guest->binding) {
        JS_FreeContext(guest->context); JS_FreeRuntime(guest->runtime); free(guest); return NULL;
    }
    JS_SetInterruptHandler(guest->runtime, interrupt, guest);
    return guest;
}
void *experiment_guest_binding(void *opaque) {
    return ((struct experiment_guest *)opaque)->binding;
}
void experiment_guest_budget(void *opaque, unsigned polls) {
    struct experiment_guest *guest = opaque;
    guest->remaining = polls;
    guest->bridge_error[0] = 0;
}
const char *experiment_guest_error(void *opaque) {
    struct experiment_guest *guest = opaque;
    return guest->bridge_error[0] ? guest->bridge_error : pocket_guest_quickjs_error(guest->binding);
}
void experiment_guest_destroy(void *opaque) {
    struct experiment_guest *guest = opaque;
    pocket_guest_quickjs_destroy(guest->binding);
    JS_FreeContext(guest->context);
    JS_FreeRuntime(guest->runtime);
    free(guest);
}

#ifdef POCKET_EMBEDDED
#include <sys/time.h>
extern uint64_t pocket_uptime_us(void);
int _gettimeofday(struct timeval *value, void *timezone) {
    (void)timezone;
    const uint64_t micros = pocket_uptime_us();
    value->tv_sec = micros / 1000000;
    value->tv_usec = micros % 1000000;
    return 0;
}
#endif

#ifdef POCKET_EMBEDDED
#undef malloc
#undef calloc
#undef realloc
#undef free
extern void *malloc(size_t);
extern void *calloc(size_t, size_t);
extern void *realloc(void *, size_t);
extern void free(void *);
#include <errno.h>
#include <reent.h>
#include <sys/types.h>
extern void pocket_uart_write(const uint8_t *bytes, size_t length);

void *_malloc_r(struct _reent *reent, size_t size) {
    void *pointer = malloc(size);
    if (!pointer) reent->_errno = ENOMEM;
    return pointer;
}
void *_calloc_r(struct _reent *reent, size_t count, size_t size) {
    void *pointer = calloc(count, size);
    if (!pointer) reent->_errno = ENOMEM;
    return pointer;
}
void *_realloc_r(struct _reent *reent, void *pointer, size_t size) {
    void *next = realloc(pointer, size);
    if (!next && size) reent->_errno = ENOMEM;
    return next;
}
void _free_r(struct _reent *reent, void *pointer) {
    (void)reent;
    free(pointer);
}
ssize_t _write(int descriptor, const void *bytes, size_t length) {
    if (descriptor != 1 && descriptor != 2) { errno = EBADF; return -1; }
    pocket_uart_write(bytes, length);
    return length;
}
#endif


static int bridge_failure(struct experiment_guest *guest) {
    JSContext *ctx = guest->context;
    JSValue error = JS_GetException(ctx);
    const char *message = JS_ToCString(ctx, error);
    snprintf(guest->bridge_error, sizeof guest->bridge_error, "%s", message ? message : "state bridge failed");
    if (message) JS_FreeCString(ctx, message);
    JS_FreeValue(ctx, error);
    return -1;
}
int experiment_guest_input(void *opaque, const char *json, size_t length, uint64_t now_us) {
    struct experiment_guest *guest = opaque;
    JSContext *ctx = guest->context;
    JSValue global = JS_GetGlobalObject(ctx);
    if (JS_SetPropertyStr(ctx, global, "__rmkTime", JS_NewFloat64(ctx, (double)now_us / 1000)) < 0)
        goto failed;
    if (json) {
        JSValue value = JS_ParseJSON(ctx, json, length, "rmk-state");
        if (JS_IsException(value)) goto failed;
        if (JS_SetPropertyStr(ctx, global, "__rmkPending", value) < 0) goto failed;
    }
    JS_FreeValue(ctx, global);
    return 0;
failed:
    JS_FreeValue(ctx, global);
    return bridge_failure(guest);
}
int experiment_guest_animating(void *opaque) {
    JSContext *ctx = ((struct experiment_guest *)opaque)->context;
    JSValue global = JS_GetGlobalObject(ctx);
    JSValue value = JS_GetPropertyStr(ctx, global, "__rmkAnimating");
    if (JS_IsException(value)) {
        JS_FreeValue(ctx, global);
        return bridge_failure(opaque);
    }
    int active = JS_ToBool(ctx, value);
    JS_FreeValue(ctx, value);
    JS_FreeValue(ctx, global);
    return active;
}
