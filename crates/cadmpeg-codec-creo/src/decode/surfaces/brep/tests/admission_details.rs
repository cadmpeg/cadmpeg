// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use super::super::FaceAdmissionDetail;

#[test]
fn rejection_vertex_lookup_refuses_before_duplicate_skip() {
    let first = crate::topology::HalfEdgeId {
        curve_id: 4,
        side: crate::topology::Side::Zero,
    };
    let second = crate::topology::HalfEdgeId { curve_id: 5, ..first };
    let loop_record = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(17), vec![first, second],
    );
    let first_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: first,
        start_vertex_id: std::num::NonZeroU32::new(9).expect("one-based vertex fixture"),
        end_vertex_id: None,
    };
    let second_binding = crate::topology::HalfEdgeVertexIncidence {
        half_edge: second, ..first_binding
    };
    let incidence = BTreeMap::from([(first, &first_binding), (second, &second_binding)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One loop visit and two half-edge visits precede the duplicate lookup.
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = FaceAdmissionDetail::unresolved_boundary(
        &ctx, 17, &[&loop_record], &BTreeMap::new(), &incidence,
    ).expect_err("duplicate vertex lookup exceeds work limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo B-rep rejection vertex lookup"));
    let detail = crate::decode::with_test_decode_ctx(|ctx| {
        FaceAdmissionDetail::unresolved_boundary(
            ctx, 17, &[&loop_record], &BTreeMap::new(), &incidence,
        )
    }).expect("service rejection detail");
    assert_eq!(detail.vertex_ids, vec![9]);
    assert_eq!(detail.boundary_half_edges, vec![first, second]);
}
