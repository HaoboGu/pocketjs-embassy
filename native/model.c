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

struct mosaico_model {
    JSContext *context;
    char bridge_error[256];
    JSValue global;
    JSAtom atoms[INPUT_ATOM_COUNT];
};
void mosaico_model_configure(JSContext *context, pocket_guest_quickjs *binding) {
    JSValue global = JS_GetGlobalObject(context);
    JS_SetPropertyStr(context, global, "__pocketRecordInput", JS_FALSE);
#ifdef POCKET_FRAME_PROFILE
    pocket_guest_quickjs_set_phase_hook(binding, profile_guest, NULL);
    JS_SetPropertyStr(context, global, "__pocketProfile", JS_NewCFunction(context, profile_app, "__pocketProfile", 1));
#else
    (void)binding;
#endif
    JS_FreeValue(context, global);
}
void *mosaico_model_create(JSContext *context) {
    struct mosaico_model *guest = calloc(1, sizeof *guest);
    if (!guest) return NULL;
    guest->context = context;
    guest->global = JS_GetGlobalObject(context);
    return guest;
}
void mosaico_model_destroy(void *opaque) {
    struct mosaico_model *guest = opaque;
    for (unsigned i = 0; i < INPUT_ATOM_COUNT; i++) JS_FreeAtom(guest->context, guest->atoms[i]);
    JS_FreeValue(guest->context, guest->global);
    free(guest);
}
const char *mosaico_model_error(void *opaque) {
    return ((struct mosaico_model *)opaque)->bridge_error;
}
static int bridge_failure(struct mosaico_model *guest) {
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
int mosaico_model_input(void *opaque, const struct input_model *state, uint64_t now_us) {
    struct mosaico_model *guest = opaque;
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
int mosaico_model_animating(void *opaque, uint64_t *next_frame_us) {
    struct mosaico_model *guest = opaque;
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
