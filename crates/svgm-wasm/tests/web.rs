use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn optimize_default() {
    let svg =
        r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect width="10" height="10"/></g></svg>"#;
    let result = svgm_wasm::optimize(svg, JsValue::UNDEFINED).unwrap();
    let data = js_sys::Reflect::get(&result, &"data".into()).unwrap();
    assert!(data.is_string());
    let iterations = js_sys::Reflect::get(&result, &"iterations".into()).unwrap();
    assert!(iterations.as_f64().unwrap() >= 1.0);
}

#[wasm_bindgen_test]
fn optimize_with_preset() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#;
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"preset".into(), &"safe".into()).unwrap();
    let result = svgm_wasm::optimize(svg, opts.into()).unwrap();
    let data = js_sys::Reflect::get(&result, &"data".into()).unwrap();
    assert!(data.is_string());
}

#[wasm_bindgen_test]
fn optimize_with_pass_override() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><desc>Created with Figma</desc><rect width="10" height="10"/></svg>"#;
    let passes = js_sys::Object::new();
    js_sys::Reflect::set(&passes, &"removeDesc".into(), &JsValue::TRUE).unwrap();
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"passes".into(), &passes).unwrap();
    let result = svgm_wasm::optimize(svg, opts.into()).unwrap();
    let data = js_sys::Reflect::get(&result, &"data".into())
        .unwrap()
        .as_string()
        .unwrap();
    assert!(!data.contains("<desc>"));
}

#[wasm_bindgen_test]
fn invalid_svg_throws() {
    let result = svgm_wasm::optimize("<not valid xml", JsValue::UNDEFINED);
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn version_returns_string() {
    let v = svgm_wasm::version();
    assert!(!v.is_empty());
    assert!(v.contains('.'));
}
