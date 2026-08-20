// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;

use super::{
    admitted_face_components, component_is_closed, legacy_body_ownership_is_unambiguous,
    native_parameter_loop_polygon, ordered_native_parameter_face_loops,
    split_neutral_component_shells, transfer_native_brep, BrepTransferDiagnostics,
    FaceAdmissionDetail, FaceAdmissionRejection, NativeCurveEvidence, NeutralShellSpec,
};

#[test]
fn face_admission_diagnostics_bound_samples_and_record_counts() {
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: 6,
        admitted_face_count: 1,
        emitted_face_count: 1,
        ..BrepTransferDiagnostics::default()
    };
    for face_id in 10..16 {
        diagnostics.reject_face(FaceAdmissionRejection::MissingLoops, face_id);
    }

    let evidence = &diagnostics.rejected_faces[&FaceAdmissionRejection::MissingLoops];
    assert_eq!(evidence.count, 6);
    assert_eq!(evidence.sample_ids, vec![10, 11, 12, 13]);
    let mut coverage = BTreeMap::new();
    diagnostics.record_coverage(&mut coverage);
    assert_eq!(coverage["brep_candidate_face_count"], 6);
    assert_eq!(coverage["brep_admitted_face_count"], 1);
    assert_eq!(coverage["brep_emitted_face_count"], 1);
    assert_eq!(coverage["brep_rejected_face_count"], 6);
    assert_eq!(coverage["brep_rejected_face_missing_loops_count"], 6);
}

#[test]
fn face_admission_diagnostics_record_unresolved_boundary_operands() {
    let resolved = crate::topology::HalfEdgeId {
        curve_id: 10,
        side: 0,
    };
    let unresolved = crate::topology::HalfEdgeId {
        curve_id: 11,
        side: 1,
    };
    let loop_record = crate::topology::Loop {
        face_id: 5,
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
    let detail = FaceAdmissionDetail::unresolved_boundary(
        5,
        &[&loop_record],
        &BTreeMap::from([(10, [1, 2])]),
        &incidence,
    );

    assert_eq!(detail.face_id, 5);
    assert_eq!(detail.boundary_half_edges, vec![unresolved]);
    assert_eq!(detail.vertex_ids, vec![3, 4]);

    let mut diagnostics = BrepTransferDiagnostics::default();
    diagnostics.reject_face_with_detail(FaceAdmissionRejection::UnresolvedBoundaryVertices, detail);
    let evidence = &diagnostics.rejected_faces[&FaceAdmissionRejection::UnresolvedBoundaryVertices];
    assert_eq!(evidence.count, 1);
    assert_eq!(evidence.sample_ids, vec![5]);
    assert_eq!(evidence.sample_details.len(), 1);
    assert_eq!(
        evidence.sample_details[0].boundary_half_edges,
        vec![unresolved]
    );
    assert_eq!(evidence.sample_details[0].vertex_ids, vec![3, 4]);
}

#[test]
fn legacy_brep_admission_retains_components_with_eligible_visible_faces() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.framing.layout = crate::container::Layout::LegacyAscii;
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: 0,
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
        admitted_face_components(&scan, &BTreeSet::from([5])),
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
        admitted_face_components(&scan, &BTreeSet::new()),
        all_components
    );

    scan.framing.layout = crate::container::Layout::LegacyAscii;
    assert!(!legacy_body_ownership_is_unambiguous(&scan, 2));
    assert!(legacy_body_ownership_is_unambiguous(&scan, 1));
    scan.framing.declared_body_count = Some(2);
    assert!(legacy_body_ownership_is_unambiguous(&scan, 2));
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
                side: 0,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: 0,
                },
                face_id: 5,
                next: None,
            },
        ),
        (
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
            crate::topology::HalfEdge {
                id: crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: 1,
                },
                face_id: 5,
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
                side: 0,
            },
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 1,
            },
        ]),
        &half_edges,
        &[5],
    ));
    assert!(!component_is_closed(
        &BTreeSet::from([7]),
        &BTreeSet::from([crate::topology::HalfEdgeId {
            curve_id: 7,
            side: 0,
        }]),
        &half_edges,
        &[5],
    ));
}

#[test]
fn native_parameter_loops_order_non_planar_cylindrical_face() {
    let surface = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let make_loop = |first_curve| crate::topology::Loop {
        face_id: 5,
        half_edges: (0_u32..4)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: first_curve + index,
                side: 0,
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
        native_parameter_loop_polygon(
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
    let ordered = ordered_native_parameter_face_loops(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &BTreeSet::new(),
            model_curves: &[],
        },
    )
    .expect("one parameter-space outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges[0].curve_id, 20);
}

#[test]
fn native_parameter_loops_admit_proven_two_edge_circles() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let outer = crate::topology::Loop {
        face_id: 5,
        half_edges: [10_u32, 11]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId { curve_id, side: 0 })
            .collect(),
    };
    let inner = crate::topology::Loop {
        face_id: 5,
        half_edges: [20_u32, 21]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId { curve_id, side: 0 })
            .collect(),
    };
    let bindings = [(10, 1, 2), (11, 2, 1), (20, 3, 4), (21, 4, 3)]
        .into_iter()
        .map(|(curve_id, start_vertex_id, end_vertex_id)| {
            crate::topology::HalfEdgeVertexIncidence {
                half_edge: crate::topology::HalfEdgeId { curve_id, side: 0 },
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
        id: CurveId(format!("creo:visibgeom:curve#{id}")),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
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
        native_parameter_loop_polygon(
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
    assert!(native_parameter_loop_polygon(
        &outer,
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        &BTreeSet::new(),
    )
    .is_none());

    let ordered = ordered_native_parameter_face_loops(
        &[&inner, &outer],
        5,
        &surface,
        &incidence,
        &solved_vertices,
        &native_pcurves,
        NativeCurveEvidence {
            typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
            model_curves: &model_curves,
        },
    )
    .expect("concentric two-edge circles have a proven outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges[0].curve_id, 20);
}

#[test]
fn native_brep_rejects_ambiguous_model_carriers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.framing.declared_body_count = Some(1);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
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
            faces: [5, 0],
            next_edges: [id, 0],
            offset: 0,
        })
        .collect();
    scan.curves.pcurves = [10_u32, 11, 12]
        .into_iter()
        .zip(points)
        .map(|(curve_id, endpoints)| crate::curve::PcurveEndpoints {
            curve_id,
            faces: [5, 0],
            face_0_endpoints: endpoints,
            face_1_endpoints: [[0.0, 0.0], [0.0, 0.0]],
            offset: 0,
        })
        .collect();
    scan.topology.half_edges = [10_u32, 11, 12]
        .into_iter()
        .map(|curve_id| crate::topology::HalfEdge {
            id: crate::topology::HalfEdgeId { curve_id, side: 0 },
            face_id: 5,
            next: None,
        })
        .chain(
            [10_u32, 11, 12]
                .into_iter()
                .map(|curve_id| crate::topology::HalfEdge {
                    id: crate::topology::HalfEdgeId { curve_id, side: 1 },
                    face_id: 0,
                    next: None,
                }),
        )
        .collect();
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: [10_u32, 11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId { curve_id, side: 0 })
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
            half_edges: vec![crate::topology::HalfEdgeId { curve_id, side: 0 }],
        })
        .collect();
    let endpoint_pairs = [(10, 1, 2), (11, 2, 3), (12, 3, 1)];
    scan.topology.half_edge_vertex_incidence = endpoint_pairs
        .into_iter()
        .flat_map(|(curve_id, start, end)| {
            [
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId { curve_id, side: 0 },
                    start_vertex_id: start,
                    end_vertex_id: Some(end),
                },
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId { curve_id, side: 1 },
                    start_vertex_id: end,
                    end_vertex_id: Some(start),
                },
            ]
        })
        .collect();

    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#5".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
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
            id: CurveId(format!("creo:visibgeom:curve#{id}")),
            geometry: CurveGeometry::Line { origin, direction },
            source_object: None,
        };
        ir.model.curves.extend([curve.clone(), curve]);
    }

    let counts = transfer_native_brep(
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    );

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
                (source.format.clone(), source.object_id.clone())
            })
            .collect::<Vec<_>>(),
        vec![
            ("creo".to_string(), "topology:vertex#1".to_string()),
            ("creo".to_string(), "topology:vertex#2".to_string()),
            ("creo".to_string(), "topology:vertex#3".to_string()),
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
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });
    for pcurve in &mut scan.curves.pcurves {
        pcurve.faces = [5, 6];
        pcurve.face_1_endpoints = pcurve.face_0_endpoints;
    }
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#5".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId("creo:visibgeom:surface#6".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });

    let counts = transfer_native_brep(
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    );

    assert_eq!(counts.topological_point_count, 3);
    assert_eq!(counts.native_topological_edge_count, 3);
    assert!(ir.model.faces.is_empty());
    assert!(ir.model.loops.is_empty());
    assert!(ir.model.coedges.is_empty());
    assert_eq!(ir.model.edges.len(), 3);
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.shells[0].wire_edges.len(), 3);
}
