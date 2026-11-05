#include "pocket_guest_quickjs.h"
#include <stdlib.h>
#include <math.h>

#ifdef POCKET_FRAME_PROFILE
extern void pocket_app_phase(uint32_t phase);
extern void pocket_guest_phase(uint32_t phase);
static JSValue profile_app(JSContext *ctx, JSValueConst self, int argc, JSValueConst *argv) {
    (void)self;
    uint32_t phase;
    if (argc != 1 || JS_ToUint32(ctx, &phase, argv[0]) < 0) return JS_EXCEPTION;
    pocket_app_phase(phase);
    return JS_UNDEFINED;
}
static void profile_guest(void *context, pocket_qjs_phase phase) {
    (void)context;
    pocket_guest_phase(phase);
}
#endif

enum input_atom { TIME, PENDING, ANIMATING, NEXT_FRAME, NEEDS_FRAME,
    LINK, USB, LAYER, KEYS, MODIFIERS, PRESSES, BATTERY_LEFT, BATTERY_RIGHT,
    CHARGING_LEFT, CHARGING_RIGHT, INPUT_ATOM_COUNT };
static const char *const input_names[INPUT_ATOM_COUNT] = {
    "__rmkTime", "__rmkPending", "__rmkAnimating", "__rmkNextFrame", "__pocketNeedsFrame",
    "link", "usb", "layer", "key_labels", "modifiers", "presses", "battery_left", "battery_right",
    "charging_left", "charging_right"
};

struct experiment_guest {
    JSRuntime *runtime;
    JSContext *context;
    pocket_guest_quickjs *binding;
    unsigned remaining;
    char bridge_error[256];
    JSValue global;
    JSAtom atoms[INPUT_ATOM_COUNT];
};

static int interrupt(JSRuntime *runtime, void *opaque) {
    (void)runtime;
    struct experiment_guest *guest = opaque;
    if (guest->remaining == 0) return 1;
    guest->remaining--;
    return 0;
}

extern size_t pocket_c_usable_size(const void *pointer);
static void *guest_calloc(void *opaque, size_t count, size_t size) {
    (void)opaque;
    return calloc(count, size);
}
static void *guest_malloc(void *opaque, size_t size) {
    (void)opaque;
    return malloc(size);
}
static void guest_free(void *opaque, void *pointer) {
    (void)opaque;
    free(pointer);
}
static void *guest_realloc(void *opaque, void *pointer, size_t size) {
    (void)opaque;
    return realloc(pointer, size);
}
static const JSMallocFunctions guest_allocator = {
    guest_calloc, guest_malloc, guest_free, guest_realloc, pocket_c_usable_size
};

void *experiment_guest_create(size_t heap_limit, size_t stack_limit) {
    struct experiment_guest *guest = calloc(1, sizeof *guest);
    if (!guest) return NULL;
    guest->runtime = JS_NewRuntime2(&guest_allocator, NULL);
    if (!guest->runtime) { free(guest); return NULL; }
    JS_SetMemoryLimit(guest->runtime, heap_limit);
    JS_SetMaxStackSize(guest->runtime, stack_limit);
    guest->context = JS_NewContext(guest->runtime);
    if (!guest->context) { JS_FreeRuntime(guest->runtime); free(guest); return NULL; }
    JSValue host_global = JS_GetGlobalObject(guest->context);
    JS_SetPropertyStr(guest->context, host_global, "__pocketRecordInput", JS_FALSE);
    JS_FreeValue(guest->context, host_global);
    guest->binding = pocket_guest_quickjs_attach(guest->context, POCKET_QJS_CANONICAL);
    if (!guest->binding) {
        JS_FreeContext(guest->context); JS_FreeRuntime(guest->runtime); free(guest); return NULL;
    }
#ifdef POCKET_FRAME_PROFILE
    pocket_guest_quickjs_set_phase_hook(guest->binding, profile_guest, NULL);
    JSValue global = JS_GetGlobalObject(guest->context);
    JS_SetPropertyStr(guest->context, global, "__pocketProfile", JS_NewCFunction(guest->context, profile_app, "__pocketProfile", 1));
    JS_FreeValue(guest->context, global);
#endif
    guest->global = JS_GetGlobalObject(guest->context);
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
    for (unsigned i = 0; i < INPUT_ATOM_COUNT; i++) JS_FreeAtom(guest->context, guest->atoms[i]);
    JS_FreeValue(guest->context, guest->global);
    JS_FreeContext(guest->context);
    JS_FreeRuntime(guest->runtime);
    free(guest);
}

#ifdef POCKET_EMBEDDED
#include <sys/time.h>
#include <time.h>
#include <errno.h>
#include <reent.h>
extern uint64_t pocket_uptime_us(void);

// Linked newlib is owned by the GUI task; radio calls use the ROM syscall table.
struct _reent *__getreent(void) {
    return _impure_ptr;
}

int clock_gettime(clockid_t clock, struct timespec *value) {
    if (clock != CLOCK_MONOTONIC) { errno = EINVAL; return -1; }
    const uint64_t micros = pocket_uptime_us();
    value->tv_sec = micros / 1000000;
    value->tv_nsec = (micros % 1000000) * 1000;
    return 0;
}
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
struct input_string { const uint8_t *data; size_t length; };
struct input_model {
    struct input_string link, usb, keys[6];
    uint32_t layer, modifiers, presses;
    int32_t battery_left, battery_right;
    uint32_t charging;
};
int experiment_guest_input(void *opaque, const struct input_model *state, uint64_t now_us) {
    struct experiment_guest *guest = opaque;
    JSContext *ctx = guest->context;
    JSValue value = JS_UNDEFINED, keys = JS_UNDEFINED;
    if (!guest->atoms[TIME]) {
        for (unsigned i = 0; i < INPUT_ATOM_COUNT; i++) {
            guest->atoms[i] = JS_NewAtom(ctx, input_names[i]);
            if (!guest->atoms[i]) {
                for (unsigned j = 0; j < i; j++) { JS_FreeAtom(ctx, guest->atoms[j]); guest->atoms[j] = 0; }
                return bridge_failure(guest);
            }
        }
    }
#define SET(object, name, expression) do { \
    JSValue field = (expression); \
    if (JS_IsException(field) || JS_SetProperty(ctx, object, guest->atoms[name], field) < 0) goto failed; \
} while (0)
    SET(guest->global, TIME, JS_NewFloat64(ctx, (double)now_us / 1000));
    if (state) {
        value = JS_NewObject(ctx);
        if (JS_IsException(value)) goto failed;
        keys = JS_NewArray(ctx);
        if (JS_IsException(keys)) goto failed;
        for (uint32_t i = 0; i < 6; i++) {
            JSValue label = JS_NewStringLen(ctx, (const char *)state->keys[i].data, state->keys[i].length);
            if (JS_IsException(label) || JS_SetPropertyUint32(ctx, keys, i, label) < 0) goto failed;
        }
        SET(value, LINK, JS_NewStringLen(ctx, (const char *)state->link.data, state->link.length));
        SET(value, USB, JS_NewStringLen(ctx, (const char *)state->usb.data, state->usb.length));
        SET(value, LAYER, JS_NewUint32(ctx, state->layer));
        SET(value, KEYS, JS_DupValue(ctx, keys));
        SET(value, MODIFIERS, JS_NewUint32(ctx, state->modifiers));
        SET(value, PRESSES, JS_NewUint32(ctx, state->presses));
        SET(value, BATTERY_LEFT, JS_NewInt32(ctx, state->battery_left));
        SET(value, BATTERY_RIGHT, JS_NewInt32(ctx, state->battery_right));
        SET(value, CHARGING_LEFT, JS_NewBool(ctx, state->charging & 1));
        SET(value, CHARGING_RIGHT, JS_NewBool(ctx, state->charging & 2));
        SET(guest->global, PENDING, JS_DupValue(ctx, value));
    }
#undef SET
    JS_FreeValue(ctx, keys); JS_FreeValue(ctx, value);
    return 0;
failed:
    JS_FreeValue(ctx, keys); JS_FreeValue(ctx, value);
    return bridge_failure(guest);
}
int experiment_guest_animating(void *opaque, uint64_t *next_frame_us) {
    struct experiment_guest *guest = opaque;
    JSContext *ctx = guest->context;
    JSValue global = guest->global;
    JSValue value = JS_GetProperty(ctx, global, guest->atoms[ANIMATING]);
    if (JS_IsException(value)) {
        return bridge_failure(opaque);
    }
    int active = JS_IsUndefined(value) ? 1 : JS_ToBool(ctx, value);
    JS_FreeValue(ctx, value);
    value = JS_GetProperty(ctx, global, guest->atoms[NEEDS_FRAME]);
    if (JS_IsException(value)) { return bridge_failure(opaque); }
    int framework_active = JS_ToBool(ctx, value);
    JS_FreeValue(ctx, value);
    value = JS_GetProperty(ctx, global, guest->atoms[NEXT_FRAME]);
    double millis = 0;
    int status = JS_IsException(value) ? -1 : JS_IsUndefined(value) ? 0 : JS_ToFloat64(ctx, &millis, value);
    JS_FreeValue(ctx, value);
    if (status < 0) return bridge_failure(opaque);
    if (isnan(millis) || millis < 0) {
        JS_ThrowRangeError(ctx, "invalid next frame time");
        return bridge_failure(opaque);
    }
    *next_frame_us = millis < (double)UINT64_MAX / 1000 ? (uint64_t)(millis * 1000) : UINT64_MAX;
    if (framework_active) *next_frame_us = 0;
    return active || framework_active;
}
