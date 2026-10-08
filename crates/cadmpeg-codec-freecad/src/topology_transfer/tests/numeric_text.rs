// SPDX-License-Identifier: Apache-2.0
//! Triangulation identity text admission.

#[test]
fn located_tessellation_decimal_key_is_scoped() {
    let bytes = super::triangulated_face_archive();
    // Earlier archive/XML scratch has a larger peak than this short key.
    crate::test_support::refusal_at(cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &bytes, "FreeCAD tessellation key", |ctx| super::decode_for_refusal(ctx, &bytes));
}

#[test]
fn unowned_triangulation_decimal_ordinal_is_scoped() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document), ("Shape.brp", brep),
    ]);
    let tshapes = crate::brep::TextTShapes::default();
    let payload = crate::brep::ShapePayloadRecord {
        id: "fcstd:native:shape-payload#Mesh".into(), property: "Property".into(),
        entry: "Shape.brp".into(), payload: crate::brep::ShapePayload::Empty,
    };
    let triangulations = [serde_json::from_value(serde_json::json!({
        "deflection": 0.02,
        "nodes": [{"x": 0.0, "y": 0.0, "z": 0.0},
            {"x": 1.0, "y": 0.0, "z": 0.0}, {"x": 0.0, "y": 1.0, "z": 0.0}],
        "triangles": [[1, 2, 3]], "uv_nodes": null, "normals": null,
    })).unwrap()];
    crate::test_support::materialized_refusal_at("FreeCAD unowned triangulation ordinal", |ctx| {
        let tables = crate::brep::Tables {
            locations: &[], curve2ds: &[], curves: &[], surfaces: &[], polygons3d: &[],
            polygons_on_triangulations: &[], triangulations: &triangulations,
            tshapes: &tshapes, roots: &[],
        };
        let builder = crate::topology_transfer::Builder::new(ctx, &payload, tables,
            crate::native::element_map::ScopedData {
                data: cadmpeg_core::text::NonBlankString::try_from("Object").unwrap(),
                _storage: ctx.reserve_scoped(0, "test source object").unwrap(),
            }, crate::topology_transfer::GeometryIndexes::new(ctx).unwrap(), None)?;
        builder.emit_unowned_triangulations(&mut cadmpeg_ir::CadIr::empty())
    });
    use cadmpeg_ir::Codec;
    let decoded = crate::FcstdCodec.decode(&mut std::io::Cursor::new(bytes),
        &cadmpeg_ir::DecodeOptions::default()).unwrap();
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    assert_eq!(decoded.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
    assert!(decoded.ir().model.tessellations[0].id.as_str().ends_with(":1"));
}
