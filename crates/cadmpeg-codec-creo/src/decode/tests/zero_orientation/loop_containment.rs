// SPDX-License-Identifier: Apache-2.0

use super::{ordered_face_loops_service, ordered_planar_face_loops_service};
use crate::decode::analytic::equations::PlaneEquation;
use crate::topology::HalfEdgeId;
use std::collections::BTreeMap;

#[test]
fn planar_loop_containment_selects_one_outer_boundary() {
    let make_loop = |face_id: u32, first_curve: u32| crate::test_support::closed_loop(std::num::NonZeroU32::new(face_id), (0_u32..4)
            .map(|index| HalfEdgeId {
                curve_id: first_curve + index,
                side: crate::topology::Side::Zero,
            })
            .collect());
    let outer = make_loop(9, 1);
    let inner = make_loop(9, 5);
    let incidences = (1..=8)
        .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: vertex,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: vertex,
            end_vertex_id: Some(if vertex % 4 == 0 {
                vertex - 3
            } else {
                vertex + 1
            }),
        })
        .collect::<Vec<_>>();
    let incidence = incidences
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let points = BTreeMap::from([
        (1, [-2.0, -2.0, 0.0]),
        (2, [2.0, -2.0, 0.0]),
        (3, [2.0, 2.0, 0.0]),
        (4, [-2.0, 2.0, 0.0]),
        (5, [-1.0, -1.0, 0.0]),
        (6, [1.0, -1.0, 0.0]),
        (7, [1.0, 1.0, 0.0]),
        (8, [-1.0, 1.0, 0.0]),
    ]);
    let plane = PlaneEquation {
        origin: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
    };

    let ordered =
        ordered_planar_face_loops_service(vec![&inner, &outer], plane, &incidence, &points)
            .expect("unique outer loop");
    assert_eq!(ordered[0].half_edges()[0].curve_id, 1);
    assert_eq!(ordered[1].half_edges()[0].curve_id, 5);

    let disjoint_points = points
        .into_iter()
        .map(|(id, mut point)| {
            if id >= 5 {
                point[0] += 10.0;
            }
            (id, point)
        })
        .collect::<BTreeMap<_, _>>();
    assert!(ordered_planar_face_loops_service(
        vec![&outer, &inner],
        plane,
        &incidence,
        &disjoint_points,
    )
    .is_none());
    assert_eq!(
        ordered_face_loops_service(&[&outer], None, &incidence, &disjoint_points),
        Some(vec![&outer])
    );
    assert!(
        ordered_face_loops_service(&[&outer, &inner], None, &incidence, &disjoint_points).is_none()
    );
}

#[test]
fn planar_loop_containment_derives_plane_from_solved_boundary_vertices() {
    let make_loop = |first_curve: u32| crate::test_support::closed_loop(std::num::NonZeroU32::new(9), (0_u32..4)
            .map(|index| HalfEdgeId {
                curve_id: first_curve + index,
                side: crate::topology::Side::Zero,
            })
            .collect());
    let outer = make_loop(1);
    let inner = make_loop(5);
    let incidences = (1..=8)
        .map(|vertex| crate::topology::HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: vertex,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: vertex,
            end_vertex_id: Some(if vertex % 4 == 0 {
                vertex - 3
            } else {
                vertex + 1
            }),
        })
        .collect::<Vec<_>>();
    let incidence = incidences
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let points = BTreeMap::from([
        (1, [-2.0, -2.0, 4.0]),
        (2, [2.0, -2.0, 4.0]),
        (3, [2.0, 2.0, 4.0]),
        (4, [-2.0, 2.0, 4.0]),
        (5, [-1.0, -1.0, 4.0]),
        (6, [1.0, -1.0, 4.0]),
        (7, [1.0, 1.0, 4.0]),
        (8, [-1.0, 1.0, 4.0]),
    ]);

    let ordered = ordered_face_loops_service(&[&inner, &outer], None, &incidence, &points)
        .expect("boundary vertices prove a unique plane");
    assert_eq!(ordered[0].half_edges()[0].curve_id, 1);
    assert_eq!(ordered[1].half_edges()[0].curve_id, 5);

    let non_planar = points
        .into_iter()
        .map(|(id, mut point)| {
            if id == 8 {
                point[2] += 1.0;
            }
            (id, point)
        })
        .collect::<BTreeMap<_, _>>();
    assert!(ordered_face_loops_service(&[&outer, &inner], None, &incidence, &non_planar).is_none());
}
