// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_circular_arc, decode_line, SketchCurveClass, CURRENT_SKETCH_NURBS_TYPE,
    SKETCH_CIRCULAR_TYPES, SKETCH_LINE_TYPES, SKETCH_TEXT_FRAME_LINE_TYPE_GUID,
};
use crate::records::sketch_geometry::SketchCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

fn tested_decode_sketch_curve_geometry(
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
    record_at: usize,
) -> Result<
    Option<crate::design::decode::sketch::DecodedSketchCurveGeometry>,
    cadmpeg_core::CodecError,
> {
    crate::design::decode::sketch::decode_sketch_curve_geometry(
        payload,
        geometry_shift,
        record_index,
        class,
        record_at,
    )
}

#[test]
fn line_components_refuse_unrepresentable_scaled_endpoints() {
    let mut values = [0.0; 12];
    values[3] = 1.0;
    values[6] = 1.0;
    values[11] = 1.0;
    values[0] = 1.0e308;
    let error = crate::design::decode::sketch::decode_line_components(
        &values,
        Vector3::new(0.0, 0.0, 1.0),
        17,
    )
    .expect_err("scaled start must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
    values[0] = 0.0;
    values[3] = 1.0e308;
    let error = crate::design::decode::sketch::decode_line_components(
        &values,
        Vector3::new(0.0, 0.0, 1.0),
        17,
    )
    .expect_err("scaled end must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
}

fn analytic_payload(values: [f64; 12]) -> Vec<u8> {
    let mut payload = vec![0; 133];
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload
}

#[test]
fn unreadable_curve_geometry_keeps_its_identity_and_sibling_geometry() {
    use crate::design::test_support::{design_type, primary_record};
    use crate::test_support::lp_ascii;

    let curve = |index: u32, start: f64, referenced: bool| {
        let mut bytes = Vec::new();
        lp_ascii(&mut bytes, "256");
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (name, value) in [
            ("crv_primary_id", u64::from(index)),
            ("crv_secondary_id", 0),
        ] {
            lp_ascii(&mut bytes, name);
            lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        if referenced {
            bytes.push(1);
            bytes.extend_from_slice(&900u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        for value in [start, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    };
    for (referenced, expected_offset) in [(false, 133), (true, 144)] {
        let mut bytes = curve(41, f64::MAX, referenced);
        let sibling_start = bytes.len();
        bytes.extend_from_slice(&curve(42, 1.0, referenced));
        let meta = crate::metastream::MetaStream {
            types: vec![design_type(
                "DCA267ED-D615-4934-B64F-AD805E8003E2",
                None,
                2,
                "Geometry",
                vec![41, 42],
            )],
            records: vec![primary_record(41, 0), primary_record(42, sibling_start)],
            secondary_records: Vec::new(),
        };
        let mut losses = Vec::new();
        let mut given_up = Vec::new();
        let curves = super::super::decode_sketch_curve_identities_from_stream(
            &bytes,
            &meta,
            "Design/BulkStream.dat",
            &mut losses,
            &mut given_up,
        )
        .unwrap();
        assert!(given_up.is_empty());
        assert_eq!(curves.len(), 2);
        assert_eq!(curves[0].record_index, 41);
        assert!(curves[0].geometry.is_none());
        assert_eq!(curves[0].geometry_offset, expected_offset);
        assert_eq!(curves[1].geometry_offset, expected_offset);
        assert!(
            matches!(curves[1].geometry, Some(SketchCurveGeometry::Line { start, .. }) if start.get() == Point3::new(10.0, 0.0, 0.0))
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::SketchCurveUndecoded.kind()
        );
    }
}

/// A curve record whose primary id is `index`.
fn curve_record(index: u32) -> Vec<u8> {
    use crate::test_support::lp_ascii;

    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&index.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (name, value) in [
        ("crv_primary_id", u64::from(index)),
        ("crv_secondary_id", 0),
    ] {
        lp_ascii(&mut bytes, name);
        lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [
        1.0f64, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decodes two curve records stored back to back under the entity ids
/// `first` and `second`.
fn decode_two_curves(
    bytes: &[u8],
    sibling_start: usize,
    [first, second]: [u64; 2],
) -> (
    Vec<super::super::SketchCurveIdentity>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    Vec<crate::design::decode::record::GivenUpFrame>,
) {
    use crate::design::test_support::{design_type, primary_record};

    let meta = crate::metastream::MetaStream {
        types: vec![design_type(
            "DCA267ED-D615-4934-B64F-AD805E8003E2",
            None,
            2,
            "Geometry",
            vec![first, second],
        )],
        records: vec![
            primary_record(first, 0),
            primary_record(second, sibling_start),
        ],
        secondary_records: Vec::new(),
    };
    let mut losses = Vec::new();
    let mut given_up = Vec::new();
    let curves = super::super::decode_sketch_curve_identities_from_stream(
        bytes,
        &meta,
        "Design/BulkStream.dat",
        &mut losses,
        &mut given_up,
    )
    .unwrap();
    (curves, losses, given_up)
}

#[test]
fn a_curve_record_without_a_usable_curve_identity_gives_up_its_frame() {
    let damages: [fn(&mut Vec<u8>); 2] = [|bytes| bytes[29] = b'x', |bytes| bytes[70..78].fill(0)];
    for damage in damages {
        let mut bytes = curve_record(41);
        damage(&mut bytes);
        let sibling_start = bytes.len();
        bytes.extend_from_slice(&curve_record(42));
        let (curves, losses, given_up) = decode_two_curves(&bytes, sibling_start, [41, 42]);
        assert_eq!(
            curves
                .iter()
                .map(|curve| curve.record_index)
                .collect::<Vec<_>>(),
            [42]
        );
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(
            given_up
                .iter()
                .map(crate::design::decode::record::GivenUpFrame::span)
                .collect::<Vec<_>>(),
            [("Design/BulkStream.dat", 0..sibling_start)]
        );
    }
}

#[test]
fn a_curve_record_whose_entity_id_exceeds_u32_gives_up_its_frame() {
    let mut bytes = curve_record(41);
    bytes[7..15].copy_from_slice(&(1u64 << 32).to_le_bytes());
    let sibling_start = bytes.len();
    bytes.extend_from_slice(&curve_record(42));
    let (curves, losses, given_up) = decode_two_curves(&bytes, sibling_start, [1 << 32, 42]);
    assert_eq!(
        curves
            .iter()
            .map(|curve| curve.record_index)
            .collect::<Vec<_>>(),
        [42]
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(
        given_up
            .iter()
            .map(crate::design::decode::record::GivenUpFrame::span)
            .collect::<Vec<_>>(),
        [("Design/BulkStream.dat", 0..sibling_start)]
    );
}

#[test]
fn typed_line_source_reports_scaled_start_overflow_at_record() {
    let payload = analytic_payload([
        f64::MAX,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    match tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 17) {
        Err(cadmpeg_core::CodecError::Malformed(message)) => {
            assert!(message.contains("byte 17"));
        }
        _ => panic!("scaled line start must be malformed at its source record"),
    }
}

#[test]
fn circular_arc_refuses_unrepresentable_scaled_center_at_source_record() {
    let payload = analytic_payload([
        f64::MAX,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
    ]);
    let error = decode_circular_arc(&payload, 17).expect_err("scaled center must fit");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error.to_string().contains("byte 17"));
}

#[test]
fn stable_type_guid_selects_line_when_the_scalar_payload_also_accepts_as_an_arc() {
    let payload = analytic_payload([
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
    ]);
    assert!(decode_line(&payload, 0).expect("line parse").is_some());
    assert!(decode_circular_arc(&payload, 0)
        .expect("arc parse")
        .is_some());

    let line = tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
        .expect("line admission")
        .expect("typed line payload");
    assert_eq!(line.geometry_offset, 133);
    assert_eq!(
        line.geometry,
        SketchCurveGeometry::line(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap()
    );

    let circular =
        tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Circular, 0)
            .expect("arc admission")
            .expect("typed circular payload");
    assert!(matches!(circular.geometry, SketchCurveGeometry::Arc { .. }));
}

#[test]
fn typed_line_accepts_the_referenced_compact_planar_form() {
    let mut payload = vec![0; 133];
    payload.push(1);
    payload.extend_from_slice(&42u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    for value in [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0] {
        payload.extend_from_slice(&f64::to_le_bytes(value));
    }
    payload.push(1);
    payload.extend_from_slice(&37u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);

    let parsed = tested_decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
        .expect("line admission")
        .expect("typed referenced compact line");
    assert_eq!(parsed.geometry_offset, 144);
    assert!(matches!(parsed.geometry, SketchCurveGeometry::Line { .. }));
}

#[test]
fn curve_type_versions_select_only_their_settled_grammars() {
    for (type_guid, version, module) in SKETCH_LINE_TYPES {
        assert_eq!(
            SketchCurveClass::of(type_guid, version, module),
            Some(SketchCurveClass::Line)
        );
    }
    for (type_guid, version, module) in SKETCH_CIRCULAR_TYPES {
        assert_eq!(
            SketchCurveClass::of(type_guid, version, module),
            Some(SketchCurveClass::Circular)
        );
    }
    assert_eq!(
        SketchCurveClass::of(
            CURRENT_SKETCH_NURBS_TYPE.0,
            CURRENT_SKETCH_NURBS_TYPE.1,
            CURRENT_SKETCH_NURBS_TYPE.2,
        ),
        Some(SketchCurveClass::Nurbs)
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_TEXT_FRAME_LINE_TYPE_GUID, 0, "MSketch"),
        Some(SketchCurveClass::TextFrameLine)
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 0, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 3, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_CIRCULAR_TYPES[0].0, 1, "Geometry"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(CURRENT_SKETCH_NURBS_TYPE.0, 2, CURRENT_SKETCH_NURBS_TYPE.2,),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_TEXT_FRAME_LINE_TYPE_GUID, 1, "MSketch"),
        None
    );
    assert_eq!(
        SketchCurveClass::of(SKETCH_LINE_TYPES[0].0, 2, "MSketch"),
        None
    );
}

#[test]
fn line_start_overflow_is_malformed() {
    assert_line_overflow_malformed(
        [
            f64::MAX,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        "F3D sketch line at byte 17 overflows millimetres",
    );
}

#[test]
fn line_end_overflow_is_malformed() {
    assert_line_overflow_malformed(
        [
            0.0,
            0.0,
            0.0,
            f64::MAX,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        "F3D sketch line at byte 17 overflows millimetres",
    );
}

#[test]
fn line_displacement_overflow_is_malformed() {
    assert_line_overflow_malformed(
        [
            -1.0e307, 0.0, 0.0, 2.0e307, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        "F3D sketch line at byte 17 has an overflowing displacement",
    );
}

fn assert_line_overflow_malformed(values: [f64; 12], expected: &str) {
    assert!(matches!(
        crate::design::decode::sketch::decode_line_components(
            &values,
            Vector3::new(0.0, 0.0, 1.0),
            17
        ),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected
    ));
}

#[test]
fn arc_center_overflow_is_malformed() {
    assert_arc_overflow_malformed([
        f64::MAX,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        1.0,
    ]);
}

#[test]
fn arc_radius_overflow_is_malformed() {
    assert_arc_overflow_malformed([
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        f64::MAX,
        0.0,
        1.0,
    ]);
}

fn assert_arc_overflow_malformed(values: [f64; 12]) {
    let payload = analytic_payload(values);
    let expected = "F3D sketch arc at byte 17 overflows millimetres";
    assert!(matches!(decode_circular_arc(&payload, 17),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));
}

/// A tail of `references` marked references, the pad, three opaque bytes, and
/// `marker`, placed after a 96-byte payload at `offset`.
fn planar_tail(offset: usize, references: usize, marker: u8) -> Vec<u8> {
    let mut bytes = vec![0xAA; offset + 96];
    for index in 0..references {
        bytes.push(1);
        bytes.extend_from_slice(&u32::try_from(index + 10).unwrap().to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend(std::iter::repeat_n(0, if references == 1 { 5 } else { 3 }));
    bytes.extend_from_slice(&[7, 8, 9, marker]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes
}

#[test]
fn the_planar_tail_marker_follows_the_pad_and_three_opaque_bytes() {
    for references in 1..=3 {
        for marker in [0, 1] {
            let bytes = planar_tail(133, references, marker);
            assert_eq!(
                crate::design::decode::sketch::planar_curve_tail(&bytes, 133)
                    .map(|tail| tail.marker),
                Some(marker)
            );
        }
    }
}

#[test]
fn a_planar_tail_with_a_nonzero_pad_or_no_reference_run_has_no_marker() {
    let mut bytes = planar_tail(133, 2, 1);
    let pad = 133 + 96 + 22;
    bytes[pad] = 1;
    assert_eq!(
        crate::design::decode::sketch::planar_curve_tail(&bytes, 133),
        None
    );
    assert_eq!(
        crate::design::decode::sketch::planar_curve_tail(&[0; 300], 133),
        None
    );
}

/// A tail of two marked references, the pad, `opaque`, a zero marker, the u32
/// `lane`, and flag byte `1`.
fn lane_tail(opaque: &[u8], lane: u32) -> Vec<u8> {
    let mut bytes = vec![0xAA; 133 + 96];
    for index in 0..2u32 {
        bytes.push(1);
        bytes.extend_from_slice(&(index + 10).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(opaque);
    bytes.push(0);
    bytes.extend_from_slice(&lane.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0]);
    bytes
}

#[test]
fn a_planar_tail_with_two_opaque_bytes_moves_its_lane_one_byte_earlier() {
    use crate::design::decode::sketch::{planar_curve_tail, PlanarCurveTail};
    for (lane, construction) in [(0, false), (1, true)] {
        assert_eq!(
            planar_curve_tail(&lane_tail(&[1, 0, 0], lane), 133),
            Some(PlanarCurveTail {
                marker: 0,
                construction,
                short: false
            })
        );
        assert_eq!(
            planar_curve_tail(&lane_tail(&[1, 0], lane), 133),
            Some(PlanarCurveTail {
                marker: 0,
                construction,
                short: true
            })
        );
    }
    let mut marked = lane_tail(&[1, 0], 1);
    marked[133 + 96 + 22 + 3 + 2] = 1;
    assert_eq!(
        planar_curve_tail(&marked, 133),
        Some(PlanarCurveTail {
            marker: 1,
            construction: true,
            short: true
        })
    );
}
