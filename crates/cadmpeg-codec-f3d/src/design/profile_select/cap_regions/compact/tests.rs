// SPDX-License-Identifier: Apache-2.0

use super::{bounded_lateral_ellipse, Piece, PlaneFrame};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalPlane,
    AsmHistoricalRelation, AsmHistoricalTopology,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const TOLERANCE: f64 = 1.0e-6;

#[test]
fn lateral_ellipse_certificate_bounds_the_complete_carrier_and_its_projection() {
    let frame = PlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    let topology = AsmHistoricalTopology {
        surface_planes: vec![AsmHistoricalPlane {
            surface: 1,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
        }],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 2,
            origin: Point3::new(0.0, 0.0, -2.0),
            axis: Vector3::new(0.0, 0.8, 0.6),
            radius: 1.0,
        }],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 1,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 2,
            },
        ],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![20],
            },
            AsmHistoricalRelation {
                owner_ref: 12,
                member_refs: vec![10],
            },
        ],
        coedge_topology: [(21, 20), (22, 10)]
            .into_iter()
            .map(|(coedge, owner_loop)| AsmHistoricalCoedge {
                coedge,
                owner_loop,
                edge: 7,
                next: 0,
                previous: 0,
                radial_next: 0,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    };
    let outline = [Piece::Line(Point2::new(-2.0, 0.0), Point2::new(2.0, 0.0))];
    assert!(bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &outline, -4.0, TOLERANCE
    ));
    assert!(!bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &outline, -2.5, TOLERANCE
    ));
    let narrow = [Piece::Line(Point2::new(-0.5, 0.0), Point2::new(0.5, 0.0))];
    assert!(!bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &narrow, -4.0, TOLERANCE
    ));
}
