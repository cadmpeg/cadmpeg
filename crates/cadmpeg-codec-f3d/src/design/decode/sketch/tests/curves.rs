// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_circular_arc, decode_line, decode_sketch_curve_geometry, SketchCurveClass,
    CURRENT_SKETCH_NURBS_TYPE, SKETCH_CIRCULAR_TYPES, SKETCH_LINE_TYPES,
    SKETCH_TEXT_FRAME_LINE_TYPE_GUID,
};
use crate::records::sketch_geometry::SketchCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

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
    match decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 17) {
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

    let line = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
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

    let circular = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Circular, 0)
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

    let parsed = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line, 0)
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
