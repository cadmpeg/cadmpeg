// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{
    admitted_face_components, component_is_closed, is_neutral_face_reference,
    legacy_body_ownership_is_unambiguous, merge_body_components, native_parameter_loop_polygon,
    ordered_native_parameter_face_loops, split_neutral_component_shells, transfer_native_brep,
    model_typed_nonlinear_curve_ids, push_native_pcurve_candidate, BrepTransferDiagnostics,
    FaceAdmissionDetail, FaceAdmissionRejection, NativeBrepCurveEvidence, NativeCurveEvidence,
    BrepEdgeIndexes, BrepFaceCandidateIndexes, BrepSourceIndexes, NativePcurveCandidates,
    NeutralShellSpec,
};

mod eligible_index;
mod body_index;

fn brep_edge_index_input() -> (
    Vec<crate::curve::CurveTopologyRow>,
    BTreeMap<u32, [u32; 2]>,
    BTreeMap<u32, [f64; 3]>,
    CadIr,
) {
    let rows = vec![crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 1,
        directions: [0, 0],
        faces: [None, None],
        next_edges: [0, 0],
        offset: 0,
    }];
    let native_vertices = BTreeMap::from([(10, [1, 2])]);
    let solved_vertices = BTreeMap::from([
        (1, [0.0, 0.0, 0.0]),
        (2, [1.0, 0.0, 0.0]),
    ]);
    (rows, native_vertices, solved_vertices, CadIr::empty())
}

fn brep_edge_index_limit_error(limit: u64) -> CodecError {
    let (rows, native_vertices, solved_vertices, ir) = brep_edge_index_input();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    BrepEdgeIndexes::from_rows(&ctx, &rows, &native_vertices, &solved_vertices, &ir)
        .err()
        .expect("edge-index node refused")
}

#[test]
fn brep_edge_vertex_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error(2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep edge-vertex nodes"));
}

#[test]
fn brep_model_curve_count_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error(3);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep model curve count nodes"));
}

#[test]
fn brep_admitted_edge_id_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error(4);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep admitted edge ID nodes"));
}

#[test]
fn brep_edge_indexes_preserve_model_curve_multiplicity() {
    let (rows, native_vertices, solved_vertices, mut ir) = brep_edge_index_input();
    let curve = Curve {
        id: CurveId::compose(&crate::identity::VISIBGEOM_CURVE, 10),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    };
    ir.model.curves.push(curve.clone());
    let one = crate::decode::with_test_decode_ctx(|ctx| {
        BrepEdgeIndexes::from_rows(ctx, &rows, &native_vertices, &solved_vertices, &ir)
    })
    .expect("one service edge admitted");
    assert_eq!(one.edge_vertices[&10], [1, 2]);
    assert_eq!(one.model_curve_counts[&10], 1);
    assert_eq!(one.admitted_edge_curves, BTreeSet::from([10]));
    ir.model.curves.push(curve);
    let duplicate = crate::decode::with_test_decode_ctx(|ctx| {
        BrepEdgeIndexes::from_rows(ctx, &rows, &native_vertices, &solved_vertices, &ir)
    })
    .expect("duplicate service edge counted");
    assert_eq!(duplicate.model_curve_counts[&10], 2);
    assert!(duplicate.admitted_edge_curves.is_empty());
}

fn face_candidate_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.layout = crate::container::Layout::Nd;
    scan.topology.loops.push(crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: Vec::new(),
    });
    scan
}

fn face_candidate_index_limit_error(limit: u64) -> CodecError {
    let scan = face_candidate_scan();
    let ir = CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    BrepFaceCandidateIndexes::from_scan(&ctx, &scan, &ir)
        .err()
        .expect("candidate index allocation refused")
}

#[test]
fn brep_face_loop_index_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error(0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face-loop index nodes"));
}

#[test]
fn brep_face_loop_references_refuse_collection_limit() {
    let error = face_candidate_index_limit_error(1);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face-loop references"));
}

#[test]
fn brep_topology_face_id_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error(2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep topology face ID nodes"));
}

#[test]
fn brep_candidate_face_id_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error(3);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep candidate face ID nodes"));
}

#[test]
fn brep_model_surface_count_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error(4);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep model surface count nodes"));
}

#[test]
fn brep_boundary_curve_id_nodes_refuse_collection_limit() {
    let mut scan = face_candidate_scan();
    scan.topology.loops[0].half_edges.push(crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = BrepFaceCandidateIndexes::from_scan(&ctx, &scan, &CadIr::empty())
        .err()
        .expect("boundary curve node refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep boundary curve ID nodes"));
}

#[test]
fn brep_face_candidate_indexes_preserve_service_selection() {
    let scan = face_candidate_scan();
    let indexes = crate::decode::with_test_decode_ctx(|ctx| {
        BrepFaceCandidateIndexes::from_scan(ctx, &scan, &CadIr::empty())
    })
    .expect("service face candidates admitted");
    assert_eq!(indexes.loops_by_face[&5].len(), 1);
    assert_eq!(indexes.candidate_face_ids, BTreeSet::from([5]));
    assert_eq!(indexes.model_surface_counts[&5], 0);
    assert!(indexes.boundary_curve_ids.is_empty());
    assert_eq!(indexes.legacy_nonvisible_face_reference_count, 0);
}

fn source_index_limit_error(kind: &str) -> CodecError {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let mut carriers = BTreeMap::new();
    let half_edge = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    };
    match kind {
        "plane" => {
            carriers.insert(
                5,
                crate::decode::analytic::equations::CarrierEquation::Plane(
                    crate::decode::analytic::equations::PlaneEquation {
                        origin: [0.0; 3],
                        normal: [0.0, 0.0, 1.0],
                    },
                ),
            );
        }
        "half_edge" => scan.topology.half_edges.push(crate::topology::HalfEdge {
            id: half_edge,
            face_id: std::num::NonZeroU32::new(5),
            next: None,
        }),
        "incidence" => scan.topology.half_edge_vertex_incidence.push(
            crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id: 1,
                end_vertex_id: Some(2),
            },
        ),
        _ => panic!("unsupported source-index fixture"),
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    BrepSourceIndexes::from_scan(&ctx, &carriers, &scan)
        .err()
        .expect("source-index node refused")
}

#[test]
fn brep_plane_index_nodes_refuse_collection_limit() {
    let error = source_index_limit_error("plane");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep plane index nodes"));
}

#[test]
fn brep_half_edge_index_nodes_refuse_collection_limit() {
    let error = source_index_limit_error("half_edge");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep half-edge index nodes"));
}

#[test]
fn brep_incidence_index_nodes_refuse_collection_limit() {
    let error = source_index_limit_error("incidence");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep incidence index nodes"));
}

#[test]
fn brep_source_indexes_keep_last_duplicate_half_edge_and_incidence() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    let id = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    };
    for face_id in [5, 6] {
        scan.topology.half_edges.push(crate::topology::HalfEdge {
            id,
            face_id: std::num::NonZeroU32::new(face_id),
            next: None,
        });
        scan.topology.half_edge_vertex_incidence.push(
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: id,
                start_vertex_id: face_id,
                end_vertex_id: None,
            },
        );
    }
    let indexes = crate::decode::with_test_decode_ctx(|ctx| {
        BrepSourceIndexes::from_scan(ctx, &BTreeMap::new(), &scan)
    })
    .expect("service source indexes admitted");
    assert_eq!(indexes.half_edges[&id].face_id.map(std::num::NonZeroU32::get), Some(6));
    assert_eq!(indexes.incidence[&id].start_vertex_id, 6);
}

fn pcurve_candidate_limit_error(limit: u64, second_on_same_key: bool) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut candidates = NativePcurveCandidates::new();
    if second_on_same_key {
        push_native_pcurve_candidate(
            &ctx,
            &mut candidates,
            10,
            5,
            [[0.0, 0.0], [1.0, 0.0]],
            4,
        )
        .expect("first candidate admitted");
    }
    push_native_pcurve_candidate(
        &ctx,
        &mut candidates,
        10,
        5,
        [[1.0, 0.0], [2.0, 0.0]],
        8,
    )
    .expect_err("candidate allocation refused")
}

#[test]
fn brep_pcurve_candidate_nodes_refuse_collection_limit() {
    let error = pcurve_candidate_limit_error(0, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidate nodes"));
}

#[test]
fn brep_pcurve_candidates_refuse_collection_limit() {
    let error = pcurve_candidate_limit_error(1, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidates"));
}

#[test]
fn brep_pcurve_candidates_reuse_nodes_and_preserve_source_order() {
    let error = pcurve_candidate_limit_error(2, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidates"
            && resource.used == 2));
    let candidates = crate::decode::with_test_decode_ctx(|ctx| {
        let mut candidates = NativePcurveCandidates::new();
        push_native_pcurve_candidate(
            ctx, &mut candidates, 10, 5, [[0.0, 0.0], [1.0, 0.0]], 4,
        )
        .expect("first service candidate admitted");
        push_native_pcurve_candidate(
            ctx, &mut candidates, 10, 5, [[1.0, 0.0], [2.0, 0.0]], 8,
        )
        .expect("second service candidate admitted");
        candidates
    });
    assert_eq!(candidates[&(10, 5)].len(), 2);
    assert_eq!(candidates[&(10, 5)][0].1, 4);
    assert_eq!(candidates[&(10, 5)][1].1, 8);
}

fn typed_curve_id_fixture() -> CadIr {
    let mut ir = CadIr::empty();
    let circle = |id: u32| Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}"))
            .expect("fixture curve identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("fixture circle"),
        )),
        source_object: None,
    };
    ir.model.curves.extend([circle(10), circle(10), circle(20)]);
    ir
}

#[test]
fn brep_typed_curve_id_nodes_refuse_collection_limit() {
    let ir = typed_curve_id_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = model_typed_nonlinear_curve_ids(
        &ctx,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("first distinct typed curve node refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep typed curve ID nodes"));
}

#[test]
fn brep_typed_curve_ids_charge_distinct_nodes_and_preserve_order() {
    let ir = typed_curve_id_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = model_typed_nonlinear_curve_ids(
        &ctx,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("second distinct typed curve node refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep typed curve ID nodes"
            && resource.used == 1));
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        model_typed_nonlinear_curve_ids(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("service typed curve nodes admitted");
    assert_eq!(ids, BTreeSet::from([10, 20]));
}

fn native_triangle_collection_error(limit: u64, ordered: bool) -> CodecError {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let lp = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: (10..13)
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    };
    let polygon = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
    let bindings = lp
        .half_edges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, half_edge)| crate::topology::HalfEdgeVertexIncidence {
            half_edge,
            start_vertex_id: u32::try_from(index + 1).expect("three vertices"),
            end_vertex_id: Some(u32::try_from((index + 1) % 3 + 1).expect("three vertices")),
        })
        .collect::<Vec<_>>();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = polygon
        .iter()
        .enumerate()
        .map(|(index, point)| {
            (
                u32::try_from(index + 1).expect("three vertices"),
                [point[0], point[1], 0.0],
            )
        })
        .collect::<BTreeMap<_, _>>();
    let native_pcurves = lp
        .half_edges
        .iter()
        .enumerate()
        .map(|(index, half_edge)| {
            (
                (half_edge.curve_id, 5),
                vec![([polygon[index], polygon[(index + 1) % 3]], 0)],
            )
        })
        .collect::<super::NativePcurveCandidates>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let typed = BTreeSet::new();
    let result = if ordered {
        ordered_native_parameter_face_loops(
            &ctx,
            &[&lp],
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            NativeCurveEvidence {
                typed_nonlinear_curve_ids: &typed,
                model_curves: &[],
                source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
            },
        )
        .map(|_| ())
    } else {
        native_parameter_loop_polygon(
            &ctx,
            &lp,
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            &typed,
        )
        .map(|_| ())
    };
    result.expect_err("native triangle collection exceeds limit")
}

fn assert_native_collection_refusal(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn native_parameter_loop_polygon_refuses_pcurve_segments() {
    assert_native_collection_refusal(
        native_triangle_collection_error(0, false),
        "creo native loop pcurve segments",
    );
}

#[test]
fn native_parameter_loop_polygon_refuses_polygon_points() {
    assert_native_collection_refusal(
        native_triangle_collection_error(3, false),
        "creo native loop polygon points",
    );
}

#[test]
fn ordered_native_parameter_face_loops_refuses_polygon_collection() {
    assert_native_collection_refusal(
        native_triangle_collection_error(6, true),
        "creo native face loop polygons",
    );
}

#[test]
fn ordered_native_parameter_face_loops_refuses_loop_references() {
    assert_native_collection_refusal(
        native_triangle_collection_error(7, true),
        "creo native face loop references",
    );
}

fn circle_order_collection_error(limit: u64) -> CodecError {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let make_loop = |base: u32| crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: [base, base + 1]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    };
    let outer = make_loop(10);
    let inner = make_loop(20);
    let make_circle = |id: u32, radius| Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    };
    let curves = vec![
        make_circle(10, 2.0),
        make_circle(11, 2.0),
        make_circle(20, 1.0),
        make_circle(21, 1.0),
    ];
    let polygons = vec![vec![[2.0, 0.0], [-2.0, 0.0]], vec![[1.0, 0.0], [-1.0, 0.0]]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    super::ordered_two_edge_circle_loops(
        &ctx,
        &[&outer, &inner],
        &polygons,
        &surface,
        &curves,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("circle loop collection exceeds limit")
}

#[test]
fn ordered_two_edge_circle_loops_refuses_geometry_collection() {
    assert_native_collection_refusal(
        circle_order_collection_error(0),
        "creo native circle loop geometry",
    );
}

#[test]
fn ordered_two_edge_circle_loops_refuses_order_collection() {
    assert_native_collection_refusal(
        circle_order_collection_error(2),
        "creo native circle loop order",
    );
}

#[test]
fn ordered_two_edge_circle_loops_refuses_ordered_output() {
    assert_native_collection_refusal(
        circle_order_collection_error(4),
        "creo native ordered circle loops",
    );
}

fn native_parameter_loop_polygon_service(
    lp: &crate::topology::Loop,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<crate::topology::HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &super::NativePcurveCandidates,
    typed_nonlinear_curve_ids: &BTreeSet<u32>,
) -> Option<Vec<[f64; 2]>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        native_parameter_loop_polygon(
            ctx,
            lp,
            face_id,
            surface,
            incidence,
            solved_vertices,
            native_pcurves,
            typed_nonlinear_curve_ids,
        )
    })
    .expect("service native loop polygon")
}

fn ordered_native_parameter_face_loops_service<'a>(
    loops: &[&'a crate::topology::Loop],
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<crate::topology::HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &super::NativePcurveCandidates,
    curve_evidence: NativeCurveEvidence<'_>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        ordered_native_parameter_face_loops(
            ctx,
            loops,
            face_id,
            surface,
            incidence,
            solved_vertices,
            native_pcurves,
            curve_evidence,
        )
    })
    .expect("service native face loop ordering")
}

#[test]
fn infinite_point_cannot_match_a_finite_point() {
    assert!(
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::INFINITY, 0.0, 0.0)).is_none()
    );
}

#[test]
fn face_admission_diagnostics_bound_samples_and_record_counts() {
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: 6,
        admitted_face_count: 1,
        emitted_face_count: 1,
        ..BrepTransferDiagnostics::default()
    };
    crate::decode::with_test_decode_ctx(|ctx| {
        for face_id in 10..16 {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingLoops, face_id)
                .expect("service rejection admitted");
        }
    });

    let (count, samples) = diagnostics.evidence(FaceAdmissionRejection::MissingLoops);
    let samples = samples.collect::<Vec<_>>();
    assert_eq!(count, 6);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![10, 11, 12, 13]
    );
    let records = crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.face_admission_rejection_records(ctx)
    })
    .expect("service rejection records admitted");
    assert_eq!(records.len(), 6);
    assert_eq!(records[0].id, "creo:brep:face_admission_rejection#10");
    assert_eq!(records[0].face_id, 10);
    assert_eq!(records[0].reason, "missing_loops");
    assert_eq!(records[5].face_id, 15);
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    diagnostics.record_coverage(&mut coverage);
    assert_eq!(coverage["brep_candidate_face_count"], 6);
    assert_eq!(coverage["brep_admitted_face_count"], 1);
    assert_eq!(coverage["brep_emitted_face_count"], 1);
    assert_eq!(coverage["brep_rejected_face_count"], 6);
    assert_eq!(coverage["brep_rejected_face_missing_loops_count"], 6);
}

#[test]
fn brep_face_rejection_diagnostics_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let mut diagnostics = BrepTransferDiagnostics::default();
    let error = diagnostics
        .reject_face(&ctx, FaceAdmissionRejection::MissingLoops, 17)
        .expect_err("rejection diagnostic refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face rejection diagnostics"));
    assert!(diagnostics.face_rejection_diagnostics.is_empty());
}

fn rejection_detail_limit_error(limit: u64) -> CodecError {
    let half_edge = crate::topology::HalfEdgeId {
        curve_id: 4,
        side: crate::topology::Side::Zero,
    };
    let loop_record = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(17),
        half_edges: vec![half_edge],
    };
    let binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge,
        start_vertex_id: 9,
        end_vertex_id: None,
    };
    let incidence = BTreeMap::from([(half_edge, &binding)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    FaceAdmissionDetail::unresolved_boundary(
        &ctx,
        17,
        &[&loop_record],
        &BTreeMap::new(),
        &incidence,
    )
    .expect_err("rejection detail refused")
}

#[test]
fn brep_rejection_boundary_samples_refuse_collection_limit() {
    let error = rejection_detail_limit_error(0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection boundary samples"));
}

#[test]
fn brep_rejection_vertex_samples_refuse_collection_limit() {
    let error = rejection_detail_limit_error(1);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection vertex samples"));
}

fn rejection_record_limit_error(
    collection_limit: Option<u64>,
    retained_limit: Option<u64>,
) -> CodecError {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(
            ctx,
            FaceAdmissionRejection::UnresolvedBoundaryVertices,
            FaceAdmissionDetail {
                face_id: 17,
                boundary_half_edges: vec![crate::topology::HalfEdgeId {
                    curve_id: 4,
                    side: crate::topology::Side::Zero,
                }],
                vertex_ids: vec![9],
            },
        )
    })
    .expect("service rejection admitted");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    diagnostics.face_admission_rejection_records(&ctx)
        .err()
        .expect("rejection record allocation refused")
}

#[test]
fn brep_rejection_record_id_refuses_retained_limit() {
    let error = rejection_record_limit_error(None, Some(0));
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo B-rep rejection record IDs"));
}

#[test]
fn brep_rejection_half_edges_refuse_collection_limit() {
    let error = rejection_record_limit_error(Some(0), None);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection half edges"));
}

#[test]
fn brep_rejection_vertex_ids_refuse_collection_limit() {
    let error = rejection_record_limit_error(Some(1), None);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection vertex IDs"));
}

#[test]
fn brep_rejection_record_rows_refuse_collection_limit() {
    let error = rejection_record_limit_error(Some(2), None);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep rejection records"));
}

#[test]
fn brep_rejection_record_preserves_nested_operands_under_service_profile() {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(
            ctx,
            FaceAdmissionRejection::UnresolvedBoundaryVertices,
            FaceAdmissionDetail {
                face_id: 17,
                boundary_half_edges: vec![crate::topology::HalfEdgeId {
                    curve_id: 4,
                    side: crate::topology::Side::Zero,
                }],
                vertex_ids: vec![9],
            },
        )
    })
    .expect("service rejection admitted");
    let records = crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.face_admission_rejection_records(ctx)
    })
    .expect("service rejection records admitted");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "creo:brep:face_admission_rejection#17");
    assert_eq!(records[0].reason, "unresolved_boundary_vertices");
    assert_eq!(records[0].boundary_half_edges[0].curve_id, 4);
    assert_eq!(records[0].vertex_ids, [9]);
}

#[test]
fn face_admission_diagnostics_report_missing_surface_carrier() {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingSurfaceCarrier, 42)
    })
    .expect("service rejection admitted");

    let (count, samples) = diagnostics.evidence(FaceAdmissionRejection::MissingSurfaceCarrier);
    let samples = samples.collect::<Vec<_>>();
    assert_eq!(count, 1);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![42]
    );
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    diagnostics.record_coverage(&mut coverage);
    assert_eq!(coverage["brep_rejected_face_count"], 1);
    assert_eq!(
        coverage["brep_rejected_face_missing_surface_carrier_count"],
        1
    );
}

#[test]
fn brep_diagnostics_report_component_gate_inputs() {
    let diagnostics = BrepTransferDiagnostics {
        admitted_component_count: 3,
        selected_body_count: None,
        ..BrepTransferDiagnostics::default()
    };
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    diagnostics.record_coverage(&mut coverage);

    assert_eq!(coverage["brep_admitted_component_count"], 3);
    assert_eq!(coverage["brep_selected_body_count"], 0);
    assert_eq!(coverage["brep_selected_body_count_unresolved"], 1);
}

#[test]
fn explicit_single_body_merges_disconnected_components() {
    let merged = crate::decode::with_test_decode_ctx(|ctx| merge_body_components(ctx, vec![
        NeutralShellSpec {
            faces: vec![1, 2],
            wire_curves: BTreeSet::from([10]),
        },
        NeutralShellSpec {
            faces: vec![3],
            wire_curves: BTreeSet::from([11, 12]),
        },
    ])).expect("service component merge admitted");

    assert_eq!(
        merged,
        vec![NeutralShellSpec {
            faces: vec![1, 2, 3],
            wire_curves: BTreeSet::from([10, 11, 12])
        }]
    );
}

#[test]
fn face_admission_diagnostics_record_unresolved_boundary_operands() {
    let resolved = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: crate::topology::Side::Zero,
    };
    let unresolved = crate::topology::HalfEdgeId {
        curve_id: 11,
        side: crate::topology::Side::One,
    };
    let loop_record = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: vec![resolved, unresolved],
    };
    let resolved_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: resolved,
        start_vertex_id: 1,
        end_vertex_id: Some(2),
    };
    let unresolved_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: unresolved,
        start_vertex_id: 3,
        end_vertex_id: Some(4),
    };
    let incidence = BTreeMap::from([
        (resolved, &resolved_binding),
        (unresolved, &unresolved_binding),
    ]);
    let detail = crate::decode::with_test_decode_ctx(|ctx| {
        FaceAdmissionDetail::unresolved_boundary(
            ctx,
            5,
            &[&loop_record],
            &BTreeMap::from([(10, [1, 2])]),
            &incidence,
        )
    })
    .expect("service rejection detail admitted");

    assert_eq!(detail.face_id, 5);
    assert_eq!(detail.boundary_half_edges, vec![unresolved]);
    assert_eq!(detail.vertex_ids, vec![3, 4]);

    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face_with_detail(ctx, FaceAdmissionRejection::UnresolvedBoundaryVertices, detail)
    })
    .expect("service rejection admitted");
    let (count, samples) = diagnostics.evidence(FaceAdmissionRejection::UnresolvedBoundaryVertices);
    let samples = samples.collect::<Vec<_>>();
    assert_eq!(count, 1);
    assert_eq!(
        samples
            .iter()
            .map(|detail| detail.face_id)
            .collect::<Vec<_>>(),
        vec![5]
    );
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].boundary_half_edges, vec![unresolved]);
    assert_eq!(samples[0].vertex_ids, vec![3, 4]);
}

#[test]
fn legacy_brep_admission_retains_components_with_eligible_visible_faces() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.layout = crate::test_support::legacy_layout();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.topology.face_components = vec![
        crate::topology::FaceComponent {
            face_ids: vec![1],
            curve_ids: vec![10],
        },
        crate::topology::FaceComponent {
            face_ids: vec![5],
            curve_ids: vec![11],
        },
        crate::topology::FaceComponent {
            face_ids: vec![1, 5],
            curve_ids: vec![12],
        },
    ];

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            admitted_face_components(ctx, &scan, &BTreeSet::from([5]))
        })
        .expect("service component references admitted")
        .into_iter()
        .cloned()
        .collect::<Vec<_>>(),
        vec![
            crate::topology::FaceComponent {
                face_ids: vec![5],
                curve_ids: vec![11],
            },
            crate::topology::FaceComponent {
                face_ids: vec![1, 5],
                curve_ids: vec![12],
            },
        ]
    );

    let all_components = scan.topology.face_components.clone();
    scan.framing.layout = crate::container::Layout::Nd;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            admitted_face_components(ctx, &scan, &BTreeSet::new())
        })
        .expect("service component references admitted")
        .into_iter()
        .cloned()
        .collect::<Vec<_>>(),
        all_components
    );

    scan.framing.layout = crate::test_support::legacy_layout();
    assert!(!legacy_body_ownership_is_unambiguous(&scan, 2));
    assert!(legacy_body_ownership_is_unambiguous(&scan, 1));
    scan.framing.declared_body_count = Some(2);
    assert!(legacy_body_ownership_is_unambiguous(&scan, 2));
}

#[test]
fn admitted_face_component_refs_refuse_collection_limit() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.topology.face_components.push(crate::topology::FaceComponent {
        face_ids: vec![5],
        curve_ids: Vec::new(),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = admitted_face_components(&ctx, &scan, &BTreeSet::from([5]))
        .expect_err("component reference refused");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep admitted component refs"));
}

#[test]
fn legacy_brep_admission_excludes_nonvisible_face_references() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.layout = crate::test_support::legacy_layout();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.surfaces
        .nonvisible_rows
        .push(crate::surface::SurfaceRow {
            id: 7,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 0,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        });

    assert!(is_neutral_face_reference(&scan, 5));
    assert!(!is_neutral_face_reference(&scan, 7));

    scan.framing.layout = crate::container::Layout::Nd;
    assert!(is_neutral_face_reference(&scan, 7));
}

#[test]
fn partitions_face_shells_and_retains_unattached_wire_curves() {
    let faces = [1, 2, 3];
    let face_adjacency = BTreeMap::from([
        (1, BTreeSet::from([2])),
        (2, BTreeSet::from([1])),
        (3, BTreeSet::new()),
    ]);
    let face_vertices = BTreeMap::from([
        (1, BTreeSet::from([10, 11])),
        (2, BTreeSet::from([11, 12])),
        (3, BTreeSet::from([30, 31])),
    ]);
    let edge_vertices = BTreeMap::from([(100, [11, 12]), (101, [40, 41])]);

    let shells = split_neutral_component_shells(
        &faces,
        &BTreeSet::from([100, 101]),
        &face_adjacency,
        &face_vertices,
        &edge_vertices,
    );

    assert_eq!(
        shells,
        vec![
            NeutralShellSpec {
                faces: vec![1, 2],
                wire_curves: BTreeSet::from([100]),
            },
            NeutralShellSpec {
                faces: vec![3],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::from([101]),
            },
        ]
    );
}

#[test]
fn retains_wire_curve_when_shell_attachment_is_ambiguous() {
    let faces = [1, 2];
    let face_adjacency = BTreeMap::from([(1, BTreeSet::new()), (2, BTreeSet::new())]);
    let face_vertices =
        BTreeMap::from([(1, BTreeSet::from([10, 11])), (2, BTreeSet::from([20, 21]))]);
    let edge_vertices = BTreeMap::from([(100, [10, 20])]);

    assert_eq!(
        split_neutral_component_shells(
            &faces,
            &BTreeSet::from([100]),
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        ),
        vec![
            NeutralShellSpec {
                faces: vec![1],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: vec![2],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::from([100]),
            },
        ]
    );
}

#[test]
fn closed_component_counts_two_uses_of_one_face() {
    let edges = BTreeMap::from([
        (
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                },
                face_id: std::num::NonZeroU32::new(5),
                next: None,
            },
        ),
        (
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::One,
                },
                face_id: std::num::NonZeroU32::new(5),
                next: None,
            },
        ),
    ]);
    let half_edges = edges
        .iter()
        .map(|(id, edge)| (*id, edge))
        .collect::<BTreeMap<_, _>>();

    assert!(component_is_closed(
        &BTreeSet::from([7]),
        &BTreeSet::from([
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
        ]),
        &half_edges,
        &[5],
    ));
    assert!(!component_is_closed(
        &BTreeSet::from([7]),
        &BTreeSet::from([crate::topology::HalfEdgeId {
            curve_id: 7,
            side: crate::topology::Side::Zero,
        }]),
        &half_edges,
        &[5],
    ));
}

#[test]
fn native_parameter_loops_order_non_planar_cylindrical_face() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    let make_loop = |first_curve| crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: (0_u32..4)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: first_curve + index,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    };
    let outer = make_loop(10);
    let inner = make_loop(20);
    let outer_polygon = [[0.0, 0.0], [1.0, 0.0], [1.0, 4.0], [0.0, 4.0]];
    let inner_polygon = [[0.25, 1.0], [0.75, 1.0], [0.75, 3.0], [0.25, 3.0]];
    let mut bindings = Vec::new();
    let mut solved_vertices = BTreeMap::new();
    let mut native_pcurves = BTreeMap::<(u32, u32), Vec<([[f64; 2]; 2], usize)>>::new();
    for (base_vertex, (lp, polygon)) in [
        (1_u32, (&outer, outer_polygon)),
        (5_u32, (&inner, inner_polygon)),
    ] {
        for index in 0..4 {
            let half_edge = lp.half_edges[index];
            let offset = u32::try_from(index).expect("four boundary edges");
            let next_offset = u32::try_from((index + 1) % 4).expect("four boundary edges");
            let start_vertex_id = base_vertex + offset;
            let end_vertex_id = base_vertex + next_offset;
            let start_uv = polygon[index];
            let end_uv = polygon[(index + 1) % 4];
            let point = cadmpeg_ir::eval::surface_point(&surface, start_uv[0], start_uv[1])
                .expect("analytic cylinder endpoint");
            solved_vertices.insert(start_vertex_id, [point.x, point.y, point.z]);
            bindings.push(crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id,
                end_vertex_id: Some(end_vertex_id),
            });
            native_pcurves
                .entry((half_edge.curve_id, 5))
                .or_default()
                .push(([start_uv, end_uv], 0));
        }
    }
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        native_parameter_loop_polygon_service(
            &outer,
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            &BTreeSet::new(),
        ),
        Some(outer_polygon.into_iter().collect())
    );
    let ordered = ordered_native_parameter_face_loops_service(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &BTreeSet::new(),
            model_curves: &[],
            source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
        },
    )
    .expect("one parameter-space outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges[0].curve_id, 20);
}

#[test]
fn native_parameter_loops_admit_proven_two_edge_circles() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let outer = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: [10_u32, 11]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    };
    let inner = crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: [20_u32, 21]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    };
    let bindings = [(10, 1, 2), (11, 2, 1), (20, 3, 4), (21, 4, 3)]
        .into_iter()
        .map(|(curve_id, start_vertex_id, end_vertex_id)| {
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: crate::topology::HalfEdgeId {
                    curve_id,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id,
                end_vertex_id: Some(end_vertex_id),
            }
        })
        .collect::<Vec<_>>();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = BTreeMap::from([
        (1, [2.0, 0.0, 0.0]),
        (2, [-2.0, 0.0, 0.0]),
        (3, [1.0, 0.0, 0.0]),
        (4, [-1.0, 0.0, 0.0]),
    ]);
    let native_pcurves = BTreeMap::from([
        ((10, 5), vec![([[2.0, 0.0], [-2.0, 0.0]], 0)]),
        ((11, 5), vec![([[-2.0, 0.0], [2.0, 0.0]], 0)]),
        ((20, 5), vec![([[1.0, 0.0], [-1.0, 0.0]], 0)]),
        ((21, 5), vec![([[-1.0, 0.0], [1.0, 0.0]], 0)]),
    ]);
    let circle = |id, radius| Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .expect("valid CircleCurve fixture"),
        )),
        source_object: None,
    };
    let model_curves = vec![
        circle(10, 2.0),
        circle(11, 2.0),
        circle(20, 1.0),
        circle(21, 1.0),
    ];
    let typed_nonlinear_curve_ids = BTreeSet::from([10, 11, 20, 21]);

    assert_eq!(
        native_parameter_loop_polygon_service(
            &outer,
            5,
            &surface,
            &incidence,
            &solved_vertices,
            &native_pcurves,
            &typed_nonlinear_curve_ids,
        ),
        Some(vec![[2.0, 0.0], [-2.0, 0.0]])
    );
    assert!(native_parameter_loop_polygon_service(
        &outer,
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        &BTreeSet::new(),
    )
    .is_none());

    let ordered = ordered_native_parameter_face_loops_service(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
            model_curves: &model_curves,
            source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
        },
    )
    .expect("concentric two-edge circles have a proven outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges[0].curve_id, 20);
}

#[test]
fn native_brep_rejects_ambiguous_model_carriers() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.declared_body_count = Some(1);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 0,
        });
    let points = [
        [[0.0, 0.0], [1.0, 0.0]],
        [[1.0, 0.0], [1.0, 1.0]],
        [[1.0, 1.0], [0.0, 0.0]],
    ];
    scan.curves.topology_rows = [10_u32, 11, 12]
        .into_iter()
        .map(|id| crate::curve::CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(5), None],
            next_edges: [id, 0],
            offset: 0,
        })
        .collect();
    scan.curves.pcurves = [10_u32, 11, 12]
        .into_iter()
        .zip(points)
        .map(|(curve_id, endpoints)| crate::curve::PcurveEndpoints {
            curve_id,
            faces: [5, 0].map(std::num::NonZeroU32::new),
            face_0_endpoints: endpoints,
            face_1_endpoints: [[0.0, 0.0], [0.0, 0.0]],
            offset: 0,
        })
        .collect();
    scan.topology.half_edges = [10_u32, 11, 12]
        .into_iter()
        .map(|curve_id| crate::topology::HalfEdge {
            id: crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(5),
            next: None,
        })
        .chain(
            [10_u32, 11, 12]
                .into_iter()
                .map(|curve_id| crate::topology::HalfEdge {
                    id: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::One,
                    },
                    face_id: None,
                    next: None,
                }),
        )
        .collect();
    scan.topology.loops.push(crate::topology::Loop {
        face_id: std::num::NonZeroU32::new(5),
        half_edges: [10_u32, 11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    });
    scan.topology
        .face_components
        .push(crate::topology::FaceComponent {
            face_ids: vec![5],
            curve_ids: vec![10, 11, 12],
        });
    scan.topology.vertices = [1_u32, 2, 3]
        .into_iter()
        .zip([10_u32, 11, 12])
        .map(|(id, curve_id)| crate::topology::TopologicalVertex {
            id,
            half_edges: vec![crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            }],
        })
        .collect();
    let endpoint_pairs = [(10, 1, 2), (11, 2, 3), (12, 3, 1)];
    scan.topology.half_edge_vertex_incidence = endpoint_pairs
        .into_iter()
        .flat_map(|(curve_id, start, end)| {
            [
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::Zero,
                    },
                    start_vertex_id: start,
                    end_vertex_id: Some(end),
                },
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::One,
                    },
                    start_vertex_id: end,
                    end_vertex_id: Some(start),
                },
            ]
        })
        .collect();

    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#5".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    for (id, origin, direction) in [
        (10, Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (11, Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (
            12,
            Point3::new(1.0, 1.0, 0.0),
            Vector3::new(-1.0, -1.0, 0.0),
        ),
    ] {
        let curve = Curve {
            id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    origin,
                    direction.unit().expect("valid LineCurve fixture"),
                )
                .expect("valid LineCurve fixture"),
            )),
            source_object: None,
        };
        ir.model.curves.extend([curve.clone(), curve]);
    }

    let counts = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_native_brep(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            NativeBrepCurveEvidence {
                derived_intersections: &BTreeSet::new(),
                nurbs_endpoints: &BTreeSet::new(),
            },
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("valid source object identity");

    assert_eq!(counts.topological_point_count, 3);
    assert_eq!(counts.native_topological_edge_count, 0);
    assert_eq!(
        ir.model
            .points
            .iter()
            .map(|point| point.id.to_string())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "creo:visibgeom:point#1".to_string(),
            "creo:visibgeom:point#2".to_string(),
            "creo:visibgeom:point#3".to_string(),
        ])
    );
    assert_eq!(
        ir.model
            .points
            .iter()
            .map(|point| {
                let source = point.source_object.as_ref().expect("point provenance");
                (source.format.as_str(), source.object_id.as_str())
            })
            .collect::<Vec<_>>(),
        vec![
            ("creo", "topology:vertex#1"),
            ("creo", "topology:vertex#2"),
            ("creo", "topology:vertex#3"),
        ]
    );
    assert!(ir.model.vertices.is_empty());
    assert!(ir.model.edges.is_empty());
    assert!(ir.model.faces.is_empty());
    assert!(ir.model.loops.is_empty());
    assert!(ir.model.coedges.is_empty());
    assert!(ir.model.bodies.is_empty());
    assert!(ir.model.regions.is_empty());
    assert!(ir.model.shells.is_empty());

    ir.model.curves.clear();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 6,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    for pcurve in &mut scan.curves.pcurves {
        pcurve.faces = [5, 6].map(std::num::NonZeroU32::new);
        pcurve.face_1_endpoints = pcurve.face_0_endpoints;
    }
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#5".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#6".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });

    let counts = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_native_brep(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            NativeBrepCurveEvidence {
                derived_intersections: &BTreeSet::new(),
                nurbs_endpoints: &BTreeSet::new(),
            },
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("valid source object identity");

    assert_eq!(counts.topological_point_count, 3);
    assert_eq!(counts.native_topological_edge_count, 3);
    assert!(ir.model.faces.is_empty());
    assert!(ir.model.loops.is_empty());
    assert!(ir.model.coedges.is_empty());
    assert_eq!(ir.model.edges.len(), 3);
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.shells[0].wire_edges().len(), 3);
}
