// SPDX-License-Identifier: Apache-2.0
//! Resource admission for mesh orientation and edge-domain search.

use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;

use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

use crate::solve::mesh_quotient::MeshQuotient;

#[test]
fn open_single_edge_boundary_has_no_orientation_options() {
    use cadmpeg_core::decode::DecodeContext;

    use std::collections::HashSet;
    use std::sync::Arc;
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };
    let quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    let run = |ctx: &DecodeContext<'_>| {
        quotient
            .assignment_options_limited(ctx, &assignment, &[vec![[0, 1]]], &HashSet::new(), 1, None)
            .map(|options| options.is_empty())
    };
    catia_test_context!(service_ctx);
    assert!(run(&service_ctx).expect("service resource budget"));
}

#[test]
fn edge_domains_outside_the_quotient_are_not_viable() {
    use std::sync::Arc;

    let mut quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    assert!(!quotient
        .edge_domains_viable(&[vec![[2, 3]]])
        .expect("service resource budget"));
}

#[test]
fn open_single_edge_boundary_has_no_fixed_direction_options() {
    use std::sync::Arc;
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };
    let quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    let options = vec![vec![vec![false]]];
    assert!(quotient
        .assignment_options_for_directions(&assignment, &options, 1, None)
        .expect("service resource budget")
        .is_empty());
}

#[test]
fn open_single_edge_boundary_cannot_merge_label_directions() {
    use std::sync::Arc;
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };

    let mut quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    assert!(quotient
        .merge_label_directions_in_place(&assignment, &[vec![false]], &[Some(false)], None,)
        .expect("service resource budget")
        .is_none());
}
