mod app;

use wasm_bindgen::prelude::*;
use web_sys::Element;

#[wasm_bindgen]
pub fn run_app(
    element: Element,
    admin_ws_js: JsValue,
    app_ws_js: JsValue,
    cell_id_js: JsValue,
) -> Result<(), JsValue> {
    let props = app::ModelProps {
        admin_ws_js,
        app_ws_js,
        cell_id_js,
    };
    yew::start_app_with_props_in_element::<app::Model>(element, props);
    Ok(())
}
