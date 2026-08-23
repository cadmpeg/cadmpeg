// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::BTreeSet;

fn parameter_record(curve_id: u32, suffix: CurveSuffixStatus) -> CurveParameterRecord {
    CurveParameterRecord {
        curve_id,
        type_byte: 0,
        body: Vec::new(),
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        skipped_references: Vec::new(),
        references: Vec::new(),
        opaque_spans: Vec::new(),
        reference_geometry: [0, 0],
        suffix,
        offset: curve_id as usize,
        body_offset: curve_id as usize,
        suffix_offset: curve_id as usize,
    }
}

#[test]
fn typed_parameter_rows_require_unique_identity_and_suffix_boundary() {
    let unique = parameter_record(7, CurveSuffixStatus::Unique);
    assert_eq!(
        uniquely_bounded_parameter_records(std::slice::from_ref(&unique)).len(),
        1
    );

    let ambiguous = parameter_record(8, CurveSuffixStatus::Ambiguous { candidate_count: 2 });
    assert!(uniquely_bounded_parameter_records(&[ambiguous]).is_empty());
    assert!(uniquely_bounded_parameter_records(&[unique.clone(), unique]).is_empty());
}

#[test]
fn pcurve_endpoint_slots_must_be_finite() {
    let nan = [0xed, 0x7f, 0xf8, 0, 0, 0, 0, 0, 0];
    let mut record = parameter_record(7, CurveSuffixStatus::Unique);
    record.body.extend_from_slice(&nan);
    record.body.extend([0x0f; 7]);
    record.scalar_values.push(f64::NAN);
    record.scalar_tokens.push(CurveParameterScalar {
        value: f64::NAN,
        raw: nan.to_vec(),
        offset: 0,
        length: nan.len(),
    });
    for offset in nan.len()..record.body.len() {
        record.scalar_values.push(0.0);
        record.scalar_tokens.push(CurveParameterScalar {
            value: 0.0,
            raw: vec![0x0f],
            offset,
            length: 1,
        });
    }
    let topology = CurveTopologyRow {
        id: 7,
        type_byte: 0,
        feature_id: 1,
        directions: [1, 1],
        faces: [2, 3],
        next_edges: [7, 7],
        offset: 1,
    };

    assert!(pcurve_endpoints(&[record], &[topology]).is_empty());
}

#[test]
fn decodes_canonical_and_positional_two_chart_sample_rows() {
    let samples = [
        0x0f, 0xe4, 0x0d, 0x18, // point 0
        0xe4, 0x0f, 0x18, 0x0d, // point 1
        0x0d, 0x18, 0xe4, 0x0f, // point 2
    ];
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 3]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 8, 9, 0, 0, 0xe3, 0xe1, 0xe3]);
    payload.extend_from_slice(&[8, 0, 4, 0xf6, 1]);
    payload.extend_from_slice(&samples);
    payload.extend_from_slice(&[10, 11, 9, 7, 0, 0, 0xe3, 0xe1, 0xe3]);

    let face_ids = BTreeSet::from([10, 11]);
    let decoded = two_chart_pcurve_samples(&payload, Some(&face_ids));
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].curve_id, 7);
    assert_eq!(decoded[0].faces, [10, 11]);
    assert_eq!(decoded[0].samples.len(), 3);
    assert_eq!(decoded[0].samples[0], [[0.0, 1.0], [-1.0, 0.0]]);
    assert_eq!(decoded[0].samples[2], [[-1.0, 0.0], [1.0, 0.0]]);
    assert_eq!(decoded[1].curve_id, 8);
    assert_eq!(decoded[1].samples, decoded[0].samples);
}

#[test]
fn two_chart_sample_rows_require_exact_counted_consumption() {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[
        7, 0, 4, 1, 0xf6, 0xfc, 2, 0x0f, 0xe4, 0x0d, 0x18, 0xe4, 0x0f, 0x18, 0x0d,
        0xff, // unclaimed body byte
        10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
    ]);

    let face_ids = BTreeSet::from([10, 11]);
    assert!(two_chart_pcurve_samples(&payload, Some(&face_ids)).is_empty());
}

#[test]
fn decodes_only_complete_fc02_short_pcurve_endpoints() {
    let token_specs = [
        (-14.5, vec![0x48, 0x45, 0x00]),
        (0.75, vec![0x2a, 0xe8, 0x00]),
        (0.0, vec![0x18]),
        (1.0, vec![0xe4]),
        (-12.5, vec![0x48, 0x41, 0x00]),
        (0.75, vec![0x2a, 0xe8, 0x00]),
        (2.0, vec![0x29, 0xff, 0xff]),
    ];
    let mut body = vec![0xfc, 0x02];
    let mut scalar_tokens = Vec::new();
    for (value, raw) in token_specs {
        let offset = body.len();
        body.extend_from_slice(&raw);
        scalar_tokens.push(CurveParameterScalar {
            value,
            raw,
            offset,
            length: body.len() - offset,
        });
    }
    body.extend_from_slice(&[0x34, 0xb0, 0x00]);
    let record = CurveParameterRecord {
        curve_id: 846,
        type_byte: 0,
        scalar_values: scalar_tokens.iter().map(|token| token.value).collect(),
        opaque_spans: vec![
            CurveParameterOpaqueSpan {
                raw: vec![0xfc, 0x02],
                offset: 0,
                length: 2,
            },
            CurveParameterOpaqueSpan {
                raw: vec![0x34, 0xb0, 0x00],
                offset: body.len() - 3,
                length: 3,
            },
        ],
        body,
        scalar_tokens,
        ..parameter_record(846, CurveSuffixStatus::Unique)
    };
    let topology = CurveTopologyRow {
        id: 846,
        type_byte: 0,
        feature_id: 57,
        directions: [0x01, 0xf6],
        faces: [43, 163],
        next_edges: [841, 164],
        offset: 100,
    };

    assert_eq!(
        fc02_short_pcurve_endpoints(
            std::slice::from_ref(&record),
            std::slice::from_ref(&topology),
        ),
        vec![Fc02ShortPcurveEndpoints {
            curve_id: 846,
            faces: [43, 163],
            face_0_endpoints: [[-14.5, 0.75], [-12.5, 0.75]],
            offset: 846,
        }]
    );

    let mut malformed = record.clone();
    malformed.scalar_values[3] = 2.0;
    malformed.scalar_tokens[3].value = 2.0;
    assert!(fc02_short_pcurve_endpoints(&[malformed], std::slice::from_ref(&topology)).is_empty());

    let mut malformed = record;
    malformed.scalar_tokens[6].raw[2] = 0xfe;
    assert!(fc02_short_pcurve_endpoints(&[malformed], &[topology]).is_empty());
}

#[test]
fn finds_labeled_prototypes_in_concatenated_namespaces() {
    let payload = b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04\
                   crv_array\0crv_id\0\x80\x80type\0\x01";
    assert_eq!(
        prototypes(payload),
        vec![
            CurvePrototype {
                id: 7,
                type_byte: 8,
                feature_id: Some(4),
                directions: None,
                offset: 0,
            },
            CurvePrototype {
                id: 128,
                type_byte: 1,
                feature_id: None,
                directions: None,
                offset: 33,
            },
        ]
    );
}

#[test]
fn ignores_incomplete_labeled_rows() {
    assert!(prototypes(b"crv_array\0crv_id\0\x07").is_empty());
}

#[test]
fn promotes_only_referenced_unique_prototype_topology() {
    let prototypes = [CurvePrototype {
        id: 44,
        type_byte: 0,
        feature_id: Some(40),
        directions: Some([0x01, 0xf6]),
        offset: 100,
    }];
    let prototype_topology = [CurvePrototypeTopology {
        curve_id: 44,
        faces: [43, 141],
        next_edges: [271, 142],
        offset: 100,
    }];
    let positional_rows = [CurveTopologyRow {
        id: 605,
        type_byte: 0,
        feature_id: 547,
        directions: [0x01, 0xf6],
        faces: [43, 235],
        next_edges: [44, 597],
        offset: 200,
    }];
    assert_eq!(
        prototype_topology_rows(
            &prototypes,
            &prototype_topology,
            &positional_rows,
            &BTreeSet::from([43, 141, 235]),
        ),
        vec![CurveTopologyRow {
            id: 44,
            type_byte: 0,
            feature_id: 40,
            directions: [0x01, 0xf6],
            faces: [43, 141],
            next_edges: [271, 142],
            offset: 100,
        }]
    );

    assert!(prototype_topology_rows(
        &prototypes,
        &prototype_topology,
        &positional_rows,
        &BTreeSet::from([43, 235]),
    )
    .is_empty());
}

#[test]
fn decodes_only_complete_explicit_curve_expression_frames() {
    let complete = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    assert_eq!(
        expression_records(complete)[0]
            .local_system
            .as_ref()
            .and_then(|frame| frame.explicit_slots),
        Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );

    let inherited = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    assert_eq!(
        expression_records(inherited)[0]
            .local_system
            .as_ref()
            .and_then(|frame| frame.explicit_slots),
        Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn decodes_compact_curve_expression_frame_extents() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x80\x88\x03\x0f\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    let records = expression_records(payload);
    let frame = records[0].local_system.as_ref().expect("local system");
    assert_eq!(frame.dimensions, 136);
    assert_eq!(frame.count, 3);
    assert_eq!(frame.body, [0x0f]);
    assert_eq!(frame.explicit_slots, None);
}

#[test]
fn recognizes_only_affine_cylindrical_helix_programs() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x05unused=external\0r=5\0theta=90+t*720\0z=-2+20*t\0note=external+1\0";
    let records = expression_records(payload);
    assert_eq!(
        expression_helix(&records[0]),
        Some(CurveExpressionHelix {
            radius: 5.0,
            height: 20.0,
            z_start: -2.0,
            revolutions: 2.0,
            start_angle: std::f64::consts::FRAC_PI_2,
            clockwise: false,
        })
    );

    let constant_functions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x03r=sqrt(25)\0theta=atan(1)+t*360\0z=t*pow(2,3)\0";
    assert_eq!(
        expression_helix(&expression_records(constant_functions)[0]),
        Some(CurveExpressionHelix {
            radius: 5.0,
            height: 8.0,
            z_start: 0.0,
            revolutions: 1.0,
            start_angle: std::f64::consts::FRAC_PI_4,
            clockwise: false,
        })
    );

    let identity_powers = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x03r=5^1\0theta=t^1*360\0z=8*t^1\0";
    assert_eq!(
        expression_helix(&expression_records(identity_powers)[0]),
        Some(CurveExpressionHelix {
            radius: 5.0,
            height: 8.0,
            z_start: 0.0,
            revolutions: 1.0,
            start_angle: 0.0,
            clockwise: false,
        })
    );

    let nonlinear = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=t*t*360\0z=20*t\0";
    assert!(expression_helix(&expression_records(nonlinear)[0]).is_none());

    let sample_alias = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x09\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=360*t+t*(t-0.5)*(t-1)\0z=20*t\0";
    assert!(expression_helix(&expression_records(sample_alias)[0]).is_none());
}

#[test]
fn decodes_a_uniquely_delimited_topology_suffix() {
    let payload = [
        b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0, 7,
        8, 4, 1, 0xf6, 0x29, 0x43, 0, // opaque row body
        10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
    ];
    assert_eq!(
        topology_rows(&payload),
        vec![CurveTopologyRow {
            id: 7,
            type_byte: 8,
            feature_id: 4,
            directions: [1, 0xf6],
            faces: [10, 11],
            next_edges: [7, 7],
            offset: 15,
        }]
    );
}

#[test]
fn retains_nonzero_reference_geometry_after_topology_references() {
    let payload = [
        b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0, 7,
        8, 4, 1, 0xf6, 0xff, // opaque parameter body
        10, 11, 7, 7, // face and next-edge references
        0, 68, // ref_geom[0] and ref_geom[1]
        0xe3, 0x81, 0x0d, // row close and array-item linkage
        0xe1, 0xe3,
    ];
    let face_ids = BTreeSet::from([10, 11]);

    assert_eq!(
        topology_rows_with_face_ids(&payload, Some(&face_ids))[0].faces,
        [10, 11]
    );
    let parameters = parameter_records_with_face_ids(&payload, Some(&face_ids));
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].body, [0xff]);
    assert_eq!(parameters[0].reference_geometry, [0, 68]);
}

#[test]
fn reference_geometry_uses_the_generic_compact_lane() {
    let row = [10, 11, 7, 7, 0x81, 0x0d, 68, 0xe3];
    assert_eq!(
        topology_suffix_candidates(&row),
        Some(vec![(0, [10, 11, 7, 7], [269, 68])])
    );
}

#[test]
fn face_namespace_resolves_ambiguous_reference_boundaries() {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[
        0x80, 0x90, // curve 144
        0x00, 0x28, 0x01, 0xf6, // type, feature, direction flags
        0xff, // opaque parameter byte
        0x80, 0x8f, 0x80, 0x8d, 0x81, 0x11, 0x2c, 0x00, 0x00, 0xe3, 0xe1, 0xf5, 0x05, 0xf6, 0xe3,
    ]);

    assert!(topology_rows(&payload).is_empty());
    assert!(parameter_records(&payload).is_empty());

    let face_ids = std::collections::BTreeSet::from([141, 143]);
    let rows = topology_rows_with_face_ids(&payload, Some(&face_ids));
    assert_eq!(
        rows,
        vec![CurveTopologyRow {
            id: 144,
            type_byte: 0,
            feature_id: 40,
            directions: [1, 0xf6],
            faces: [143, 141],
            next_edges: [273, 44],
            offset: 15,
        }]
    );

    let parameters = parameter_records_with_face_ids(&payload, Some(&face_ids));
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].curve_id, 144);
    assert_eq!(parameters[0].body, [0xff]);
}

#[test]
fn topology_evidence_resolves_an_ambiguous_suffix_with_an_unmaterialized_face() {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[
        7, 8, 4, 1, 0xf6, 10, 0x80, 0x8d, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
    ]);
    payload.extend_from_slice(&[
        0x80, 0x90, 0x00, 0x28, 0x01, 0xf6, 0xff, 0x80, 0x8f, 0x80, 0x8d, 0x81, 0x11, 0x2c, 0x00,
        0x00, 0xe3, 0xe1, 0xf5, 0x05, 0xf6, 0xe3,
    ]);

    let face_ids = std::collections::BTreeSet::from([143]);
    let rows = topology_rows_with_face_ids(&payload, Some(&face_ids));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].id, 144);
    assert_eq!(rows[1].faces, [143, 141]);
    assert_eq!(rows[1].next_edges, [273, 44]);
}

#[test]
fn materialized_face_evidence_precedes_namespace_face_evidence() {
    let row = [0x81, 0x73, 0x81, 0x71, 0x81, 0x4b, 0x81, 0x29, 0, 0, 0xe3];
    let materialized_face_ids = std::collections::BTreeSet::from([369, 371]);
    let namespace_face_ids = std::collections::BTreeSet::from([115, 369, 371]);

    assert_eq!(
        topology_suffix_with_face_ids(
            &row,
            Some(&materialized_face_ids),
            Some(&namespace_face_ids),
        ),
        Some((0, [371, 369, 331, 297], [0, 0]))
    );
}

#[test]
fn parameter_records_withhold_rows_with_ambiguous_terminal_suffixes() {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 8, 4, 1, 0xf6]);
    payload.extend_from_slice(&[1, 2, 3, 4, 5]);
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0xe3, 0xe1, 0xe3]);

    assert!(topology_rows(&payload).is_empty());
    assert!(parameter_records(&payload).is_empty());
}

#[test]
fn row_boundary_outweighs_prefix_like_bytes_inside_a_dense_body() {
    let payload = [
        b't', b'o', b'p', b'o', b'l', b'_', b'r', b'e', b'f', b'_', b'd', b'a', b't', b'a', 0,
        0xff, 0xe1, 0xe3, // named prototype segment
        7, 8, 4, 1, 0xf6, // row prefix
        0xfc, 5, 9, 8, 4, 1, 0xf6, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, // dense body with a false prefix
        10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3,
    ];

    assert_eq!(topology_rows(&payload).len(), 1);
    assert_eq!(
        parameter_records(&payload)[0].body[0..7],
        [0xfc, 5, 9, 8, 4, 1, 0xf6]
    );
}

#[test]
fn final_curve_row_uses_the_next_array_boundary() {
    let payload = b"topol_ref_data\0\x07\x08\x04\x01\xf6\xff\x0a\x0b\x07\x07\0\0\xe3\x80\xe0\xe1\xf5\x05\xf6\xe0\0lo_array\0";
    assert_eq!(
        topology_rows(payload),
        vec![CurveTopologyRow {
            id: 7,
            type_byte: 8,
            feature_id: 4,
            directions: [1, 0xf6],
            faces: [10, 11],
            next_edges: [7, 7],
            offset: 15,
        }]
    );
}

#[test]
fn decodes_complete_depdb_one_sided_curve_array() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x09\x0a\0\xe1\xe0next_record\0";

    let rows = depdb_cross_section_rows(payload);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 7);
    assert_eq!(rows[0].type_byte, 8);
    assert_eq!(rows[0].feature_id, 4);
    assert_eq!(rows[0].directions, [1, 0xf6]);
    assert_eq!(rows[0].suffix, [0, 9, 10, 0]);
    assert_eq!(rows[0].body, [0xe4, 0xff]);
    assert_eq!(rows[0].scalar_tokens.len(), 1);
    assert_eq!(rows[0].scalar_tokens[0].value, 1.0);
    assert_eq!(rows[0].opaque_spans.len(), 1);
    assert_eq!(rows[0].opaque_spans[0].raw, [0xff]);
}

#[test]
fn row_terminator_selects_the_first_short_or_long_marker() {
    let short_then_long = [0xe1, 0xe3, 0, 0xe1, 0xf5, 0x05, 0xf6, 0xe3];
    assert_eq!(
        row_terminator(&short_then_long, 0, short_then_long.len()),
        Some((0, 2))
    );
    let long_then_short = [0xe1, 0xf5, 0x05, 0xf6, 0xe3, 0, 0xe1, 0xe3];
    assert_eq!(
        row_terminator(&long_then_short, 0, long_then_short.len()),
        Some((0, 5))
    );
}

#[test]
fn binds_agreeing_fc05_caps_to_one_typed_cylinder() {
    let circle = |curve_id, ordinate, offset| Fc05Circle {
        curve_id,
        center_row_frame: [3.0, 4.0],
        radius_mm: 2.0,
        sample_direction_row_frame: [1.0, 0.0],
        reference_direction_row_frame: Some([1.0, 0.0]),
        parameter_sign: Some(1),
        cap_ordinate_row_frame: Some(ordinate),
        point_count: 8,
        max_residual: 0.0,
        angle_parameter_consistent: true,
        offset,
    };
    let topology = |curve_id, plane_id, offset| CurveTopologyRow {
        id: curve_id,
        type_byte: 5,
        feature_id: 4,
        directions: [1, 0xf6],
        faces: [10, plane_id],
        next_edges: [curve_id, curve_id],
        offset,
    };
    let surface = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture id fits usize"),
    };
    let pairs = fc05_cylinder_cap_pairs(
        &[circle(20, -5.0, 100), circle(21, 7.0, 200)],
        &[topology(20, 11, 100), topology(21, 12, 200)],
        &[
            surface(10, crate::surface::SurfaceKind::Cylinder),
            surface(11, crate::surface::SurfaceKind::Plane),
            surface(12, crate::surface::SurfaceKind::Plane),
        ],
    );

    assert_eq!(
        pairs,
        vec![Fc05CylinderCapPair {
            surface_id: 10,
            curve_ids: vec![20, 21],
            cap_plane_ids: vec![11, 12],
            curve_cap_ordinates_row_frame: vec![-5.0, 7.0],
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            reference_direction_row_frame: [1.0, 0.0],
            parameter_sign: 1,
            cap_ordinates_row_frame: vec![-5.0, 7.0],
            offset: 100,
        }]
    );
}

#[test]
fn fc05_cap_pairs_require_unique_topology_and_surface_identities() {
    let circle = |curve_id, ordinate, offset| Fc05Circle {
        curve_id,
        center_row_frame: [3.0, 4.0],
        radius_mm: 2.0,
        sample_direction_row_frame: [1.0, 0.0],
        reference_direction_row_frame: Some([1.0, 0.0]),
        parameter_sign: Some(1),
        cap_ordinate_row_frame: Some(ordinate),
        point_count: 8,
        max_residual: 0.0,
        angle_parameter_consistent: true,
        offset,
    };
    let topology = |curve_id, plane_id, offset| CurveTopologyRow {
        id: curve_id,
        type_byte: 5,
        feature_id: 4,
        directions: [1, 0xf6],
        faces: [10, plane_id],
        next_edges: [curve_id, curve_id],
        offset,
    };
    let surface = |id, kind: crate::surface::SurfaceKind, offset| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset,
    };
    let circles = [circle(20, -5.0, 100), circle(21, 7.0, 200)];
    let topology_rows = [topology(20, 11, 100), topology(21, 12, 200)];
    let surfaces = [
        surface(10, crate::surface::SurfaceKind::Cylinder, 10),
        surface(11, crate::surface::SurfaceKind::Plane, 11),
        surface(12, crate::surface::SurfaceKind::Plane, 12),
    ];

    let mut duplicate_topology = topology_rows.to_vec();
    duplicate_topology.push(topology(20, 11, 300));
    assert!(fc05_cylinder_cap_pairs(&circles, &duplicate_topology, &surfaces).is_empty());

    let mut duplicate_surfaces = surfaces.to_vec();
    duplicate_surfaces.push(surface(10, crate::surface::SurfaceKind::Cylinder, 20));
    assert!(fc05_cylinder_cap_pairs(&circles, &topology_rows, &duplicate_surfaces).is_empty());

    let duplicate_circles = [
        circle(20, -5.0, 100),
        circle(20, 7.0, 150),
        circle(21, 7.0, 200),
    ];
    assert!(fc05_cylinder_cap_pairs(&duplicate_circles, &topology_rows, &surfaces).is_empty());
}

#[test]
fn decodes_fc05_two_near_lane() {
    let bytes = [0x8b, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4];
    assert_eq!(
        fc05_scalar(&bytes, 0),
        Some((
            f64::from_be_bytes([0x40, 0x00, 0x13, 0x11, 0x71, 0x7e, 0xcd, 0xf4]),
            7
        ))
    );
    let lower = [0x71, 0x68, 0xf7, 0x91, 0x89, 0x97, 0x45, 0x2d];
    assert_eq!(
        fc05_scalar(&lower, 0),
        Some((
            f64::from_be_bytes([0x3f, 0xe6, 0x68, 0xf7, 0x91, 0x89, 0x97, 0x45]),
            7
        ))
    );
    let upper = [0xa3, 0x36, 0x6d, 0x17, 0x70, 0xe4, 0xb3];
    assert_eq!(
        fc05_scalar(&upper, 0),
        Some((
            f64::from_be_bytes([0x40, 0x18, 0x36, 0x6d, 0x17, 0x70, 0xe4, 0xb3]),
            7
        ))
    );
}

#[test]
fn withholds_fc05_caps_without_distinct_ordinates() {
    let circles = [Fc05Circle {
        curve_id: 20,
        center_row_frame: [3.0, 4.0],
        radius_mm: 2.0,
        sample_direction_row_frame: [1.0, 0.0],
        reference_direction_row_frame: Some([1.0, 0.0]),
        parameter_sign: Some(1),
        cap_ordinate_row_frame: Some(5.0),
        point_count: 8,
        max_residual: 0.0,
        angle_parameter_consistent: true,
        offset: 100,
    }];
    assert!(fc05_cylinder_cap_pairs(&circles, &[], &[]).is_empty());
}
