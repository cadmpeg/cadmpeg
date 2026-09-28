// SPDX-License-Identifier: Apache-2.0

use super::tests::{assert_codec_collection_refusal, assert_codec_retained_refusal, triangulated_face_archive};
use super::{connected_components, copy_shape_for_transfer, pcurve_geometry, pcurve_loss, source_topology_indices, transform_curve, transform_surface, Builder, PcurveGeometryError};
use crate::brep::{NurbsCurve2d, ShapePayload, ShapePayloadRecord, Tables, TextCurve2d, TextEdgeRepresentation, TextPolygon3d, TextTShape, TextTShapeGeometry, TextTShapes};
use crate::test_support::assert_retained_refusal_at;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::{CurveId, RegionId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::scalar::NonNegativeReal;
use cadmpeg_ir::transform::Transform;

#[test]
fn face_ring_diagnostic_refuses_at_matching_retained_limit() {
    let error = cadmpeg_ir::topology::LoopRing::new(Vec::new(), Vec::new())
        .expect_err("empty ring is invalid");
    assert_retained_refusal_at(&[], "FreeCAD face ring diagnostic", |ctx| {
        Err::<(), _>(crate::resource::malformed_charged(
            ctx,
            format_args!("FCStd face {} loop {} has invalid ring: {error}", "face-input", 1),
            "FreeCAD face ring diagnostic",
        ))
    });
}

#[test]
fn connected_component_comparison_refuses_at_work_limit() {
    let connected = std::collections::HashSet::from(["edge".to_owned()]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(connected_components(&ctx, &[connected.clone(), connected]),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD connected-component comparison"));
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let tables = Tables {
        locations: &[], curve2ds: &[], curves: &[], surfaces: &[],
        polygons3d: &[], polygons_on_triangulations: &[],
        tshapes: &shapes, triangulations: &[], roots: &roots,
    };
    assert!(matches!(source_topology_indices(&ctx, tables),
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD source topology scan"));
}

#[test]
fn pcurve_loss_message_refuses_at_retained_limit() {
    let payload_id = "fcstd:native:shape#Source";
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(pcurve_loss(&ctx, payload_id, 2, None).expect("loss note").message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry");
    assert_retained_refusal_at(&[], "FreeCAD pcurve loss", |ctx| pcurve_loss(ctx, payload_id, 2, None));
}

#[test]
fn pcurve_malformed_loss_message_refuses_at_retained_limit() {
    let payload_id = "fcstd:native:shape#Source";
    let error = PcurveGeometryError::Nurbs(cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
        "knots must be non-decreasing".into()));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(pcurve_loss(&ctx, payload_id, 2, Some(&error)).expect("loss note").message,
        "payload fcstd:native:shape#Source curve2ds index 2 could not enter neutral geometry: knots must be non-decreasing");
    assert_retained_refusal_at(&[], "FreeCAD pcurve loss", |ctx| pcurve_loss(ctx, payload_id, 2, Some(&error)));
}

#[test]
fn placed_nurbs_curve_basis_refuses_at_collection_limit() {
    let nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_finite_lanes(
        1,
        vec![FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ONE],
        vec![FinitePoint3::ZERO; 2],
        None,
        false,
    ).expect("valid NURBS curve");
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(nurbs));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(transform_curve(&ctx, &geometry, placed_transform()),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD NURBS curve copy"));
}

#[test]
fn placed_nurbs_surface_basis_refuses_at_collection_limit() {
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let knots = vec![FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ONE];
    let axis = || NurbsSurfaceAxis::new(1, knots.clone(), false);
    let nurbs = NurbsSurface::from_finite_lanes(
        axis(), axis(), NurbsSurfaceLanes::new(vec![vec![FinitePoint3::ZERO; 2]; 2], None), false,
    ).expect("valid NURBS surface");
    let geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(nurbs));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 13;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(transform_surface(&ctx, &geometry, placed_transform()),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD NURBS surface copy"));
}

#[test]
fn placed_geometry_source_association_refuses_at_retained_limit() {
    let source = cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Fcstd,
        object_id: cadmpeg_core::text::NonBlankString::new("source").expect("nonblank source"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    assert_retained_refusal_at(&[], "FreeCAD geometry source association", |ctx| {
        crate::brep::clone_source_association(ctx, &source)
    });
}

fn pcurve_nurbs(rational: bool) -> TextCurve2d {
    TextCurve2d::Nurbs(NurbsCurve2d {
        degree: 1,
        knots: [FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ONE].to_vec(),
        control_points: [
            cadmpeg_ir::units::FinitePoint2::ZERO,
            cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(1.0, 0.0))
                .expect("finite point"),
        ].to_vec(),
        weights: rational.then(|| vec![FiniteReal::ONE; 2]),
        periodic: false,
    })
}

fn assert_pcurve_collection_refusal(curve: &TextCurve2d, limit: u64, operation: &str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(pcurve_geometry(&ctx, curve),
        Err(PcurveGeometryError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.operation == operation));
}

#[test]
fn pcurve_polynomial_poles_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(false), 1, "FreeCAD pcurve polynomial poles");
}

#[test]
fn pcurve_rational_poles_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(true), 1, "FreeCAD pcurve rational poles");
}

#[test]
fn pcurve_knots_refuse_at_collection_limit() {
    assert_pcurve_collection_refusal(&pcurve_nurbs(false), 5, "FreeCAD pcurve knots");
}

#[test]
fn pcurve_nested_basis_refuses_at_depth_limit() {
    let curve = TextCurve2d::Offset {
        distance: FiniteReal::ONE,
        basis: crate::brep::NestedCurve2d::try_new(TextCurve2d::Line {
            origin: cadmpeg_ir::units::FinitePoint2::ZERO,
            direction: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(1.0, 0.0))
                .expect("finite direction"),
        }).expect("one nested basis"),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(pcurve_geometry(&ctx, &curve),
        Err(PcurveGeometryError::Resource(CodecError::ResourceLimit(refusal)))
            if refusal.operation == "FreeCAD pcurve geometry nesting"));
}

#[test]
fn topology_occurrence_property_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD topology occurrence property");
}

#[test]
fn polygonal_surface_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD polygonal surface identity");
}

#[test]
fn triangulated_surface_emitted_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD emitted surface identity");
}

#[test]
fn triangulated_surface_set_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD emitted surfaces");
}

#[test]
fn emitted_triangulations_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD emitted triangulations");
}

#[test]
fn tessellation_key_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD tessellation key");
}

#[test]
fn tessellation_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD tessellation identity");
}

#[test]
fn tessellation_body_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD tessellation body identity");
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
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face connectivity edge keys");
}

#[test]
fn face_connectivity_vertex_keys_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face connectivity vertex keys");
}

#[test]
fn face_connectivity_edge_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD face connectivity edge identity");
}

#[test]
fn face_connectivity_vertex_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD face connectivity vertex identity");
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
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD indexed polygon points");
}

#[test]
fn indexed_polygon_parameters_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD indexed polygon parameters");
}

#[test]
fn region_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD region shape copy");
}

#[test]
fn shell_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD shell shape copy");
}

#[test]
fn face_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face shape copy");
}

#[test]
fn wire_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD wire shape copy");
}

#[test]
fn edge_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD edge shape copy");
}

#[test]
fn edge_representation_continuity_refuses_at_retained_limit() {
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        copy_shape_for_transfer(&ctx, &shape, "FreeCAD edge shape copy"),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD edge shape copy"
    ));
}

#[test]
fn pcurve_pair_continuity_refuses_at_retained_limit() {
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        copy_shape_for_transfer(&ctx, &shape, "FreeCAD edge shape copy"),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD edge shape copy"
    ));
}

#[test]
fn topology_occurrence_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD topology occurrence identity");
}

macro_rules! triangulated_identity_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_codec_retained_refusal(&triangulated_face_archive(), $operation);
        }
    };
}

triangulated_identity_refusal!(body_identity_refuses_at_retained_limit, "FreeCAD body identity");
triangulated_identity_refusal!(region_identity_refuses_at_retained_limit, "FreeCAD region identity");
triangulated_identity_refusal!(shell_identity_refuses_at_retained_limit, "FreeCAD shell identity");
triangulated_identity_refusal!(face_identity_refuses_at_retained_limit, "FreeCAD face identity");
triangulated_identity_refusal!(triangulation_surface_identity_refuses_at_retained_limit, "FreeCAD triangulation surface identity");
triangulated_identity_refusal!(triangulation_surface_key_refuses_at_retained_limit, "FreeCAD triangulation surface key");
triangulated_identity_refusal!(face_loop_identity_refuses_at_retained_limit, "FreeCAD face loop identity");
triangulated_identity_refusal!(face_loop_key_refuses_at_retained_limit, "FreeCAD face loop key");
triangulated_identity_refusal!(face_coedge_identity_refuses_at_retained_limit, "FreeCAD face coedge identity");
triangulated_identity_refusal!(face_coedge_key_refuses_at_retained_limit, "FreeCAD face coedge key");
triangulated_identity_refusal!(edge_identity_refuses_at_retained_limit, "FreeCAD edge identity");
triangulated_identity_refusal!(point_identity_refuses_at_retained_limit, "FreeCAD point identity");
triangulated_identity_refusal!(vertex_identity_refuses_at_retained_limit, "FreeCAD vertex identity");
triangulated_identity_refusal!(cached_edge_identity_refuses_at_retained_limit, "FreeCAD cached edge identity");
triangulated_identity_refusal!(cached_vertex_identity_refuses_at_retained_limit, "FreeCAD cached vertex identity");
triangulated_identity_refusal!(current_body_identity_copy_refuses_at_retained_limit, "FreeCAD current body identity");
triangulated_identity_refusal!(body_record_identity_copy_refuses_at_retained_limit, "FreeCAD body record identity");
triangulated_identity_refusal!(region_record_identity_copy_refuses_at_retained_limit, "FreeCAD region record identity");
triangulated_identity_refusal!(region_body_identity_copy_refuses_at_retained_limit, "FreeCAD region body identity");
triangulated_identity_refusal!(first_shell_component_identity_copy_refuses_at_retained_limit, "FreeCAD first shell component identity");
triangulated_identity_refusal!(component_shell_record_identity_copy_refuses_at_retained_limit, "FreeCAD component shell record identity");
triangulated_identity_refusal!(component_shell_region_identity_copy_refuses_at_retained_limit, "FreeCAD component shell region identity");
triangulated_identity_refusal!(coedge_radial_identity_copy_refuses_at_retained_limit, "FreeCAD coedge radial identity");
triangulated_identity_refusal!(coedge_record_identity_copy_refuses_at_retained_limit, "FreeCAD coedge record identity");
triangulated_identity_refusal!(coedge_loop_identity_copy_refuses_at_retained_limit, "FreeCAD coedge loop identity");
triangulated_identity_refusal!(loop_record_identity_copy_refuses_at_retained_limit, "FreeCAD loop record identity");
triangulated_identity_refusal!(loop_face_identity_copy_refuses_at_retained_limit, "FreeCAD loop face identity");
triangulated_identity_refusal!(face_record_identity_copy_refuses_at_retained_limit, "FreeCAD face record identity");
triangulated_identity_refusal!(face_shell_identity_copy_refuses_at_retained_limit, "FreeCAD face shell identity");
triangulated_identity_refusal!(edge_record_identity_copy_refuses_at_retained_limit, "FreeCAD edge record identity");
triangulated_identity_refusal!(point_record_identity_copy_refuses_at_retained_limit, "FreeCAD point record identity");
triangulated_identity_refusal!(vertex_record_identity_copy_refuses_at_retained_limit, "FreeCAD vertex record identity");

macro_rules! direct_identity_copy_refusal {
    ($name:ident, $type:ty, $id:literal, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = $id.len() as u64 - 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result: Result<$type, _> = crate::resource::copied_identity(&ctx, $id, $operation);
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.operation == $operation));
        }
    };
}

direct_identity_copy_refusal!(vertex_shell_record_identity_copy_refuses_at_retained_limit, ShellId, "fcstd:model:shell#Payload:1", "FreeCAD vertex shell record identity");
direct_identity_copy_refusal!(vertex_shell_region_identity_copy_refuses_at_retained_limit, RegionId, "fcstd:model:region#Payload:1", "FreeCAD vertex shell region identity");
direct_identity_copy_refusal!(shell_record_identity_copy_refuses_at_retained_limit, ShellId, "fcstd:model:shell#Payload:1", "FreeCAD shell record identity");
direct_identity_copy_refusal!(shell_region_identity_copy_refuses_at_retained_limit, RegionId, "fcstd:model:region#Payload:1", "FreeCAD shell region identity");
direct_identity_copy_refusal!(cached_edge_lookup_identity_copy_refuses_at_retained_limit, EdgeId, "fcstd:model:edge#Payload:1", "FreeCAD cached edge lookup identity");
direct_identity_copy_refusal!(cached_vertex_lookup_identity_copy_refuses_at_retained_limit, VertexId, "fcstd:model:vertex#Payload:1", "FreeCAD cached vertex lookup identity");
direct_identity_copy_refusal!(located_curve_record_identity_copy_refuses_at_retained_limit, CurveId, "fcstd:model:curve#Payload:1", "FreeCAD located curve record identity");
direct_identity_copy_refusal!(located_surface_record_identity_copy_refuses_at_retained_limit, SurfaceId, "fcstd:model:surface#Payload:1", "FreeCAD located surface record identity");
direct_identity_copy_refusal!(procedural_surface_owner_identity_copy_refuses_at_retained_limit, SurfaceId, "fcstd:model:surface#Payload:1", "FreeCAD procedural surface owner identity");

#[test]
fn cached_edges_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD cached edges");
}

#[test]
fn cached_vertices_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD cached vertices");
}

fn empty_builder<'a, 'c, 'r>(
    ctx: &'c DecodeContext<'r>,
    payload: &'a ShapePayloadRecord,
    tshapes: &'a TextTShapes,
) -> Result<Builder<'a, 'c, 'r>, CodecError> {
    Builder::new(ctx, payload, Tables {
        locations: &[], curve2ds: &[], curves: &[], surfaces: &[],
        polygons3d: &[], polygons_on_triangulations: &[],
        tshapes, triangulations: &[], roots: &[],
    }, cadmpeg_core::text::NonBlankString::new("Object".to_owned()).unwrap())
}

fn assert_empty_builder_refusal(
    operation: &str,
    call: impl Fn(&mut Builder<'_, '_, '_>) -> Result<(), CodecError>,
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

fn assert_empty_builder_collection_refusal(
    operation: &str,
    call: impl Fn(&mut Builder<'_, '_, '_>) -> Result<(), CodecError>,
) {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:entry#Payload".to_owned(),
        property: "Property".to_owned(),
        entry: "Entry".to_owned(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut builder = empty_builder(&ctx, &payload, &tshapes).unwrap();
    assert!(matches!(call(&mut builder),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
}

fn placed_transform() -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]).unwrap()
}

#[test]
fn pcurve_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD pcurve key", |builder| builder.pcurve_id(1, 0, false).map(|_| ()));
}

#[test]
fn pcurve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD pcurve identity", |builder| builder.pcurve_id(1, 0, false).map(|_| ()));
}

#[test]
fn topology_root_label_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD topology root label", |builder| {
        builder.root_discriminator = Some(2);
        builder.topology_label(1, Transform::identity()).map(|_| ())
    });
}

#[test]
fn shell_component_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD shell component key", |builder| builder.shell_component_id("1", 1).map(|_| ()));
}

#[test]
fn shell_component_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD shell component identity", |builder| builder.shell_component_id("1", 1).map(|_| ()));
}

#[test]
fn base_curve_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base curve key", |builder| builder.located_curve(&mut CadIr::empty(), 1, Transform::identity()).map(|_| ()));
}

#[test]
fn base_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base curve identity", |builder| builder.located_curve(&mut CadIr::empty(), 1, Transform::identity()).map(|_| ()));
}

#[test]
fn located_curve_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located curve key", |builder| builder.located_curve(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn located_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located curve identity", |builder| builder.located_curve(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn emitted_curve_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD emitted curve identity", |builder| builder.located_curve(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn emitted_curves_refuse_at_collection_limit() {
    assert_empty_builder_collection_refusal("FreeCAD emitted curves", |builder| builder.located_curve(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn base_surface_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base surface key", |builder| builder.located_surface(&mut CadIr::empty(), 1, Transform::identity()).map(|_| ()));
}

#[test]
fn base_surface_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD base surface identity", |builder| builder.located_surface(&mut CadIr::empty(), 1, Transform::identity()).map(|_| ()));
}

#[test]
fn located_surface_key_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located surface key", |builder| builder.located_surface(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn located_surface_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD located surface identity", |builder| builder.located_surface(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn emitted_surface_identity_refuses_at_retained_limit() {
    assert_empty_builder_refusal("FreeCAD emitted surface identity", |builder| builder.located_surface(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

#[test]
fn emitted_surfaces_refuse_at_collection_limit() {
    assert_empty_builder_collection_refusal("FreeCAD emitted surfaces", |builder| builder.located_surface(&mut CadIr::empty(), 1, placed_transform()).map(|_| ()));
}

fn assert_standalone_polygon_refusal(
    retained_limit: Option<u64>,
    collection_limit: Option<u64>,
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
        nodes: vec![FinitePoint3::ZERO,
            FinitePoint3::from_coordinates(FiniteReal::ONE, FiniteReal::ZERO, FiniteReal::ZERO)],
        parameters: Some(vec![FiniteReal::ZERO, FiniteReal::ONE]),
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut builder = Builder::new(&ctx, &payload, Tables {
        locations: &[], curve2ds: &[], curves: &[], surfaces: &[],
        polygons3d: &polygons, polygons_on_triangulations: &[],
        tshapes: &tshapes, triangulations: &[], roots: &[],
    }, cadmpeg_core::text::NonBlankString::new("Object".to_owned()).unwrap()).unwrap();
    let edge = EdgeId::mint("fcstd:model:edge#Payload:1").unwrap();
    let result = builder.polygon_curve(
        &mut CadIr::empty(), &edge, 0,
        &TextEdgeRepresentation::Polygon3d { polygon: 1, location: 0 },
        Transform::identity(),
    );
    assert!(matches!(result, Err(CodecError::ResourceLimit(ref limit)) if limit.operation == operation),
        "expected {operation} refusal, got {result:?}");
}

#[test]
fn standalone_polygon_nodes_refuse_at_collection_limit() {
    assert_standalone_polygon_refusal(None, Some(1), "FreeCAD standalone polygon nodes");
}

#[test]
fn standalone_polygon_parameters_refuse_at_collection_limit() {
    assert_standalone_polygon_refusal(None, Some(3), "FreeCAD standalone polygon parameters");
}

#[test]
fn polygon_curve_identity_refuses_at_retained_limit() {
    const ID: &str = "fcstd:model:edge#Payload:1:polygon:1";
    assert_standalone_polygon_refusal(Some(ID.len() as u64 - 1), None, "FreeCAD polygon curve identity");
}

#[test]
fn polygon_curve_record_identity_refuses_at_retained_limit() {
    const ID: &str = "fcstd:model:edge#Payload:1:polygon:1";
    assert_standalone_polygon_refusal(Some(2 * ID.len() as u64 - 1), None, "FreeCAD polygon curve record identity");
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
    assert_eq!(builder.polygon_curve_id(&edge, 0, false).unwrap().as_str(),
        "fcstd:model:edge#Payload:1:polygon:1");
    assert_eq!(builder.polygon_curve_id(&edge, 0, true).unwrap().as_str(),
        "fcstd:model:edge#Payload:1:polygon:1:secondary");
}
