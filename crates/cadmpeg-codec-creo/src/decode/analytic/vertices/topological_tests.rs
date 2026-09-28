use crate::decode::analytic::vertices::{
    conic_conic_intersections, finite_model_point, incident_analytic_vertex_domain,
    line_conic_intersections, line_line_intersection, model_points_agree, solve_topological_vertices,
};
use crate::decode::surfaces::intersection_resolve::curve_contains_points;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn incident_line_collection_error(limit: u64) -> CodecError {
    let first = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let second = line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    incident_analytic_vertex_domain(&ctx, &[&first, &second])
        .expect_err("incident candidate collection exceeds limit")
}

fn one_carrier_vertex_collection_error(limit: u64) -> CodecError {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let half_edge = crate::topology::HalfEdgeId {
        curve_id: 7,
        side: crate::topology::Side::Zero,
    };
    scan.topology.vertices.push(crate::topology::TopologicalVertex {
        id: 1,
        half_edges: vec![half_edge],
    });
    scan.topology.half_edges.push(crate::topology::HalfEdge {
        id: half_edge,
        face_id: std::num::NonZeroU32::new(5),
        next: None,
    });
    let carriers = std::collections::BTreeMap::from([(
        5,
        crate::decode::analytic::equations::CarrierEquation::Plane(
            crate::decode::analytic::equations::PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            },
        ),
    )]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    solve_topological_vertices(
        &ctx,
        &scan,
        &cadmpeg_ir::document::CadIr::empty(),
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("vertex collection exceeds limit")
}

fn assert_vertex_collection_refusal(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

fn conic_model_intersection_result(limit: u64) -> Result<Vec<[f64; 3]>, CodecError> {
    let circle = |center: [f64; 3]| {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::from(center),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CircleCurve fixture"),
        ))
    };
    let first = circle([0.0, 0.0, 0.0]);
    let second = circle([2.0, 0.0, 0.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    conic_conic_intersections(&ctx, &first, &second)
}

#[test]
fn conic_conic_intersections_refuse_model_intersection_reservation() {
    assert_vertex_collection_refusal(
        conic_model_intersection_result(249).expect_err("intersection vector exceeds limit"),
        "creo conic model intersections",
    );
}

#[test]
fn solve_topological_vertices_refuses_incident_face_ids() {
    assert_vertex_collection_refusal(
        one_carrier_vertex_collection_error(3),
        "creo carrier incident face IDs",
    );
}

#[test]
fn solve_topological_vertices_refuses_incident_carriers() {
    assert_vertex_collection_refusal(
        one_carrier_vertex_collection_error(4),
        "creo vertex incident carriers",
    );
}

#[test]
fn solve_topological_vertices_refuses_sample_face_ids() {
    assert_vertex_collection_refusal(
        one_carrier_vertex_collection_error(5),
        "creo carrier rejection face IDs",
    );
}

#[test]
fn solve_topological_vertices_refuses_sample_carrier_kinds() {
    assert_vertex_collection_refusal(
        one_carrier_vertex_collection_error(6),
        "creo carrier rejection kinds",
    );
}

#[test]
fn solve_topological_vertices_refuses_sample_collection() {
    assert_vertex_collection_refusal(
        one_carrier_vertex_collection_error(7),
        "creo carrier rejection samples",
    );
}

#[test]
fn solve_topological_vertices_refuses_carrier_point_node() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut carriers = std::collections::BTreeMap::new();
    let mut half_edges = Vec::new();
    for (curve_id, face_id, axis) in [(10, 5, 0), (11, 6, 1), (12, 7, 2)] {
        let id = crate::topology::HalfEdgeId {
            curve_id,
            side: crate::topology::Side::Zero,
        };
        half_edges.push(id);
        scan.topology.half_edges.push(crate::topology::HalfEdge {
            id,
            face_id: std::num::NonZeroU32::new(face_id),
            next: None,
        });
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        carriers.insert(
            face_id,
            crate::decode::analytic::equations::CarrierEquation::Plane(
                crate::decode::analytic::equations::PlaneEquation {
                    origin: [0.0, 0.0, 0.0],
                    normal,
                },
            ),
        );
    }
    scan.topology.vertices.push(crate::topology::TopologicalVertex {
        id: 1,
        half_edges,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 18;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = solve_topological_vertices(
        &ctx,
        &scan,
        &cadmpeg_ir::document::CadIr::empty(),
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("carrier point node exceeds limit");
    assert_vertex_collection_refusal(error, "creo carrier vertex point nodes");
}

fn pcurve_vertex_case() -> (
    crate::container::ContainerScan<'static>,
    cadmpeg_ir::document::CadIr,
    std::collections::BTreeMap<u32, crate::decode::analytic::equations::CarrierEquation>,
) {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let side_zero = crate::topology::HalfEdgeId {
        curve_id: 7,
        side: crate::topology::Side::Zero,
    };
    let side_one = crate::topology::HalfEdgeId {
        curve_id: 7,
        side: crate::topology::Side::One,
    };
    scan.curves.topology_rows.push(crate::curve::CurveTopologyRow {
        id: 7,
        type_byte: 0,
        feature_id: 0,
        directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
        next_edges: [7, 7],
        offset: 0,
    });
    scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
        curve_id: 7,
        faces: [10, 11].map(std::num::NonZeroU32::new),
        face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
        face_1_endpoints: [[1.0, 2.0], [3.0, 4.0]],
        offset: 0,
    });
    scan.topology.loops.push(crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(10),
        half_edges: vec![side_zero],
    });
    for (vertex_id, id, face_id, end_vertex_id) in [
        (1, side_zero, 10, 2),
        (2, side_one, 11, 1),
    ] {
        scan.topology.vertices.push(crate::topology::TopologicalVertex {
            id: vertex_id,
            half_edges: vec![id],
        });
        scan.topology.half_edges.push(crate::topology::HalfEdge {
            id,
            face_id: std::num::NonZeroU32::new(face_id),
            next: None,
        });
        scan.topology.half_edge_vertex_incidence.push(
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: id,
                start_vertex_id: vertex_id,
                end_vertex_id: Some(end_vertex_id),
            },
        );
    }
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for face_id in [10, 11] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!(
                "creo:visibgeom:surface#{face_id}"
            ))
            .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid PlaneSurface fixture"),
                ),
            ),
            source_object: None,
        });
    }
    let carriers = [10, 11]
        .into_iter()
        .map(|face_id| {
            (
                face_id,
                crate::decode::analytic::equations::CarrierEquation::Plane(
                    crate::decode::analytic::equations::PlaneEquation {
                        origin: [0.0, 0.0, 0.0],
                        normal: [0.0, 0.0, 1.0],
                    },
                ),
            )
        })
        .collect();
    (scan, ir, carriers)
}

fn pcurve_vertex_result(
    limit: u64,
) -> Result<super::SolvedTopologicalVertices, CodecError> {
    let (scan, ir, carriers) = pcurve_vertex_case();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    solve_topological_vertices(
        &ctx,
        &scan,
        &ir,
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

fn analytic_vertex_result(
    limit: u64,
    two_curves: bool,
) -> Result<super::SolvedTopologicalVertices, CodecError> {
    let (mut scan, mut ir, carriers) = pcurve_vertex_case();
    let make_curve = |curve_id: u32, direction: [f64; 3]| cadmpeg_ir::geometry::Curve {
        id: cadmpeg_ir::ids::CurveId::mint(format!(
            "creo:visibgeom:curve#{curve_id}"
        ))
        .expect("identity grammar"),
        geometry: line([0.0, 0.0, 0.0], direction),
        source_object: None,
    };
    ir.model.curves.push(make_curve(7, [1.0, 0.0, 0.0]));
    if two_curves {
        let id = crate::topology::HalfEdgeId {
            curve_id: 8,
            side: crate::topology::Side::Zero,
        };
        scan.topology.vertices[0].half_edges.push(id);
        scan.topology.half_edges.push(crate::topology::HalfEdge {
            id,
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        });
        scan.curves.topology_rows.push(crate::curve::CurveTopologyRow {
            id: 8,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(10), None],
            next_edges: [8, 0],
            offset: 0,
        });
        ir.model.curves.push(make_curve(8, [0.0, 1.0, 0.0]));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    solve_topological_vertices(
        &ctx,
        &scan,
        &ir,
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

fn ambiguous_vertex_result(limit: u64) -> Result<super::SolvedTopologicalVertices, CodecError> {
    let (mut scan, ir, carriers) = pcurve_vertex_case();
    let side_zero = crate::topology::HalfEdgeId {
        curve_id: 8,
        side: crate::topology::Side::Zero,
    };
    let side_one = crate::topology::HalfEdgeId {
        curve_id: 8,
        side: crate::topology::Side::One,
    };
    scan.curves.topology_rows.push(crate::curve::CurveTopologyRow {
        id: 8,
        type_byte: 0,
        feature_id: 0,
        directions: [0x01, 0xf6],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
        next_edges: [8, 8],
        offset: 0,
    });
    scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
        curve_id: 8,
        faces: [10, 11].map(std::num::NonZeroU32::new),
        face_0_endpoints: [[5.0, 6.0], [7.0, 8.0]],
        face_1_endpoints: [[5.0, 6.0], [7.0, 8.0]],
        offset: 0,
    });
    scan.topology.loops.push(crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(10),
        half_edges: vec![side_zero],
    });
    for (vertex_index, id, face_id, end_vertex_id) in [
        (0, side_zero, 10, 2),
        (1, side_one, 11, 1),
    ] {
        scan.topology.vertices[vertex_index].half_edges.push(id);
        scan.topology.half_edges.push(crate::topology::HalfEdge {
            id,
            face_id: std::num::NonZeroU32::new(face_id),
            next: None,
        });
        scan.topology.half_edge_vertex_incidence.push(
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: id,
                start_vertex_id: u32::try_from(vertex_index + 1).expect("two vertices"),
                end_vertex_id: Some(end_vertex_id),
            },
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    solve_topological_vertices(
        &ctx,
        &scan,
        &ir,
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

fn authoritative_vertex_result(limit: u64) -> Result<super::SolvedTopologicalVertices, CodecError> {
    let (mut scan, ir, carriers) = pcurve_vertex_case();
    scan.curves.pcurves.clear();
    scan.curves.two_chart_pcurves.push(crate::curve::TwoChartPcurveSamples {
        curve_id: 7,
        faces: [10, 11],
        samples: vec![
            [[1.0, 2.0], [1.0, 2.0]],
            [[3.0, 4.0], [3.0, 4.0]],
        ],
        offset: 0,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    solve_topological_vertices(
        &ctx,
        &scan,
        &ir,
        &carriers,
        &std::collections::BTreeSet::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

#[test]
fn authoritative_vertex_fixture_keeps_service_result() {
    let result = authoritative_vertex_result(1_000_000).expect("service authoritative solve");
    assert_eq!(result.diagnostics.pcurve.accepted_records, 1);
    assert_eq!(result.diagnostics.pcurve_constraints, 1);
    assert_eq!(result.points.len(), 2);
}

#[test]
fn solve_topological_vertices_refuses_authoritative_point_node() {
    assert_vertex_collection_refusal(
        authoritative_vertex_result(38).expect_err("authoritative node exceeds limit"),
        "creo authoritative vertex point nodes",
    );
}

#[test]
fn ambiguous_vertex_fixture_keeps_service_result() {
    let result = ambiguous_vertex_result(1_000_000).expect("service ambiguous solve");
    assert_eq!(result.diagnostics.pcurve.accepted_records, 2);
    assert_eq!(result.diagnostics.pcurve_ambiguous_endpoint_vertices, 2);
}

#[test]
fn solve_topological_vertices_refuses_ambiguous_pcurve_vertex_node() {
    assert_vertex_collection_refusal(
        ambiguous_vertex_result(46).expect_err("ambiguous vertex node exceeds limit"),
        "creo ambiguous pcurve vertex nodes",
    );
}

#[test]
fn analytic_vertex_fixture_keeps_service_result() {
    let result = analytic_vertex_result(1_000_000, true).expect("service analytic vertex solve");
    assert_eq!(result.diagnostics.analytic_domain_vertices, 1);
}

#[test]
fn solve_topological_vertices_refuses_analytic_curve_lookup_node() {
    assert_vertex_collection_refusal(
        analytic_vertex_result(42, true).expect_err("lookup node exceeds limit"),
        "creo analytic curve lookup nodes",
    );
}

#[test]
fn solve_topological_vertices_refuses_incident_analytic_curve() {
    assert_vertex_collection_refusal(
        analytic_vertex_result(44, true).expect_err("incident curve exceeds limit"),
        "creo incident analytic curves",
    );
}

#[test]
fn solve_topological_vertices_refuses_incident_analytic_curve_node() {
    assert_vertex_collection_refusal(
        analytic_vertex_result(46, true).expect_err("incident node exceeds limit"),
        "creo incident analytic curve nodes",
    );
}

#[test]
fn solve_topological_vertices_refuses_analytic_domain_node() {
    assert_vertex_collection_refusal(
        analytic_vertex_result(51, true).expect_err("domain node exceeds limit"),
        "creo analytic vertex domain nodes",
    );
}

#[test]
fn pcurve_vertex_fixture_keeps_service_result() {
    let result = pcurve_vertex_result(1_000_000).expect("service vertex solve");
    assert_eq!(result.diagnostics.pcurve.accepted_records, 1);
    assert_eq!(result.diagnostics.pcurve_constraints, 1);
    assert_eq!(result.points.len(), 2);
}

#[test]
fn solve_topological_vertices_refuses_pcurve_endpoint_node() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(28).expect_err("endpoint node exceeds limit"),
        "creo vertex pcurve endpoint nodes",
    );
}

#[test]
fn solve_topological_vertices_refuses_pcurve_candidate_node() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(31).expect_err("candidate node exceeds limit"),
        "creo vertex pcurve candidate nodes",
    );
}

#[test]
fn solve_topological_vertices_refuses_pcurve_candidate_point() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(32).expect_err("candidate point exceeds limit"),
        "creo vertex pcurve candidate points",
    );
}

#[test]
fn solve_topological_vertices_refuses_pcurve_constraint() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(35).expect_err("pcurve constraint exceeds limit"),
        "creo vertex pcurve constraints",
    );
}

#[test]
fn solve_topological_vertices_refuses_endpoint_constraint() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(36).expect_err("endpoint constraint exceeds limit"),
        "creo vertex endpoint constraints",
    );
}

#[test]
fn solve_topological_vertices_refuses_fixed_point_node() {
    assert_vertex_collection_refusal(
        pcurve_vertex_result(37).expect_err("fixed point node exceeds limit"),
        "creo fixed vertex point nodes",
    );
}

#[test]
fn incident_analytic_vertex_domain_refuses_candidate_points() {
    assert!(matches!(incident_line_collection_error(0), CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo incident analytic candidates"));
}

#[test]
fn incident_analytic_vertex_domain_refuses_unique_points() {
    assert!(matches!(incident_line_collection_error(1), CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique analytic candidates"));
}

/// Whether two finite fixture points agree.
fn agree(first: [f64; 3], second: [f64; 3]) -> bool {
    model_points_agree(
        finite_model_point(first).expect("finite fixture point"),
        finite_model_point(second).expect("finite fixture point"),
    )
}

fn line(origin: [f64; 3], direction: [f64; 3]) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::from(origin),
            Vector3::from(direction)
                .unit()
                .expect("nonzero fixture direction"),
        )
        .expect("valid LineCurve fixture"),
    ))
}

#[test]
fn incident_lines_define_one_validated_vertex() {
    let first = line([1.0, 2.0, 3.0], [2.0, 0.0, 0.0]);
    let second = line([1.0, -4.0, 3.0], [0.0, 3.0, 0.0]);
    let third = line([1.0, 2.0, -5.0], [0.0, 0.0, 4.0]);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| incident_analytic_vertex_domain(
            ctx,
            &[&first, &second, &third]
        ))
        .expect("test carrier solve"),
        [[1.0, 2.0, 3.0]]
    );
}

#[test]
fn incident_lines_reject_skew_parallel_and_disagreeing_candidates() {
    let x = line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let skew_y = line([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let parallel = line([0.0, 1.0, 0.0], [2.0, 0.0, 0.0]);
    let crossing_y = line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let displaced_z = line([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

    assert_eq!(line_line_intersection(&x, &skew_y), None);
    assert_eq!(line_line_intersection(&x, &parallel), None);
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| incident_analytic_vertex_domain(
            ctx,
            &[&x, &crossing_y, &displaced_z]
        ))
        .expect("test carrier solve")
        .is_empty()
    );
}

#[test]
fn line_conic_candidates_cover_periodic_and_nonperiodic_families() {
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let secant = line([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let tangent = line([-3.0, 2.0, 0.0], [1.0, 0.0, 0.0]);
    let skew = line([-3.0, 0.0, 1.0], [1.0, 0.0, 0.0]);

    // The points are in ascending order of the line parameter. The secant runs
    // from (-3, 0, 0) along +x, so it meets the circle of radius 2 at the
    // parameters 1 and 5.
    assert_eq!(
        line_conic_intersections(&secant, &circle),
        [[-2.0, 0.0, 0.0], [2.0, 0.0, 0.0]]
    );
    assert_eq!(
        line_conic_intersections(&tangent, &circle),
        [[0.0, 2.0, 0.0]]
    );
    assert!(line_conic_intersections(&skew, &circle).is_empty());

    let ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ));
    assert_eq!(
        line_conic_intersections(&secant, &ellipse),
        [[-3.0, 0.0, 0.0], [3.0, 0.0, 0.0]]
    );

    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid ParabolaCurve fixture"),
    ));
    assert_eq!(
        line_conic_intersections(&line([1.0, -3.0, 0.0], [0.0, 1.0, 0.0]), &parabola),
        [[1.0, -2.0, 0.0], [1.0, 2.0, 0.0]]
    );
    assert_eq!(
        line_conic_intersections(&line([-3.0, 2.0, 0.0], [1.0, 0.0, 0.0]), &parabola),
        [[1.0, 2.0, 0.0]]
    );

    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        cadmpeg_ir::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
        )
        .expect("valid HyperbolaCurve fixture"),
    ));
    let hyperbola_points =
        line_conic_intersections(&line([4.0, -3.0, 0.0], [0.0, 1.0, 0.0]), &hyperbola);
    assert_eq!(hyperbola_points.len(), 2);
    assert!(hyperbola_points.iter().all(|point| {
        agree(*point, [4.0, 3.0_f64.sqrt(), 0.0]) || agree(*point, [4.0, -3.0_f64.sqrt(), 0.0])
    }));
    assert_eq!(
        line_conic_intersections(&line([-3.0, 0.0, 0.0], [1.0, 0.0, 0.0]), &hyperbola),
        [[2.0, 0.0, 0.0]]
    );
}

#[test]
fn conic_pair_candidates_cover_coplanar_and_transverse_planes() {
    let circle = |center: [f64; 3], axis: [f64; 3], radius| {
        let reference = if axis[0].abs() > 0.5 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::from(center),
                Vector3::from(axis),
                Vector3::from(reference),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        ))
    };
    let first = circle([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0);
    let secant = circle([2.0, 0.0, 0.0], [0.0, 0.0, -1.0], 2.0);
    let tangent = circle([4.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0);
    let transverse = circle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 2.0);

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| conic_conic_intersections(ctx, &first, &first))
            .expect("test carrier solve")
            .is_empty()
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| conic_conic_intersections(
            ctx,
            &first,
            &circle([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 2.0),
        ))
        .expect("test carrier solve")
        .is_empty()
    );
    let secant_points =
        crate::decode::with_test_decode_ctx(|ctx| conic_conic_intersections(ctx, &first, &secant))
            .expect("test carrier solve");
    assert_eq!(secant_points.len(), 2);
    assert!(secant_points.iter().all(|point| {
        agree(*point, [1.0, 3.0_f64.sqrt(), 0.0]) || agree(*point, [1.0, -3.0_f64.sqrt(), 0.0])
    }));
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| conic_conic_intersections(ctx, &first, &tangent))
            .expect("test carrier solve"),
        [[2.0, 0.0, 0.0]]
    );
    let transverse_points = crate::decode::with_test_decode_ctx(|ctx| {
        conic_conic_intersections(ctx, &first, &transverse)
    })
    .expect("test carrier solve");
    assert_eq!(transverse_points.len(), 2);
    assert!(transverse_points
        .iter()
        .all(|point| { agree(*point, [0.0, 2.0, 0.0]) || agree(*point, [0.0, -2.0, 0.0]) }));

    let ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ));
    let ellipse_points =
        crate::decode::with_test_decode_ctx(|ctx| conic_conic_intersections(ctx, &first, &ellipse))
            .expect("test carrier solve");
    assert_eq!(ellipse_points.len(), 2);
    assert!(ellipse_points
        .iter()
        .all(|point| { agree(*point, [0.0, 2.0, 0.0]) || agree(*point, [0.0, -2.0, 0.0]) }));
    let diagonal_ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 1.0, 0.0)
                .unit()
                .expect("nonzero fixture direction"),
            3.0,
            2.0,
        )
        .expect("valid EllipseCurve fixture"),
    ));
    let larger_circle = circle([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.5);
    let diagonal_points = crate::decode::with_test_decode_ctx(|ctx| {
        conic_conic_intersections(ctx, &larger_circle, &diagonal_ellipse)
    })
    .expect("test carrier solve");
    assert_eq!(diagonal_points.len(), 4);
    assert!(diagonal_points.iter().all(|point| {
        curve_contains_points(&larger_circle, [*point, *point])
            && curve_contains_points(&diagonal_ellipse, [*point, *point])
    }));

    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        cadmpeg_ir::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid ParabolaCurve fixture"),
    ));
    let tangent_circle = circle([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0);
    let tangent_points = crate::decode::with_test_decode_ctx(|ctx| {
        conic_conic_intersections(ctx, &parabola, &tangent_circle)
    })
    .expect("test carrier solve");
    assert_eq!(tangent_points.len(), 1);
    assert!(agree(tangent_points[0], [0.0, 0.0, 0.0]));
}
