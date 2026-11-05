#![no_std]

extern crate alloc;
mod c_alloc;
pub mod state;

use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::{
    ffi::{CStr, c_char, c_void},
    ptr::NonNull,
};
use pocket_drivers::abi::{PocketGuestV1, PocketResult, PocketRuntime};
use pocket_render::damage::{DamagePlan, DamageTarget, DamageTracker};
use pocket_runtime::{
    Cow, Guest, GuestEnvironment, GuestFrame, InputClasses, Profile, RuntimeConfig, RuntimeError,
    abi::{self, CGuest, Handle},
};
use state::Model;

pub const JS_STACK_LIMIT: usize = 128 * 1024;
pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 480;
const JAVASCRIPT: &[u8] = include_bytes!("../ui/generated/app.js");
const ASSETS: &[u8] = include_bytes!("../ui/generated/app.pak");

#[repr(C)]
struct NativeString {
    data: *const u8,
    length: usize,
}
impl From<&str> for NativeString {
    fn from(value: &str) -> Self {
        Self {
            data: value.as_ptr(),
            length: value.len(),
        }
    }
}
#[repr(C)]
struct NativeModel {
    link: NativeString,
    usb: NativeString,
    keys: [NativeString; 6],
    layer: u32,
    modifiers: u32,
    presses: u32,
    battery_left: i32,
    battery_right: i32,
    charging: u32,
}

unsafe extern "C" {
    fn experiment_guest_create(heap_limit: usize, stack_limit: usize) -> *mut c_void;
    fn experiment_guest_binding(guest: *mut c_void) -> *mut c_void;
    fn experiment_guest_budget(guest: *mut c_void, polls: u32);
    fn experiment_guest_error(guest: *mut c_void) -> *const c_char;
    fn experiment_guest_destroy(guest: *mut c_void);
    fn experiment_guest_input(guest: *mut c_void, state: *const NativeModel, now_us: u64) -> i32;
    fn experiment_guest_animating(guest: *mut c_void, next_frame_us: *mut u64) -> i32;
    fn pocket_guest_quickjs_vtable() -> *const PocketGuestV1;
}

#[inline(always)]
fn profile_frame(phase: u32) {
    #[cfg(all(feature = "frame-profile", target_os = "none"))]
    unsafe {
        unsafe extern "C" {
            fn pocket_frame_phase(phase: u32);
        }
        pocket_frame_phase(phase);
    }
    #[cfg(not(all(feature = "frame-profile", target_os = "none")))]
    let _ = phase;
}

#[inline(always)]
fn profile_detail(phase: u32) {
    #[cfg(all(feature = "frame-profile", target_os = "none"))]
    unsafe {
        unsafe extern "C" {
            fn pocket_bridge_phase(phase: u32);
        }
        pocket_bridge_phase(phase);
    }
    #[cfg(not(all(feature = "frame-profile", target_os = "none")))]
    let _ = phase;
}

#[derive(Debug)]
pub enum Error {
    Allocation,
    Serialization(serde_json::Error),
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
    model: Option<Model>,
    clock_tick: u64,
    last_us: u64,
    animating: bool,
    next_js_us: u64,
    damage: DamageTracker<8>,
}
impl JsUi {
    pub fn new() -> Result<Self, Error> {
        let handle = Handle::new(runtime_config())?;
        let engine =
            NonNull::new(unsafe { experiment_guest_create(4 * 1024 * 1024, JS_STACK_LIMIT) })
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
            model: None,
            clock_tick: 0,
            last_us: 0,
            animating: true,
            next_js_us: 0,
            damage: DamageTracker::new(),
        };
        let context = ui.handle.op_context();
        context
            .core_mut()?
            .ui
            .set_viewport(WIDTH as f32, HEIGHT as f32);
        context.core_mut()?.ui.set_stable_motion_paint(true);
        assert!(context.core_mut()?.ui.set_tick_rate(60));
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
        Ok(ui)
    }

    pub fn evaluate(&mut self, source: &[u8]) -> Result<(), Error> {
        unsafe { experiment_guest_budget(self.engine.as_ptr(), 256) };
        self.guest.evaluate(source).map_err(|_| self.exception())?;
        self.animating = true;
        self.next_js_us = 0;
        Ok(())
    }

    pub fn frame(
        &mut self,
        model: Model,
        now_us: u64,
        force: bool,
    ) -> Result<Option<DamagePlan<8>>, Error> {
        if now_us < self.last_us {
            return Err(RuntimeError::InvalidArgument.into());
        }
        self.last_us = now_us;
        let changed = self.model.as_ref() != Some(&model);
        if !changed && !self.animating && !force {
            return Ok(None);
        }
        profile_frame(0);
        let was_animating = self.model.is_some() && self.animating;
        let target_tick = now_us.saturating_mul(60) / 1_000_000;
        if !was_animating {
            self.clock_tick = target_tick;
        }
        let ticks = target_tick - self.clock_tick;
        let context = self.handle.op_context();
        for _ in 1..ticks {
            context.core_mut()?.ui.tick();
        }
        profile_frame(1);
        if changed || force || now_us >= self.next_js_us {
            profile_detail(0);
            let input = changed.then(|| NativeModel {
                link: model.link.into(),
                usb: model.usb.into(),
                keys: model
                    .key_labels
                    .each_ref()
                    .map(|label| label.as_str().into()),
                layer: model.layer.into(),
                modifiers: model.modifiers.into(),
                presses: model.presses.into(),
                battery_left: model.battery_left,
                battery_right: model.battery_right,
                charging: u32::from(model.charging_left) | (u32::from(model.charging_right) << 1),
            });
            unsafe { experiment_guest_budget(self.engine.as_ptr(), 32) };
            let pointer = input
                .as_ref()
                .map_or(core::ptr::null(), core::ptr::from_ref);
            profile_detail(1);
            // All borrowed strings are copied into JS values during this call.
            if unsafe { experiment_guest_input(self.engine.as_ptr(), pointer, now_us) } != 0 {
                return Err(self.exception());
            }
            profile_detail(2);
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
            profile_detail(3);
            let active =
                unsafe { experiment_guest_animating(self.engine.as_ptr(), &mut self.next_js_us) };
            if active < 0 {
                return Err(self.exception());
            }
            self.animating = active != 0;
            profile_detail(4);
        }
        self.model = Some(model);
        self.clock_tick = target_tick;
        profile_frame(2);
        let mut state = context.core_mut()?;
        if ticks != 0 || !was_animating {
            state.ui.tick();
        }
        state.ui.draw();
        let words = &state.ui.current_draw_list().words;
        if force {
            self.damage.invalidate();
        }
        profile_frame(3);
        let target = DamageTarget::new(WIDTH as u32, HEIGHT as u32, 1, 0);
        let mut damage = self
            .damage
            .prepare_with_motion(&state.ui, words, target)
            .map_err(|_| Error::Runtime(RuntimeError::InvalidState))?;
        damage.repaint_motion();
        profile_frame(4);
        pocket_raster::render_scaled_rgb565_regions(
            &state.ui,
            words,
            &mut self.pixels,
            1,
            damage.regions(),
        );
        profile_frame(5);
        self.damage.commit(&state.ui, words, target);
        profile_frame(6);
        Ok(if damage.regions().is_empty() {
            None
        } else {
            Some(damage)
        })
    }

    pub fn animating(&self) -> bool {
        self.animating
    }
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }

    #[cfg(not(target_os = "none"))]
    pub fn reference_pixels(&self) -> Vec<u16> {
        let context = self.handle.op_context();
        let state = context.core().expect("frame is not borrowed");
        let mut pixels = vec![0; WIDTH * HEIGHT];
        pocket_raster::render_scaled_rgb565(
            &state.ui,
            &state.ui.current_draw_list().words,
            &mut pixels,
            1,
        );
        pixels
    }

    pub fn contains_text(&self, expected: &str) -> bool {
        let context = self.handle.op_context();
        let Ok(state) = context.core() else {
            return false;
        };
        let mut nodes = vec![pocket_drivers::spec::ROOT_ID];
        while let Some(id) = nodes.pop() {
            if state.ui.node_text(id) == Some(expected) {
                return true;
            }
            nodes.extend_from_slice(state.ui.node_children(id));
        }
        false
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
