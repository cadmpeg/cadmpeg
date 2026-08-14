// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use super::{
    decode_circular_arc, decode_line, decode_sketch_curve_geometry, SketchCurveClass,
    CURRENT_SKETCH_NURBS_TYPE, SKETCH_CIRCULAR_TYPES, SKETCH_LINE_TYPES,
    SKETCH_TEXT_FRAME_LINE_TYPE_GUID,
};
use crate::records::SketchCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

fn analytic_payload(values: [f64; 12]) -> Vec<u8> {
    let mut payload = vec![0; 133];
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload
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
    assert!(decode_line(&payload).is_some());
    assert!(decode_circular_arc(&payload).is_some());

    let line = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line)
        .expect("typed line payload");
    assert_eq!(line.geometry_offset, 133);
    assert_eq!(
        line.geometry,
        SketchCurveGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(0.0, 0.0, 10.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }
    );

    let circular = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Circular)
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

    let parsed = decode_sketch_curve_geometry(&payload, 0, 41, SketchCurveClass::Line)
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
