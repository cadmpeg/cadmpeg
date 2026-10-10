// SPDX-License-Identifier: Apache-2.0

use super::*;

struct BrepEdgeIndexInput {
    rows: Vec<crate::curve::CurveTopologyRow>,
    native_vertices: BTreeMap<u32, [std::num::NonZeroU32; 2]>,
    solved_vertices: BTreeMap<u32, [f64; 3]>,
    ir: CadIr,
}

fn brep_edge_index_input() -> BrepEdgeIndexInput {
    let rows = vec![crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 1,
        directions: [0, 0],
        faces: [None, None],
        next_edges: [0, 0],
        offset: 0,
    }];
    let native_vertices = BTreeMap::from([(
        10,
        [1, 2].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")),
    )]);
    let solved_vertices = BTreeMap::from([(1, [0.0, 0.0, 0.0]), (2, [1.0, 0.0, 0.0])]);
    BrepEdgeIndexInput {
        rows,
        native_vertices,
        solved_vertices,
        ir: CadIr::empty(),
    }
}

fn brep_edge_index_limit_error(operation: &'static str) -> CodecError {
    let BrepEdgeIndexInput {
        rows,
        native_vertices,
        solved_vertices,
        ir,
    } = brep_edge_index_input();
    crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| BrepEdgeIndexes::from_rows(ctx, &rows, &native_vertices, &solved_vertices, &ir),
    )
}

#[test]
fn brep_edge_vertex_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error("creo B-rep edge-vertex nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep edge-vertex nodes"));
}

#[test]
fn brep_model_curve_count_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error("creo B-rep model curve count nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep model curve count nodes"));
}

#[test]
fn brep_admitted_edge_id_nodes_refuse_collection_limit() {
    let error = brep_edge_index_limit_error("creo B-rep admitted edge ID nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep admitted edge ID nodes"));
}

#[test]
fn brep_edge_indexes_preserve_model_curve_multiplicity() {
    let BrepEdgeIndexInput {
        rows,
        native_vertices,
        solved_vertices,
        mut ir,
    } = brep_edge_index_input();
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
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    scan.topology.loops.push(crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }],
    ));
    scan
}

fn face_candidate_index_limit_error(operation: &'static str) -> CodecError {
    let scan = face_candidate_scan();
    let ir = CadIr::empty();
    crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| BrepFaceCandidateIndexes::from_scan(ctx, &scan, &ir),
    )
}

#[test]
fn brep_face_loop_index_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error("creo B-rep face-loop index nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face-loop index nodes"));
}

#[test]
fn brep_face_loop_references_refuse_collection_limit() {
    let error = face_candidate_index_limit_error("creo B-rep face-loop references");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep face-loop references"));
}

#[test]
fn brep_topology_face_id_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error("creo B-rep topology face ID nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep topology face ID nodes"));
}

#[test]
fn brep_candidate_face_id_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error("creo B-rep candidate face ID nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep candidate face ID nodes"));
}

#[test]
fn brep_model_surface_count_nodes_refuse_collection_limit() {
    let error = face_candidate_index_limit_error("creo B-rep model surface count nodes");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep model surface count nodes"));
}

#[test]
fn brep_boundary_curve_id_nodes_refuse_collection_limit() {
    let scan = face_candidate_scan();
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo B-rep boundary curve ID nodes",
        |ctx| BrepFaceCandidateIndexes::from_scan(ctx, &scan, &CadIr::empty()),
    );
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
    assert_eq!(indexes.boundary_curve_ids, BTreeSet::from([10]));
    assert_eq!(indexes.legacy_nonvisible_face_reference_count, 0);
}

fn source_index_limit_error(kind: &str) -> CodecError {
    let mut scan = crate::test_support::empty_container_scan();
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
                start_vertex_id: std::num::NonZeroU32::new(1).expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(2),
            },
        ),
        _ => panic!("unsupported source-index fixture"),
    }
    let operation = match kind {
        "plane" => "creo B-rep plane index nodes",
        "half_edge" => "creo B-rep half-edge index nodes",
        "incidence" => "creo B-rep incidence index nodes",
        _ => panic!("unsupported source-index fixture"),
    };
    crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| BrepSourceIndexes::from_scan(ctx, &carriers, &scan),
    )
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
    let mut scan = crate::test_support::empty_container_scan();
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
        scan.topology
            .half_edge_vertex_incidence
            .push(crate::topology::HalfEdgeVertexIncidence {
                half_edge: id,
                start_vertex_id: std::num::NonZeroU32::new(face_id)
                    .expect("one-based vertex fixture"),
                end_vertex_id: None,
            });
    }
    let indexes = crate::decode::with_test_decode_ctx(|ctx| {
        BrepSourceIndexes::from_scan(ctx, &BTreeMap::new(), &scan)
    })
    .expect("service source indexes admitted");
    assert_eq!(
        indexes.half_edges[&id]
            .face_id
            .map(std::num::NonZeroU32::get),
        Some(6)
    );
    assert_eq!(indexes.incidence[&id].start_vertex_id.get(), 6);
}

fn pcurve_candidate_limit_error(operation: &'static str, second_on_same_key: bool) -> CodecError {
    crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            let mut candidates = NativePcurveCandidates::new();
            if second_on_same_key {
                push_native_pcurve_candidate(
                    ctx,
                    &mut candidates,
                    10,
                    5,
                    [[0.0, 0.0], [1.0, 0.0]],
                    4,
                )?;
            }
            push_native_pcurve_candidate(ctx, &mut candidates, 10, 5, [[1.0, 0.0], [2.0, 0.0]], 8)
        },
    )
}

#[test]
fn brep_pcurve_candidate_nodes_refuse_collection_limit() {
    let error = pcurve_candidate_limit_error("creo B-rep pcurve candidate nodes", false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidate nodes"));
}

#[test]
fn brep_pcurve_candidates_refuse_collection_limit() {
    let error = pcurve_candidate_limit_error("creo B-rep pcurve candidates", false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidates"));
}

#[test]
fn brep_pcurve_candidates_reuse_nodes_and_preserve_source_order() {
    let error = pcurve_candidate_limit_error("creo B-rep pcurve candidates", true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep pcurve candidates"
            && resource.used == 2));
    let candidates = crate::decode::with_test_decode_ctx(|ctx| {
        let mut candidates = NativePcurveCandidates::new();
        push_native_pcurve_candidate(ctx, &mut candidates, 10, 5, [[0.0, 0.0], [1.0, 0.0]], 4)
            .expect("first service candidate admitted");
        push_native_pcurve_candidate(ctx, &mut candidates, 10, 5, [[1.0, 0.0], [2.0, 0.0]], 8)
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
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("fixture curve identity"),
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
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo B-rep typed curve ID nodes",
        |ctx| {
            model_typed_nonlinear_curve_ids(
                ctx,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep typed curve ID nodes"));
}

#[test]
fn brep_typed_curve_ids_charge_distinct_nodes_and_preserve_order() {
    let ir = typed_curve_id_fixture();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo B-rep typed curve ID nodes",
        |ctx| {
            model_typed_nonlinear_curve_ids(
                ctx,
                &ir,
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
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

#[test]
fn brep_fixed_curve_namespace_charges_only_the_record_visit() {
    let mut ir = typed_curve_id_fixture();
    ir.model.curves.truncate(1);
    ir.model.curves[0].id = CurveId::mint("creo:other:curve#7").expect("fixture identity");
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        model_typed_nonlinear_curve_ids(
            &ctx, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let below = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits, Some("creo model typed nonlinear curve ids curves traversal"), run,
    );
    for cap in [below, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = model_typed_nonlinear_curve_ids(
            &ctx, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(),
        );
        if cap == 1 {
            assert!(result.expect("one existing record visit").is_empty());
        } else {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("record visit must refuse");
            };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "creo model typed nonlinear curve ids curves traversal");
            assert_eq!(refusal.used, 0);
            assert_eq!(refusal.additional, 1);
            assert!(matches!(model_typed_nonlinear_curve_ids(
                &ctx, &CadIr::empty(), &crate::decode::source_carriers::SourceUnitCarriers::default(),
            ), Err(CodecError::ResourceLimit(original)) if original == refusal));
        }
    }
}

#[test]
fn brep_fixed_curve_namespace_keeps_numeric_suffix_admission() {
    let mut ir = typed_curve_id_fixture();
    ir.model.curves.truncate(1);
    ir.model.curves[0].id = CurveId::mint("creo:visibgeom:curve#x").expect("fixture identity");
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        model_typed_nonlinear_curve_ids(
            &ctx, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let below = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits, Some("creo nonlinear curve number"), run,
    );
    for cap in [below, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = model_typed_nonlinear_curve_ids(
            &ctx, &ir, &crate::decode::source_carriers::SourceUnitCarriers::default(),
        );
        if cap == 2 {
            assert!(result.expect("record visit plus one suffix byte").is_empty());
        } else {
            assert!(matches!(result, Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "creo nonlinear curve number"
                    && refusal.used == 1 && refusal.additional == 1));
        }
    }
}
