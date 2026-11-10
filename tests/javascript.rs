use pocketjs_embassy::{Error, JsUi, WIDTH, state::Model};

fn connected() -> Model {
    Model {
        link: "Connected",
        usb: "Ready",
        battery_left: 85,
        battery_right: 72,
        ..Model::default()
    }
}

#[test]
fn keyboard_state_updates_native_text_and_settled_idle_sleeps() {
    let mut ui = JsUi::new().unwrap();
    let mut state = connected();
    state.layer = 2;
    state.key_labels[0].push_str("A").unwrap();
    state.key_labels[1].push_str("Enter").unwrap();
    ui.frame(state.clone(), 0, false).unwrap();
    ui.frame(state.clone(), 700_000, false).unwrap();
    for label in [
        "Connected",
        "USB / Ready",
        "02",
        "2 held",
        "A",
        "Enter",
        "85%",
        "72%",
    ] {
        assert!(ui.contains_text(label), "missing {label}");
    }
    assert!(!ui.animating());
    assert!(ui.frame(state, 800_000, false).unwrap().is_none());
}

#[test]
fn keyboard_labels_are_data_not_javascript() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"globalThis.x = 0").unwrap();
    let mut state = connected();
    state.key_labels[0].push_str("\";x=1").unwrap();
    ui.frame(state, 0, false).unwrap();
    assert!(ui.contains_text("\";x=1"));
    ui.evaluate(b"if (globalThis.x !== 0) throw Error('injection')")
        .unwrap();
}

#[test]
fn release_and_modifier_frames_match_full_redraw_without_color_flash() {
    let mut ui = JsUi::new().unwrap();
    let mut state = connected();
    ui.frame(state.clone(), 0, false).unwrap();
    ui.frame(state.clone(), 1_000_000, false).unwrap();
    let idle = ui.pixels().to_vec();
    state.key_labels[0].push_str("A").unwrap();
    state.modifiers = 4;
    state.presses = 1;
    ui.frame(state.clone(), 1_020_000, false).unwrap();
    let key_fill = ((0xefu16 >> 3) << 11) | ((0xf2u16 >> 2) << 5) | (0xf5u16 >> 3);
    assert_eq!(ui.pixels()[175 * WIDTH + 204], key_fill);
    assert_eq!(ui.pixels()[322 * WIDTH + 256], key_fill);
    assert_eq!(ui.pixels(), ui.reference_pixels());
    state.key_labels[0].clear();
    state.modifiers = 0;
    ui.frame(state.clone(), 1_020_500, false).unwrap();
    let released_fill = ((0xf6u16 >> 3) << 11) | ((0xf8u16 >> 2) << 5) | (0xfau16 >> 3);
    assert_eq!(ui.pixels()[175 * WIDTH + 204], released_fill);
    assert_eq!(ui.pixels()[322 * WIDTH + 256], 0xffff);
    for time in (1_040_000..=2_000_000).step_by(20_000) {
        ui.frame(state.clone(), time, false).unwrap();
        assert_eq!(ui.pixels(), ui.reference_pixels(), "residue at {time}");
    }
    assert!(ui.contains_text("Ready"));
    assert!(!ui.animating());
    assert_eq!(
        ui.pixels(),
        idle,
        "settled key/modifier/activity state must leave no residue"
    );
}

#[test]
fn layer_motion_uses_elapsed_time() {
    let mut fast = JsUi::new().unwrap();
    let mut slow = JsUi::new().unwrap();
    let mut state = connected();
    for ui in [&mut fast, &mut slow] {
        ui.frame(state.clone(), 0, false).unwrap();
        ui.frame(state.clone(), 1_000_000, false).unwrap();
    }
    state.layer = 3;
    fast.frame(state.clone(), 1_100_000, false).unwrap();
    slow.frame(state.clone(), 1_100_000, false).unwrap();
    for time in (1_110_000..1_250_000).step_by(10_000) {
        fast.frame(state.clone(), time, false).unwrap();
    }
    fast.frame(state.clone(), 1_250_000, false).unwrap();
    slow.frame(state, 1_250_000, false).unwrap();
    assert_eq!(fast.pixels(), slow.pixels());
}

#[test]
fn charging_icon_outline_stays_fixed() {
    let mut ui = JsUi::new().unwrap();
    let mut state = connected();
    state.charging_left = true;
    ui.frame(state.clone(), 0, false).unwrap();
    ui.frame(state.clone(), 1_000_000, false).unwrap();
    let points = [(55, 384), (62, 384), (48, 389), (71, 389)];
    let baseline = points.map(|(x, y)| ui.pixels()[y * WIDTH + x]);
    for time in (1_020_000..=1_800_000).step_by(20_000) {
        ui.frame(state.clone(), time, false).unwrap();
        assert_eq!(points.map(|(x, y)| ui.pixels()[y * WIDTH + x]), baseline);
        assert_eq!(ui.pixels(), ui.reference_pixels());
    }
}

#[test]
fn javascript_exception_and_runaway_script_are_reported() {
    let mut ui = JsUi::new().unwrap();
    match ui.evaluate(b"throw Error('probe failure')") {
        Err(Error::JavaScript(message)) => assert!(message.contains("probe failure")),
        other => panic!("expected JS exception, got {other:?}"),
    }
    match ui.evaluate(b"while (true) {}") {
        Err(Error::JavaScript(message)) => assert!(message.contains("interrupted")),
        other => panic!("expected interruption, got {other:?}"),
    }
    ui.frame(connected(), 0, false).unwrap();
    assert!(ui.contains_text("Connected"));
}

#[test]
fn promise_jobs_and_integer_conversion_still_work() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"Promise.all([Promise.resolve(parseInt('7',10)),Promise.resolve(2)]).then(v=>globalThis.result=v[0]+v[1])").unwrap();
    ui.evaluate(b"if (globalThis.result !== 9) throw Error('promise result')")
        .unwrap();
}

#[test]
fn borrowed_assets_reject_writes_and_allow_readonly_views() {
    let mut ui = JsUi::new().unwrap();
    for source in [
        "new Uint8Array(__pak).reverse()",
        "let v=new Uint8Array([1]);v.constructor={[Symbol.species]:function(){return new Uint8Array(__pak)}};v.map(x=>x)",
    ] {
        assert!(matches!(
            ui.evaluate(source.as_bytes()),
            Err(Error::JavaScript(_))
        ));
    }
    ui.evaluate(b"if (new Uint8Array(__pak).subarray(0,1).length !== 1) throw Error('view')")
        .unwrap();
}

#[test]
fn heap_limit_and_monotonic_frame_boundary_are_enforced() {
    let mut ui = JsUi::new().unwrap();
    assert!(matches!(
        ui.evaluate(b"new ArrayBuffer(8*1024*1024)"),
        Err(Error::JavaScript(_))
    ));
    ui.frame(connected(), 10_000, false).unwrap();
    assert!(matches!(
        ui.frame(connected(), 9_000, false),
        Err(Error::Runtime(_))
    ));
}

#[test]
fn animation_status_errors_are_returned_to_the_owner() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"Object.defineProperty(globalThis,'__rmkAnimating',{set(value){},get(){throw Error('status failed')}})").unwrap();
    match ui.frame(connected(), 0, false) {
        Err(Error::JavaScript(message)) => assert!(message.contains("status failed")),
        _ => panic!("expected the status bridge error"),
    }
}

#[test]
fn native_animation_frames_skip_js_until_state_or_deadline_changes() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"globalThis.frameCalls=0;const originalFrame=globalThis.frame;globalThis.frame=(...args)=>{frameCalls++;return originalFrame(...args)}").unwrap();
    let mut state = connected();
    state.charging_left = true;
    for time in [0, 100_000, 200_000] {
        ui.frame(state.clone(), time, false).unwrap();
    }
    ui.evaluate(b"if(frameCalls!==1)throw Error('native animation called JS')")
        .unwrap();
    ui.frame(state.clone(), 700_000, false).unwrap();
    ui.evaluate(b"if(frameCalls!==2)throw Error('missed settling deadline')")
        .unwrap();
    state.key_labels[0].push_str("A").unwrap();
    state.presses = 1;
    ui.frame(state.clone(), 800_000, false).unwrap();
    ui.frame(state.clone(), 810_000, false).unwrap();
    assert!(ui.contains_text("A"));
    ui.evaluate(b"if(frameCalls!==3)throw Error('state update did not run once')")
        .unwrap();
    state.key_labels[0].clear();
    ui.frame(state.clone(), 820_000, false).unwrap();
    ui.frame(state, 1_300_001, false).unwrap();
    assert!(ui.contains_text("Ready"));
    ui.evaluate(b"if(frameCalls!==5)throw Error('missed typing deadline')")
        .unwrap();
}

#[test]
fn generic_frame_callbacks_run_without_keyboard_schedule_globals() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"delete globalThis.__rmkAnimating;delete globalThis.__rmkNextFrame;globalThis.frameCalls=0;globalThis.frame=()=>{frameCalls++}").unwrap();
    for time in [0, 20_000, 40_000] {
        ui.frame(connected(), time, false).unwrap();
    }
    ui.evaluate(b"if(frameCalls!==3)throw Error('generic frame stopped')")
        .unwrap();
}

#[test]
fn framework_work_keeps_an_idle_application_running() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"globalThis.__rmkAnimating=false;globalThis.__rmkNextFrame=Infinity;Object.defineProperty(globalThis,'__pocketNeedsFrame',{value:true,writable:true,configurable:true});globalThis.frameCalls=0;globalThis.frame=()=>{frameCalls++}").unwrap();
    for time in [0, 20_000, 40_000] {
        ui.frame(connected(), time, false).unwrap();
    }
    ui.evaluate(b"if(frameCalls!==3)throw Error('framework work stopped');globalThis.__pocketNeedsFrame=false").unwrap();
    ui.frame(connected(), 60_000, false).unwrap();
    assert!(!ui.animating());
    assert!(ui.frame(connected(), 80_000, false).unwrap().is_none());
}

#[test]
#[cfg(not(feature = "perf-baseline"))]
fn native_animation_waits_for_a_tick_but_input_is_immediate() {
    let mut ui = JsUi::new().unwrap();
    let mut model = connected();
    model.charging_left = true;
    ui.frame(model.clone(), 0, true).unwrap();
    ui.frame(model.clone(), 1_000_000, false).unwrap();
    assert_eq!(ui.next_frame_us(), Some(1_016_667));
    assert!(ui.frame(model.clone(), 1_000_001, false).unwrap().is_none());
    model.key_labels[0].push_str("A").unwrap();
    model.presses = 1;
    assert!(ui.frame(model.clone(), 1_000_002, false).unwrap().is_some());
    assert!(ui.contains_text("A"));
    model.key_labels[0].clear();
    ui.frame(model, 1_000_003, false).unwrap();
    assert!(ui.contains_text("Ready"));
}

#[test]
fn reload_owns_assets_and_keeps_current_keyboard_state() {
    let mut ui = JsUi::new().unwrap();
    let mut model = connected();
    model.layer = 2;
    model.key_labels[0].push_str("Enter").unwrap();
    ui.frame(model.clone(), 1_000_000, true).unwrap();
    for n in 0..8 {
        ui.reload(
            include_bytes!("../ui/generated/app.js"),
            include_bytes!("../ui/generated/app.pak").to_vec(),
            2_000_000 + n * 1_000_000,
        )
        .unwrap();
        assert!(ui.contains_text("Enter") && ui.contains_text("Connected"));
        ui.frame(model.clone(), 2_700_000 + n * 1_000_000, false)
            .unwrap();
        assert_eq!(ui.pixels(), ui.reference_pixels());
    }
}

#[test]
fn failed_reload_preserves_the_live_scene() {
    let mut ui = JsUi::new().unwrap();
    let state = connected();
    ui.frame(state.clone(), 1_000_000, true).unwrap();
    ui.frame(state.clone(), 2_000_000, false).unwrap();
    let before = ui.pixels().to_vec();
    for script in [
        b"let =".as_slice(),
        b"globalThis.frame = () => { throw Error('first frame failed'); }",
    ] {
        assert!(ui.reload(script, Vec::new(), 3_000_000).is_err());
        assert_eq!(ui.pixels(), before);
        assert!(ui.contains_text("Connected"));
    }
    ui.frame(state, 4_000_000, false).unwrap();
    assert_eq!(ui.pixels(), before);
}
