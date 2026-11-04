#include "pocket_guest_quickjs.h"
#include <stdlib.h>

struct experiment_guest {
    JSRuntime *runtime;
    JSContext *context;
    pocket_guest_quickjs *binding;
    unsigned remaining;
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
    ((struct experiment_guest *)opaque)->remaining = polls;
}
const char *experiment_guest_error(void *opaque) {
    return pocket_guest_quickjs_error(((struct experiment_guest *)opaque)->binding);
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

#ifdef POCKET_EMBEDDED
/* Keep plain and reentrant newlib allocation on the same Rust heap. */
#undef malloc
#undef calloc
#undef realloc
#undef free
void *malloc(size_t size) { return pocket_c_malloc(size); }
void *calloc(size_t count, size_t size) { return pocket_c_calloc(count, size); }
void *realloc(void *pointer, size_t size) { return pocket_c_realloc(pointer, size); }
void free(void *pointer) { pocket_c_free(pointer); }
#endif
