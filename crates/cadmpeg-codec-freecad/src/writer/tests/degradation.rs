// SPDX-License-Identifier: Apache-2.0
//! Independent degradation of topology members.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn unsupported_shell_keeps_a_supported_sibling_as_a_shell() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 2\n1 0 0 0 0 0 1 1 0 0 0 1 0\n1 0 0 1 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 5\nFa 0 0.001 1 0 1001000 *\nFa 0 0.001 2 0 1001000 *\nSh 1001000 +5 0 *\nSh 1001000 +4 0 *\nSo 1001000 +3 0 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.faces.len(), 2);
    let mut ir = input.ir().clone();
    let shell = &ir.model.regions[0].shells[0];
    let face = ir
        .model
        .shells
        .iter()
        .find(|record| record.id == *shell)
        .unwrap()
        .faces()[0]
        .clone();
    let surface = ir
        .model
        .faces
        .iter()
        .find(|record| record.id == face)
        .unwrap()
        .surface
        .clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), 1);
    let namespace = output.ir().native.namespace("fcstd").unwrap();
    let payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let polygons = namespace
        .arena_as::<crate::brep::Polygon3dRecord>("polygons3d")
        .unwrap();
    let indexed = namespace
        .arena_as::<crate::brep::PolygonOnTriangulationRecord>("polygons_on_triangulations")
        .unwrap();
    let tables = crate::brep::Tables::from_payload(&payloads[0], &polygons, &indexed)
        .unwrap()
        .unwrap();
    assert!(tables
        .tshapes
        .iter()
        .any(|shape| matches!(&shape.geometry, crate::brep::TextTShapeGeometry::Shell)));
}
