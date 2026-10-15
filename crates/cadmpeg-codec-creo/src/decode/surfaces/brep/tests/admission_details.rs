// SPDX-License-Identifier: Apache-2.0

use super::super::FaceAdmissionDetail;
use std::collections::BTreeMap;

#[test]
fn rejection_incidence_lookup_refuses_before_duplicate_skip() {
    let first = crate::topology::HalfEdgeId {
        curve_id: 4,
        side: crate::topology::Side::Zero,
    };
    let second = crate::topology::HalfEdgeId {
        curve_id: 5,
        ..first
    };
    let loop_record =
        crate::test_support::closed_loop(std::num::NonZeroU32::new(17), vec![first, second]);
    let first_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: first,
        start_vertex_id: std::num::NonZeroU32::new(9).expect("one-based vertex fixture"),
        end_vertex_id: None,
    };
    let second_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: second,
        ..first_binding
    };
    let incidence = BTreeMap::from([(first, &first_binding), (second, &second_binding)]);
    let detail = crate::test_support::assert_work_boundaries(&["creo incidence lookup"], |ctx| {
        FaceAdmissionDetail::unresolved_boundary(
            ctx,
            17,
            &[&loop_record],
            &BTreeMap::new(),
            &incidence,
        )
    });
    assert_eq!(detail.vertex_ids, vec![9]);
    assert_eq!(detail.boundary_half_edges, vec![first, second]);
}

#[test]
fn rejection_incidence_lookup_refuses_and_preserves_end_vertices() {
    let first = crate::topology::HalfEdgeId {
        curve_id: 4,
        side: crate::topology::Side::Zero,
    };
    let second = crate::topology::HalfEdgeId {
        curve_id: 5,
        ..first
    };
    let loop_record =
        crate::test_support::closed_loop(std::num::NonZeroU32::new(17), vec![first, second]);
    let first_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: first,
        start_vertex_id: std::num::NonZeroU32::new(9).expect("one-based start vertex"),
        end_vertex_id: std::num::NonZeroU32::new(10),
    };
    let second_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: second,
        start_vertex_id: std::num::NonZeroU32::new(11).expect("one-based start vertex"),
        end_vertex_id: std::num::NonZeroU32::new(12),
    };
    let incidence = BTreeMap::from([(first, &first_binding), (second, &second_binding)]);
    let detail = crate::test_support::assert_work_boundaries(&["creo incidence lookup"], |ctx| {
        FaceAdmissionDetail::unresolved_boundary(
            ctx,
            17,
            &[&loop_record],
            &BTreeMap::new(),
            &incidence,
        )
    });
    assert_eq!(detail.boundary_half_edges, vec![first, second]);
    assert_eq!(detail.vertex_ids, vec![9, 10, 11, 12]);
}
