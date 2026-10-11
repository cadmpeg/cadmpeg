// SPDX-License-Identifier: Apache-2.0

use super::tests::{
    assert_codec_collection_refusal, assert_codec_retained_refusal, assert_codec_work_refusal,
    repeated_shape_roots_archive, triangulated_face_archive,
};
use super::{
    connected_components, pcurve_geometry, pcurve_loss, source_topology_indices, transform_curve,
    transform_surface, Builder, PcurveGeometryError,
};
use crate::brep::{
    NurbsCurve2d, ShapePayload, ShapePayloadRecord, Tables, TextCurve2d, TextEdgeRepresentation,
    TextPolygon3d, TextTShape, TextTShapeGeometry, TextTShapes,
};
use crate::test_support::assert_retained_refusal_at;
use crate::test_support::test_archive::archive_entries;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::scalar::NonNegativeReal;
use cadmpeg_ir::transform::Transform;

fn topology_occurrence_copy_archive() -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="1.0" file="Shape.brp"/><ElementMap count="1"><Element value="Edge1" key="stable"/></ElementMap></Property></Properties></Object></ObjectData></Document>"#;
    let mut brep = String::from("CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *\n");
    // Repeated roots retain independent bindings for the element-map consumer.
    brep.push_str(&"+1 0 ".repeat(256));
    brep.push('*');
    archive_entries(&[("Document.xml", document), ("Shape.brp", brep.as_bytes())])
}

fn assert_topology_occurrence_materialized_refusal(operation: &str) {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    let input = topology_occurrence_copy_archive();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&input), &DecodeOptions::default())
        .expect("map-bearing repeated roots");
    assert_eq!(decoded.ir().model.edges.len(), 256);
    let namespace = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace");
    let payloads = namespace
        .arena_as::<ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let maps = namespace
        .arena_as::<crate::native::element_map::ElementMapRecord>("element_maps")
        .unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(
        maps[0].maps.root().groups[0].names[1][0].topology_ids.len(),
        256
    );

    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        &input,
        operation,
        |ctx| {
            let mut ir = CadIr::empty();
            let (curves, surfaces) =
                crate::brep::transfer_text_geometry(ctx, &payloads, &properties)?;
            ir.model.curves = curves.curves;
            ir.model.surfaces = surfaces.surfaces;
            super::transfer(
                ctx,
                &mut ir,
                &payloads,
                &properties,
                &mut Vec::new(),
                !maps.is_empty(),
            )
            .map(|_| ())
        },
    );
}

fn unowned_triangulation_archive() -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 1
3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3
TShapes 0";
    archive_entries(&[("Document.xml", document), ("Shape.brp", brep)])
}

#[test]
fn unowned_triangulation_nodes_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &unowned_triangulation_archive(),
        "FreeCAD unowned triangulation nodes",
    );
}

#[test]
fn unowned_triangulation_triangles_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &unowned_triangulation_archive(),
        "FreeCAD unowned triangulation triangles",
    );
}

#[test]
fn unowned_triangulation_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&unowned_triangulation_archive(), "FreeCAD model identity");
}

#[test]
fn unowned_triangulation_source_refuses_at_retained_limit() {
    assert_codec_retained_refusal(
        &unowned_triangulation_archive(),
        "FreeCAD topology source association",
    );
}

#[test]
fn placed_triangulation_source_refuses_at_retained_limit() {
    assert_codec_retained_refusal(
        &triangulated_face_archive(),
        "FreeCAD topology source association",
    );
}

#[test]
fn placed_triangulation_nodes_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD placed triangulation nodes",
    );
}

#[test]
fn placed_triangulation_triangles_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD placed triangulation triangles",
    );
}

#[test]
fn polygonal_surface_vertices_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD polygonal surface vertices",
    );
}

#[test]
fn polygonal_surface_triangles_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD polygonal surface triangles",
    );
}

#[test]
fn tessellation_faces_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD tessellation faces");
}

#[test]
fn face_ring_diagnostic_refuses_at_matching_retained_limit() {
    let error = cadmpeg_ir::topology::LoopRing::new(
        &cadmpeg_test_support::service_decode_context(),
        Vec::new(),
        Vec::new(),
    )
    .expect("fixture ring admission")
    .expect_err("empty ring is invalid");
    assert_retained_refusal_at(&[], "FreeCAD face ring diagnostic", |ctx| {
        Err::<(), _>(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "FCStd face {} loop {} has invalid ring: {error}",
                "face-input", 1
            ),
            "FreeCAD face ring diagnostic",
        ))
    });
}

#[test]
fn connected_component_comparison_refuses_at_work_limit() {
    let connected = std::collections::BTreeSet::from(["edge".to_owned()]);
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD connected-component comparison",
        |ctx| connected_components(ctx, &[connected.clone(), connected.clone()]),
    );
}

#[test]
fn connected_component_member_scan_refuses_at_work_limit() {
    let connected = std::collections::BTreeSet::from(["edge".to_owned()]);
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD connected-component members",
        |ctx| connected_components(ctx, &[connected.clone(), connected.clone()]),
    );
}

#[test]
fn source_topology_scan_refuses_at_work_limit() {
    let shapes = TextTShapes::from(vec![TextTShape {
        geometry: TextTShapeGeometry::Vertex {
            tolerance: FiniteReal::ONE,
            point: FinitePoint3::ZERO,
            representations: Vec::new(),
        },
        flags: [false; 7],
        children: Vec::new(),
    }]);
    let roots = [crate::brep::TextShapeUse {
        shape: 1,
        orientation: crate::brep::TextOrientation::Forward,
        location: 0.into(),
    }];
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &shapes,
        triangulations: &[],
        roots: &roots,
    };
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD source topology scan",
        |ctx| source_topology_indices(ctx, tables),
    );
}

#[test]
fn pcurve_loss_message_refuses_at_retained_limit() {
    let payload_id = "fcstd:native:shape#Source";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(
        pcurve_loss(&ctx, payload_id, 2, None)
            .expect("loss note")
            .message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry"
    );
    assert_retained_refusal_at(&[], "FreeCAD pcurve loss", |ctx| {
        pcurve_loss(ctx, payload_id, 2, None)
    });
}

#[test]
fn pcurve_malformed_loss_message_refuses_at_retained_limit() {
    let payload_id = "fcstd:native:shape#Source";
    let error = PcurveGeometryError::Nurbs(cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
        "knots must be non-decreasing".into(),
    ));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(pcurve_loss(&ctx, payload_id, 2, Some(&error)).expect("loss note").message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry: knots must be non-decreasing");
    assert_retained_refusal_at(&[], "FreeCAD pcurve loss", |ctx| {
        pcurve_loss(ctx, payload_id, 2, Some(&error))
    });
}

#[test]
fn placed_nurbs_curve_basis_refuses_at_collection_limit() {
    let nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_finite_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![
            FiniteReal::ZERO,
            FiniteReal::ZERO,
            FiniteReal::ONE,
            FiniteReal::ONE,
        ],
        vec![FinitePoint3::ZERO; 2],
        None,
        false,
    )
    .expect("fixture pole pairing admission")
    .expect("valid NURBS curve");
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs),
    );
    crate::test_support::assert_collection_refusal_at(&[], "FreeCAD NURBS curve copy", |ctx| {
        transform_curve(ctx, &geometry, placed_transform())
    });
}

#[test]
fn placed_nurbs_surface_basis_refuses_at_collection_limit() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let knots = vec![
        FiniteReal::ZERO,
        FiniteReal::ZERO,
        FiniteReal::ONE,
        FiniteReal::ONE,
    ];
    let axis = || NurbsSurfaceAxis::new(1, knots.clone(), false);
    let nurbs = NurbsSurface::from_finite_lanes(
        &cadmpeg_test_support::service_decode_context(),
        axis(),
        axis(),
        NurbsSurfaceLanes::new(vec![vec![FinitePoint3::ZERO; 2]; 2], None),
        false,
    )
    .expect("fixture pole pairing admission")
    .expect("valid NURBS surface");
    let geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(nurbs),
    );
    crate::test_support::assert_collection_refusal_at(&[], "FreeCAD NURBS surface copy", |ctx| {
        transform_surface(ctx, &geometry, placed_transform())
    });
}

#[test]
fn placed_geometry_source_association_refuses_at_retained_limit() {
    let source = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::try_from("source").expect("nonblank source"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    assert_retained_refusal_at(&[], "FreeCAD geometry source association", |ctx| {
        source.try_clone_for_decode(ctx, "FreeCAD geometry source association")
    });
}

fn pcurve_nurbs(rational: bool) -> TextCurve2d {
    TextCurve2d::Nurbs(NurbsCurve2d {
        degree: 1,
        knots: [
            FiniteReal::ZERO,
            FiniteReal::ZERO,
            FiniteReal::ONE,
            FiniteReal::ONE,
        ]
        .to_vec(),
        control_points: [
            cadmpeg_ir::units::FinitePoint2::ZERO,
            cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(1.0, 0.0))
                .expect("finite point"),
        ]
        .to_vec(),
        weights: rational.then(|| vec![FiniteReal::ONE; 2]),
        periodic: false,
    })
}

fn assert_pcurve_collection_refusal(curve: &TextCurve2d, operation: &str) {
    crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
        pcurve_geometry(ctx, curve).map_err(CodecError::from)
    });
}

#[test]
fn pcurve_polynomial_poles_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(false), "FreeCAD pcurve polynomial poles");
}

#[test]
fn pcurve_rational_poles_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(true), "FreeCAD pcurve rational poles");
}

#[test]
fn pcurve_knots_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(false), "FreeCAD pcurve knots");
}

#[test]
fn pcurve_nested_basis_refuses_at_depth_limit() {
    let curve = TextCurve2d::Offset {
        distance: FiniteReal::ONE,
        basis: crate::brep::NestedCurve2d::try_new(TextCurve2d::Line {
            origin: cadmpeg_ir::units::FinitePoint2::ZERO,
            direction: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                1.0, 0.0,
            ))
            .expect("finite direction"),
        })
        .expect("one nested basis"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(pcurve_geometry(&ctx, &curve),
        Err(PcurveGeometryError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.operation == "FreeCAD pcurve geometry nesting"));
}

fn triangulated_face_with_normals_archive() -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 1
1 1 0 0 10 0 1 0 0 0 0 1 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 1
2 1 2 p 0.01 1 0 1
Surfaces 0
Triangulations 1
3 1 0 1 0.02 0 0 0 1 0 0 0 1 0 1 2 3 0 0 1 0 0 1 0 0 1
TShapes 7
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 6 1 1 0 0 1001000 +7 0 -6 0 *
Wi 1001000 +5 0 *
Fa 0 0.001 0 1 2 1 1001000 +4 0 *
Sh 1001000 +3 0 *
So 1001000 +2 0 *
+1 0 *";
    archive_entries(&[
        ("Document.xml", document.as_slice()),
        ("Shape.brp", brep.as_slice()),
    ])
}

fn unowned_triangulation_with_normals_archive() -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects><ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 1
3 1 0 1 0.02 0 0 0 1 0 0 0 1 0 1 2 3 0 0 1 0 0 1 0 0 1
TShapes 0";
    archive_entries(&[
        ("Document.xml", document.as_slice()),
        ("Shape.brp", brep.as_slice()),
    ])
}

#[test]
fn placed_triangulation_normals_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_with_normals_archive(),
        "FreeCAD placed triangulation normals",
    );
}

#[test]
fn unowned_triangulation_normals_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &unowned_triangulation_with_normals_archive(),
        "FreeCAD unowned triangulation normals",
    );
}

#[test]
fn topology_occurrence_property_refuses_at_materialized_limit() {
    assert_topology_occurrence_materialized_refusal("FreeCAD topology occurrence property");
}

#[test]
fn element_map_demand_admits_returned_topology_occurrences() {
    assert_codec_collection_refusal(
        &repeated_shape_roots_archive(true),
        "FreeCAD topology occurrences",
    );
}

#[test]
fn element_map_demand_charges_occurrence_copies_when_they_are_built() {
    let input = repeated_shape_roots_archive(true);
    for operation in [
        "FreeCAD topology occurrence property",
        "FreeCAD topology occurrence identity",
    ] {
        assert_codec_work_refusal(&input, operation);
    }
}

#[test]
fn identity_edge_reads_the_curve_position_index_without_an_element_map() {
    assert_codec_work_refusal(
        &repeated_shape_roots_archive(false),
        "FreeCAD curve index scan",
    );
}

#[test]
fn polygonal_surface_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(
        &triangulated_face_archive(),
        "FreeCAD polygonal surface identity",
    );
}

#[test]
fn triangulated_surface_emitted_identity_refuses_at_materialized_limit() {
    let input = triangulated_face_archive();
    let mut options = cadmpeg_ir::DecodeOptions::default();
    options.policy.limits.max_retained_bytes = u64::MAX;
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "FreeCAD emitted surface identity",
        None,
    );
    {
        use cadmpeg_ir::Codec;
        crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(&input), &options)
            .expect("the archive retains no emitted-set identity");
    }
    drop(probe);
    let triangulations = [serde_json::from_value(serde_json::json!({
        "deflection": 0.02,
        "nodes": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}, {"x": 0.0, "y": 1.0, "z": 0.0}],
        "triangles": [[1, 2, 3]], "uv_nodes": null, "normals": null
    })).unwrap()];
    let shapes: TextTShapes = serde_json::from_value(serde_json::json!([{
        "index": 1, "kind": "face",
        "geometry": {"kind": "face", "natural_restriction": false, "tolerance": 0.0, "surface": 0, "location": 0, "triangulation": 1},
        "flags": [false, false, false, false, false, false, false], "children": []
    }])).unwrap();
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#MeshShape:Shape:Shape.brp".into(),
        property: "Shape".into(),
        entry: "Shape.brp".into(),
        payload: ShapePayload::Empty,
    };
    crate::test_support::materialized_refusal_at("FreeCAD emitted surface identity", |ctx| {
        let tables = Tables {
            locations: &[],
            curve2ds: &[],
            curves: &[],
            surfaces: &[],
            polygons3d: &[],
            polygons_on_triangulations: &[],
            tshapes: &shapes,
            triangulations: &triangulations,
            roots: &[],
        };
        let mut builder = Builder::new(
            ctx,
            &payload,
            tables,
            test_source_object(ctx)?,
            super::GeometryIndexes::new(ctx)?,
            None,
        )?;
        builder.append_face(
            &mut CadIr::empty(),
            &cadmpeg_ir::ids::ShellId::mint("fcstd:model:shell#MeshShape:Shape:Shape.brp:6")
                .unwrap(),
            &crate::brep::TextShapeUse {
                shape: 1,
                orientation: crate::brep::TextOrientation::Forward,
                location: 0.into(),
            },
            Transform::identity(),
            false,
        )
    });
}

#[test]
fn triangulated_surface_set_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD emitted surfaces");
}

#[test]
fn emitted_triangulations_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD emitted triangulations",
    );
}

#[test]
fn tessellation_key_refuses_at_work_limit() {
    assert_codec_work_refusal(&triangulated_face_archive(), "FreeCAD tessellation key");
}

#[test]
fn tessellation_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(
        &triangulated_face_archive(),
        "FreeCAD tessellation identity",
    );
}

#[test]
fn tessellation_body_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(
        &triangulated_face_archive(),
        "FreeCAD tessellation body identity",
    );
}

#[test]
fn topology_body_roots_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD topology body roots");
}

#[test]
fn shell_face_uses_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD shell face uses");
}

#[test]
fn face_connectivity_edge_keys_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD face connectivity edge keys",
    );
}

#[test]
fn face_connectivity_vertex_keys_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD face connectivity vertex keys",
    );
}

#[test]
fn face_connectivity_edge_identity_refuses_at_work_limit() {
    assert_codec_work_refusal(
        &triangulated_face_archive(),
        "FreeCAD face connectivity edge identity",
    );
}

#[test]
fn face_connectivity_vertex_identity_refuses_at_work_limit() {
    assert_codec_work_refusal(
        &triangulated_face_archive(),
        "FreeCAD face connectivity vertex identity",
    );
}

#[test]
fn wire_edge_uses_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD wire edge uses");
}

#[test]
fn loop_coedge_ids_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD loop coedge IDs");
}

#[test]
fn indexed_polygon_points_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD indexed polygon points",
    );
}

#[test]
fn indexed_polygon_parameters_refuse_at_collection_limit() {
    assert_codec_collection_refusal(
        &triangulated_face_archive(),
        "FreeCAD indexed polygon parameters",
    );
}

fn assert_archive_shape_copy_absent(operation: &'static str) {
    use cadmpeg_ir::Codec;
    let input = triangulated_face_archive();
    let mut options = cadmpeg_ir::DecodeOptions::default();
    options.policy.limits.max_collection_items = u64::MAX;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        None,
    );
    crate::FcstdCodec
        .decode(&mut std::io::Cursor::new(&input), &options)
        .expect("native shapes are borrowed without allocating transfer copies");
}

fn assert_native_shape_borrowed(shape: TextTShape) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Entry".to_owned(),
        payload: ShapePayload::Empty,
    };
    let shapes = TextTShapes::from(vec![shape]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let builder = empty_builder(&ctx, &payload, &shapes).unwrap();
    let borrowed = builder.shape(1).unwrap();
    assert!(std::ptr::eq(borrowed, &raw const shapes[0]));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn region_shape_transfer_keeps_native_shape_storage() {
    assert_archive_shape_copy_absent("FreeCAD region shape copy");
}

#[test]
fn shell_shape_transfer_keeps_native_shape_storage() {
    assert_archive_shape_copy_absent("FreeCAD shell shape copy");
}

#[test]
fn face_shape_transfer_keeps_native_shape_storage() {
    assert_archive_shape_copy_absent("FreeCAD face shape copy");
}

#[test]
fn wire_shape_transfer_keeps_native_shape_storage() {
    assert_archive_shape_copy_absent("FreeCAD wire shape copy");
}

#[test]
fn edge_shape_transfer_keeps_native_shape_storage() {
    assert_archive_shape_copy_absent("FreeCAD edge shape copy");
}

#[test]
fn edge_representation_continuity_is_borrowed_without_storage() {
    let shape = TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ONE,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: vec![TextEdgeRepresentation::Regularity {
                continuity: "C1".to_owned(),
                surfaces: [1, 2],
                locations: [0, 0],
            }],
        },
        flags: [false; 7],
        children: Vec::new(),
    };
    assert_native_shape_borrowed(shape);
}

#[test]
fn pcurve_pair_continuity_is_borrowed_without_storage() {
    let shape = TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ONE,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: vec![TextEdgeRepresentation::PcurvePair {
                curves: [1, 2],
                continuity: "C1".to_owned(),
                surface: 1,
                location: 0,
                parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                uv_endpoints: None,
            }],
        },
        flags: [false; 7],
        children: Vec::new(),
    };
    assert_native_shape_borrowed(shape);
}

#[test]
fn topology_occurrence_identity_refuses_at_materialized_limit() {
    assert_topology_occurrence_materialized_refusal("FreeCAD topology occurrence identity");
}

macro_rules! triangulated_scratch_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let input = triangulated_face_archive();
            let mut options = cadmpeg_ir::DecodeOptions::default();
            options.policy.limits.max_retained_bytes = u64::MAX;
            let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                $operation,
                None,
            );
            {
                use cadmpeg_ir::Codec;
                crate::FcstdCodec
                    .decode(&mut std::io::Cursor::new(&input), &options)
                    .expect("scratch identities are not retained");
            }
            drop(probe);
            assert_codec_work_refusal(&input, $operation);
        }
    };
}

macro_rules! triangulated_identity_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_codec_retained_refusal(&triangulated_face_archive(), $operation);
        }
    };
}

fn assert_scoped_model_identity_refusal(kind: &str, operation: &'static str) {
    let input = triangulated_face_archive();
    let mut options = cadmpeg_ir::DecodeOptions::default();
    options.policy.limits.max_retained_bytes = u64::MAX;
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        operation,
        None,
    );
    {
        use cadmpeg_ir::Codec;
        crate::FcstdCodec
            .decode(&mut std::io::Cursor::new(&input), &options)
            .expect("the archive keeps no primary scratch identity");
    }
    drop(probe);
    assert_empty_builder_materialized_refusal(operation, |builder| {
        builder
            .ctx
            .with_scoped_storage(operation, || {
                crate::native::model_id_charged_at(
                    builder.ctx,
                    kind,
                    &builder.payload.id,
                    "1",
                    operation,
                )
            })
            .map(|_| ())
    });
}

#[test]
fn body_identity_refuses_at_materialized_limit() {
    assert_scoped_model_identity_refusal("body", "FreeCAD body identity");
}

#[test]
fn shell_identity_refuses_at_materialized_limit() {
    assert_scoped_model_identity_refusal("shell", "FreeCAD shell identity");
}
triangulated_identity_refusal!(
    region_identity_refuses_at_retained_limit,
    "FreeCAD region identity"
);
triangulated_identity_refusal!(
    face_identity_refuses_at_retained_limit,
    "FreeCAD face identity"
);
triangulated_identity_refusal!(
    triangulation_surface_identity_refuses_at_retained_limit,
    "FreeCAD triangulation surface identity"
);
triangulated_scratch_refusal!(
    triangulation_surface_key_refuses_at_work_limit,
    "FreeCAD triangulation surface key"
);
triangulated_identity_refusal!(
    face_loop_identity_refuses_at_retained_limit,
    "FreeCAD face loop identity"
);
triangulated_scratch_refusal!(face_loop_key_refuses_at_work_limit, "FreeCAD face loop key");
triangulated_identity_refusal!(
    face_coedge_identity_refuses_at_retained_limit,
    "FreeCAD face coedge identity"
);
triangulated_scratch_refusal!(
    face_coedge_key_refuses_at_work_limit,
    "FreeCAD face coedge key"
);
triangulated_identity_refusal!(
    edge_identity_refuses_at_retained_limit,
    "FreeCAD edge identity"
);
triangulated_identity_refusal!(
    point_identity_refuses_at_retained_limit,
    "FreeCAD point identity"
);
triangulated_identity_refusal!(
    vertex_identity_refuses_at_retained_limit,
    "FreeCAD vertex identity"
);
triangulated_scratch_refusal!(
    cached_edge_identity_refuses_at_work_limit,
    "FreeCAD cached edge identity"
);
triangulated_scratch_refusal!(
    cached_vertex_identity_refuses_at_work_limit,
    "FreeCAD cached vertex identity"
);
triangulated_scratch_refusal!(
    current_body_identity_copy_refuses_at_work_limit,
    "FreeCAD current body identity"
);
triangulated_identity_refusal!(
    body_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD body record identity"
);
triangulated_identity_refusal!(
    region_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD region record identity"
);
triangulated_identity_refusal!(
    region_body_identity_copy_refuses_at_retained_limit,
    "FreeCAD region body identity"
);
triangulated_identity_refusal!(
    first_shell_component_identity_copy_refuses_at_retained_limit,
    "FreeCAD first shell component identity"
);
triangulated_identity_refusal!(
    component_shell_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD component shell record identity"
);
triangulated_identity_refusal!(
    component_shell_region_identity_copy_refuses_at_retained_limit,
    "FreeCAD component shell region identity"
);
triangulated_identity_refusal!(
    coedge_radial_identity_copy_refuses_at_retained_limit,
    "FreeCAD coedge radial identity"
);
triangulated_identity_refusal!(
    coedge_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD coedge record identity"
);
triangulated_identity_refusal!(
    coedge_loop_identity_copy_refuses_at_retained_limit,
    "FreeCAD coedge loop identity"
);
triangulated_identity_refusal!(
    loop_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD loop record identity"
);
triangulated_identity_refusal!(
    loop_face_identity_copy_refuses_at_retained_limit,
    "FreeCAD loop face identity"
);
triangulated_identity_refusal!(
    face_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD face record identity"
);
triangulated_identity_refusal!(
    face_shell_identity_copy_refuses_at_retained_limit,
    "FreeCAD face shell identity"
);
triangulated_identity_refusal!(
    edge_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD edge record identity"
);
triangulated_identity_refusal!(
    point_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD point record identity"
);
triangulated_identity_refusal!(
    vertex_record_identity_copy_refuses_at_retained_limit,
    "FreeCAD vertex record identity"
);

#[test]
fn cached_edges_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD cached edges");
}

#[test]
fn cached_vertices_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD cached vertices");
}

fn empty_builder<'a, 'c, 'r, 'occ>(
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tshapes: &'a TextTShapes,
) -> Result<Builder<'a, 'c, 'r, 'occ>, CodecError> {
    Builder::new(
        ctx,
        payload,
        Tables {
            locations: &[],
            curve2ds: &[],
            curves: &[],
            surfaces: &[],
            polygons3d: &[],
            polygons_on_triangulations: &[],
            tshapes,
            triangulations: &[],
            roots: &[],
        },
        test_source_object(ctx)?,
        super::GeometryIndexes::new(ctx)?,
        None,
    )
}

fn test_source_object<'c>(
    ctx: &'c DecodeContext<'_>,
) -> Result<super::ScopedData<'c, cadmpeg_core::text::NonBlankString>, CodecError> {
    Ok(super::ScopedData {
        data: cadmpeg_core::text::NonBlankString::try_from("Object".to_owned())
            .map_err(CodecError::malformed)?,
        storage: ctx.reserve_scoped(0, "test topology source object")?,
    })
}

fn assert_empty_builder_refusal(
    operation: &str,
    call: impl Fn(&mut Builder<'_, '_, '_, '_>) -> Result<(), CodecError>,
) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Shape.brp".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    assert_retained_refusal_at(&[], operation, |ctx| {
        let mut builder = empty_builder(ctx, &payload, &tshapes)?;
        call(&mut builder)
    });
}

fn assert_empty_builder_materialized_refusal(
    operation: &str,
    call: impl Fn(&mut Builder<'_, '_, '_, '_>) -> Result<(), CodecError>,
) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Shape.brp".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    crate::test_support::materialized_refusal_at(operation, |ctx| {
        let mut builder = empty_builder(ctx, &payload, &tshapes)?;
        call(&mut builder)
    });
}

fn assert_empty_builder_collection_refusal(
    operation: &str,
    call: impl Fn(&mut Builder<'_, '_, '_, '_>) -> Result<(), CodecError>,
) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Entry".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    crate::test_support::assert_collection_refusal_at(&[], operation, |ctx| {
        let mut builder = empty_builder(ctx, &payload, &tshapes)?;
        call(&mut builder)
    });
}

fn placed_transform() -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap()
}

#[test]
fn pcurve_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD pcurve key", |builder| {
        builder.pcurve_id(1, 0, false).map(|_| ())
    });
}

#[test]
fn pcurve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD pcurve identity", |builder| {
        builder.pcurve_id(1, 0, false).map(|_| ())
    });
}

#[test]
fn topology_root_label_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD topology root label", |builder| {
        builder.root_discriminator = Some(2);
        builder.topology_label(1, Transform::identity()).map(|_| ())
    });
}

#[test]
fn shell_component_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD shell component key", |builder| {
        builder.shell_component_id("1", 1).map(|_| ())
    });
}

#[test]
fn shell_component_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD shell component identity", |builder| {
        builder.shell_component_id("1", 1).map(|_| ())
    });
}

#[test]
fn base_curve_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD base curve key", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, Transform::identity())
            .map(|_| ())
    });
}

#[test]
fn base_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base curve identity", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, Transform::identity())
            .map(|_| ())
    });
}

#[test]
fn located_curve_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD located curve key", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn located_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located curve identity", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn emitted_curve_identity_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD emitted curve identity", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn emitted_curves_refuse_at_collection_limit() {
    assert_empty_builder_collection_refusal("FreeCAD emitted curves", |builder| {
        builder
            .located_curve(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn base_surface_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD base surface key", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, Transform::identity())
            .map(|_| ())
    });
}

#[test]
fn base_surface_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base surface identity", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, Transform::identity())
            .map(|_| ())
    });
}

#[test]
fn located_surface_key_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD located surface key", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn located_surface_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located surface identity", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn emitted_surface_identity_refuses_at_materialized_limit() {
    assert_empty_builder_materialized_refusal("FreeCAD emitted surface identity", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

#[test]
fn emitted_surfaces_refuse_at_collection_limit() {
    assert_empty_builder_collection_refusal("FreeCAD emitted surfaces", |builder| {
        builder
            .located_surface(&mut CadIr::empty(), 1, placed_transform())
            .map(|_| ())
    });
}

fn assert_standalone_polygon_refusal(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Entry".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    let polygons = [TextPolygon3d {
        deflection: NonNegativeReal::ZERO,
        nodes: vec![
            FinitePoint3::ZERO,
            FinitePoint3::from_coordinates(FiniteReal::ONE, FiniteReal::ZERO, FiniteReal::ZERO),
        ],
        parameters: Some(vec![FiniteReal::ZERO, FiniteReal::ONE]),
    }];
    crate::test_support::refusal_at(dimension, &[], operation, |ctx| {
        let mut builder = Builder::new(
            ctx,
            &payload,
            Tables {
                locations: &[],
                curve2ds: &[],
                curves: &[],
                surfaces: &[],
                polygons3d: &polygons,
                polygons_on_triangulations: &[],
                tshapes: &tshapes,
                triangulations: &[],
                roots: &[],
            },
            test_source_object(ctx)?,
            super::GeometryIndexes::new(ctx)?,
            None,
        )?;
        let edge = EdgeId::mint("fcstd:model:edge#Payload:1").unwrap();
        builder.polygon_curve(
            &mut CadIr::empty(),
            &edge,
            0,
            &TextEdgeRepresentation::Polygon3d {
                polygon: 1,
                location: 0,
            },
            Transform::identity(),
        )
    });
}

#[test]
fn standalone_polygon_nodes_refuse_at_collection_limit() {
    assert_standalone_polygon_refusal(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "FreeCAD standalone polygon nodes",
    );
}

#[test]
fn standalone_polygon_parameters_refuse_at_collection_limit() {
    assert_standalone_polygon_refusal(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "FreeCAD standalone polygon parameters",
    );
}

#[test]
fn polygon_curve_identity_refuses_at_retained_limit() {
    assert_standalone_polygon_refusal(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "FreeCAD polygon curve identity",
    );
}

#[test]
fn polygon_curve_record_identity_refuses_at_retained_limit() {
    assert_standalone_polygon_refusal(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "FreeCAD polygon curve record identity",
    );
}

#[test]
fn secondary_polygon_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD secondary polygon curve identity", |builder| {
        let edge = EdgeId::mint("fcstd:model:edge#Payload:1").unwrap();
        builder.polygon_curve_id(&edge, 0, true).map(|_| ())
    });
}

#[test]
fn polygon_curve_id_spelling_is_preserved() {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Entry".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let builder = empty_builder(&ctx, &payload, &tshapes).unwrap();
    let edge = EdgeId::mint("fcstd:model:edge#Payload:1").unwrap();
    assert_eq!(
        builder.polygon_curve_id(&edge, 0, false).unwrap().as_str(),
        "fcstd:model:edge#Payload:1:polygon:1"
    );
    assert_eq!(
        builder.polygon_curve_id(&edge, 0, true).unwrap().as_str(),
        "fcstd:model:edge#Payload:1:polygon:1:secondary"
    );
}

#[test]
fn geometry_position_indexes_charge_work_and_scoped_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    let mut ir = CadIr::empty();
    let id = cadmpeg_ir::ids::CurveId::mint("fcstd:model:curve#Index:1").unwrap();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: id.clone(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Polyline(
                cadmpeg_ir::geometry::sampled::PolylineCurve::from_scaled_deflection(
                    cadmpeg_ir::geometry::sampled::PolylineSamples::Unparameterized {
                        points: vec![
                            FinitePoint3::ZERO,
                            FinitePoint3::from_coordinates(
                                FiniteReal::ONE,
                                FiniteReal::ZERO,
                                FiniteReal::ZERO,
                            ),
                        ]
                        .try_into()
                        .unwrap(),
                    },
                    NonNegativeReal::ZERO,
                    cadmpeg_ir::scalar::PositiveReal::ONE,
                    &cadmpeg_test_support::service_decode_context(),
                )
                .unwrap()
                .unwrap(),
            ),
        ),
        source_object: None,
    });
    crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "FreeCAD curve position lookup",
        |ctx| {
            let mut indexes = super::GeometryIndexes::new(ctx)?;
            indexes.curve_position(ctx, &ir, &id).map(|_| ())
        },
    );
    crate::test_support::materialized_refusal_at("FreeCAD curve position key", |ctx| {
        let mut indexes = super::GeometryIndexes::new(ctx)?;
        indexes.curve_position(ctx, &ir, &id).map(|_| ())
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut indexes = super::GeometryIndexes::new(ctx).unwrap();
        assert_eq!(indexes.curve_position(ctx, &ir, &id).unwrap(), Some(0));
        indexes.index_curve(ctx, &id, 9).unwrap();
        assert_eq!(indexes.curve_position(ctx, &ir, &id).unwrap(), Some(0));
        let next = cadmpeg_ir::ids::CurveId::mint("fcstd:model:curve#Index:2").unwrap();
        indexes.index_curve(ctx, &next, 1).unwrap();
        assert_eq!(indexes.curve_position(ctx, &ir, &next).unwrap(), Some(1));
    });
}

#[test]
fn occurrence_lookup_keys_refuse_as_scoped_storage() {
    crate::test_support::materialized_refusal_at("FreeCAD occurrence key", |ctx| {
        ctx.with_scoped_storage("FreeCAD occurrence lookup scratch", || {
            super::OccurrenceKey::new(ctx, 7, Transform::identity())
        })
        .map(|_| ())
    });
    crate::test_support::materialized_refusal_at("FreeCAD source occurrence key", |ctx| {
        ctx.with_scoped_storage("FreeCAD source occurrence scratch", || {
            super::SourceOccurrenceKey::new(ctx, 7, Transform::identity())
        })
        .map(|_| ())
    });
}

#[test]
fn procedural_indexes_keep_presence_and_reject_ambiguous_owners() {
    use cadmpeg_ir::geometry::{
        ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};

    let construction = ProceduralSurfaceId::mint("fcstd:model:surface#Index:construction").unwrap();
    let first = SurfaceId::mint("fcstd:model:surface#Index:1").unwrap();
    let second = SurfaceId::mint("fcstd:model:surface#Index:2").unwrap();
    let mut ir = CadIr::empty();
    for id in [first.clone(), second] {
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
    }
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Replica {
            source: first.clone(),
            transform: Transform::identity(),
        },
        None,
    ));
    crate::test_support::with_service_context(&[], |ctx| {
        let mut indexes = super::GeometryIndexes::new(ctx).unwrap();
        indexes.ensure_procedural(ctx, &ir).unwrap();
        assert!(indexes
            .procedural
            .as_ref()
            .unwrap()
            .procedural_surfaces
            .contains(&construction));
        assert_eq!(
            indexes
                .procedural
                .as_ref()
                .unwrap()
                .construction_owners
                .get(&construction),
            Some(&None)
        );
        assert_eq!(ir.model.procedural_surface_owner(&construction), None);
        assert_eq!(indexes.surface_position(ctx, &ir, &first).unwrap(), Some(0));
        indexes.index_surface(ctx, &first, 9).unwrap();
        assert_eq!(indexes.surface_position(ctx, &ir, &first).unwrap(), Some(0));
        let appended = SurfaceId::mint("fcstd:model:surface#Index:appended").unwrap();
        indexes
            .index_surface(ctx, &appended, ir.model.surfaces.len())
            .unwrap();
        assert_eq!(
            indexes.surface_position(ctx, &ir, &appended).unwrap(),
            Some(2)
        );

        ir.model.surfaces.pop();
        let mut indexes = super::GeometryIndexes::new(ctx).unwrap();
        indexes.ensure_procedural(ctx, &ir).unwrap();
        assert_eq!(
            indexes
                .procedural
                .as_ref()
                .unwrap()
                .construction_owners
                .get(&construction),
            Some(&Some(0))
        );
        assert_eq!(
            ir.model.procedural_surface_owner(&construction),
            Some(&first)
        );
    });
}

#[test]
fn edge_endpoint_search_stops_before_unused_children() {
    let children = vec![
        crate::brep::TextShapeUse {
            shape: 1,
            orientation: crate::brep::TextOrientation::Forward,
            location: 0.into()
        };
        128
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::edge_endpoint_uses(&ctx, 9, &children),
        Err(CodecError::Malformed(_))
    ));
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD edge endpoint search",
        |ctx| super::edge_endpoint_uses(ctx, 9, &children),
    );
}
