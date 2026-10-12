// SPDX-License-Identifier: Apache-2.0
//! GUI state identity text admission.

#[test]
fn gui_state_decimal_order_key_is_scoped_and_keeps_identity() {
    let xml = "<Camera/>";
    let document = roxmltree::Document::parse(xml).unwrap();
    crate::test_support::materialized_refusal_at("FCStd GUI state identity key", |ctx| {
        crate::gui::gui_state(ctx, xml, 255, document.root_element())
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let state = crate::gui::gui_state(ctx, xml, 255, document.root_element()).unwrap();
        assert_eq!(state.id, "fcstd:native:gui-state#Camera%3A255");
        assert_eq!(state.kind, "Camera");
    });
}
