// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::evaluation::HistoricalFaceSurface;
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::math::{Point3, Vector3};

use super::{edge_is_tangent, support_boundary_edge_continuities, BoundaryEdgeContinuities};
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalFaceSample, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::topology::historical_context::{
    DesignHistoricalFaceBoundaryContext, DesignHistoricalFaceLoopContext,
    DesignHistoricalFaceSupportContext, DesignHistoricalLoopBoundary, DesignHistoricalLoopCoedge,
};

const TOP: i64 = 1;
const FILLET: i64 = 2;
const PLUS_X: i64 = 3;
const TANGENT_EDGE: i64 = 10;
const SHARP_EDGE: i64 = 11;
const OPEN_EDGE: i64 = 13;

fn coedge(coedge: i64, owner_loop: i64, edge: i64) -> AsmHistoricalCoedge {
    AsmHistoricalCoedge {
        coedge,
        owner_loop,
        edge,
        next: coedge,
        previous: coedge,
        radial_next: coedge,
    }
}

fn face(face: i64, surface: HistoricalFaceSurface) -> AsmHistoricalFaceSample {
    AsmHistoricalFaceSample {
        face,
        surface,
        reversed: false,
    }
}

/// The top face `z = 10` of a 20 mm box, a 2 mm fillet cylinder on its
/// `y = 20` edge, and its `x = 20` side face. The top face meets the
/// cylinder at `y = 18` and the side face at `x = 20`. A third top-face edge
/// has one coedge.
fn filleted_box_top() -> AsmHistoricalTopology {
    let top = PlaneSurface::try_new(
        Point3::new(10.0, 10.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let fillet = CylinderSurface::try_new(
        Point3::new(0.0, 18.0, 8.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        2.0,
    )
    .unwrap();
    let side = PlaneSurface::try_new(
        Point3::new(20.0, 10.0, 5.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    )
    .unwrap();
    AsmHistoricalTopology {
        faces: vec![TOP, FILLET, PLUS_X],
        edges: vec![TANGENT_EDGE, SHARP_EDGE, OPEN_EDGE],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: TOP,
                member_refs: vec![101],
            },
            AsmHistoricalRelation {
                owner_ref: FILLET,
                member_refs: vec![102],
            },
            AsmHistoricalRelation {
                owner_ref: PLUS_X,
                member_refs: vec![103],
            },
        ],
        coedge_topology: vec![
            coedge(201, 101, TANGENT_EDGE),
            coedge(202, 102, TANGENT_EDGE),
            coedge(203, 101, SHARP_EDGE),
            coedge(204, 103, SHARP_EDGE),
            coedge(205, 101, OPEN_EDGE),
        ],
        edge_samples: vec![
            crate::test_support::history::edge_sample(
                TANGENT_EDGE,
                Point3::new(10.0, 18.0, 10.0),
                Some(Vector3::new(1.0, 0.0, 0.0)),
            ),
            crate::test_support::history::edge_sample(
                SHARP_EDGE,
                Point3::new(20.0, 10.0, 10.0),
                Some(Vector3::new(0.0, 1.0, 0.0)),
            ),
            crate::test_support::history::edge_sample(
                OPEN_EDGE,
                Point3::new(0.0, 10.0, 10.0),
                Some(Vector3::new(0.0, 1.0, 0.0)),
            ),
        ],
        face_samples: vec![
            face(TOP, HistoricalFaceSurface::Plane(top)),
            face(FILLET, HistoricalFaceSurface::Cylinder(fillet)),
            face(PLUS_X, HistoricalFaceSurface::Plane(side)),
        ],
        ..AsmHistoricalTopology::default()
    }
}

#[test]
fn plane_and_fillet_cylinder_meet_tangentially_and_two_planes_meet_sharply() {
    let topology = filleted_box_top();
    assert_eq!(
        edge_is_tangent(TANGENT_EDGE, &topology).unwrap(),
        Some(true)
    );
    assert_eq!(edge_is_tangent(SHARP_EDGE, &topology).unwrap(), Some(false));
    assert_eq!(edge_is_tangent(OPEN_EDGE, &topology).unwrap(), None);
}

#[test]
fn support_boundary_edges_split_into_tangent_and_undetermined_slots() {
    let topology = filleted_box_top();
    let context = DesignHistoricalFaceSupportContext {
        active_face_slot: 30,
        surface_slot: 40,
        preceding_face_slots: vec![TOP],
        preceding_face_boundaries: vec![DesignHistoricalFaceBoundaryContext {
            face_slot: TOP,
            loops: vec![DesignHistoricalFaceLoopContext {
                loop_slot: 101,
                boundary: DesignHistoricalLoopBoundary::Coedges(
                    [(201, TANGENT_EDGE), (203, SHARP_EDGE), (205, OPEN_EDGE)]
                        .into_iter()
                        .map(|(coedge_slot, edge_slot)| DesignHistoricalLoopCoedge {
                            coedge_slot,
                            edge_slot,
                        })
                        .collect(),
                ),
            }],
        }],
        changed_preceding_face_slots: vec![TOP],
        preceding_surface: None,
    };
    assert_eq!(
        support_boundary_edge_continuities([&context], &topology).unwrap(),
        BoundaryEdgeContinuities {
            tangent: vec![TANGENT_EDGE],
            undetermined: vec![OPEN_EDGE],
        }
    );
}
