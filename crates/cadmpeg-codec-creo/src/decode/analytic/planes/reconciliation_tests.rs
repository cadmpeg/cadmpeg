use crate::decode::analytic::carriers::transfer_topology_bound_planes;
use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::analytic::planes::{
    agreed_plane, agreed_plane_surface, agreed_topology_bound_plane, analytic_boundary_line,
    analytic_curve_plane, envelope_reconciled_plane_candidate, frame_bound_outline_plane_candidate,
    held_coordinate_plane, plane_candidates, reconciled_model_plane, topology_bound_line_plane,
    topology_bound_plane, BoundaryLine, PlaneCandidate, PlaneChart,
};
use crate::decode::surfaces::fc05_cap_pair_model_frame;
use crate::surface::{
    LocalSystemClassification, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
};
use crate::vecmath::dot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn topology_bound_plane_service(
    points: impl IntoIterator<Item = [f64; 3]>,
) -> Option<PlaneEquation> {
    crate::decode::with_test_decode_ctx(|ctx| topology_bound_plane(ctx, points))
        .expect("service topology plane")
}

fn analytic_curve_plane_service(geometry: &CurveGeometry) -> Option<PlaneEquation> {
    crate::decode::with_test_decode_ctx(|ctx| analytic_curve_plane(ctx, geometry))
        .expect("service analytic curve plane")
}

fn one_positional_plane_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes.positional_frames.push(OutlinePlane {
        surface_id: 7,
        origin: [0.0, 0.0, 1.0],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 7,
    });
    scan
}

fn positional_plane_limit_error(limit: u64, surface: bool) -> CodecError {
    let scan = one_positional_plane_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    if surface {
        crate::decode::analytic::planes::placed_plane_surfaces(&ctx, &scan)
            .expect_err("plane surface exceeds collection limit")
    } else {
        crate::decode::analytic::planes::placed_planes(&ctx, &scan)
            .expect_err("plane exceeds collection limit")
    }
}

fn candidate_limit_error(scan: &crate::container::ContainerScan<'_>, limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    plane_candidates(&ctx, scan)
        .err()
        .expect("plane candidates exceed collection limit")
}

fn one_held_plane_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes.envelopes.push(PlaneEnvelopeRecord {
        surface_id: 7,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-1.0), Some(0.0), Some(1.0)],
                [Some(1.0), Some(0.0), Some(-1.0)],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 1,
        offset: 2,
    });
    scan
}

#[test]
fn held_plane_group_node_refuses_collection_limit() {
    let error = candidate_limit_error(&one_held_plane_scan(), 0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo held plane group nodes"));
}

#[test]
fn held_plane_equation_refuses_collection_limit() {
    let error = candidate_limit_error(&one_held_plane_scan(), 1);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo held plane equations"));
}

#[test]
fn agreed_held_plane_node_refuses_collection_limit() {
    let error = candidate_limit_error(&one_held_plane_scan(), 2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo agreed held plane nodes"));
}

#[test]
fn local_plane_chart_id_node_refuses_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 7,
        body: Vec::new(),
        slots: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 1,
        offset: 2,
    });
    let error = candidate_limit_error(&scan, 2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo local plane chart ID nodes"));
}

#[test]
fn matrix_plane_frame_id_node_refuses_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 7,
        body: Vec::new(),
        slots: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::MatrixColumns),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 1,
        offset: 2,
    });
    let error = candidate_limit_error(&scan, 0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo matrix plane frame ID nodes"));
}

fn held_plane_with_frame_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = one_held_plane_scan();
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 7,
        body: Vec::new(),
        slots: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 1,
        offset: 3,
    });
    scan
}

#[test]
fn frame_bound_outline_node_refuses_collection_limit() {
    let error = candidate_limit_error(&held_plane_with_frame_scan(), 3);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo frame-bound outline nodes"));
}

#[test]
fn frame_bound_outline_vector_refuses_collection_limit() {
    let error = candidate_limit_error(&held_plane_with_frame_scan(), 4);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo frame-bound outlines"));
}

#[test]
fn positional_plane_candidate_vector_refuses_collection_limit() {
    let error = positional_plane_limit_error(0, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane candidates"));
}

#[test]
fn positional_plane_candidate_node_refuses_collection_limit() {
    let error = positional_plane_limit_error(1, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane candidate nodes"));
}

#[test]
fn placed_plane_node_refuses_collection_limit() {
    let error = positional_plane_limit_error(2, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo placed plane nodes"));
}

#[test]
fn placed_plane_surface_node_refuses_collection_limit() {
    let error = positional_plane_limit_error(2, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo placed plane surface nodes"));
}

#[test]
fn positional_plane_admission_preserves_service_geometry() {
    let scan = one_positional_plane_scan();
    let plane = crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::analytic::planes::placed_planes(ctx, &scan)
    })
    .expect("service plane admitted");
    assert_eq!(
        plane.get(&7).map(|plane| plane.origin),
        Some([0.0, 0.0, 1.0])
    );
    assert_eq!(
        plane.get(&7).map(|plane| plane.normal),
        Some([0.0, 0.0, 1.0])
    );
}

#[test]
fn reconciled_plane_uses_source_carrier_after_millimeter_admission() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::new(
        cadmpeg_ir::scalar::PositiveReal::new(25.4),
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        source_carriers.admit_surface(
            ctx,
            &mut ir,
            Surface {
                id: SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, 7),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(1.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                    )
                    .expect("source plane"),
                )),
                source_object: None,
            },
        )
    })
    .expect("surface admission");
    let local = std::collections::BTreeMap::from([(
        7,
        PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
    )]);
    assert_eq!(
        reconciled_model_plane(&local, &ir, &source_carriers, 7).map(|plane| plane.origin),
        Some([1.0, 0.0, 0.0])
    );
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
        &ir.model.surfaces[0].geometry
    else {
        panic!("surface changed family");
    };
    assert_eq!(plane.origin().get(), Point3::new(25.4, 0.0, 0.0));
}

fn nurbs_curve(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<Point3>,
    weights: Option<Vec<f64>>,
) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            knots,
            control_points,
            weights,
            false,
        )
        .expect("fixture constructor admission")
        .expect("cardinality-valid test curve"),
    ))
}

#[test]
fn topology_boundary_points_define_one_plane() {
    let plane = topology_bound_plane_service([
        [4.0, 0.0, 2.0],
        [1.0, 3.0, 2.0],
        [1.0, 0.0, 2.0],
        [4.0, 3.0, 2.0],
        [1.0, 0.0, 2.0],
    ])
    .expect("non-collinear coplanar points");
    assert_eq!(plane.origin, [1.0, 0.0, 2.0]);
    assert_eq!(plane.normal, [0.0, 0.0, 1.0]);

    assert!(topology_bound_plane_service([[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]).is_none());
    assert!(topology_bound_plane_service([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ])
    .is_none());
}

#[test]
fn topology_bound_plane_refuses_candidate_point_vector() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let Err(error) =
        topology_bound_plane(&ctx, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
    else {
        panic!("one topology point exceeds collection limit")
    };
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo topology plane candidate points"));
}

#[test]
fn analytic_conic_boundary_defines_its_plane() {
    let plane = analytic_curve_plane_service(&CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(3.0, 4.0, 5.0),
            Vector3::new(0.0, 0.0, -2.0)
                .unit()
                .expect("nonzero fixture direction"),
            Vector3::new(1.0, 0.0, 0.0),
            7.0,
        )
        .expect("valid CircleCurve fixture"),
    )))
    .expect("circle plane");
    assert_eq!(plane.origin, [3.0, 4.0, 5.0]);
    assert_eq!(plane.normal, [0.0, 0.0, -1.0]);
    assert!(
        analytic_curve_plane_service(&CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .expect("valid LineCurve fixture")
        )))
        .is_none()
    );
}

#[test]
fn complete_nurbs_boundaries_supply_only_provable_plane_evidence() {
    let planar = nurbs_curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 2.0),
        ],
        None,
    );
    let plane = analytic_curve_plane_service(&planar).expect("planar NURBS boundary");
    assert_eq!(plane.origin[2], 2.0);
    assert_eq!(plane.normal, [0.0, 0.0, 1.0]);

    let nonplanar = nurbs_curve(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 3.0),
            Point3::new(0.0, 1.0, 2.0),
        ],
        None,
    );
    assert!(analytic_curve_plane_service(&nonplanar).is_none());

    let line = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 2.0, 4.0), Point3::new(3.0, 2.0, 4.0)],
        Some(vec![2.0, 1.0]),
    );
    let line = analytic_boundary_line(&line).expect("degree-one NURBS line");
    assert_eq!(line.origin, [0.0, 2.0, 4.0]);
    assert_eq!(line.direction, [1.0, 0.0, 0.0]);

    let bent = nurbs_curve(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
    );
    assert!(analytic_boundary_line(&bent).is_none());
}

#[test]
fn analytic_nurbs_plane_refuses_control_point_vector() {
    let geometry = nurbs_curve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 2.0),
        ],
        None,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let Err(error) = analytic_curve_plane(&ctx, &geometry) else {
        panic!("one NURBS control point exceeds collection limit")
    };
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo topology plane candidate points"));
}

#[test]
fn every_solved_boundary_vertex_must_lie_on_the_analytic_plane() {
    let plane = PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    };
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        agreed_topology_bound_plane(ctx, [[3.0, 4.0, 2.0]], [plane], [])
    })
    .expect("service boundary plane")
    .is_some());
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        agreed_topology_bound_plane(ctx, [[3.0, 4.0, 3.0]], [plane], [])
    })
    .expect("service boundary plane")
    .is_none());
}

fn boundary_plane_collection_error(limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    match agreed_topology_bound_plane(
        &ctx,
        [[0.0, 0.0, 0.0]],
        [PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        }],
        [BoundaryLine {
            origin: [0.0, 0.0, 0.0],
            direction: [1.0, 0.0, 0.0],
        }],
    ) {
        Ok(_) => panic!("one boundary plane exceeds collection limit"),
        Err(error) => error,
    }
}

fn assert_boundary_plane_refusal(limit: u64, operation: &'static str) {
    let error = boundary_plane_collection_error(limit);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn agreed_topology_plane_refuses_boundary_point_vector() {
    assert_boundary_plane_refusal(0, "creo plane boundary points");
}

#[test]
fn agreed_topology_plane_refuses_boundary_line_vector() {
    assert_boundary_plane_refusal(1, "creo plane boundary lines");
}

#[test]
fn agreed_topology_plane_refuses_candidate_point_copy() {
    assert_boundary_plane_refusal(2, "creo topology plane candidate points");
}

#[test]
fn agreed_topology_plane_refuses_candidate_vector() {
    assert_boundary_plane_refusal(3, "creo plane boundary candidates");
}

#[test]
fn distinct_boundary_lines_define_one_plane() {
    let line = |origin, direction| BoundaryLine { origin, direction };
    let plane = topology_bound_line_plane(&[
        line([0.0, 0.0, 3.0], [1.0, 0.0, 0.0]),
        line([0.0, 2.0, 3.0], [1.0, 0.0, 0.0]),
        line([4.0, 0.0, 3.0], [0.0, 1.0, 0.0]),
    ])
    .expect("coplanar boundary lines");
    assert_eq!(plane.normal, [0.0, 0.0, 1.0]);

    assert!(topology_bound_line_plane(&[
        line([0.0, 0.0, 3.0], [1.0, 0.0, 0.0]),
        line([0.0, 2.0, 4.0], [1.0, 0.0, 0.0]),
        line([4.0, 0.0, 3.0], [0.0, 1.0, 0.0]),
    ])
    .is_none());

    let analytic = analytic_boundary_line(&CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(2.0, 0.0, 0.0)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    )))
    .expect("analytic line");
    assert_eq!(analytic.direction, [1.0, 0.0, 0.0]);
}

#[test]
fn unique_native_conic_loop_places_its_plane_surface() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 10,
    });
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 1,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(5), None],
            next_edges: [11, 0],
            offset: 20,
        });
    scan.topology.loops.push(crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 11,
            side: crate::topology::Side::Zero,
        }],
    ));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("creo:visibgeom:curve#11".to_string()).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(2.0, 3.0, 4.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    });

    let transferred = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_topology_bound_planes(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("valid source object identity");

    assert_eq!(transferred, 1);
    let plane = ir
        .model
        .surfaces
        .iter()
        .find(|surface| {
            surface.id
                == SurfaceId::mint("creo:visibgeom:surface#5".to_string())
                    .expect("identity grammar")
        })
        .expect("topology-bound plane");
    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = plane.geometry.solved() else {
        panic!("expected plane geometry");
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.frame().axis().as_raw();
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(origin.z, 4.0);

    scan.curves
        .topology_rows
        .push(scan.curves.topology_rows[0].clone());
    ir.model.surfaces.clear();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| transfer_topology_bound_planes(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        0
    );
}

#[test]
fn unique_nurbs_line_loop_places_its_plane_surface() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 10,
    });
    for id in [11, 12] {
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id,
                type_byte: 0,
                feature_id: 1,
                directions: [0; 2],
                faces: [std::num::NonZeroU32::new(5), None],
                next_edges: [if id == 11 { 12 } else { 11 }, 0],
                offset: 20,
            });
    }
    scan.topology.loops.push(crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        [11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    ));
    let mut ir = cadmpeg_ir::CadIr::empty();
    for (id, origin, direction) in [
        (11, Point3::new(0.0, 0.0, 4.0), Vector3::new(1.0, 0.0, 0.0)),
        (12, Point3::new(0.0, 2.0, 4.0), Vector3::new(0.0, 1.0, 0.0)),
    ] {
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
            geometry: nurbs_curve(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    origin,
                    Point3::new(
                        origin.x + direction.x,
                        origin.y + direction.y,
                        origin.z + direction.z,
                    ),
                ],
                None,
            ),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| transfer_topology_bound_planes(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        1
    );
    assert!(ir.model.surfaces.iter().any(|surface| {
        surface.id
            == SurfaceId::mint("creo:visibgeom:surface#5".to_string()).expect("identity grammar")
            && matches!(&surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))
                        if {
                            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
                            origin.z == 4.0 && *normal == Vector3::new(0.0, 0.0, 1.0)
                        })
    }));
}

#[test]
fn reconciles_equivalent_plane_frames_and_rejects_conflicts() {
    let first = PlaneEquation {
        origin: [1.0, 2.0, 3.0],
        normal: [0.0, 0.0, 2.0],
    };
    let equivalent = PlaneEquation {
        origin: [-4.0, 9.0, 3.0],
        normal: [0.0, 0.0, -1.0],
    };
    let agreed = agreed_plane(&[first, equivalent]).expect("equivalent planes agree");
    assert_eq!(agreed.normal, [0.0, 0.0, 1.0]);
    assert_eq!(dot(agreed.normal, agreed.origin), 3.0);

    let conflicting = PlaneEquation {
        origin: [0.0, 0.0, 4.0],
        normal: [0.0, 0.0, 1.0],
    };
    assert!(agreed_plane(&[first, conflicting]).is_none());
}

#[test]
fn plane_surface_reconciliation_requires_one_chart_direction() {
    let plane = PlaneEquation {
        origin: [0.0, 0.0, 3.0],
        normal: [0.0, 0.0, 1.0],
    };
    let candidate = |origin, u_axis, offset| PlaneCandidate {
        equation: plane,
        chart: Some(PlaneChart {
            origin,
            normal: plane.normal,
            u_axis,
        }),
        offset,
    };
    assert!(agreed_plane_surface(&[
        candidate([0.0, 0.0, 3.0], [1.0, 0.0, 0.0], 20),
        candidate([0.0, 0.0, 3.0], [2.0, 0.0, 0.0], 10),
    ])
    .is_some_and(|(_, u_axis, offset)| u_axis == [1.0, 0.0, 0.0] && offset == 10));
    assert!(agreed_plane_surface(&[
        candidate([0.0, 0.0, 3.0], [1.0, 0.0, 0.0], 10),
        candidate([0.0, 0.0, 3.0], [0.0, 1.0, 0.0], 20),
    ])
    .is_none());
    assert!(agreed_plane_surface(&[
        candidate([0.0, 0.0, 3.0], [1.0, 0.0, 0.0], 10),
        candidate([1.0, 0.0, 3.0], [1.0, 0.0, 0.0], 20),
    ])
    .is_none());
}

#[test]
fn complete_envelope_held_coordinate_defines_only_the_plane_equation() {
    let envelope = PlaneEnvelopeRecord {
        surface_id: 12,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[Some(-2.0), Some(-3.0)], [Some(2.0), Some(3.0)]],
            corners_3d: [
                [Some(-2.0), Some(8.0), Some(-3.0)],
                [Some(2.0), Some(8.0), Some(3.0)],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    };
    let plane = held_coordinate_plane(&envelope).expect("held-coordinate plane");
    assert_eq!(plane.origin, [-2.0, 8.0, -3.0]);
    assert_eq!(plane.normal, [0.0, 1.0, 0.0]);

    let mut unresolved = envelope;
    unresolved.corner_coordinate_equal[2] = None;
    assert!(held_coordinate_plane(&unresolved).is_none());
}

#[test]
fn held_envelope_assigns_mixed_support_frame_roles() {
    let equation = PlaneEquation {
        origin: [0.0, 0.0, -0.85],
        normal: [0.0, 0.0, 1.0],
    };
    let mut frame = PlaneLocalSystem {
        surface_id: 141,
        body: Vec::new(),
        slots: [
            Some(0.0),
            Some(0.0),
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(8.0),
            Some(0.0),
            Some(-0.85),
        ],
        layout: Some(crate::scalar::PlaneSupportFrameLayout::SupportTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 10,
        offset: 20,
    };
    let candidate = envelope_reconciled_plane_candidate(&frame, equation).expect("mixed frame");
    assert_eq!(candidate.equation.origin, equation.origin);
    assert_eq!(candidate.equation.normal, equation.normal);
    assert_eq!(candidate.chart.expect("chart").u_axis, [1.0, 0.0, 0.0]);

    frame.slots[11] = Some(1.0);
    assert!(envelope_reconciled_plane_candidate(&frame, equation).is_none());
}

#[test]
fn frame_bound_outline_supplies_the_plane_chart_origin() {
    let frame = PlaneLocalSystem {
        surface_id: 52,
        body: Vec::new(),
        slots: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -9.0, 48.0, 0.0].map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 10,
        offset: 20,
    };
    let outline = OutlinePlane {
        surface_id: 52,
        origin: [0.0, -4.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        offset: 15,
    };
    let candidate = frame_bound_outline_plane_candidate(&frame, &outline).expect("composite chart");
    assert_eq!(candidate.equation.origin, outline.origin);
    assert_eq!(candidate.equation.normal, outline.normal());
    assert_eq!(candidate.chart.expect("chart").origin, [-9.0, -4.0, 0.0]);

    let mut conflicting = outline;
    conflicting.u_axis = cadmpeg_ir::units::UnitVector3::X_AXIS;
    assert!(frame_bound_outline_plane_candidate(&frame, &conflicting).is_none());
}

#[test]
fn support_frame_selects_one_axis_from_a_line_shaped_plane_outline() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 42,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 10,
    });
    scan.planes.envelopes.push(PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), Some(7.0)],
                [Some(-3.0), Some(-4.0), Some(9.0)],
            ],
        },
        corner_coordinate_equal: [Some(true), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    });
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 100.0, 200.0, 300.0,
        ]
        .map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    });
    scan.planes.outlines = crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::placed_outline_planes(
            ctx,
            &scan.planes.envelopes,
            &scan.planes.local_systems,
        )
    })
    .expect("service outline planes");

    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&42).expect("plane candidates");
    let (plane, u_axis, _) =
        agreed_plane_surface(candidates).expect("frame-selected outline plane");
    assert_eq!(plane.origin, [100.0, -4.0, 300.0]);
    assert_eq!(plane.normal, [0.0, 1.0, 0.0]);
    assert_eq!(u_axis, [0.0, 0.0, 1.0]);
}

#[test]
fn matrix_frame_owns_conflicting_held_coordinate_plane() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 42,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code01,
        next_surface: 0,
        offset: 10,
    });
    scan.planes.envelopes.push(PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-1.0), Some(0.0), Some(1.0)],
                [Some(1.0), Some(0.0), Some(-1.0)],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    });
    let component = std::f64::consts::FRAC_1_SQRT_2;
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [
            Some(1.0),
            Some(0.0),
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(-1.0),
            Some(0.0),
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
        ],
        layout: Some(crate::scalar::PlaneSupportFrameLayout::MatrixColumns),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    });
    scan.planes.outlines = crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::placed_outline_planes(
            ctx,
            &scan.planes.envelopes,
            &scan.planes.local_systems,
        )
    })
    .expect("service outline planes");

    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&42).expect("plane candidates");
    let (plane, u_axis, _) = agreed_plane_surface(candidates).expect("matrix frame plane");
    assert_eq!(plane.normal, [component, 0.0, component]);
    assert_eq!(u_axis, [component, 0.0, -component]);
}

#[test]
fn fc05_cap_pair_tangency_selects_one_stored_plane_branch() {
    const EPS_BRANCH_TEST: f64 = 1e-12;

    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (1, crate::surface::SurfaceKind::Plane),
        (2, crate::surface::SurfaceKind::Plane),
        (5, crate::surface::SurfaceKind::Plane),
        (7, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.planes.outlines.extend([
        OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 10,
        },
        OutlinePlane {
            surface_id: 2,
            origin: [0.0, 38.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 20,
        },
    ]);
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 10,
            type_byte: 0,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(7), std::num::NonZeroU32::new(5)],
            next_edges: [0; 2],
            offset: 30,
        });
    scan.curves
        .fc05_cylinder_cap_pairs
        .push(crate::curve::Fc05CylinderCapPair {
            surface_id: 7,
            cap_edges: vec![
                crate::curve::Fc05CapEdge {
                    curve_id: 11,
                    cap_plane_id: 1,
                    cap_ordinate_row_frame: 0.0,
                },
                crate::curve::Fc05CapEdge {
                    curve_id: 12,
                    cap_plane_id: 2,
                    cap_ordinate_row_frame: 38.0,
                },
            ],
            center_row_frame: [2.0, 3.0],
            radius_mm: 0.5,
            reference_direction_row_frame: [1.0, 0.0],
            parameter_sense: crate::curve::ParameterSense::Increasing,
            cap_ordinates_row_frame: vec![0.0, 38.0],
            offset: 40,
        });
    let origin_z = -(17.0 / 8.0);
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 5,
        body: Vec::new(),
        slots: [
            Some(0.8),
            Some(0.0),
            Some(-0.6),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.6),
            Some(0.0),
            Some(0.8),
            Some(0.0),
            Some(0.0),
            Some(origin_z),
        ],
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 50,
        offset: 60,
    });

    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let [candidate] = candidates.get(&5).expect("plane candidates").as_slice() else {
        panic!("one tangent branch must be selected");
    };
    assert!((candidate.equation.normal[0] - 0.6).abs() < EPS_BRANCH_TEST);
    assert!((candidate.equation.normal[2] + 0.8).abs() < EPS_BRANCH_TEST);
    assert!((candidate.equation.origin[2] + origin_z).abs() < EPS_BRANCH_TEST);
    assert!((candidate.chart.expect("stored chart").u_axis[2] - 0.6).abs() < EPS_BRANCH_TEST);
}

#[test]
fn fc05_cap_pair_frame_reconstructs_parameter_origin_from_cap_spans() {
    const EPS_FC05_FRAME_TEST: f64 = 1e-12;

    let mut scan = crate::test_support::empty_container_scan();
    scan.planes.outlines.extend([
        OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 10,
        },
        OutlinePlane {
            surface_id: 2,
            origin: [0.0, 38.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 20,
        },
    ]);
    let pair = crate::curve::Fc05CylinderCapPair {
        surface_id: 7,
        cap_edges: vec![
            crate::curve::Fc05CapEdge {
                curve_id: 11,
                cap_plane_id: 1,
                cap_ordinate_row_frame: -87.5368,
            },
            crate::curve::Fc05CapEdge {
                curve_id: 12,
                cap_plane_id: 2,
                cap_ordinate_row_frame: -49.5368,
            },
        ],
        center_row_frame: [2.0, 3.0],
        radius_mm: 0.5,
        reference_direction_row_frame: [1.0, 0.0],
        parameter_sense: crate::curve::ParameterSense::Increasing,
        cap_ordinates_row_frame: vec![-87.5368, -49.5368],
        offset: 30,
    };

    let frame = fc05_cap_pair_model_frame(&scan, &pair).expect("unit cap-span frame");
    assert_eq!(frame.unit_vector(), [0.0, 1.0, 0.0]);
    assert!((frame.origin[0] - 2.0).abs() <= EPS_FC05_FRAME_TEST);
    assert!((frame.origin[1] - 87.5368).abs() <= EPS_FC05_FRAME_TEST);
    assert!((frame.origin[2] - 3.0).abs() <= EPS_FC05_FRAME_TEST);

    let reversed = crate::curve::Fc05CylinderCapPair {
        cap_edges: vec![
            crate::curve::Fc05CapEdge {
                curve_id: 11,
                cap_plane_id: 2,
                cap_ordinate_row_frame: -87.5368,
            },
            crate::curve::Fc05CapEdge {
                curve_id: 12,
                cap_plane_id: 1,
                cap_ordinate_row_frame: -49.5368,
            },
        ],
        cap_ordinates_row_frame: vec![-87.5368, -49.5368],
        ..pair
    };
    let reversed_frame =
        fc05_cap_pair_model_frame(&scan, &reversed).expect("reversed unit cap-span frame");
    assert_eq!(reversed_frame.unit_vector(), [0.0, -1.0, 0.0]);
    assert!((reversed_frame.origin[1] + 49.5368).abs() <= EPS_FC05_FRAME_TEST);
}

#[test]
fn fc05_strict_cap_pair_accepts_a_reference_frame_when_tangency_improves() {
    const EPS_BRANCH_TEST: f64 = 1e-12;

    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (1, crate::surface::SurfaceKind::Plane),
        (2, crate::surface::SurfaceKind::Plane),
        (5, crate::surface::SurfaceKind::Plane),
        (7, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.planes.outlines.extend([
        OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 10,
        },
        OutlinePlane {
            surface_id: 2,
            origin: [0.0, 38.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 20,
        },
    ]);
    scan.curves.fc05_circles.extend([
        crate::curve::Fc05Circle {
            curve_id: 11,
            center_row_frame: [2.0, 3.0],
            radius_mm: 0.5,
            sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length(
                [1.0, 0.0],
            )
            .expect("unit sample direction")
            .0,
            angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
                sense: crate::curve::ParameterSense::Increasing,
                reference_direction_row_frame: [1.0, 0.0],
            },
            cap_ordinate_row_frame: Some(0.0),
            point_count: 8,
            max_residual: 0.0,
            offset: 30,
        },
        crate::curve::Fc05Circle {
            curve_id: 12,
            center_row_frame: [2.0, 3.0],
            radius_mm: 0.5,
            sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length(
                [1.0, 0.0],
            )
            .expect("unit sample direction")
            .0,
            angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
                sense: crate::curve::ParameterSense::Increasing,
                reference_direction_row_frame: [1.0, 0.0],
            },
            cap_ordinate_row_frame: Some(38.0),
            point_count: 8,
            max_residual: 0.0,
            offset: 31,
        },
    ]);
    scan.curves.topology_rows.extend([
        crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(7), std::num::NonZeroU32::new(1)],
            next_edges: [0; 2],
            offset: 40,
        },
        crate::curve::CurveTopologyRow {
            id: 12,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(7), std::num::NonZeroU32::new(2)],
            next_edges: [0; 2],
            offset: 41,
        },
        crate::curve::CurveTopologyRow {
            id: 13,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(7), std::num::NonZeroU32::new(5)],
            next_edges: [0; 2],
            offset: 42,
        },
    ]);
    scan.curves
        .fc05_cylinder_cap_pairs
        .push(crate::curve::Fc05CylinderCapPair {
            surface_id: 7,
            cap_edges: vec![
                crate::curve::Fc05CapEdge {
                    curve_id: 11,
                    cap_plane_id: 1,
                    cap_ordinate_row_frame: 0.0,
                },
                crate::curve::Fc05CapEdge {
                    curve_id: 12,
                    cap_plane_id: 2,
                    cap_ordinate_row_frame: 38.0,
                },
            ],
            center_row_frame: [2.0, 3.0],
            radius_mm: 0.5,
            reference_direction_row_frame: [1.0, 0.0],
            parameter_sense: crate::curve::ParameterSense::Increasing,
            cap_ordinates_row_frame: vec![0.0, 38.0],
            offset: 43,
        });
    scan.references.circles.extend([
        crate::reference::ReferenceCircle::try_new(
            11,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new([2.0, 0.0, -3.0].into())
                    .expect("finite center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(0.5).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::Y_AXIS,
            [
                cadmpeg_ir::features::FinitePoint3::new([2.5, 0.0, -3.0].into())
                    .expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new([2.0, 0.0, -2.5].into())
                    .expect("finite end"),
            ],
            50,
        )
        .expect("checked reference geometry"),
        crate::reference::ReferenceCircle::try_new(
            12,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new([2.0, 38.0, -3.0].into())
                    .expect("finite center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(0.5).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::Y_AXIS,
            [
                cadmpeg_ir::features::FinitePoint3::new([2.5, 38.0, -3.0].into())
                    .expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new([2.0, 38.0, -2.5].into())
                    .expect("finite end"),
            ],
            51,
        )
        .expect("checked reference geometry"),
    ]);
    let origin_z = -(17.0 / 8.0);
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 5,
        body: Vec::new(),
        slots: [
            Some(0.8),
            Some(0.0),
            Some(-0.6),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.6),
            Some(0.0),
            Some(0.8),
            Some(0.0),
            Some(0.0),
            Some(origin_z),
        ],
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 50,
        offset: 60,
    });

    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let [candidate] = candidates.get(&5).expect("plane candidates").as_slice() else {
        panic!("the reference-tangent branch must be selected");
    };
    assert!((candidate.equation.normal[0] - 0.6).abs() < EPS_BRANCH_TEST);
    assert!((candidate.equation.normal[2] - 0.8).abs() < EPS_BRANCH_TEST);
    assert!((candidate.equation.origin[2] - origin_z).abs() < EPS_BRANCH_TEST);
}

mod branch_witnesses;
