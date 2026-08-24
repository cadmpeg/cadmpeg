// SPDX-License-Identifier: Apache-2.0

use super::super::*;

#[test]
fn round_edge_endpoint_coordinate_is_not_a_terminal_radius() {
    let mut body = vec![0x18];
    body.extend_from_slice(&[0x56, 0, 0, 0, 0, 0, 0]);
    body.push(0x12);
    body.extend_from_slice(&[0x6b, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f, 0xe4, 0x2f, 0x00, 0x00]);
    body.extend_from_slice(&[0x54, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a]);
    body.extend_from_slice(&[0xf7, 0x17]);
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let record = parameter_records(&payload).remove(0);

    assert!(record.type24_round_edge_envelope(0x24).is_some());
    assert!(record.type24_generated_round_radius(0x24).is_none());
}

#[test]
fn perpendicular_round_edge_uses_equal_endpoint_deltas_as_radius() {
    let mut body = vec![0x34, 0xe0, 0x00];
    body.extend_from_slice(&[0x56, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x00, 0x12, 0x68]);
    body.extend_from_slice(&[0x6b, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x0f, 0xe4, 0x2f, 0x00, 0x00]);
    body.extend_from_slice(&[0x0d, 0x2f, 0x00, 0x00, 0x0f]);
    body.extend_from_slice(&[0xf7, 0x17]);
    let record = SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    assert!(record.type24_round_edge_envelope(0x24).is_some());
    assert_eq!(record.type24_generated_round_radius(0x24), Some(1.0));
    assert!(record.type24_round_edge_envelope(0x25).is_none());
}

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

#[test]
fn axial_interval_corner_envelope_retains_all_radial_quadrants() {
    let push_first = |body: &mut Vec<u8>, value: f64| {
        let raw = value.to_be_bytes();
        assert_eq!(raw[0], 0x40);
        body.push(0x2d);
        body.extend_from_slice(&raw[1..]);
    };
    let push_second = |body: &mut Vec<u8>, value: f64| {
        let raw = value.to_be_bytes();
        assert_eq!(raw[0], 0x40);
        body.push(0x46);
        body.extend_from_slice(&raw[1..]);
    };
    let mut body = vec![0x18];
    push_first(&mut body, 2.0);
    body.push(0x12);
    push_first(&mut body, 8.0);
    for corner in [[12.0, 3.0, 5.0], [18.0, 7.0, 9.0]] {
        push_first(&mut body, corner[0]);
        push_second(&mut body, corner[1]);
        push_second(&mut body, corner[2]);
    }
    body.extend_from_slice(&[0xf7, 0x17]);

    let candidates =
        decode_type24_axial_interval_corner_candidates(&body, &scalar::ScalarCache::default())
            .expect("complete axial-interval corner envelope");
    assert_eq!(candidates.len(), 4);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.origin)
            .collect::<Vec<_>>(),
        [
            [10.0, 7.0, 9.0],
            [10.0, 7.0, 5.0],
            [10.0, 3.0, 5.0],
            [10.0, 3.0, 9.0],
        ]
    );
    assert!(candidates.iter().all(|candidate| {
        candidate.axis == [1.0, 0.0, 0.0]
            && candidate.radius == 4.0
            && candidate.length == Some(6.0)
    }));
}
