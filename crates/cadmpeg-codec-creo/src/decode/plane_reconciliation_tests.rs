use super::{
    agreed_plane, agreed_plane_surface, dot, envelope_reconciled_plane_candidate,
    frame_bound_outline_plane_candidate, held_coordinate_plane, plane_candidates, PlaneCandidate,
    PlaneChart, PlaneEquation,
};
use crate::surface::{
    LocalSystemClassification, OutlinePlane, PlaneEnvelope, PlaneEnvelopeRecord, PlaneLocalSystem,
};

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
