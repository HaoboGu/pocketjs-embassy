use alloc::alloc::{alloc, dealloc};
use core::{alloc::Layout, ffi::c_void, ptr};

#[repr(C, align(16))]
struct Header {
    size: usize,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_c_malloc(size: usize) -> *mut c_void {
    let Some(total) = size.max(1).checked_add(size_of::<Header>()) else {
        return ptr::null_mut();
    };
    let Ok(layout) = Layout::from_size_align(total, align_of::<Header>()) else {
        return ptr::null_mut();
    };
    let header = unsafe { alloc(layout) }.cast::<Header>();
    if header.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        header.write(Header { size: size.max(1) });
        header.add(1).cast()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_c_free(pointer: *mut c_void) {
    if !pointer.is_null() {
        unsafe {
            let header = pointer.cast::<Header>().sub(1);
            let layout = Layout::from_size_align_unchecked(
                (*header).size + size_of::<Header>(),
                align_of::<Header>(),
            );
            dealloc(header.cast(), layout);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_c_calloc(count: usize, size: usize) -> *mut c_void {
    let Some(size) = count.checked_mul(size) else {
        return ptr::null_mut();
    };
    let pointer = unsafe { pocket_c_malloc(size) };
    if !pointer.is_null() {
        unsafe { pointer.cast::<u8>().write_bytes(0, size) };
    }
    pointer
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_c_usable_size(pointer: *const c_void) -> usize {
    if pointer.is_null() {
        0
    } else {
        unsafe { (*pointer.cast::<Header>().sub(1)).size }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_c_realloc(pointer: *mut c_void, size: usize) -> *mut c_void {
    if size == 0 {
        unsafe { pocket_c_free(pointer) };
        return ptr::null_mut();
    }
    let next = unsafe { pocket_c_malloc(size) };
    if !next.is_null() && !pointer.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(
                pointer.cast::<u8>(),
                next.cast::<u8>(),
                pocket_c_usable_size(pointer).min(size),
            );
            pocket_c_free(pointer);
        }
    }
    next
}
