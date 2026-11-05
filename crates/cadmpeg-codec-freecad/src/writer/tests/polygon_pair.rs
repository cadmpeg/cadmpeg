// SPDX-License-Identifier: Apache-2.0
//! Triangulation polygon pairs.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn paired_polygons_stay_attached_to_their_triangulation() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 2\n2 1 2 p 0.01 1 0 1\n2 2 1 p 0.02 1 1 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 7\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 7 1 2 1 0 0 1001000 +7 0 -6 0 *\nWi 1001000 +5 0 *\nFa 0 0.001 0 0 2 1 1001000 +4 0 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.curves.len(), 2);
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.curves.len(), 2);
    for curve in &input.ir().model.curves {
        assert!(output
            .ir()
            .model
            .curves
            .iter()
            .any(|written| written.geometry == curve.geometry));
    }
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
            if representations.iter().any(|value| matches!(value, crate::brep::TextEdgeRepresentation::PolygonPair { polygons: [1, 2], triangulation: 1, .. }))
    )));
    let mut edited = input.ir().clone();
    let primary = edited.model.edges[0].curve().unwrap().clone();
    let secondary = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id != primary)
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Polyline(polyline),
    ) = &mut secondary.geometry
    else {
        panic!("expected a secondary polygon");
    };
    polyline.set_chordal_deflection(0.05).unwrap();
    let mesh = edited.model.tessellations[0].clone();
    let mut vertices = mesh.vertices();
    vertices.swap(0, 1);
    let mut triangles = mesh.triangles();
    for triangle in &mut triangles {
        for vertex in triangle {
            *vertex = match *vertex {
                0 => 1,
                1 => 0,
                other => other,
            };
        }
    }
    let reordered = cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
        vertices, triangles, None,
    )
    .unwrap();
    edited.model.tessellations[0] = mesh.with_mesh(reordered, Vec::new()).unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let reread = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for curve in &edited.model.curves {
        assert!(reread
            .ir()
            .model
            .curves
            .iter()
            .any(|written| written.geometry == curve.geometry));
    }
    let namespace = reread.ir().native.namespace("fcstd").unwrap();
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
    assert!(tables.tshapes.iter().any(|shape| match &shape.geometry {
        crate::brep::TextTShapeGeometry::Edge {
            representations, ..
        } => representations.iter().any(|value| match value {
            crate::brep::TextEdgeRepresentation::PolygonPair { polygons, .. } => polygons[1]
                .checked_sub(1)
                .and_then(|index| tables.polygons_on_triangulations.get(index))
                .is_some_and(|polygon| polygon.nodes == [1, 2] && polygon.deflection.get() == 0.05),
            _ => false,
        }),
        _ => false,
    }));
}
