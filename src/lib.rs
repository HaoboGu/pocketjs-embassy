#![no_std]

extern crate alloc;
mod c_alloc;

use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
};
use pocket_drivers::abi::{PocketGuestV1, PocketResult, PocketRuntime};
use pocket_runtime::{
    Cow, Guest, GuestEnvironment, GuestFrame, InputClasses, Profile, RuntimeConfig, RuntimeError,
    abi::{self, CGuest, Handle},
};

pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 480;
const JAVASCRIPT: &[u8] = include_bytes!("../ui/generated/app.js");
const ASSETS: &[u8] = include_bytes!("../ui/generated/app.pak");

unsafe extern "C" {
    fn experiment_guest_create(heap_limit: usize, stack_limit: usize) -> *mut c_void;
    fn experiment_guest_binding(guest: *mut c_void) -> *mut c_void;
    fn experiment_guest_budget(guest: *mut c_void, polls: u32);
    fn experiment_guest_error(guest: *mut c_void) -> *const c_char;
    fn experiment_guest_destroy(guest: *mut c_void);
    fn pocket_guest_quickjs_vtable() -> *const PocketGuestV1;
}

#[derive(Debug)]
pub enum Error {
    Allocation,
    Runtime(RuntimeError),
    JavaScript(String),
}
impl From<RuntimeError> for Error {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

/// One owner thread drives the QuickJS guest and its native UI.
pub struct JsUi {
    handle: Box<Handle>,
    guest: CGuest,
    engine: NonNull<c_void>,
    pixels: Vec<u16>,
}
impl JsUi {
    pub fn new() -> Result<Self, Error> {
        let handle = Handle::new(runtime_config())?;
        let engine = NonNull::new(unsafe { experiment_guest_create(4 * 1024 * 1024, 128 * 1024) })
            .ok_or(Error::Allocation)?;
        let guest = match unsafe {
            CGuest::new(
                pocket_guest_quickjs_vtable(),
                experiment_guest_binding(engine.as_ptr()),
                handle.as_ptr(),
            )
        } {
            Ok(guest) => guest,
            Err(error) => {
                unsafe { experiment_guest_destroy(engine.as_ptr()) };
                return Err(Error::Runtime(error.into()));
            }
        };
        let mut ui = Self {
            handle,
            guest,
            engine,
            pixels: vec![0; WIDTH * HEIGHT],
        };
        let context = ui.handle.op_context();
        context
            .core_mut()?
            .ui
            .set_viewport(WIDTH as f32, HEIGHT as f32);
        let environment = GuestEnvironment {
            host: "mosaico-experiment",
            host_abi: pocket_drivers::spec::BINDING_GENERATION,
            tick_hz: 60,
            sim_hz: 60,
            viewport: [WIDTH as u32, HEIGHT as u32],
            auxiliary_viewport: None,
            auxiliary_root: -1,
            raster_density: 1,
            heap_limit: 4 * 1024 * 1024,
            pak: ASSETS,
            worker_javascript: &[],
            assets: None,
        };
        unsafe { experiment_guest_budget(ui.engine.as_ptr(), 256) };
        if ui.guest.initialize(context, &environment).is_err() {
            return Err(ui.exception());
        }
        ui.evaluate(JAVASCRIPT)?;
        ui.frame()?;
        Ok(ui)
    }

    pub fn evaluate(&mut self, source: &[u8]) -> Result<(), Error> {
        unsafe { experiment_guest_budget(self.engine.as_ptr(), 256) };
        self.guest.evaluate(source).map_err(|_| self.exception())
    }

    pub fn frame(&mut self) -> Result<&[u16], Error> {
        unsafe { experiment_guest_budget(self.engine.as_ptr(), 32) };
        let frame = GuestFrame {
            buttons: 0,
            analog: 0x8080,
            touches: &[],
            hits: &[],
            touch_surfaces: &[],
            right_analog: 0x8080,
            axis_deltas: &[],
            motion: None,
            text: &[],
        };
        if self.guest.frame(&frame).is_err() {
            return Err(self.exception());
        }
        let context = self.handle.op_context();
        let mut state = context.core_mut()?;
        state.ui.tick();
        state.ui.draw();
        pocket_raster::render_scaled_rgb565(
            &state.ui,
            &state.ui.current_draw_list().words,
            &mut self.pixels,
            1,
        );
        Ok(&self.pixels)
    }

    pub fn contains_text(&self, expected: &str) -> bool {
        let context = self.handle.op_context();
        let Ok(state) = context.core() else {
            return false;
        };
        (0..256).any(|id| state.ui.node_text(id) == Some(expected))
    }

    fn exception(&self) -> Error {
        let error = unsafe { CStr::from_ptr(experiment_guest_error(self.engine.as_ptr())) };
        Error::JavaScript(error.to_string_lossy().into_owned())
    }
}
impl Drop for JsUi {
    fn drop(&mut self) {
        self.guest.shutdown();
        unsafe { experiment_guest_destroy(self.engine.as_ptr()) };
    }
}

// The C guest uses these two operations; the upstream ABI owns their validation.
#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_runtime_op(
    runtime: *mut PocketRuntime,
    op: u16,
    args: *const c_void,
    length: usize,
    out: *mut PocketResult,
) -> i32 {
    unsafe { abi::pocket_runtime_op(runtime, op, args, length, out) }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pocket_runtime_supports_op(runtime: *const PocketRuntime, op: u16) -> u32 {
    unsafe { abi::pocket_runtime_supports_op(runtime, op) }
}

fn runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        // A local fixture identity; an application package needs matching exported facts.
        profile_hash: [0x5a; 32],
        host_abi: pocket_drivers::spec::BINDING_GENERATION,
        heap_limit: 4 * 1024 * 1024,
        frame_buffer_budget: 480 * 480 * 4,
        profile: Profile {
            id: Cow::Borrowed("mosaico-experiment"),
            cadences: Cow::Borrowed(&[60]),
            viewport: [480, 480],
            logical_viewports: Cow::Borrowed(&[[480, 480]]),
            dynamic_viewport: None,
            presentations: Cow::Borrowed(&[Cow::Borrowed("native")]),
            auxiliary: None,
            auxiliary_logical: Cow::Borrowed(&[]),
            density: 1,
            animation: Cow::Borrowed("composited"),
            capabilities: Cow::Borrowed(&[]),
            required_inputs: InputClasses::NONE,
        },
    }
}
