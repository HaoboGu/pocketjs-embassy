#include <stdint.h>
#include <stdlib.h>
#ifdef POCKET_EMBEDDED
#include <sys/time.h>
#include <time.h>
#include <errno.h>
#include <reent.h>

extern uint64_t pocket_uptime_us(void);

int clock_gettime(clockid_t clock, struct timespec *value) {
    if (clock != CLOCK_MONOTONIC) { errno = EINVAL; return -1; }
    const uint64_t micros = pocket_uptime_us();
    value->tv_sec = micros / 1000000;
    value->tv_nsec = (micros % 1000000) * 1000;
    return 0;
}
// This receiver has no wall clock; keep its existing uptime-based Date behavior.
int pocket_qjs_gettimeofday(struct timeval *value, void *zone) {
    (void)zone;
    const uint64_t micros = pocket_uptime_us();
    value->tv_sec = micros / 1000000;
    value->tv_usec = micros % 1000000;
    return 0;
}
int _gettimeofday(struct timeval *value, void *zone) {
    return pocket_qjs_gettimeofday(value, zone);
}

// Linked newlib is owned by the GUI task; radio calls use the ROM syscall table.
struct _reent *__getreent(void) {
    return _impure_ptr;
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


