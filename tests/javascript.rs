use pocketjs_embassy::{Error, JsUi};

#[test]
fn solid_signal_changes_native_text_and_pixels() {
    let mut ui = JsUi::new().unwrap();
    assert!(ui.contains_text("0"));
    let before = ui.frame().unwrap().to_vec();
    ui.evaluate(b"setKeyCount(7)").unwrap();
    let after = ui.frame().unwrap().to_vec();
    assert!(ui.contains_text("7"));
    assert_ne!(before, after);
}

#[test]
fn javascript_exception_reaches_rust() {
    let mut ui = JsUi::new().unwrap();
    match ui.evaluate(b"throw Error('probe failure')") {
        Err(Error::JavaScript(message)) => assert!(message.contains("probe failure")),
        other => panic!("expected JS exception, got {other:?}"),
    }
}

#[test]
fn runaway_script_is_interrupted_and_guest_can_continue() {
    let mut ui = JsUi::new().unwrap();
    match ui.evaluate(b"while (true) {}") {
        Err(Error::JavaScript(message)) => assert!(message.contains("interrupted")),
        other => panic!("expected interruption, got {other:?}"),
    }
    ui.evaluate(b"setKeyCount(3)").unwrap();
    ui.frame().unwrap();
    assert!(ui.contains_text("3"));
}

#[test]
fn promise_jobs_and_integer_conversion_update_solid() {
    let mut ui = JsUi::new().unwrap();
    ui.evaluate(b"Promise.all([Promise.resolve(parseInt('7', 10)), Promise.resolve(2)]).then(values => setKeyCount(values[0] + values[1]))").unwrap();
    ui.frame().unwrap();
    assert!(ui.contains_text("9"));
}

#[test]
fn borrowed_assets_reject_writes_and_allow_readonly_views() {
    let mut ui = JsUi::new().unwrap();
    let pixels = ui.frame().unwrap().to_vec();
    for source in [
        "new Uint8Array(__pak).reverse()",
        "let values = new Uint8Array([1]); values.constructor = { [Symbol.species]: function() { return new Uint8Array(__pak); } }; values.map(x => x)",
    ] {
        assert!(matches!(
            ui.evaluate(source.as_bytes()),
            Err(Error::JavaScript(_))
        ));
    }
    ui.evaluate(b"if (new Uint8Array(__pak).subarray(0, 1).length !== 1) throw Error('view')")
        .unwrap();
    assert_eq!(pixels, ui.frame().unwrap());
}

#[test]
fn javascript_heap_limit_rejects_oversized_allocation() {
    let mut ui = JsUi::new().unwrap();
    assert!(matches!(
        ui.evaluate(b"new ArrayBuffer(8 * 1024 * 1024)"),
        Err(Error::JavaScript(_))
    ));
}
