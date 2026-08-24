// SPDX-License-Identifier: Apache-2.0

use super::super::*;

#[test]
fn selector_corner_interval_cylinders_resolve_axis_origin_and_radius() {
    let build = |selectors: [u8; 2], values: [f64; 8]| {
        let mut body = vec![selectors[0]];
        for (index, value) in values.into_iter().enumerate() {
            if index == 1 {
                body.push(selectors[1]);
            }
            let raw = value.to_be_bytes();
            assert_eq!(raw[0], 0x40, "test value uses the positive directrix form");
            body.push(0x2d);
            body.extend_from_slice(&raw[1..]);
        }
        body.extend_from_slice(&[0xf7, 0x17]);
        body
    };
    let cache = scalar::ScalarCache::default();
    let forward = build([0x12, 0x11], [2.0, 8.0, 12.0, 3.0, 5.0, 18.0, 7.0, 9.0]);
    assert_eq!(
        decode_selector_corner_interval_cylinder_frame(&forward, &cache),
        Some(PositionalCylinderFrame {
            origin: [10.0, 7.0, 9.0],
            axis: [1.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 4.0,
            length: Some(6.0),
        })
    );
    let mut split_second_selector = forward.clone();
    split_second_selector.splice(9..10, [0x00, 0x11, 0x13]);
    assert_eq!(
        decode_selector_corner_interval_cylinder_frame(&split_second_selector, &cache),
        decode_selector_corner_interval_cylinder_frame(&forward, &cache)
    );
    let mut placeholder_corner = forward.clone();
    placeholder_corner.splice(58..66, [0x92, 0, 0, 0, 0, 0, 1]);
    assert_eq!(
        decode_selector_corner_interval_cylinder_frame(&placeholder_corner, &cache),
        decode_selector_corner_interval_cylinder_frame(&forward, &cache)
    );

    let reversed = build([0x14, 0x13], [2.0, 8.0, 18.0, 3.0, 5.0, 12.0, 7.0, 9.0]);
    assert_eq!(
        decode_selector_corner_interval_cylinder_frame(&reversed, &cache),
        Some(PositionalCylinderFrame {
            origin: [20.0, 3.0, 5.0],
            axis: [-1.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 4.0,
            length: Some(6.0),
        })
    );
    let mut split_first_selector = build([0x13, 0x12], [2.0, 8.0, 18.0, 3.0, 5.0, 12.0, 7.0, 9.0]);
    split_first_selector.splice(0..1, [0x00, 0x13, 0x1a]);
    assert!(
        decode_selector_corner_interval_cylinder_frame(&split_first_selector, &cache).is_some()
    );

    let unequal_transverse_spans = build([0x12, 0x11], [2.0, 8.0, 12.0, 3.0, 5.0, 18.0, 7.0, 10.0]);
    assert!(
        decode_selector_corner_interval_cylinder_frame(&unequal_transverse_spans, &cache).is_none()
    );
}
