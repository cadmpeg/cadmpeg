// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

#[test]
fn finds_one_byte_and_two_byte_surface_rows() {
    let payload = [
        7, 0x22, 4, 0x01, 0, 0x80, 0x80, // plane id 7 -> 128
        0x80, 0x80, 0x24, 0x81, 0x01, 0xf6, 0x06, 7,
    ]; // cylinder id 128, feature 257, reversed -> 7
    let decoded = rows(&payload);
    assert_eq!(
        decoded,
        vec![
            SurfaceRow {
                id: 7,
                type_byte: 0x22,
                kind: SurfaceKind::Plane,
                feature_id: 4,
                reversed: false,
                boundary_type: 0,
                next_surface: 128,
                offset: 0,
            },
            SurfaceRow {
                id: 128,
                type_byte: 0x24,
                kind: SurfaceKind::Cylinder,
                feature_id: 257,
                reversed: true,
                boundary_type: 6,
                next_surface: 7,
                offset: 7,
            },
        ]
    );
    assert_eq!(
        unique_surface_row(&decoded, 7).map(|row| row.offset),
        Some(0)
    );
    let mut duplicate = decoded.clone();
    duplicate.push(decoded[0].clone());
    assert!(unique_surface_row(&duplicate, 7).is_none());
}

#[test]
fn accepts_type24_row_with_boundary_type_eight() {
    let payload = b"srf_array\0\xf8\x01\xae\x71\x24\xae\x5a\xf6\x08\0";
    let decoded = rows(payload);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].id, 11_889);
    assert_eq!(decoded[0].kind, SurfaceKind::Cylinder);
    assert_eq!(decoded[0].feature_id, 11_866);
    assert_eq!(decoded[0].boundary_type, 0x08);
}

#[test]
fn positional_spline_replay_uses_the_named_array_extents() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x28, 4, 0x01, 0, 8, 0xe3]);
    payload.extend_from_slice(b"srf_prim_ptr(splsrf)\0");
    payload.extend_from_slice(b"\xe0\x01tan_cond\0\xf8\x02\x03\xe4");
    for name in ["i_points", "end_u_tangts", "end_v_tangts", "end_uv_deriv"] {
        payload.extend_from_slice(&[0xe0, 0x02]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(b"\0\xf9\x04\x03");
        payload.extend(std::iter::repeat_n(0x0f, 12));
    }
    for name in ["u_params", "v_params"] {
        payload.extend_from_slice(&[0xe0, 0x01]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(b"\0\xf8\x02\x0f\xe4");
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x28, 4, 0x01, 0, 0, 0xe3, 0x03, 0xe4]);
    payload.extend(std::iter::repeat_n(0x0f, 48));
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let decoded_rows = rows(&payload);
    assert_eq!(decoded_rows.len(), 2);
    let later = decoded_rows.iter().find(|row| row.id == 8).unwrap();
    let prototype = positional_spline_replay_prototype(&payload, &decoded_rows, later).unwrap();
    assert_eq!(spline_replay_shape(&prototype).unwrap().point_count, 4);

    let parameters = parameter_records(&payload);
    let later_parameter = unique_surface_parameter(&parameters, 8).unwrap();
    assert_eq!(later_parameter.boundary, SurfaceBodyBoundary::CompoundClose);
    assert!(later_parameter.body.len() > 1);
    let cache = scalar::ScalarCache::from_section(&payload);
    let replay =
        decode_positional_spline_replay(&later_parameter.body, &prototype, &cache).unwrap();
    assert_eq!(replay.points.len(), 4);
    assert_eq!(replay.u_derivatives.len(), 4);
    assert_eq!(replay.v_derivatives.len(), 4);
    assert_eq!(replay.mixed_derivatives.len(), 4);
    assert_eq!(replay.u_parameters, [0.0, 1.0]);
    assert_eq!(replay.v_parameters, [0.0, 1.0]);
    let body_start = positional_body_start(&payload, later).unwrap();
    assert_eq!(
        positional_spline_replay_body_end(
            &payload,
            &decoded_rows,
            later,
            body_start,
            payload.len(),
            &cache,
        ),
        Some(later_parameter.body_offset + later_parameter.body.len())
    );
}

#[test]
fn cross_section_filters_boundary_one_body_candidate() {
    let payload =
        b"Sld_Xsections\0srf_array\0\xf8\x01\x07\x24\x04\x01\x06\0\x2d\x25\x32\xf6\x01\x01\xe2";

    assert!(rows(payload).is_empty());
    let rows = cross_section_rows(payload);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 7);
    assert_eq!(rows[0].boundary_type, 0x06);
    let parameters = cross_section_parameter_records(payload);
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].surface_id, 7);
    assert!(parameters[0]
        .body
        .windows(6)
        .any(|bytes| bytes == b"\x2d\x25\x32\xf6\x01\x01"));
}

#[test]
fn cross_section_plane_envelope_retains_its_namespace_geometry() {
    let payload = b"Sld_Xsections\0srf_array\0\xf8\x01\x07\x22\x04\x01\x06\0\xe4\xe4\xe4\xe4\x0f\x0f\x0f\xe4\x0f\xe4\xe3";

    let envelopes = cross_section_plane_envelopes(payload);
    assert_eq!(envelopes.len(), 1);
    let planes = outline_planes(&envelopes);
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].surface_id, 7);
    assert_eq!(planes[0].origin, [0.0, 0.0, 0.0]);
    assert_eq!(planes[0].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn plane_records_end_at_the_next_surface_family() {
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, // plane row
        0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3, // envelope
        8, 0x24, 4, 0x01, 0, 0, // cylinder row
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe3,
    ];
    let rows = rows(&payload);

    assert!(matches!(
        rows.as_slice(),
        [
            SurfaceRow {
                kind: SurfaceKind::Plane,
                ..
            },
            SurfaceRow {
                kind: SurfaceKind::Cylinder,
                ..
            }
        ]
    ));
    assert_eq!(plane_envelopes_for_rows(&payload, &rows).len(), 1);
    assert!(plane_local_systems_for_rows(&payload, &rows).is_empty());
}

#[test]
fn plane_local_system_follows_a_parameter_scalar_containing_a_named_record_header() {
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, // plane row
        0x5b, 0xc1, 0xab, 0x04, 0x64, 0x8d, 0x4f, 0x32, 0xe0, 0x1a, 0x1d, 0xa7, 0x0d, 0x5c, 0x0c,
        0x2d, 0x1b, 0xb6, 0xbb, 0xc4, 0x23, 0x5c, 0xc5, 0xa3, 0x0c, 0xb3, 0xbb, 0x89, 0xf1, 0x2c,
        0xc8, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0xd4, 0x2f, 0x31, 0x80, 0xdc, 0xaa, 0xa1, 0x13, 0xdd,
        0x13, 0xf1, 0xb6, 0x45, 0xd0, 0x67, 0x81, 0xe7, 0x8a, 0x2d, 0x37, 0x77, 0xb3, 0xe6, 0xb6,
        0xcc, 0xe5, 0x95, 0xaa, 0xa1, 0x13, 0xdd, 0x13, 0xef, 0xe3, // parameter body
        0x18, 0x28, 0xbf, 0x32, 0xd4, 0x4c, 0x4f, 0x62, 0xd3, 0xc2, 0xc2, 0xf0, 0x25, 0xa2, 0x3e,
        0x8b, 0x18, 0x7a, 0xc2, 0xf0, 0x25, 0xa2, 0x3e, 0x8b, 0x28, 0xbf, 0x32, 0xd4, 0x4c, 0x4f,
        0x62, 0xd3, 0x0f, 0x18, 0xe4, 0xc8, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0xd3, 0x2f, 0x31, 0x80,
        0xdd, 0xc2, 0xd6, 0x74, 0x69, 0xa5, 0x9b, 0xe3, // local system
        8, 0x24, 4, 0x01, 0, 0, // next surface row
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe3,
    ];
    let rows = rows(&payload);

    let systems = plane_local_systems_for_rows(&payload, &rows);
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0].surface_id, 7);
    assert_eq!(
        systems[0].origin,
        Some([-1.335_000_000_000_026_4, 17.5, -3.595_135_602_449_500_5])
    );
    assert_eq!(
        systems[0].u_axis,
        Some([0.0, 0.121_869_343_405_147_49, -0.992_546_151_641_322_1])
    );
    assert_eq!(systems[0].normal, Some([1.0, 0.0, 0.0]));
    assert_eq!(systems[0].body.len(), 52);
}

#[test]
fn compound_bounded_cylinder_local_system_retains_its_terminal_radius() {
    let body = [
        0xe3, // preceding envelope close
        0xc2, 0xc2, 0xf0, 0x25, 0xa2, 0x3e, 0x8e, 0x41, 0xbf, 0x32, 0xd4, 0x4c, 0x4f, 0x62, 0x28,
        0x18, 0x28, 0xbf, 0x32, 0xd4, 0x4c, 0x4f, 0x62, 0x28, 0xc2, 0xc2, 0xf0, 0x25, 0xa2, 0x3e,
        0x8e, 0x18, 0xe5, 0x0f, 0x45, 0x40, 0x15, 0xaa, 0x6c, 0xe9, 0x90, 0x2d, 0x37, 0x64, 0xc9,
        0x7b, 0x47, 0x11, 0xb1, 0x46, 0x30, 0x0d, 0x52, 0x7e, 0x52, 0x15, 0x76, 0x6e, 0x66, 0xd0,
        0x97, 0x1d, 0xc9, 0xe3, 0xe3, // radius and close
        0x82, 0x52, 0x01, // following row-local control payload
    ];

    let frame = decode_compound_local_system_cylinder_frame(&body, &scalar::ScalarCache::default())
        .expect("complete compound-bounded frame");
    assert_eq!(
        frame.origin,
        [
            -0.000_490_864_005_609_825_7,
            23.393_699_364_519_936,
            -16.052_039_999_999_998
        ]
    );
    assert_eq!(frame.axis, [0.0, 0.0, -1.0]);
    assert_eq!(
        frame.ref_direction,
        [-0.992_546_151_641_322_3, 0.121_869_343_405_145_1, 0.0]
    );
    assert_eq!(frame.radius, 0.606_300_635_480_064_5);
    assert_eq!(frame.length, None);
}

#[test]
fn rejects_duplicate_surface_ids() {
    let duplicate_ids = [
        7, 0x22, 4, 0x01, 0, 0, // first id 7
        7, 0x24, 4, 0x01, 0, 0, // second id 7
    ];
    assert!(rows(&duplicate_ids).is_empty());
}

#[test]
fn unique_surface_projection_excludes_every_collided_identity() {
    let row = |id, offset| SurfaceRow {
        id,
        type_byte: 0x22,
        kind: SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset,
    };
    let rows = [row(7, 10), row(8, 20), row(7, 30)];

    assert_eq!(
        uniquely_identified_rows(&rows)
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [8]
    );
}

#[test]
fn surface_array_frame_excludes_following_curve_namespace_bytes() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf8\x00");
    payload.extend_from_slice(&[8, 0x24, 5, 0x01, 0, 0]);

    assert_eq!(
        rows(&payload).iter().map(|row| row.id).collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn sparse_surface_array_retains_rows_but_not_complete_frame() {
    let mut payload = b"srf_array\0\xf8\x03".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[8, 0x24, 5, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf8\x00");

    assert_eq!(
        rows(&payload).iter().map(|row| row.id).collect::<Vec<_>>(),
        [7, 8]
    );
    assert!(counted_row_bounds(&payload).is_empty());
    assert!(complete_surface_array_bounds(&payload).is_empty());
}

#[test]
fn named_prototype_parameter_body_cannot_start_a_surface_row() {
    let payload = b"srf_array\0\xf8\x01srf_prim_ptr(torus)\0\xe3\
        \xe0\x02radius1\0\x07\x26\x04\x01\x00\x00\
        \xe0\x02radius2\0\xe4\xe3";

    assert!(rows(payload).is_empty());
}

#[test]
fn signed_surface_dict_scalar_owns_its_tail() {
    let body = [0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0];
    let tokens = scalar_tokens(
        SurfaceKind::TorusOrSphere,
        &body,
        &scalar::ScalarCache::default(),
    );

    assert_eq!(tokens.len(), 1);
    assert_eq!(
        tokens[0].value,
        Some(f64::from_be_bytes([
            0x3f, 0xe8, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0
        ]))
    );
    assert_eq!(tokens[0].offset, 0);
    assert_eq!(tokens[0].length, 7);
    assert_eq!(tokens[0].raw, body);
}

#[test]
fn rejects_rows_without_the_fixed_discriminators() {
    assert!(rows(&[7, 0x22, 4, 0x02, 0, 8]).is_empty());
    assert!(rows(&[7, 0x22, 4, 0x01, 0x20, 8]).is_empty());
}

#[test]
fn decodes_named_prototype_scalars_without_promoting_them_to_instances() {
    let payload = b"srf_prim_ptr\0geom_type\0\x24radius\0\x2a\xf4\0\
                    srf_prim_ptr\0geom_type\0\x25half_angle\0\x74\x21\xfb\x54\x44\x2d\x23";
    assert_eq!(prototype_count(payload), 2);
}

#[test]
fn bounds_last_named_prototype_field_at_compound_close() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3\
                    \x07\x26\x04\x01\0\0";
    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].parameters.len(), 1);
    let field = &records[0].parameters[0];
    assert_eq!(field.name, "radius2");
    assert_eq!(field.body, [0x2e, 0x05, 0x33, 0xf1, 0xf7, 0x0e]);
    assert_eq!(field.value, SurfaceNamedValue::ScalarSequence(vec![2.65]));
}

#[test]
fn parenthesized_prototype_ends_at_legacy_prototype_record() {
    let payload = b"srf_prim_ptr(plane)\0\xe0\x02local_sys\0\xf9\x04\x03\
        \x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
        \xe0\x00srf_prim_ptr\0geom_type\0\x24\
        \xe0\x02radius\0\x2f\x05\x00";

    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].family, SurfacePrototypeFamily::Plane);
    assert_eq!(records[0].parameters.len(), 1);
    assert_eq!(records[0].parameters[0].name, "local_sys");
}

#[test]
fn parenthesized_prototype_ends_at_peer_entity_record() {
    let payload = b"srf_prim_ptr(plane)\0\xe0\x02local_sys\0\xf9\x04\x03\
        \x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
        \xe0\x00entity_ptr(coord_sys)\0\xe3\
        \xe0\x02radius\0\x2f\x05\x00";

    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].parameters.len(), 1);
    assert_eq!(records[0].parameters[0].name, "local_sys");
}

#[test]
fn analytic_prototype_does_not_claim_nested_curve_parameters() {
    let payload = b"srf_prim_ptr(torus)\0\
        \xe0\x02local_sys\0\xf9\x04\x03\x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
        \xe0\x02radius1\0\x18\
        \xe0\x02radius2\0\x2f\x05\x00\
        \xe0\x00curve(b_spline)\0\xe3\
        \xe0\x00c_pnts\0\xf8\x04\xf7\x50\xfb";

    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["local_sys", "radius1", "radius2"]
    );
}

#[test]
fn terminal_zero_decodes_in_a_bounded_named_scalar_field() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe3";
    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].parameters.len(), 1);
    assert_eq!(
        records[0].parameters[0].value,
        SurfaceNamedValue::ScalarSequence(vec![0.0])
    );
}

#[test]
fn summarizes_parenthesized_analytic_prototypes() {
    let payload =
        b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3";

    assert_eq!(prototype_count(payload), 1);
}

#[test]
fn distinguishes_spline_and_fillet_surface_families() {
    let payload = b"srf_prim_ptr(splsrf)\0\xe3srf_prim_ptr(fillet_srf)\0\xe3";
    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].family, SurfacePrototypeFamily::Spline);
    assert_eq!(records[1].family, SurfacePrototypeFamily::Fillet);
    assert_eq!(prototype_count(payload), 2);
}

#[test]
fn retains_named_spline_point_and_tangent_arrays() {
    let payload = b"srf_prim_ptr(splsrf)\0\
        \xe0\x02i_points\0\xf9\x02\x02\xe4\x0f\xe4\x0f\
        \xe0\x02end_u_tangts\0\xf9\x01\x02\x0f\xe4\
        \xe0\x02u_params\0\xf8\x02\x0f\xe4\xe3";
    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].family, SurfacePrototypeFamily::Spline);
    assert_eq!(
        records[0].field("i_points").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray {
            dimensions: 2,
            count: 2,
            values: vec![Some(1.0), Some(0.0), Some(1.0), Some(0.0)],
            tokens: vec![vec![0xe4], vec![0x0f], vec![0xe4], vec![0x0f]],
        })
    );
    assert_eq!(
        records[0].field("end_u_tangts").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray {
            dimensions: 1,
            count: 2,
            values: vec![Some(0.0), Some(1.0)],
            tokens: vec![vec![0x0f], vec![0xe4]],
        })
    );
    assert_eq!(
        records[0].field("u_params").map(|field| &field.value),
        Some(&SurfaceNamedValue::CountedScalarArray {
            count: 2,
            values: vec![Some(0.0), Some(1.0)],
            tokens: vec![vec![0x0f], vec![0xe4]],
        })
    );
}

#[test]
fn spline_slots_consume_unresolved_tokens_without_scanning_their_payloads() {
    let body = [0xaa, 0xe4, 1, 2, 3, 4, 5, 0xe4];
    let slots = named_spline_scalar_slots(
        &SurfacePrototypeFamily::Spline,
        "tangts",
        &body,
        2,
        &scalar::ScalarCache::default(),
    );

    assert_eq!(
        slots,
        [
            (None, vec![0xaa, 0xe4, 1, 2, 3, 4, 5]),
            (Some(1.0), vec![0xe4]),
        ]
    );
}

#[test]
fn interpolation_point_aliases_expand_continuation_and_terminal_zero() {
    let body = [0xe4, 0x0f, 0xe4, 0xf9, 0x00, 0x2f, 0x14, 0x00, 0x18];
    for name in ["i_pnts", "i_points"] {
        let slots = named_spline_scalar_slots(
            &SurfacePrototypeFamily::Spline,
            name,
            &body,
            6,
            &scalar::ScalarCache::default(),
        );
        assert_eq!(
            slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
            [
                Some(1.0),
                Some(0.0),
                Some(1.0),
                Some(5.0),
                Some(0.0),
                Some(0.0)
            ]
        );
        assert_eq!(slots[3].1, [0x2f, 0x14, 0x00]);
        assert!(slots[5].1.is_empty());
    }
}

#[test]
fn spline_tangents_use_the_signed_coordinate_dict_lattice() {
    let body = [
        0xce, 1, 2, 3, 4, 5, 6, 0x2d, 1, 2, 3, 4, 5, 6, 7, 0x46, 1, 2, 3, 4, 5, 6, 7,
    ];
    for name in ["end_v_tangts", "end_tangts"] {
        let slots = named_spline_scalar_slots(
            &SurfacePrototypeFamily::Spline,
            name,
            &body,
            3,
            &scalar::ScalarCache::default(),
        );

        assert_eq!(
            slots[0].0,
            Some(f64::from_be_bytes([0xbf, 0xfb, 1, 2, 3, 4, 5, 6]))
        );
        assert_eq!(slots[0].1, body[..7]);
        assert_eq!(
            slots[1].0,
            Some(f64::from_be_bytes([0xc0, 1, 2, 3, 4, 5, 6, 7]))
        );
        assert_eq!(slots[1].1, body[7..15]);
        assert_eq!(
            slots[2].0,
            Some(f64::from_be_bytes([0x40, 1, 2, 3, 4, 5, 6, 7]))
        );
        assert_eq!(slots[2].1, body[15..]);
    }
}

#[test]
fn tabulated_cylinder_parameters_end_the_tangent_field() {
    let payload = b"srf_prim_ptr(tab_cyl)\0\
        \xe0\x02end_tangts\0\xf9\x02\x03\x0f\xe4\x0f\xe4\x0f\x18\
        \xe0\x02params\0\xf8\x03\x0f\
        \x2d\x00\x00\x00\x00\x00\x00\x00\
        \x2d\x08\x00\x00\x00\x00\x00\x00\xe3";
    let records = named_prototype_records(payload);

    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].field("end_tangts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray { values, .. }) if values.len() == 6
    ));
    assert_eq!(
        records[0].field("params").map(|field| &field.value),
        Some(&SurfaceNamedValue::CountedScalarArray {
            count: 3,
            values: vec![Some(0.0), Some(2.0), Some(3.0)],
            tokens: vec![
                vec![0x0f],
                vec![0x2d, 0, 0, 0, 0, 0, 0, 0],
                vec![0x2d, 8, 0, 0, 0, 0, 0, 0],
            ],
        })
    );
}

#[test]
fn tabulated_cylinder_control_point_field_expands_contiguous_ids() {
    let payload = b"srf_prim_ptr(tab_cyl)\0\
        \xe0\x00c_pnts\0\xf8\x04\xf7\x50\xfb";

    let records = named_prototype_records(payload);

    assert_eq!(
        records[0].tabulated_cylinder_control_point_ids(),
        Some([80, 81, 82, 83])
    );
}

#[test]
fn counted_parameters_expand_compact_zero_runs() {
    let body = [0xe4, 0xe5, 0x0f, 0xe6];

    assert_eq!(
        counted_parameter_scalar_slots(&body, 7, &scalar::ScalarCache::default()),
        Some(vec![
            (Some(1.0), vec![0xe4]),
            (Some(0.0), vec![0xe5]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![0x0f]),
            (Some(0.0), vec![0xe6]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![]),
        ])
    );
}

#[test]
fn counted_parameters_use_the_exact_extent_to_select_cache_or_zero() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);

    assert_eq!(
        counted_parameter_scalar_slots(&[0x18, 0x00], 1, &cache),
        Some(vec![(Some(2.0), vec![0x18, 0x00])])
    );
    assert_eq!(
        counted_parameter_scalar_slots(&[0x18, 0, 1, 2, 3, 4, 5, 6], 2, &cache),
        Some(vec![
            (Some(0.0), vec![0x18]),
            (
                Some(f64::from_be_bytes([0x40, 0x75, 1, 2, 3, 4, 5, 6])),
                vec![0, 1, 2, 3, 4, 5, 6],
            ),
        ])
    );
}

#[test]
fn counted_parameters_reject_multiple_complete_tokenizations() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    let body = [0x18, 0, 0xe5, 0x29, 0x18, 4, 0x29, 5, 0xe6];

    assert_eq!(counted_parameter_scalar_slots(&body, 5, &cache), None);
}

#[test]
fn counted_parameters_require_exact_zero_run_cardinality() {
    let body = [0xe4, 0xe5, 0x0f, 0xe6];

    assert_eq!(
        counted_parameter_scalar_slots(&body, 6, &scalar::ScalarCache::default()),
        None
    );
    assert_eq!(
        counted_parameter_scalar_slots(&body, 8, &scalar::ScalarCache::default()),
        None
    );
}

#[test]
fn tabulated_cylinder_frame_owns_compound_close_bytes_inside_scalars() {
    let mut body = vec![0x00, 0x0c, 0x9a];
    body.extend_from_slice(&[0x4a, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00]);
    body.extend_from_slice(&[0xe4, 0x0f]);
    body.extend_from_slice(&[0x4a, 0x13, 0x1f, 0x1c, 0x0b, 0x00, 0x00]);
    body.extend_from_slice(&[0xe4, 0x0f, 0xf7, 0x23, 0xe3]);

    let (frame, frame_end) =
        decode_tabulated_cylinder_frame(&body, &scalar::ScalarCache::default())
            .expect("complete tabulated-cylinder frame");
    assert_eq!(frame.prefixes, [0x4a, 0xe4, 0x0f, 0x4a, 0xe4, 0x0f]);
    assert_eq!(frame_end, body.len() - 3);

    assert_eq!(
        surface_body_compound_close(
            SurfaceKind::Extrusion,
            &body,
            &scalar::ScalarCache::default(),
        ),
        Some(body.len() - 1)
    );
}

#[test]
fn tabulated_cylinder_zero_sweep_bound_does_not_consume_the_next_slot() {
    let body = [
        0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x46, 0x15, 0x64, 0x7b, 0x0d, 0xc3, 0x21, 0xe2, 0x42,
        0xb9, 0x99, 0x78, 0x6b, 0xf6, 0xdd, 0x26, 0xcc, 0x10, 0x4a, 0x14, 0x70, 0xf7, 0x8b, 0x00,
        0x00, 0x18, 0x7b, 0x59, 0x2f, 0x66, 0xa2, 0x53, 0xc6,
    ];

    let (frame, end) = decode_tabulated_cylinder_frame(&body, &scalar::ScalarCache::default())
        .expect("complete zero-bound frame");

    assert_eq!(frame.prefixes, [0x46, 0x42, 0x78, 0x4a, 0x18, 0x7b]);
    assert_eq!(frame.values[4], 0.0);
    assert_eq!(end, body.len());
}
