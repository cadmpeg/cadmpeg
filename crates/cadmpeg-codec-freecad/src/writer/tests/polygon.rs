// SPDX-License-Identifier: Apache-2.0
//! Polygon edge synthesis.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn standalone_polygon_edge_keeps_its_samples_and_carrier() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 1\n2 1 0.01 0 0 0 1 0 0 0 1\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 4\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 5 1 0 0 1001000 +4 0 -3 0 *\nWi 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let curve = |ir: &cadmpeg_ir::CadIr| {
        let id = ir.model.edges[0].curve().unwrap();
        ir.model
            .curves
            .iter()
            .find(|curve| curve.id == *id)
            .unwrap()
            .geometry
            .clone()
    };
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(curve(output.ir()), curve(input.ir()));
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
    assert!(tables.tshapes.iter().any(|shape| matches!(
        &shape.geometry,
        crate::brep::TextTShapeGeometry::Edge { representations, .. }
            if representations.iter().any(|value| matches!(value, crate::brep::TextEdgeRepresentation::Polygon3d { .. }))
    )));
}
