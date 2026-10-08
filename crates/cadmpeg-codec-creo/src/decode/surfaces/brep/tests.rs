// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;

use super::{
    admitted_face_components, component_is_closed, is_neutral_face_reference,
    legacy_body_ownership_is_unambiguous, merge_body_components, model_typed_nonlinear_curve_ids,
    native_parameter_loop_polygon, ordered_native_parameter_face_loops,
    push_native_pcurve_candidate, split_neutral_component_shells, transfer_native_brep,
    BrepEdgeIndexes, BrepFaceCandidateIndexes, BrepSourceIndexes, BrepTransferDiagnostics,
    FaceAdmissionDetail, FaceAdmissionRejection, NativeBrepCurveEvidence, NativeCurveEvidence,
    NativePcurveCandidates, NeutralShellSpec,
};

mod admission_details;
mod body_index;
mod component_topology;
mod contains_set;
mod eligible_index;
mod empty_traversal;
mod face_references;
mod loop_ring;
mod ordering_storage;
mod pcurve_emission;
mod shell_references;
mod split_shells;

#[test]
fn closed_component_refuses_before_curve_traversal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        Some("creo B-rep closed component curve traversal"),
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            component_is_closed(
                &ctx,
                &BTreeSet::from([7]),
                &BTreeSet::new(),
                &BTreeMap::new(),
                &[5],
            )
            .map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = component_is_closed(
        &ctx,
        &BTreeSet::from([7]),
        &BTreeSet::new(),
        &BTreeMap::new(),
        &[5],
    )
    .expect_err("curve traversal needs work");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo B-rep closed component curve traversal"));
}

#[test]
fn rejection_evidence_refuses_before_diagnostic_count() {
    let mut diagnostics = BrepTransferDiagnostics::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingLoops, 42)
    })
    .expect("service rejection admitted");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo B-rep rejection evidence count"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            diagnostics
                .evidence(&ctx, FaceAdmissionRejection::MissingLoops)
                .map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(
        diagnostics.evidence(&ctx, FaceAdmissionRejection::MissingLoops),
        Err(CodecError::ResourceLimit(resource))
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo B-rep rejection evidence count"
    ));
}

#[test]
fn brep_coverage_refuses_before_first_report_node() {
    let diagnostics = BrepTransferDiagnostics::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("decode coverage nodes"),
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            diagnostics.record_coverage(&ctx, &mut cadmpeg_ir::report::decode::Coverage::default())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = diagnostics
        .record_coverage(&ctx, &mut cadmpeg_ir::report::decode::Coverage::default())
        .expect_err("first B-rep coverage node exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "decode coverage nodes"));
}

#[test]
fn infinite_point_cannot_match_a_finite_point() {
    assert!(
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(f64::INFINITY, 0.0, 0.0)).is_none()
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
    crate::decode::with_test_decode_ctx(|ctx| diagnostics.record_coverage(ctx, &mut coverage))
        .expect("service coverage admitted");

    assert_eq!(coverage["brep_admitted_component_count"], 3);
    assert_eq!(coverage["brep_selected_body_count"], 0);
    assert_eq!(coverage["brep_selected_body_count_unresolved"], 1);
}

#[test]
fn explicit_single_body_merges_disconnected_components() {
    let merged = crate::decode::with_test_decode_ctx(|ctx| {
        merge_body_components(
            ctx,
            vec![
                NeutralShellSpec {
                    faces: vec![1, 2],
                    wire_curves: BTreeSet::from([10]),
                },
                NeutralShellSpec {
                    faces: vec![3],
                    wire_curves: BTreeSet::from([11, 12]),
                },
            ],
        )
    })
    .expect("service component merge admitted");

    assert_eq!(
        merged,
        vec![NeutralShellSpec {
            faces: vec![1, 2, 3],
            wire_curves: BTreeSet::from([10, 11, 12])
        }]
    );
}

#[test]
fn legacy_brep_admission_retains_components_with_eligible_visible_faces() {
    let mut scan = crate::test_support::empty_container_scan();
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
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![1], vec![10])
        })
        .expect("component admission")
        .expect("valid component fixture"),
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![5], vec![11])
        })
        .expect("component admission")
        .expect("valid component fixture"),
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![1, 5], vec![12])
        })
        .expect("component admission")
        .expect("valid component fixture"),
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
            crate::decode::with_test_decode_ctx(
                |ctx| crate::topology::FaceComponent::new_for_test(ctx, vec![5], vec![11])
            )
            .expect("component admission")
            .expect("valid component fixture"),
            crate::decode::with_test_decode_ctx(
                |ctx| crate::topology::FaceComponent::new_for_test(ctx, vec![1, 5], vec![12])
            )
            .expect("component admission")
            .expect("valid component fixture"),
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
    let mut scan = crate::test_support::empty_container_scan();
    scan.topology.face_components.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![5], Vec::new())
        })
        .expect("component admission")
        .expect("valid component fixture"),
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo B-rep admitted component refs",
        |ctx| admitted_face_components(ctx, &scan, &BTreeSet::from([5])),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo B-rep admitted component refs"));
}

#[test]
fn legacy_brep_admission_excludes_nonvisible_face_references() {
    let mut scan = crate::test_support::empty_container_scan();
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

    let shells = crate::decode::with_test_decode_ctx(|ctx| {
        split_neutral_component_shells(
            ctx,
            &faces,
            &BTreeSet::from([100, 101]),
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        )
    })
    .expect("service shell partition admitted");

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
        crate::decode::with_test_decode_ctx(|ctx| {
            split_neutral_component_shells(
                ctx,
                &faces,
                &BTreeSet::from([100]),
                &face_adjacency,
                &face_vertices,
                &edge_vertices,
            )
        })
        .expect("service ambiguous wire admitted"),
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

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| component_is_closed(
            ctx,
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
        ))
        .expect("closed component search")
    );
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| component_is_closed(
            ctx,
            &BTreeSet::from([7]),
            &BTreeSet::from([crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            }]),
            &half_edges,
            &[5],
        ))
        .expect("closed component search")
    );
}

#[test]
fn native_brep_rejects_ambiguous_model_carriers() {
    let mut scan = crate::test_support::empty_container_scan();
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
    scan.topology.loops.push(crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        [10_u32, 11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    ));
    scan.topology.face_components.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![5], vec![10, 11, 12])
        })
        .expect("component admission")
        .expect("valid component fixture"),
    );
    scan.topology.vertices = [1_u32, 2, 3]
        .into_iter()
        .zip([10_u32, 11, 12])
        .map(|(id, curve_id)| {
            crate::decode::with_test_decode_ctx(|ctx| {
                crate::topology::TopologicalVertex::new_for_test(
                    ctx,
                    id,
                    vec![crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::Zero,
                    }],
                )
            })
            .expect("vertex admission")
            .expect("valid vertex fixture")
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
                    start_vertex_id: std::num::NonZeroU32::new(start)
                        .expect("one-based vertex fixture"),
                    end_vertex_id: std::num::NonZeroU32::new(end),
                },
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::One,
                    },
                    start_vertex_id: std::num::NonZeroU32::new(end)
                        .expect("one-based vertex fixture"),
                    end_vertex_id: std::num::NonZeroU32::new(start),
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
        .map(|(summary, _storage)| summary)
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
        .map(|(summary, _storage)| summary)
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

mod diagnostics;
mod parameter_loops;
mod source_indexes;
