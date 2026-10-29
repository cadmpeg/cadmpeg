// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::curve::depdb_cross_section_rows;
use crate::curve::expression_records;
use crate::curve::fc02_short_pcurve_endpoints;
use crate::curve::fc05_cylinder_cap_pairs;
use crate::curve::fc05_scalar;
use crate::curve::parameter_records;
use crate::curve::parameter_records_with_face_ids;
use crate::curve::pcurve_endpoints;
use crate::curve::prototype_pcurve_endpoints;
use crate::curve::prototype_topology_rows;
use crate::curve::prototypes;
use crate::curve::row_terminator;
use crate::curve::topology_rows;
use crate::curve::topology_rows_with_face_ids;
use crate::curve::topology_suffix_candidates;
use crate::curve::topology_suffix_with_face_ids;
use crate::curve::two_chart_pcurve_samples;
use crate::curve::CurveExpressionHelix;
use crate::curve::CurveParameterOpaqueSpan;
use crate::curve::CurveParameterRecord;
use crate::curve::CurveParameterScalar;
use crate::curve::CurvePrototype;
use crate::curve::CurvePrototypeTopology;
use crate::curve::CurveTopologyRow;
use crate::curve::DepdbCurveSuffix;
use crate::curve::Fc02ShortPcurveEndpoints;
use crate::curve::Fc05Circle;
use crate::curve::Fc05CylinderCapPair;
use crate::curve::TopologySuffixCandidate;
use std::collections::BTreeSet;
use std::num::NonZeroU32;

fn expression_helix(record: &crate::curve::CurveExpressionRecord) -> Option<CurveExpressionHelix> {
    crate::decode::with_service_context(|ctx| crate::curve::expression_helix(ctx, record))
        .expect("decode the expression helix")
}

fn one_prototype_pcurve_input() -> Vec<u8> {
    let mut payload = b"crv_array\0crv_id\0\x07crv_pnt_arr\0\xf9\x02\x04".to_vec();
    payload.extend_from_slice(&[0x12; 8]);
    payload
}

#[test]
fn prototype_pcurve_endpoint_record_preserves_eight_slots() {
    let payload = one_prototype_pcurve_input();
    let records = prototype_pcurve_endpoints(&payload);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].curve_id, 7);
    assert_eq!(records[0].face_0_endpoints, [[0.0; 2]; 2]);
    assert_eq!(records[0].face_1_endpoints, [[0.0; 2]; 2]);
}

fn pcurve_zero_lane_input() -> (CurveParameterRecord, CurveTopologyRow) {
    let mut record = parameter_record(7);
    record.body = vec![0x12; 8];
    let topology = CurveTopologyRow {
        id: 7,
        type_byte: 0,
        feature_id: 1,
        directions: [1, 1],
        faces: [NonZeroU32::new(2), NonZeroU32::new(3)],
        next_edges: [7, 7],
        offset: 1,
    };
    (record, topology)
}

#[test]
fn pcurve_endpoints_preserve_eight_zero_slots() {
    let (record, topology) = pcurve_zero_lane_input();
    let endpoints = pcurve_endpoints(&[record], &[topology]);
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].face_0_endpoints, [[0.0, 0.0]; 2]);
    assert_eq!(endpoints[0].face_1_endpoints, [[0.0, 0.0]; 2]);
}

fn parameter_record(curve_id: u32) -> CurveParameterRecord {
    CurveParameterRecord {
        curve_id,
        type_byte: 0,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        references: Vec::new(),
        opaque_spans: Vec::new(),
        reference_geometry: [0, 0],
        offset: usize::try_from(curve_id).expect("fixture index fits usize"),
        body_offset: usize::try_from(curve_id).expect("fixture index fits usize"),
        suffix_offset: usize::try_from(curve_id).expect("fixture index fits usize"),
    }
}

#[test]
fn typed_parameter_rows_require_unique_identity() {
    let unique = parameter_record(7);
    assert_eq!(
        crate::identity::uniquely_identified_rows(std::slice::from_ref(&unique), |record| record
            .curve_id,)
        .len(),
        1
    );
    let duplicates = [unique.clone(), unique];
    assert!(
        crate::identity::uniquely_identified_rows(&duplicates, |record| record.curve_id).is_empty()
    );
}

#[test]
fn pcurve_endpoint_slots_must_be_finite() {
    let nan = [0xed, 0x7f, 0xf8, 0, 0, 0, 0, 0, 0];
    let mut record = parameter_record(7);
    record.body.extend_from_slice(&nan);
    record.body.extend([0x0f; 7]);
    record.scalar_tokens.push(CurveParameterScalar {
        value: f64::NAN,
        raw: nan.to_vec(),
        offset: 0,
    });
    for offset in nan.len()..record.body.len() {
        record.scalar_tokens.push(CurveParameterScalar {
            value: 0.0,
            raw: vec![0x0f],
            offset,
        });
    }
    let topology = CurveTopologyRow {
        id: 7,
        type_byte: 0,
        feature_id: 1,
        directions: [1, 1],
        faces: [std::num::NonZeroU32::new(2), std::num::NonZeroU32::new(3)],
        next_edges: [7, 7],
        offset: 1,
    };

    assert!(pcurve_endpoints(&[record], &[topology]).is_empty());
}

fn canonical_and_positional_two_chart_input() -> (Vec<u8>, BTreeSet<u32>) {
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

    (payload, BTreeSet::from([10, 11]))
}

#[test]
fn decodes_canonical_and_positional_two_chart_sample_rows() {
    let (payload, face_ids) = canonical_and_positional_two_chart_input();
    let decoded = two_chart_pcurve_samples(&payload, Some(&face_ids), &BTreeSet::new());
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
    assert!(two_chart_pcurve_samples(&payload, Some(&face_ids), &BTreeSet::new()).is_empty());
}

#[test]
fn fc08_body_is_an_eight_sample_two_chart_count() {
    let points = [
        [0x0f, 0xe4, 0x0d, 0x18],
        [0xe4, 0x0f, 0x18, 0x0d],
        [0x0d, 0x18, 0xe4, 0x0f],
    ];
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 8]);
    for point in points.iter().cycle().take(8) {
        payload.extend_from_slice(point);
    }
    payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);

    let decoded =
        two_chart_pcurve_samples(&payload, Some(&BTreeSet::from([10, 11])), &BTreeSet::new());
    let [record] = decoded.as_slice() else {
        panic!("one counted fc 08 body");
    };
    assert_eq!(record.samples.len(), 8);
    assert_eq!(record.samples[0], [[0.0, 1.0], [-1.0, 0.0]]);
    assert_eq!(record.samples[7], [[1.0, 0.0], [0.0, -1.0]]);
}

fn two_chart_body_payload(count: u8, first_row: &[u8]) -> Vec<u8> {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, count]);
    payload.extend_from_slice(first_row);
    for _ in 1..count {
        payload.extend_from_slice(&[0x18; 4]);
    }
    payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);
    payload
}

#[test]
fn counted_two_chart_bodies_read_2d_positive_and_46_negative() {
    let mut first_row = vec![0x2d, 0x08, 0, 0, 0, 0, 0, 0];
    first_row.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    first_row.extend_from_slice(&[0x46, 0x14, 0, 0, 0, 0, 0, 0]);
    first_row.extend_from_slice(&[0x2d, 0x14, 0, 0, 0, 0, 0, 0]);
    let face_ids = BTreeSet::from([10, 11]);

    let counted = two_chart_pcurve_samples(
        &two_chart_body_payload(3, &first_row),
        Some(&face_ids),
        &BTreeSet::new(),
    );
    let [record] = counted.as_slice() else {
        panic!("one counted fc 03 body");
    };
    assert_eq!(record.samples[0], [[3.0, -3.0], [-5.0, 5.0]]);

    let short_forms = [
        0x2f, 0x08, 0x00, 0x48, 0x08, 0x00, 0x48, 0x14, 0x00, 0x2f, 0x14, 0x00,
    ];
    let counted = two_chart_pcurve_samples(
        &two_chart_body_payload(3, &short_forms),
        Some(&face_ids),
        &BTreeSet::new(),
    );
    let [record] = counted.as_slice() else {
        panic!("one counted fc 03 body with short forms");
    };
    assert_eq!(record.samples[0], [[3.0, -3.0], [-5.0, 5.0]]);

    let two_sample = two_chart_pcurve_samples(
        &two_chart_body_payload(2, &first_row),
        Some(&face_ids),
        &BTreeSet::new(),
    );
    let [record] = two_sample.as_slice() else {
        panic!("one counted fc 02 body");
    };
    assert_eq!(record.samples[0], [[-3.0, 3.0], [5.0, -5.0]]);
}

#[test]
fn two_sample_cylinder_plane_endpoints_match_stored_step_edges() {
    const EPS_STEP_ENDPOINT_MM: f64 = 1.0e-6;
    let cases = [
        (
            [
                0x2d, 0x19, 0x3d, 0xb7, 0xd5, 0xa1, 0x3e, 0x28, 0xda, 0x8f, 0x21, 0xc7, 0x6c, 0x7e,
                0x9d, 0x18, 0x4f, 0xa6, 0xdb, 0x73, 0x90, 0x20, 0x40, 0x2d, 0x19, 0x3d, 0xb7, 0xd5,
                0xa1, 0x3e, 0x28, 0xd9, 0x8f, 0xf9, 0x08, 0xb7, 0x0d, 0x64, 0x18, 0x5b, 0xcc, 0xb3,
                0xaf, 0x73, 0x99, 0xe8,
            ],
            -3.207793225694403,
            6.31027158543251,
            [
                [30.0, -3.069888648376, 6.310271585914],
                [30.0, -2.945299213492, 6.310271585433],
            ],
        ),
        (
            [
                0x2d, 0x19, 0x3d, 0xb7, 0xd5, 0xa1, 0x3e, 0x28, 0xc4, 0x3b, 0x52, 0x45, 0xbd, 0x75,
                0xea, 0x18, 0x4f, 0xa6, 0xdb, 0x73, 0x90, 0x20, 0x48, 0x2d, 0x19, 0x3d, 0xb7, 0xd5,
                0xa1, 0x3e, 0x28, 0xc1, 0x3f, 0x60, 0x6d, 0x7f, 0xe9, 0xf0, 0x18, 0x5c, 0x41, 0xf5,
                0xf5, 0xbe, 0x13, 0xea,
            ],
            -1.214887336587364,
            6.31027158543251,
            [
                [30.0, -0.9452364100137, 6.310271585433],
                [30.0, -1.076982757961, 6.310271585457],
            ],
        ),
        (
            [
                0x2d, 0x19, 0x3d, 0xb7, 0xd5, 0xa1, 0x3e, 0x28, 0x78, 0x88, 0xd7, 0xbf, 0x28, 0xc5,
                0xff, 0x18, 0x4f, 0xa6, 0xdb, 0x73, 0x90, 0x20, 0x44, 0x2d, 0x19, 0x3d, 0xb7, 0xd5,
                0xa1, 0x3e, 0x28, 0x7b, 0xe1, 0x07, 0x7e, 0xce, 0x16, 0x1e, 0x18, 0x5c, 0x45, 0xdc,
                0x36, 0xae, 0xdc, 0x9c,
            ],
            0.7850498599345295,
            6.31027158543251,
            [
                [30.0, 1.054938790492, 6.310271586123],
                [30.0, 0.9229544385509, 6.310271585433],
            ],
        ),
    ];
    for (body, origin_y, origin_z, expected) in cases {
        let mut payload = b"topol_ref_data\0".to_vec();
        payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 2]);
        payload.extend_from_slice(&body);
        payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);
        let records =
            two_chart_pcurve_samples(&payload, Some(&BTreeSet::from([10, 11])), &BTreeSet::new());
        let [record] = records.as_slice() else {
            panic!("one complete two-sample curve");
        };
        assert_eq!(record.samples.len(), 2);
        let actual = record
            .samples
            .iter()
            .map(|sample| {
                assert_eq!(sample[1][0], 0.0);
                [
                    [30.0, sample[0][1], -sample[0][0]],
                    [30.0, origin_y + sample[1][1], origin_z],
                ]
            })
            .collect::<Vec<_>>();
        for chart in 0..2 {
            let forward = actual.iter().zip(expected).all(|(pair, point)| {
                pair[chart]
                    .into_iter()
                    .zip(point)
                    .all(|(a, b)| (a - b).abs() <= EPS_STEP_ENDPOINT_MM)
            });
            let reverse = actual.iter().rev().zip(expected).all(|(pair, point)| {
                pair[chart]
                    .into_iter()
                    .zip(point)
                    .all(|(a, b)| (a - b).abs() <= EPS_STEP_ENDPOINT_MM)
            });
            assert!(forward || reverse);
        }
    }
}

#[test]
fn counted_two_chart_first_coordinate_reads_positive_dict_prefixes() {
    let mut first_row = vec![0x4b, 0x20, 0, 0, 0, 0, 0];
    first_row.extend_from_slice(&[0x18, 0x18, 0x18]);
    let face_ids = BTreeSet::from([10, 11]);

    let counted = two_chart_pcurve_samples(
        &two_chart_body_payload(3, &first_row),
        Some(&face_ids),
        &BTreeSet::new(),
    );
    let [record] = counted.as_slice() else {
        panic!("one counted fc 03 body");
    };
    assert_eq!(
        record.samples[0][0][0],
        f64::from_be_bytes([0x3f, 0xc0, 0x20, 0, 0, 0, 0, 0])
    );
}

#[test]
fn two_chart_sample_rows_do_not_claim_fc05_circle_bodies() {
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 5]);
    payload.extend(std::iter::repeat_n(0x18, 20));
    payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);

    let face_ids = BTreeSet::from([10, 11]);
    assert!(two_chart_pcurve_samples(&payload, Some(&face_ids), &BTreeSet::new()).is_empty());
}

#[test]
fn two_chart_replay_consumes_curve_local_scalar_forms() {
    let first = [0x32, 0, 0, 0, 0, 0, 0, 0];
    let second = [0x56, 0, 0, 0, 0, 0, 0];
    let mut samples = Vec::new();
    for _ in 0..2 {
        samples.extend_from_slice(&first);
        samples.extend_from_slice(&second);
        samples.extend_from_slice(&first);
        samples.extend_from_slice(&second);
    }
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 2]);
    payload.extend(samples);
    payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);

    let decoded =
        two_chart_pcurve_samples(&payload, Some(&BTreeSet::from([10, 11])), &BTreeSet::new());
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].samples.len(), 2);
    let expected_first = f64::from_be_bytes([0x3f, 0, 0, 0, 0, 0, 0, 0]);
    let expected_second = f64::from_be_bytes([0x3f, 0xcb, 0, 0, 0, 0, 0, 0]);
    for sample in &decoded[0].samples {
        for chart in sample {
            assert!((chart[0] - expected_first).abs() <= f64::EPSILON);
            assert!((chart[1] - expected_second).abs() <= f64::EPSILON);
        }
    }
}

fn fc02_short_input() -> (CurveParameterRecord, CurveTopologyRow) {
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
        scalar_tokens.push(CurveParameterScalar { value, raw, offset });
    }
    body.extend_from_slice(&[0x34, 0xb0, 0x00]);
    let record = CurveParameterRecord {
        curve_id: 846,
        type_byte: 0,
        opaque_spans: vec![
            CurveParameterOpaqueSpan {
                raw: vec![0xfc, 0x02],
                offset: 0,
            },
            CurveParameterOpaqueSpan {
                raw: vec![0x34, 0xb0, 0x00],
                offset: body.len() - 3,
            },
        ],
        body,
        scalar_tokens,
        ..parameter_record(846)
    };
    let topology = CurveTopologyRow {
        id: 846,
        type_byte: 0,
        feature_id: 57,
        directions: [0x01, 0xf6],
        faces: [
            std::num::NonZeroU32::new(43),
            std::num::NonZeroU32::new(163),
        ],
        next_edges: [841, 164],
        offset: 100,
    };
    (record, topology)
}

#[test]
fn decodes_only_complete_fc02_short_pcurve_endpoints() {
    let (record, topology) = fc02_short_input();

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
        faces: [
            std::num::NonZeroU32::new(43),
            std::num::NonZeroU32::new(141),
        ],
        next_edges: [271, 142],
        offset: 100,
    }];
    let positional_rows = [CurveTopologyRow {
        id: 605,
        type_byte: 0,
        feature_id: 547,
        directions: [0x01, 0xf6],
        faces: [
            std::num::NonZeroU32::new(43),
            std::num::NonZeroU32::new(235),
        ],
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
            faces: [
                std::num::NonZeroU32::new(43),
                std::num::NonZeroU32::new(141)
            ],
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
            .and_then(|frame| frame.explicit_slots)
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );

    let inherited = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x01r=5\0";
    assert_eq!(
        expression_records(inherited)[0]
            .local_system
            .as_ref()
            .and_then(|frame| frame.explicit_slots)
            .map(cadmpeg_ir::units::FiniteVector::get),
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
            radius: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            height: cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite helix coordinate"),
            z_start: cadmpeg_ir::scalar::FiniteReal::new(-2.0).expect("finite helix coordinate"),
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            clockwise: false,
        })
    );

    let constant_functions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x03r=sqrt(25)\0theta=atan(1)+t*360\0z=t*pow(2,3)\0";
    assert_eq!(
        expression_helix(&expression_records(constant_functions)[0]),
        Some(CurveExpressionHelix {
            radius: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            height: cadmpeg_ir::scalar::FiniteReal::new(8.0).expect("finite helix coordinate"),
            z_start: cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite helix coordinate"),
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
            clockwise: false,
        })
    );

    let identity_powers = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x08\
        \xe0\x0aexpression\0\xf8\x03r=5^1\0theta=t^1*360\0z=8*t^1\0";
    assert_eq!(
        expression_helix(&expression_records(identity_powers)[0]),
        Some(CurveExpressionHelix {
            radius: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            height: cadmpeg_ir::scalar::FiniteReal::new(8.0).expect("finite helix coordinate"),
            z_start: cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite helix coordinate"),
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
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
            faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
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
        [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)]
    );
    let parameters = crate::decode::with_service_context(|ctx| {
        parameter_records_with_face_ids(ctx, &payload, Some(&face_ids))
    })
    .expect("curve parameter records");
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].body, [0xff]);
    assert_eq!(parameters[0].reference_geometry, [0, 68]);
}

#[test]
fn reference_geometry_uses_the_generic_compact_lane() {
    let row = [10, 11, 7, 7, 0x81, 0x0d, 68, 0xe3];
    assert_eq!(
        topology_suffix_candidates(&row)
            .map(|candidates| candidates.into_iter().flatten().collect::<Vec<_>>()),
        Some(vec![TopologySuffixCandidate {
            start: 0,
            faces: [10, 11].map(NonZeroU32::new),
            next_edges: [7, 7],
            reference_geometry: [269, 68],
        }])
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
            faces: [
                std::num::NonZeroU32::new(143),
                std::num::NonZeroU32::new(141)
            ],
            next_edges: [273, 44],
            offset: 15,
        }]
    );

    let parameters = crate::decode::with_service_context(|ctx| {
        parameter_records_with_face_ids(ctx, &payload, Some(&face_ids))
    })
    .expect("curve parameter records");
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
    assert_eq!(
        rows[1].faces,
        [
            std::num::NonZeroU32::new(143),
            std::num::NonZeroU32::new(141)
        ]
    );
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
        Some((
            TopologySuffixCandidate {
                start: 0,
                faces: [371, 369].map(NonZeroU32::new),
                next_edges: [331, 297],
                reference_geometry: [0, 0],
            },
            false
        ))
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
            faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
            next_edges: [7, 7],
            offset: 15,
        }]
    );
}

#[test]
fn depdb_four_field_cross_section_array_withholds_rows() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x09\x0a\0\xe1\xe0next_record\0";
    let rows = crate::decode::with_service_context(|ctx| depdb_cross_section_rows(ctx, payload))
        .expect("DEPDB cross-section rows");
    assert!(rows.is_empty());
}

#[test]
fn decodes_complete_depdb_one_sided_curve_array() {
    let payload = b"crv_array\0\xf2\xf8\x02crv_id\0\x06type\0\x08feat_id\0\x04topol_ref_data\0\x07\x08\x04\x01\xf6\xe4\xff\0\x08\0\x09\x0a\0\xe1\xe0next_record\0";

    let rows = crate::decode::with_service_context(|ctx| depdb_cross_section_rows(ctx, payload))
        .expect("DEPDB cross-section rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 7);
    assert_eq!(rows[0].type_byte, 8);
    assert_eq!(rows[0].feature_id, 4);
    assert_eq!(rows[0].directions, [1, 0xf6]);
    assert_eq!(
        rows[0].suffix,
        DepdbCurveSuffix {
            section_plane: 8,
            next_curve: 9,
            reference_surface: 10
        }
    );
    assert_eq!(rows[0].body, [0xe4, 0xff]);
    assert!(rows[0].scalar_tokens.is_empty());
    assert!(rows[0].references.is_empty());
    assert_eq!(rows[0].opaque_spans.len(), 1);
    assert_eq!(rows[0].opaque_spans[0].offset, 0);
    assert_eq!(rows[0].opaque_spans[0].raw, [0xe4, 0xff]);
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
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(ordinate),
        point_count: 8,
        max_residual: 0.0,
        offset,
    };
    let topology = |curve_id, plane_id, offset| CurveTopologyRow {
        id: curve_id,
        type_byte: 5,
        feature_id: 4,
        directions: [1, 0xf6],
        faces: [
            std::num::NonZeroU32::new(10),
            std::num::NonZeroU32::new(plane_id),
        ],
        next_edges: [curve_id, curve_id],
        offset,
    };
    let surface = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
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
            cap_edges: vec![
                crate::curve::Fc05CapEdge {
                    curve_id: 20,
                    cap_plane_id: 11,
                    cap_ordinate_row_frame: -5.0
                },
                crate::curve::Fc05CapEdge {
                    curve_id: 21,
                    cap_plane_id: 12,
                    cap_ordinate_row_frame: 7.0
                }
            ],
            center_row_frame: [3.0, 4.0],
            radius_mm: 2.0,
            reference_direction_row_frame: [1.0, 0.0],
            parameter_sense: crate::curve::ParameterSense::Increasing,
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
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(ordinate),
        point_count: 8,
        max_residual: 0.0,
        offset,
    };
    let topology = |curve_id, plane_id, offset| CurveTopologyRow {
        id: curve_id,
        type_byte: 5,
        feature_id: 4,
        directions: [1, 0xf6],
        faces: [
            std::num::NonZeroU32::new(10),
            std::num::NonZeroU32::new(plane_id),
        ],
        next_edges: [curve_id, curve_id],
        offset,
    };
    let surface = |id, kind: crate::surface::SurfaceKind, offset| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
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
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(5.0),
        point_count: 8,
        max_residual: 0.0,
        offset: 100,
    }];
    assert!(fc05_cylinder_cap_pairs(&circles, &[], &[]).is_empty());
}

#[test]
fn numerical_ranges_fc05_cap_agreement_separates_lengths_and_directions() {
    let circle = |curve_id, ordinate, offset| Fc05Circle {
        curve_id,
        center_row_frame: [0.0, 0.0],
        radius_mm: 2.0,
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(ordinate),
        point_count: 8,
        max_residual: 0.0,
        offset,
    };
    let topology = |curve_id, plane_id, offset| CurveTopologyRow {
        id: curve_id,
        type_byte: 5,
        feature_id: 4,
        directions: [1, 0xf6],
        faces: [
            std::num::NonZeroU32::new(10),
            std::num::NonZeroU32::new(plane_id),
        ],
        next_edges: [curve_id, curve_id],
        offset,
    };
    let surface = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture id fits usize"),
    };

    let topology = [topology(20, 11, 100), topology(21, 12, 200)];
    let surfaces = [
        surface(10, crate::surface::SurfaceKind::Cylinder),
        surface(11, crate::surface::SurfaceKind::Plane),
        surface(12, crate::surface::SurfaceKind::Plane),
    ];
    for radius in [1e-6, 1.0, 1e12] {
        let mut caps = [
            circle(20, -radius * 10., 100),
            circle(21, radius * 10., 200),
        ];
        for cap in &mut caps {
            cap.radius_mm = radius;
        }
        assert_eq!(
            fc05_cylinder_cap_pairs(&caps, &topology, &surfaces).len(),
            1
        );
        let mut wrong_radius = caps.clone();
        wrong_radius[1].radius_mm *= 1.0005;
        assert!(fc05_cylinder_cap_pairs(&wrong_radius, &topology, &surfaces).is_empty());
        let mut wrong_center = caps.clone();
        wrong_center[1].center_row_frame[0] = radius * 0.0005;
        assert!(fc05_cylinder_cap_pairs(&wrong_center, &topology, &surfaces).is_empty());
        caps[1].angle_parameter = crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [0., 1.],
        };
        assert!(fc05_cylinder_cap_pairs(&caps, &topology, &surfaces).is_empty());
    }
}

#[test]
fn exact_helix_constructor_rejects_nonfinite_axial_coordinates() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CurveExpressionHelix::new(1.0, value, 0.0, 1.0, 0.0, false).is_none());
        assert!(CurveExpressionHelix::new(1.0, 0.0, value, 1.0, 0.0, false).is_none());
    }
    let helix = CurveExpressionHelix::new(1.0, -2.0, 3.0, 1.0, 0.0, false)
        .expect("finite signed coordinates");
    assert_eq!(helix.height.get(), -2.0);
    assert_eq!(helix.z_start.get(), 3.0);
}

#[test]
fn cross_section_suffix_keeps_pointer_roles_out_of_parameters() {
    let tail = [0, 0x85, 0x00, 0, 0x86, 0x00, 0x87, 0x00, 0];
    let mut segment = vec![21, 0, 4, 1, 0xf6, 0xe4];
    segment.extend(tail);
    let row = crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 100)
    })
    .expect("cross-section row")
    .expect("complete stored pointers");
    assert_eq!(row.body, [0xe4]);
    assert!(row.scalar_tokens.is_empty());
    assert!(row.references.is_empty());
    assert_eq!(row.opaque_spans.len(), 1);
    assert_eq!(row.opaque_spans[0].raw, [0xe4]);
    assert_eq!(
        serde_json::to_value(row.suffix).expect("stored pointer tuple"),
        serde_json::json!([0, 1280, 0, 1536, 1792, 0]),
    );
}

#[test]
fn cross_section_suffix_requires_six_complete_typed_references() {
    for tail in [
        vec![0, 9, 10, 0],
        vec![0, 8, 0, 9, 0xe4, 0],
        vec![0, 8, 0, 9, 0x80, 1, 0],
        vec![0, 8, 0, 9, 10, 0x81],
        vec![1, 8, 0, 9, 10, 0],
        vec![0, 8, 1, 9, 10, 0],
        vec![0, 8, 0, 9, 10, 1],
    ] {
        let mut segment = vec![21, 0, 4, 1, 0xf6, 0xff];
        segment.extend(tail);
        assert!(crate::decode::with_service_context(|ctx| {
            super::super::parse_depdb_curve_segment(ctx, &segment, 0)
        })
        .expect("typed pointer validation")
        .is_none());
    }
}

#[test]
fn cross_section_suffix_canonical_reference_widths() {
    for (tail, expected) in [
        (vec![0, 0, 0, 127, 127, 0], [0, 0, 0, 127, 127, 0]),
        (
            vec![0, 0x80, 0x80, 0, 0x80, 0x80, 0x80, 0x80, 0],
            [0, 128, 0, 128, 128, 0],
        ),
        (
            vec![0, 0xbf, 0xff, 0, 0xbf, 0xff, 0xbf, 0xff, 0],
            [0, 16383, 0, 16383, 16383, 0],
        ),
    ] {
        assert_eq!(
            super::super::cross_section_suffix_fields(&tail, 0),
            Some(expected)
        );
        for length in 0..tail.len() {
            assert!(super::super::cross_section_suffix_fields(&tail[..length], 0).is_none());
        }
    }
}

#[test]
fn cross_section_body_retains_terminal_byte_without_numeric_roles() {
    let segment = [21, 0, 4, 1, 0xf6, 0x18, 0, 8, 0, 9, 10, 0];
    let row = crate::decode::with_service_context(|ctx| {
        super::super::parse_depdb_curve_segment(ctx, &segment, 0)
    })
    .expect("cross-section scalar lane")
    .expect("complete six-reference tail");
    assert_eq!(row.body, [0x18]);
    assert!(row.scalar_tokens.is_empty());
    assert!(row.references.is_empty());
    assert_eq!(row.opaque_spans.len(), 1);
    assert_eq!(row.opaque_spans[0].raw, [0x18]);
}

#[test]
fn complete_count_body_precedes_materialized_face_suffix_preference() {
    for count in [3_u8, 6, 11] {
        let mut row = vec![7, 0, 3, 1, 0xf6, 0xfc, count];
        for _ in 0..count {
            row.extend_from_slice(&[0x18; 4]);
        }
        let suffix_start = row.len();
        row.extend_from_slice(&[0x81, 0x2c, 0, 0x81, 0x2d, 0x81, 0x2e, 0, 7, 0xe3]);
        let materialized = BTreeSet::from([44]);
        let (suffix, counted_body) =
            topology_suffix_with_face_ids(&row, Some(&materialized), Some(&materialized))
                .expect("unique complete count body owns its exact end");
        assert!(counted_body);
        assert_eq!(suffix.start, suffix_start);
        assert_eq!(suffix.faces, [300, 0].map(NonZeroU32::new));
        assert_eq!(suffix.next_edges, [301, 302]);
        assert_eq!(suffix.reference_geometry, [0, 7]);
        assert!(!materialized.contains(&300));
    }
}

#[test]
fn count_selected_frames_keep_nonliteral_numeric_roles_opaque() {
    let mut row = vec![7, 0, 3, 1, 0xf6, 0xfc, 2];
    row.extend_from_slice(&[0x18, 0x77, 0x60, 0, 0, 0, 0, 0]);
    row.extend_from_slice(&[0x18; 6]);
    row.extend_from_slice(&[0x81, 0x2c, 0, 0x81, 0x2d, 0x81, 0x2e, 0, 7, 0xe3]);
    let mut payload = b"topol_ref_data\0".to_vec();
    payload.extend_from_slice(&row);
    payload.extend_from_slice(&[0xe1, 0xe3]);
    let faces = BTreeSet::from([44]);
    let records = crate::decode::with_service_context(|ctx| {
        parameter_records_with_face_ids(ctx, &payload, Some(&faces))
    })
    .expect("bounded counted record");
    let [record] = records.as_slice() else {
        panic!("one record");
    };
    assert_eq!(record.scalar_tokens.len(), 7);
    assert!(record
        .scalar_tokens
        .iter()
        .all(|token| token.value.to_bits() == 0.0_f64.to_bits() && token.raw == [0x18]));
    assert_eq!(record.opaque_spans.len(), 2);
    assert_eq!(record.opaque_spans[0].raw, [0xfc, 2]);
    assert_eq!(record.opaque_spans[1].raw, [0x77, 0x60, 0, 0, 0, 0, 0]);
}

fn declared_bare_curve_rows(named: bool, replay_owner: u8, replay_type: u8) -> Vec<u8> {
    let mut payload = b"crv_array\0".to_vec();
    if named {
        payload.extend_from_slice(b"\xe0\x01crv_id\0\x07\xe0\x01type\0\x08\xe0\x01feat_id\0\x03\xe0\x02crv_pnt_arr\0\xf9\x04\x04");
        payload.extend_from_slice(&[0x18; 16]);
        payload.extend_from_slice(b"\xe0\x01crv_hdr_geom_ptr[0]\0\x01");
    }
    payload.extend_from_slice(b"topol_ref_data\0\xe3\xf7\x16\x6b\xe1\xf2\xe3");
    if !named {
        payload.extend_from_slice(&[7, 8, 3, 1, 0xf6, 0xfc, 4]);
        payload.extend_from_slice(&[0x18; 16]);
        payload.extend_from_slice(&[1, 2, 7, 8, 0, 0, 0xe3, 0x6b, 0xe1, 0xe3]);
    }
    payload.extend_from_slice(&[9, replay_type, replay_owner, 1, 0xf6]);
    payload.extend_from_slice(&[0x18; 16]);
    payload.extend_from_slice(&[
        0x81, 0x2c, 0, 0x81, 0x2d, 0x81, 0x2e, 0, 7, 0xe3, 0x6b, 0xe1, 0xe3,
    ]);
    payload
}

#[test]
fn declared_bare_rows_use_complete_parameter_boundary() {
    for named in [false, true] {
        let payload = declared_bare_curve_rows(named, 3, 8);
        let faces = BTreeSet::from([44, 300]);
        let rows = topology_rows_with_face_ids(&payload, Some(&faces));
        let replay = rows
            .iter()
            .find(|row| row.id == 9)
            .expect("complete declared replay");
        assert_eq!(replay.faces, [300, 0].map(NonZeroU32::new));
        assert_eq!(replay.next_edges, [301, 302]);
        let parameters = crate::decode::with_service_context(|ctx| {
            parameter_records_with_face_ids(ctx, &payload, Some(&faces))
        })
        .expect("raw replay parameter retention");
        let replay = parameters
            .iter()
            .find(|row| row.curve_id == 9)
            .expect("replay raw body");
        assert_eq!(replay.body, [0x18; 16]);
        assert!(replay.scalar_tokens.is_empty());
        assert!(replay.references.is_empty());
        assert_eq!(replay.opaque_spans.len(), 1);
        assert_eq!(replay.opaque_spans[0].raw, [0x18; 16]);
        assert!(
            two_chart_pcurve_samples(&payload, Some(&faces), &BTreeSet::new())
                .iter()
                .all(|sample| sample.curve_id != 9)
        );
    }
}

#[test]
fn declared_bare_rows_require_preceding_same_owner_and_type() {
    let faces = BTreeSet::from([44, 300]);
    for named in [false, true] {
        for (owner, kind) in [(4, 8), (3, 6)] {
            let payload = declared_bare_curve_rows(named, owner, kind);
            assert!(topology_rows_with_face_ids(&payload, Some(&faces))
                .iter()
                .all(|row| row.id != 9));
        }
    }
}

#[test]
fn declared_bare_rows_do_not_borrow_another_array_schema() {
    let payload = declared_bare_curve_rows(false, 3, 8);
    let declaration_end = payload
        .iter()
        .rposition(|&byte| byte == 9)
        .expect("replay id");
    let mut isolated = payload[..declaration_end].to_vec();
    isolated.extend_from_slice(b"crv_array\0topol_ref_data\0\xe3\xe1\xe3");
    isolated.extend_from_slice(&payload[declaration_end..]);
    let faces = BTreeSet::from([44, 300]);
    assert!(topology_rows_with_face_ids(&isolated, Some(&faces))
        .iter()
        .all(|row| row.id != 9));
}

#[test]
fn declared_bare_rows_keep_parameter_classes_separate() {
    let mut payload = declared_bare_curve_rows(false, 3, 8);
    let class_at = payload.len() - 3;
    assert_eq!(payload[class_at], 0x6b);
    payload.splice(class_at..=class_at, [0x82, 0x25]);
    let faces = BTreeSet::from([44, 300]);
    assert!(topology_rows_with_face_ids(&payload, Some(&faces))
        .iter()
        .all(|row| row.id != 9));
}

#[test]
fn declared_bare_rows_discard_incomplete_redeclarations() {
    let payload = declared_bare_curve_rows(false, 3, 8);
    let replay_at = payload
        .iter()
        .rposition(|&byte| byte == 9)
        .expect("replay id");
    let mut invalidated = payload[..replay_at].to_vec();
    invalidated.extend_from_slice(&[8, 8, 3, 1, 0xf6, 0xfc, 4]);
    invalidated.extend_from_slice(&[0x18; 15]);
    invalidated.extend_from_slice(&[1, 2, 7, 8, 0, 0, 0xe3, 0x6b, 0xe1, 0xe3]);
    invalidated.extend_from_slice(&payload[replay_at..]);
    let faces = BTreeSet::from([44, 300]);
    assert!(topology_rows_with_face_ids(&invalidated, Some(&faces))
        .iter()
        .all(|row| row.id != 9));
}

#[test]
fn declared_bare_rows_correct_materialized_low_byte_boundaries() {
    let payload = declared_bare_curve_rows(false, 3, 8);
    let faces = BTreeSet::from([44]);
    let rows = topology_rows_with_face_ids(&payload, Some(&faces));
    let replay = rows
        .iter()
        .find(|row| row.id == 9)
        .expect("complete declared frame");
    assert_eq!(replay.faces, [300, 0].map(NonZeroU32::new));
}

#[test]
fn declared_bare_rows_use_latest_complete_shape() {
    let payload = declared_bare_curve_rows(false, 3, 8);
    let replay_at = payload
        .iter()
        .rposition(|&byte| byte == 9)
        .expect("replay id");
    let mut redeclared = payload[..replay_at].to_vec();
    redeclared.extend_from_slice(&[8, 8, 3, 1, 0xf6, 0xfc, 2]);
    redeclared.extend_from_slice(&[0x18; 8]);
    redeclared.extend_from_slice(&[1, 2, 7, 8, 0, 0, 0xe3, 0x6b, 0xe1, 0xe3]);
    redeclared.extend_from_slice(&payload[replay_at..]);
    let faces = BTreeSet::from([44, 300]);
    assert!(topology_rows_with_face_ids(&redeclared, Some(&faces))
        .iter()
        .all(|row| row.id != 9));
}

#[test]
fn declared_bare_rows_replay_extended_named_entry_footer() {
    let payload = declared_bare_curve_rows(true, 3, 8);
    let old = b"\xe1\xf2\xe3";
    let at = payload
        .windows(old.len())
        .position(|bytes| bytes == old)
        .expect("named entry footer");
    let mut extended = payload;
    extended.splice(at..at + old.len(), [0xe1, 0xf5, 0x05, 0xf6, 0xf2, 0xe3]);
    let faces = BTreeSet::from([44, 300]);
    let rows = topology_rows_with_face_ids(&extended, Some(&faces));
    let replay = rows
        .iter()
        .find(|row| row.id == 9)
        .expect("complete named replay");
    assert_eq!(replay.faces, [300, 0].map(NonZeroU32::new));
    assert_eq!(replay.next_edges, [301, 302]);
}

#[test]
fn unit_plane_two_sample_roles_match_source_chart_coordinates() {
    let cases = [
        ("fc026afffffffffdee2d4de66666666666184ef90032ec36e06686f1c49bfba62d4de666666666661856eb1ca9b43ba0", 0, [[0.4999999999999706, 59.8], [0.43011135292175096, 59.8]]),
        ("327aaaaaaaa503ba5699999999996455f1d8055bb26229c999327aaaaaaaaaaa192d4de6666666666655f1d8055bb2622d4de66666666666", 1, [[0.19488048802976993, 0.19999999999999998], [0.19488048802976993, 59.8]]),
    ];
    for (raw, side, expected) in cases {
        let mut body = raw
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        if body.starts_with(&[0xfc, 2]) {
            body.drain(..2);
        }
        let mut payload = b"topol_ref_data\0".to_vec();
        payload.extend_from_slice(&[7, 0, 4, 1, 0xf6, 0xfc, 2]);
        payload.extend(&body);
        payload.extend_from_slice(&[10, 11, 7, 7, 0, 0, 0xe3, 0xe1, 0xe3]);
        let ids = BTreeSet::from([10, 11]);
        let before = two_chart_pcurve_samples(&payload, Some(&ids), &BTreeSet::new());
        let after =
            two_chart_pcurve_samples(&payload, Some(&ids), &BTreeSet::from([10 + side as u32]));
        assert_eq!(after.len(), 1);
        for sample in 0..2 {
            assert_eq!(after[0].samples[sample][side], expected[sample]);
            if let Some(before) = before.first() {
                assert_eq!(
                    after[0].samples[sample][1 - side],
                    before.samples[sample][1 - side]
                );
            }
        }
    }
}

#[test]
fn unit_plane_roles_do_not_change_counted_sample_lanes() {
    let mut first = vec![0x2d, 0x08, 0, 0, 0, 0, 0, 0];
    first.extend([0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    first.extend([0x18, 0x18]);
    let payload = two_chart_body_payload(3, &first);
    let ids = BTreeSet::from([10, 11]);
    let ordinary = two_chart_pcurve_samples(&payload, Some(&ids), &BTreeSet::new());
    let contextual = two_chart_pcurve_samples(&payload, Some(&ids), &ids);
    assert_eq!(ordinary.len(), 1);
    assert_eq!(ordinary, contextual);
}
