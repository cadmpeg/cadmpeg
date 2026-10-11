// SPDX-License-Identifier: Apache-2.0
//! Dictionary prefix and decimal element resolution.

#[test]
fn mapped_dictionary_decimal_element_is_retained_with_its_prefix() {
    let postfixes = ["Face".to_owned()];
    let encoded = ":1.ff.0.a";
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD mapped name base", |ctx| {
        crate::element_map::parse_mapped_name(ctx, encoded, &postfixes)
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mapped = crate::element_map::parse_mapped_name(ctx, encoded, &postfixes).unwrap();
        assert_eq!(mapped.encoded, encoded);
        assert_eq!(mapped.resolved.as_deref(), Some("Face255"));
        assert_eq!(mapped.string_ids, [10]);
    });
}
