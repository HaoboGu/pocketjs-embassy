#![no_std]

extern crate alloc;
pub mod reload;
pub mod state;

use alloc::{
    boxed::Box,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::cell::Cell;
use pocket_guest_rquickjs::RquickjsGuest;
use pocket_render::damage::{DamagePlan, DamageTarget, DamageTracker};
use pocket_runtime::{
    Cow, Guest, GuestEnvironment, GuestFrame, InputClasses, Profile, Runtime, RuntimeConfig,
    RuntimeError,
};
use rquickjs::{Array, CatchResultExt, Coerced, FromJs, Object, Value};
use state::Model;

pub const JS_STACK_LIMIT: usize = 128 * 1024;
pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 480;
const JAVASCRIPT: &[u8] = include_bytes!("../ui/generated/app.js");
const ASSETS: &[u8] = include_bytes!("../ui/generated/app.pak");

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

// Keep native drawing separate from the guest bridge for code generation.
#[inline(never)]
fn draw_native(state: &mut pocket_runtime::CoreState, advance: bool) {
    if advance {
        state.ui.tick();
    }
    state.ui.draw();
}

#[derive(Debug)]
pub enum Error {
    Allocation,
    Runtime(RuntimeError),
    JavaScript(String),
}
impl From<RuntimeError> for Error {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

/// One owner thread drives the QuickJS guest and its native UI.
pub struct JsUi {
    // The guest drops before the native runtime.
    js: RquickjsGuest,
    runtime: Box<Runtime>,
    interrupt_polls: Rc<Cell<u32>>,
    pixels: Framebuffer,
    model: Option<Model>,
    clock_tick: u64,
    last_us: u64,
    animating: bool,
    next_js_us: u64,
    damage: DamageTracker<8>,
    pending_frame: bool,
}
#[repr(C, align(64))]
#[derive(Clone)]
struct PixelBlock([u16; 32]);
struct Framebuffer(Vec<PixelBlock>);
impl Framebuffer {
    fn new() -> Self {
        Self(vec![PixelBlock([0; 32]); WIDTH * HEIGHT / 32])
    }
}
impl core::ops::Deref for Framebuffer {
    type Target = [u16];
    fn deref(&self) -> &[u16] {
        // Every aligned block occupies exactly its 32 pixels.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr().cast(), self.0.len() * 32) }
    }
}
impl core::ops::DerefMut for Framebuffer {
    fn deref_mut(&mut self) -> &mut [u16] {
        unsafe { core::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast(), self.0.len() * 32) }
    }
}

/// A renderer owns the framebuffer until every submitted operation completes.
/// The future stays on the UI's owning executor.
#[allow(async_fn_in_trait)]
pub trait FrameRenderer {
    async fn render(
        &mut self,
        resources: &impl pocket_drivers::RenderResources,
        commands: pocket_render::damage::PreparedDrawList<'_>,
        pixels: &mut [u16],
        regions: &[pocket_render::damage::DamageRect],
    );
}

impl JsUi {
    pub fn new() -> Result<Self, Error> {
        Self::create(JAVASCRIPT, Cow::Borrowed(ASSETS))
    }

    pub fn from_bundle(javascript: &[u8], assets: Vec<u8>) -> Result<Self, Error> {
        Self::create(javascript, Cow::Owned(assets))
    }

    /// Keep the live scene until the replacement has initialized and rendered.
    pub fn reload(&mut self, javascript: &[u8], assets: Vec<u8>, now_us: u64) -> Result<(), Error> {
        let mut replacement = Self::from_bundle(javascript, assets)?;
        replacement.frame(self.model.clone().unwrap_or_default(), now_us, true)?;
        *self = replacement;
        Ok(())
    }

    fn create(javascript: &[u8], assets: Cow<'static, [u8]>) -> Result<Self, Error> {
        let runtime = Box::new(Runtime::new(runtime_config())?);
        let js = RquickjsGuest::default();
        let interrupt_polls = Rc::new(Cell::new(256));
        let mut ui = Self {
            js,
            runtime,
            interrupt_polls,
            pixels: Framebuffer::new(),
            model: None,
            clock_tick: 0,
            last_us: 0,
            animating: true,
            next_js_us: 0,
            damage: DamageTracker::new(),
            pending_frame: false,
        };
        let context = ui.runtime.op_context();
        context
            .core_mut()?
            .ui
            .set_viewport(WIDTH as f32, HEIGHT as f32);
        context.core_mut()?.ui.set_stable_motion_paint(true);
        context.core_mut()?.ui.set_fast_absolute_layout(true);
        context
            .core_mut()?
            .ui
            .set_draw_cache_enabled(!cfg!(feature = "perf-no-cache"));
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
            pak: &assets,
            worker_javascript: &[],
            assets: None,
        };
        ui.js
            .initialize(context, &environment)
            .map_err(|_| Error::JavaScript(ui.js.last_error().unwrap_or("guest failed").into()))?;
        let polls = Rc::clone(&ui.interrupt_polls);
        ui.js.with_runtime(|runtime| {
            runtime.set_max_stack_size(JS_STACK_LIMIT);
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let remaining = polls.get();
                polls.set(remaining.saturating_sub(1));
                remaining == 0
            })));
        });
        ui.js
            .with(|ctx| {
                ctx.globals().set("__pocketRecordInput", false)?;
                #[cfg(all(feature = "frame-profile", target_os = "none"))]
                ctx.globals().set(
                    "__pocketProfile",
                    rquickjs::Function::new(ctx.clone(), |phase: u32| {
                        unsafe extern "C" {
                            fn pocket_app_phase(phase: u32);
                        }
                        unsafe { pocket_app_phase(phase) };
                    })?,
                )?;
                Ok::<_, rquickjs::Error>(())
            })
            .unwrap()
            .map_err(|error| Error::JavaScript(error.to_string()))?;
        ui.evaluate(javascript)?;
        Ok(ui)
    }

    pub fn evaluate(&mut self, source: &[u8]) -> Result<(), Error> {
        self.interrupt_polls.set(256);
        self.js.evaluate(source).map_err(|_| {
            Error::JavaScript(self.js.last_error().unwrap_or("guest failed").into())
        })?;
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
        let Some(damage) = self.prepare_frame(model, now_us, force)? else {
            return Ok(None);
        };
        let context = self.runtime.op_context();
        let state = context.core()?;
        if let Some(commands) = self.damage.prepared_commands() {
            pocket_raster::render_prepared_rgb565_regions(
                &state.ui,
                commands,
                &mut self.pixels,
                1,
                damage.regions(),
            );
        }
        profile_frame(5);
        self.damage.commit(
            &state.ui,
            &state.ui.current_draw_list().words,
            DamageTarget::new(WIDTH as u32, HEIGHT as u32, 1, 0),
        );
        profile_frame(6);
        self.pending_frame = false;
        Ok((!damage.regions().is_empty()).then_some(damage))
    }

    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "The exclusive JsUi borrow prevents guest reentry while render resources are in use."
    )]
    pub async fn frame_with_renderer(
        &mut self,
        model: Model,
        now_us: u64,
        force: bool,
        renderer: &mut impl FrameRenderer,
    ) -> Result<Option<DamagePlan<8>>, Error> {
        let Some(damage) = self.prepare_frame(model, now_us, force)? else {
            return Ok(None);
        };
        let context = self.runtime.op_context();
        let state = context.core()?;
        if let Some(commands) = self.damage.prepared_commands() {
            renderer
                .render(&state.ui, commands, &mut self.pixels, damage.regions())
                .await;
        }
        profile_frame(5);
        self.damage.commit(
            &state.ui,
            &state.ui.current_draw_list().words,
            DamageTarget::new(WIDTH as u32, HEIGHT as u32, 1, 0),
        );
        profile_frame(6);
        self.pending_frame = false;
        Ok((!damage.regions().is_empty()).then_some(damage))
    }

    fn prepare_frame(
        &mut self,
        model: Model,
        now_us: u64,
        force: bool,
    ) -> Result<Option<DamagePlan<8>>, Error> {
        let force = force || self.pending_frame;
        if now_us < self.last_us {
            return Err(RuntimeError::InvalidArgument.into());
        }
        self.last_us = now_us;
        let changed = self.model.as_ref() != Some(&model);
        if !changed && !self.animating && !force {
            return Ok(None);
        }
        let target_tick = now_us.saturating_mul(60) / 1_000_000;
        if !cfg!(feature = "perf-baseline")
            && !changed
            && !force
            && target_tick == self.clock_tick
            && now_us < self.next_js_us
        {
            return Ok(None);
        }
        profile_frame(0);
        let was_animating = self.model.is_some() && self.animating;
        if !was_animating {
            self.clock_tick = target_tick;
        }
        let ticks = target_tick - self.clock_tick;
        let context = self.runtime.op_context();
        for _ in 1..ticks {
            context.core_mut()?.ui.tick();
        }
        profile_frame(1);
        if changed || force || now_us >= self.next_js_us {
            profile_detail(0);
            self.interrupt_polls.set(32);
            profile_detail(1);
            self.js
                .with(|ctx| {
                    let publish = || -> rquickjs::Result<()> {
                        let global = ctx.globals();
                        global.set("__rmkTime", now_us as f64 / 1000.0)?;
                        if changed {
                            let input = Object::new(ctx.clone())?;
                            input.set("link", model.link)?;
                            input.set("usb", model.usb)?;
                            input.set("layer", model.layer)?;
                            input.set("modifiers", model.modifiers)?;
                            input.set("presses", model.presses)?;
                            input.set("battery_left", model.battery_left)?;
                            input.set("battery_right", model.battery_right)?;
                            input.set("charging_left", model.charging_left)?;
                            input.set("charging_right", model.charging_right)?;
                            let keys = Array::new(ctx.clone())?;
                            for (index, label) in model.key_labels.iter().enumerate() {
                                keys.set(index, label.as_str())?;
                            }
                            input.set("key_labels", keys)?;
                            global.set("__rmkPending", input)?;
                        }
                        Ok(())
                    };
                    publish()
                        .catch(&ctx)
                        .map_err(|error| Error::JavaScript(error.to_string()))
                })
                .unwrap()?;
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
            self.js.frame(&frame).map_err(|_| {
                Error::JavaScript(self.js.last_error().unwrap_or("guest failed").into())
            })?;
            profile_detail(3);
            (self.animating, self.next_js_us) = self
                .js
                .with(|ctx| {
                    let status = || -> rquickjs::Result<(bool, u64)> {
                        let globals = ctx.globals();
                        let value: Value = globals.get("__rmkAnimating")?;
                        let active =
                            value.is_undefined() || Coerced::<bool>::from_js(&ctx, value)?.0;
                        let framework_active: Coerced<bool> = globals.get("__pocketNeedsFrame")?;
                        let next: Value = globals.get("__rmkNextFrame")?;
                        let millis = if next.is_undefined() {
                            0.0
                        } else {
                            Coerced::<f64>::from_js(&ctx, next)?.0
                        };
                        if millis.is_nan() || millis < 0.0 {
                            return Err(rquickjs::Exception::throw_range(
                                &ctx,
                                "invalid next frame time",
                            ));
                        }
                        let deadline = if framework_active.0 {
                            0
                        } else {
                            (millis * 1000.0) as u64
                        };
                        Ok((active || framework_active.0, deadline))
                    };
                    status()
                        .catch(&ctx)
                        .map_err(|error| Error::JavaScript(error.to_string()))
                })
                .unwrap()?;
            profile_detail(4);
        }
        self.model = Some(model);
        self.clock_tick = target_tick;
        profile_frame(2);
        let mut state = context.core_mut()?;
        draw_native(&mut state, ticks != 0 || !was_animating);
        let words = &state.ui.current_draw_list().words;
        if force {
            self.damage.invalidate();
        }
        profile_frame(3);
        let target = DamageTarget::new(WIDTH as u32, HEIGHT as u32, 1, 0);
        let damage = self
            .damage
            .prepare_cached(&state.ui, words, target)
            .map_err(|_| Error::Runtime(RuntimeError::InvalidState))?;
        profile_frame(4);
        self.pending_frame = true;
        Ok(Some(damage))
    }

    /// Next native animation tick or JavaScript deadline; state changes can wake sooner.
    pub fn next_frame_us(&self) -> Option<u64> {
        if self.pending_frame {
            return Some(self.last_us);
        }
        if self.model.is_none() {
            return Some(0);
        }
        if !self.animating {
            return None;
        }
        if cfg!(feature = "perf-baseline") {
            return Some(self.last_us);
        }
        let native = self
            .clock_tick
            .saturating_add(1)
            .saturating_mul(1_000_000)
            .div_ceil(60);
        Some(native.min(self.next_js_us))
    }

    pub fn cache_info(&self) -> (bool, bool, usize, u32) {
        self.runtime
            .op_context()
            .core()
            .unwrap()
            .ui
            .draw_cache_info()
    }

    pub fn animating(&self) -> bool {
        self.animating
    }
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }

    #[cfg(not(target_os = "none"))]
    pub fn reference_pixels(&self) -> Vec<u16> {
        let context = self.runtime.op_context();
        let mut state = context.core_mut().expect("frame is not borrowed");
        state.ui.draw_uncached();
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
        let context = self.runtime.op_context();
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

#[cfg(test)]
mod layout_differential {
    use super::*;
    #[test]
    fn async_renderer_matches_software_after_yielding() {
        use core::{
            future::{Future, poll_fn},
            task::{Context, Poll, Waker},
        };
        struct YieldingRenderer;
        impl FrameRenderer for YieldingRenderer {
            async fn render(
                &mut self,
                resources: &impl pocket_drivers::RenderResources,
                commands: pocket_render::damage::PreparedDrawList<'_>,
                pixels: &mut [u16],
                regions: &[pocket_render::damage::DamageRect],
            ) {
                let mut first = true;
                poll_fn(|cx| {
                    if core::mem::replace(&mut first, false) {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
                pocket_raster::render_prepared_rgb565_regions(
                    resources, commands, pixels, 1, regions,
                );
            }
        }
        let mut software = JsUi::new().unwrap();
        let mut asynchronous = JsUi::new().unwrap();
        let mut renderer = YieldingRenderer;
        let mut model = Model {
            link: "Connected",
            usb: "Ready",
            ..Model::default()
        };
        for frame in 0..48 {
            model.layer = (frame / 12) as u8;
            model.modifiers = if frame % 3 == 0 { 5 } else { 0 };
            model.key_labels[0].clear();
            if frame % 2 == 0 {
                model.key_labels[0].push_str("Space").unwrap();
            }
            software
                .frame(model.clone(), frame * 20_000, frame == 0)
                .unwrap();
            {
                let mut next = core::pin::pin!(asynchronous.frame_with_renderer(
                    model.clone(),
                    frame * 20_000,
                    frame == 0,
                    &mut renderer
                ));
                let mut context = Context::from_waker(Waker::noop());
                let first = next.as_mut().poll(&mut context);
                let complete = if first.is_pending() {
                    next.as_mut().poll(&mut context)
                } else {
                    first
                };
                assert!(matches!(complete, Poll::Ready(Ok(_))));
            }
            assert_eq!(software.pixels(), asynchronous.pixels(), "frame {frame}");
            assert_eq!(software.next_frame_us(), asynchronous.next_frame_us());
        }
    }

    #[test]
    fn cancelled_renderer_forces_a_complete_retry() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        struct PendingRenderer;
        impl FrameRenderer for PendingRenderer {
            async fn render(
                &mut self,
                _: &impl pocket_drivers::RenderResources,
                _: pocket_render::damage::PreparedDrawList<'_>,
                pixels: &mut [u16],
                _: &[pocket_render::damage::DamageRect],
            ) {
                pixels[0] ^= 0xffff;
                core::future::pending::<()>().await;
            }
        }
        let mut ui = JsUi::new().unwrap();
        let mut model = Model {
            link: "Connected",
            usb: "Ready",
            ..Model::default()
        };
        ui.frame(model.clone(), 0, true).unwrap();
        ui.frame(model.clone(), 1_000_000, false).unwrap();
        model.key_labels[0].push_str("A").unwrap();
        {
            let mut renderer = PendingRenderer;
            let mut frame = core::pin::pin!(ui.frame_with_renderer(
                model.clone(),
                1_020_000,
                false,
                &mut renderer
            ));
            let mut context = Context::from_waker(Waker::noop());
            assert!(matches!(frame.as_mut().poll(&mut context), Poll::Pending));
        }
        assert_eq!(ui.next_frame_us(), Some(1_020_000));
        let damage = ui.frame(model, 1_020_000, false).unwrap().unwrap();
        assert!(damage.is_full_redraw());
        assert_eq!(ui.pixels(), ui.reference_pixels());
    }

    #[test]
    fn keyboard_scene_matches_general_layout_across_state_changes() {
        let mut fast = JsUi::new().unwrap();
        let mut general = JsUi::new().unwrap();
        general
            .runtime
            .op_context()
            .core_mut()
            .unwrap()
            .ui
            .set_fast_absolute_layout(false);
        let mut model = Model {
            link: "Connected",
            usb: "Ready",
            battery_left: 85,
            battery_right: 72,
            ..Model::default()
        };
        for frame in 0..180u64 {
            model.key_labels[0].clear();
            if frame % 2 == 0 {
                model.key_labels[0]
                    .push_str(if frame % 4 == 0 { "A" } else { "Space" })
                    .unwrap();
            }
            model.presses = (frame / 2) as u8;
            model.modifiers = if frame % 3 == 0 { 5 } else { 0 };
            model.layer = (frame / 15) as u8;
            model.charging_left = frame % 40 < 20;
            model.battery_right = 72 - (frame / 30) as i32;
            fast.frame(model.clone(), frame * 20_000, frame == 0)
                .unwrap();
            general
                .frame(model.clone(), frame * 20_000, frame == 0)
                .unwrap();
            assert_eq!(fast.pixels(), general.pixels(), "frame {frame}");
        }
    }
}
