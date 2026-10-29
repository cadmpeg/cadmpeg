// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::super::{parse_property_set_stream, PropertyValue};

#[test]
fn callbacks_construct_values_from_original_nonzero_origin_views() {
    let payload = super::fixture();
    let mut bytes = b"prefix!!".to_vec();
    bytes.extend_from_slice(&payload);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual source context");
    let source = root.child(8, bytes.len()).expect("actual property stream");
    let parsed = parse_property_set_stream(&ctx, source).expect("actual callbacks parse metadata");
    assert_eq!(parsed.sections.len(), 1);
    assert_eq!(parsed.sections[0].properties.len(), 3);
    assert!(matches!(
        &parsed.sections[0].properties[1].value,
        PropertyValue::String { value, .. } if value == "Synthetic title"
    ));
    assert!(parsed.sections[0].properties[2]
        .raw
        .window()
        .ends_with(b"synthetic\0\0\0"));
    for property in &parsed.sections[0].properties {
        assert!(property.raw.start() >= 8 + 48 + 32);
        assert!(property.raw.end() <= bytes.len());
    }
}
