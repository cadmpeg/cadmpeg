use crate::decode::analytic::{
    agreed_plane, agreed_plane_surface, agreed_topology_bound_plane, analytic_boundary_line,
    analytic_curve_plane, dot, envelope_reconciled_plane_candidate,
    frame_bound_outline_plane_candidate, held_coordinate_plane, plane_candidates,
    stored_parameter_normal_candidates, topology_bound_line_plane, topology_bound_plane,
    transfer_topology_bound_planes, BoundaryLine, PlaneCandidate, PlaneChart, PlaneEquation,
};
use crate::surface::{
    LocalSystemClassification, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn topology_boundary_points_define_one_plane() {
    let plane = topology_bound_plane([
        [4.0, 0.0, 2.0],
        [1.0, 3.0, 2.0],
        [1.0, 0.0, 2.0],
        [4.0, 3.0, 2.0],
        [1.0, 0.0, 2.0],
    ])
    .expect("non-collinear coplanar points");
    assert_eq!(plane.origin, [1.0, 0.0, 2.0]);
    assert_eq!(plane.normal, [0.0, 0.0, 1.0]);

    assert!(topology_bound_plane([[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]).is_none());
    assert!(topology_bound_plane([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ])
    .is_none());
}

#[test]
fn analytic_conic_boundary_defines_its_plane() {
    let plane = analytic_curve_plane(&CurveGeometry::Circle {
        center: Point3::new(3.0, 4.0, 5.0),
        axis: Vector3::new(0.0, 0.0, -2.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 7.0,
    })
    .expect("circle plane");
    assert_eq!(plane.origin, [3.0, 4.0, 5.0]);
    assert_eq!(plane.normal, [0.0, 0.0, -1.0]);
    assert!(analytic_curve_plane(&CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    })
    .is_none());
}

#[test]
fn complete_nurbs_boundaries_supply_only_provable_plane_evidence() {
    let planar = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 2.0),
        ],
        weights: None,
        periodic: false,
    });
    let plane = analytic_curve_plane(&planar).expect("planar NURBS boundary");
    assert_eq!(plane.origin[2], 2.0);
    assert_eq!(plane.normal, [0.0, 0.0, 1.0]);

    let nonplanar = CurveGeometry::Nurbs(NurbsCurve {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 1.0, 3.0),
            Point3::new(0.0, 1.0, 2.0),
        ],
        weights: None,
        periodic: false,
    });
    assert!(analytic_curve_plane(&nonplanar).is_none());

    let line = CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(0.0, 2.0, 4.0), Point3::new(3.0, 2.0, 4.0)],
        weights: Some(vec![2.0, 1.0]),
        periodic: false,
    });
    let line = analytic_boundary_line(&line).expect("degree-one NURBS line");
    assert_eq!(line.origin, [0.0, 2.0, 4.0]);
    assert_eq!(line.direction, [1.0, 0.0, 0.0]);

    let bent = CurveGeometry::Nurbs(NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        periodic: false,
    });
    assert!(analytic_boundary_line(&bent).is_none());
}

#[test]
fn every_solved_boundary_vertex_must_lie_on_the_analytic_plane() {
    let plane = PlaneEquation {
        origin: [0.0, 0.0, 2.0],
        normal: [0.0, 0.0, 1.0],
    };
    assert!(agreed_topology_bound_plane([[3.0, 4.0, 2.0]], [plane], []).is_some());
    assert!(agreed_topology_bound_plane([[3.0, 4.0, 3.0]], [plane], []).is_none());
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

    let analytic = analytic_boundary_line(&CurveGeometry::Line {
        origin: Point3::new(1.0, 2.0, 3.0),
        direction: Vector3::new(2.0, 0.0, 0.0),
    })
    .expect("analytic line");
    assert_eq!(analytic.direction, [1.0, 0.0, 0.0]);
}

#[test]
fn unique_native_conic_loop_places_its_plane_surface() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: false,
        boundary_type: 1,
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
            faces: [5, 0],
            next_edges: [11, 0],
            offset: 20,
        });
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: vec![crate::topology::HalfEdgeId {
            curve_id: 11,
            side: 0,
        }],
    });
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("creo:visibgeom:curve#11".to_string()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    });

    let transferred = transfer_topology_bound_planes(
        &scan,
        &mut ir,
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(transferred, 1);
    let plane = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == SurfaceId("creo:visibgeom:surface#5".to_string()))
        .expect("topology-bound plane");
    let SurfaceGeometry::Plane { origin, normal, .. } = &plane.geometry else {
        panic!("expected plane geometry");
    };
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(origin.z, 4.0);

    scan.curves
        .topology_rows
        .push(scan.curves.topology_rows[0].clone());
    ir.model.surfaces.clear();
    assert_eq!(
        transfer_topology_bound_planes(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
        ),
        0
    );
}

#[test]
fn unique_nurbs_line_loop_places_its_plane_surface() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: false,
        boundary_type: 1,
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
                faces: [5, 0],
                next_edges: [if id == 11 { 12 } else { 11 }, 0],
                offset: 20,
            });
    }
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: [11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId { curve_id, side: 0 })
            .collect(),
    });
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    for (id, origin, direction) in [
        (11, Point3::new(0.0, 0.0, 4.0), Vector3::new(1.0, 0.0, 0.0)),
        (12, Point3::new(0.0, 2.0, 4.0), Vector3::new(0.0, 1.0, 0.0)),
    ] {
        ir.model.curves.push(Curve {
            id: CurveId(format!("creo:visibgeom:curve#{id}")),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    origin,
                    Point3::new(
                        origin.x + direction.x,
                        origin.y + direction.y,
                        origin.z + direction.z,
                    ),
                ],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        });
    }

    assert_eq!(
        transfer_topology_bound_planes(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
        ),
        1
    );
    assert!(ir.model.surfaces.iter().any(|surface| {
        surface.id == SurfaceId("creo:visibgeom:surface#5".to_string())
            && matches!(
                &surface.geometry,
                SurfaceGeometry::Plane { origin, normal, .. }
                    if origin.z == 4.0 && *normal == Vector3::new(0.0, 0.0, 1.0)
            )
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
        slots: vec![
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
        origin: Some([8.0, 0.0, -0.85]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, 1.0, 0.0]),
        classification: LocalSystemClassification::Simple,
        row_offset: 10,
        offset: 20,
    };
    let candidate = envelope_reconciled_plane_candidate(&frame, equation).expect("mixed frame");
    assert_eq!(candidate.equation.origin, equation.origin);
    assert_eq!(candidate.equation.normal, equation.normal);
    assert_eq!(candidate.chart.expect("chart").u_axis, [1.0, 0.0, 0.0]);

    frame.origin = Some([8.0, 0.0, 1.0]);
    assert!(envelope_reconciled_plane_candidate(&frame, equation).is_none());
}

#[test]
fn frame_bound_outline_supplies_the_plane_chart_origin() {
    let frame = PlaneLocalSystem {
        surface_id: 52,
        body: Vec::new(),
        slots: vec![Some(0.0); 12],
        origin: Some([-9.0, 48.0, 0.0]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, 1.0, 0.0]),
        classification: LocalSystemClassification::Simple,
        row_offset: 10,
        offset: 20,
    };
    let outline = OutlinePlane {
        surface_id: 52,
        origin: [0.0, -4.0, 0.0],
        normal: [0.0, 1.0, 0.0],
        u_axis: [0.0, 0.0, 1.0],
        offset: 15,
    };
    let candidate = frame_bound_outline_plane_candidate(&frame, &outline).expect("composite chart");
    assert_eq!(candidate.equation.origin, outline.origin);
    assert_eq!(candidate.equation.normal, outline.normal);
    assert_eq!(candidate.chart.expect("chart").origin, [-9.0, -4.0, 0.0]);

    let mut conflicting = outline;
    conflicting.u_axis = [1.0, 0.0, 0.0];
    assert!(frame_bound_outline_plane_candidate(&frame, &conflicting).is_none());
}

#[test]
fn support_frame_selects_one_axis_from_a_line_shaped_plane_outline() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 42,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: 1,
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
        slots: Vec::new(),
        origin: Some([100.0, 200.0, 300.0]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, 1.0, 0.0]),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    });
    scan.planes.outlines =
        crate::surface::placed_outline_planes(&scan.planes.envelopes, &scan.planes.local_systems);

    let candidates = plane_candidates(&scan);
    let candidates = candidates.get(&42).expect("plane candidates");
    let (plane, u_axis, _) =
        agreed_plane_surface(candidates).expect("frame-selected outline plane");
    assert_eq!(plane.origin, [100.0, -4.0, 300.0]);
    assert_eq!(plane.normal, [0.0, 1.0, 0.0]);
    assert_eq!(u_axis, [0.0, 0.0, 1.0]);
}

#[test]
fn matrix_frame_owns_conflicting_held_coordinate_plane() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 42,
        type_byte: 0x22,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: 1,
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
        slots: vec![
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
        origin: Some([0.0, 0.0, 0.0]),
        u_axis: Some([component, 0.0, -component]),
        normal: Some([component, 0.0, component]),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    });
    scan.planes.outlines =
        crate::surface::placed_outline_planes(&scan.planes.envelopes, &scan.planes.local_systems);

    let candidates = plane_candidates(&scan);
    let candidates = candidates.get(&42).expect("plane candidates");
    let (plane, u_axis, _) = agreed_plane_surface(candidates).expect("matrix frame plane");
    assert_eq!(plane.normal, [component, 0.0, component]);
    assert_eq!(u_axis, [component, 0.0, -component]);
}

fn stored_frame_branch_scan(with_pcurve: bool) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    for id in [1, 2] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: 0x22,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: 1,
            next_surface: 0,
            offset: id as usize,
        });
    }
    scan.planes.local_systems.extend([
        crate::surface::PlaneLocalSystem {
            surface_id: 1,
            body: Vec::new(),
            slots: vec![
                Some(0.6),
                Some(0.0),
                Some(-0.8),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.8),
                Some(0.0),
                Some(0.6),
                Some(0.0),
                Some(0.0),
                Some(0.0),
            ],
            origin: Some([0.0, 0.0, 0.0]),
            u_axis: Some([0.6, 0.0, 0.8]),
            normal: Some([0.8, 0.0, -0.6]),
            classification: LocalSystemClassification::Unclassified,
            row_offset: 1,
            offset: 10,
        },
        crate::surface::PlaneLocalSystem {
            surface_id: 2,
            body: Vec::new(),
            slots: vec![None; 12],
            origin: Some([0.0, 1.0, 0.0]),
            u_axis: Some([1.0, 0.0, 0.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: LocalSystemClassification::Simple,
            row_offset: 2,
            offset: 20,
        },
    ]);
    if with_pcurve {
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [1, 2],
            face_0_endpoints: [[1.0, 1.0], [2.0, 1.0]],
            face_1_endpoints: [[0.6, 0.8], [1.2, 1.6]],
            offset: 30,
        });
    }
    scan
}

#[test]
fn stored_parameter_normal_frame_exposes_both_mirror_branches() {
    let scan = stored_frame_branch_scan(false);
    let frame = &scan.planes.local_systems[0];
    let candidates = stored_parameter_normal_candidates(frame).expect("ambiguous frame");
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|candidate| {
        candidate.equation.normal == [0.8, 0.0, 0.6]
            && candidate.chart.expect("chart").u_axis == [0.6, 0.0, -0.8]
    }));
    assert!(candidates.iter().any(|candidate| {
        candidate.equation.normal == [0.8, 0.0, -0.6]
            && candidate.chart.expect("chart").u_axis == [0.6, 0.0, 0.8]
    }));

    let mut invalid = frame.clone();
    invalid.slots[4] = Some(1.0);
    assert!(stored_parameter_normal_candidates(&invalid).is_none());
}

#[test]
fn stored_parameter_normal_branch_uses_unique_pcurve_endpoint_witness() {
    let scan = stored_frame_branch_scan(true);
    let candidates = plane_candidates(&scan);
    let candidates = candidates.get(&1).expect("selected plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, 0.6]);
    assert_eq!(candidates[0].chart.expect("chart").u_axis, [0.6, 0.0, -0.8]);
}

#[test]
fn stored_parameter_normal_branch_keeps_existing_frame_without_witness() {
    let scan = stored_frame_branch_scan(false);
    let candidates = plane_candidates(&scan);
    let candidates = candidates.get(&1).expect("existing plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, -0.6]);
    assert_eq!(candidates[0].chart.expect("chart").u_axis, [0.6, 0.0, 0.8]);
}
