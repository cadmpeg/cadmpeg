// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

use crate::design::geometry::AtomicSketchRegion;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalEdgeSample,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};

use super::{face_cap, outlines_agree, Sampled};

const TOLERANCE: f64 = 1e-6;
/// The cap face every test topology draws.
const CAP: i64 = 100;

fn xy_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#cap-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn slot(index: usize) -> i64 {
    i64::try_from(index).unwrap()
}

/// A topology whose cap face has one loop of `edges`, each `(start, end)`
/// vertex pair stored in that direction, at height 2 above the sketch.
fn loop_topology(corners: &[(f64, f64)], edges: &[(usize, usize)]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: CAP,
            member_refs: vec![200],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 200,
            member_refs: (0..edges.len()).map(|k| 300 + slot(k)).collect(),
        }],
        ..AsmHistoricalTopology::default()
    };
    for (k, (u, v)) in corners.iter().enumerate() {
        let k = slot(k);
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 500 + k,
            carrier: 600 + k,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 600 + k,
            position: Point3::new(*u, *v, 2.0),
        });
    }
    for (k, (start, end)) in edges.iter().enumerate() {
        let k = slot(k);
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 300 + k,
            owner_loop: 200,
            edge: 400 + k,
            next: 0,
            previous: 0,
            radial_next: 0,
        });
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge: 400 + k,
            start_vertex: 500 + slot(*start),
            end_vertex: 500 + slot(*end),
        });
    }
    topology
}

/// Gives every edge a straight carrier through its stored ends.
fn straight_carriers(topology: &mut AsmHistoricalTopology) {
    for row in topology.edge_vertices.clone() {
        let at = |vertex: i64| {
            let point = topology
                .vertex_points
                .iter()
                .find(|binding| binding.entity == vertex)
                .unwrap()
                .carrier;
            topology
                .point_positions
                .iter()
                .find(|position| position.point == point)
                .unwrap()
                .position
        };
        let (start, end) = (at(row.start_vertex), at(row.end_vertex));
        topology
            .edge_curves
            .push(AsmHistoricalOptionalCarrierBinding {
                entity: row.edge,
                carrier: Some(row.edge + 1000),
            });
        topology.curve_axes.push(AsmHistoricalCurveAxis {
            curve: row.edge + 1000,
            origin: start,
            direction: end.vector_from(start),
        });
    }
}

fn unit_square_corners() -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
}

#[test]
fn a_loop_whose_first_edge_is_stored_backwards_keeps_every_corner() {
    // The first edge is stored from corner 1 to corner 0; the loop runs
    // 0 -> 1 -> 2 -> 3, so it must be drawn from 0 to 1 before the rest.
    let mut topology = loop_topology(&unit_square_corners(), &[(1, 0), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).unwrap();
    assert_eq!(cap.loops[0].len(), 4);
    for corner in unit_square_corners() {
        assert!(
            cap.loops[0]
                .iter()
                .any(|point| (point.u - corner.0).hypot(point.v - corner.1) < 1e-12),
            "{corner:?} missing from {:?}",
            cap.loops[0]
        );
    }
    assert!((cap.area - 1.0).abs() < 1e-12);
}

#[test]
fn a_loop_that_does_not_close_proves_no_cap() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 1)]);
    straight_carriers(&mut topology);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());
}

#[test]
fn an_edge_without_a_proven_carrier_proves_no_cap() {
    // An edge with no curve axis may be a spline through the same points.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes.remove(2);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());

    // A curve axis across the chord is a circle's or an ellipse's.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes[2].direction = Vector3::new(0.0, 0.0, 1.0);
    assert!(face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).is_none());
}

#[test]
fn a_closed_edge_is_a_circle_only_on_a_cylinder_about_the_cap_normal() {
    // One closed edge on a circle of radius 2 about (1, 1).
    let circle_topology = |axis: Vector3| {
        let mut topology = loop_topology(&[(3.0, 1.0)], &[(0, 0)]);
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: 101,
            member_refs: vec![201],
        });
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 350,
            owner_loop: 201,
            edge: 400,
            next: 0,
            previous: 0,
            radial_next: 0,
        });
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: 101,
            carrier: 700,
        });
        topology.surface_cylinders.push(AsmHistoricalCylinder {
            surface: 700,
            origin: Point3::new(1.0, 1.0, -5.0),
            axis,
            radius: 2.0,
        });
        topology.edge_samples.push(AsmHistoricalEdgeSample {
            edge: 400,
            interior: Point3::new(-1.0, 1.0, 2.0),
            tangent: None,
        });
        topology
    };
    let cap = face_cap(
        &xy_sketch(),
        &circle_topology(Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let circle = std::f64::consts::PI * 4.0;
    assert!((cap.area - circle).abs() < 1e-2 * circle);
    assert!(cap.boundary_distance(Point2::new(1.0, 3.0)) < 1e-12);
    assert!(face_cap(
        &xy_sketch(),
        &circle_topology(Vector3::new(1.0, 0.0, 0.0)),
        CAP,
        TOLERANCE,
    )
    .is_none());
}

/// One region of area 1 sampled at the unit square's centre.
fn sampled(region: &AtomicSketchRegion) -> Vec<Sampled<'_>> {
    vec![Sampled {
        region,
        point: Point2::new(0.5, 0.5),
        area: 1.0,
        nested: Vec::new(),
    }]
}

#[test]
fn regions_of_the_caps_area_must_also_share_its_outline() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_sketch(), &topology, CAP, TOLERANCE).unwrap();
    let square = |du: f64| AtomicSketchRegion {
        boundary: Vec::new(),
        polyline: unit_square_corners()
            .iter()
            .map(|(u, v)| Point2::new(u + du, *v))
            .collect(),
        encloses_island: false,
    };
    let same = square(0.0);
    assert!(outlines_agree(&cap, &sampled(&same), &[0], TOLERANCE));
    // Equal area and a shared interior point, but another outline.
    let shifted = square(0.4);
    assert!(!outlines_agree(&cap, &sampled(&shifted), &[0], TOLERANCE));
}
