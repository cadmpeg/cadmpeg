// SPDX-License-Identifier: Apache-2.0
//! Unit tests for JT topological dual-mesh reconstruction.

#![allow(clippy::unwrap_used)]

#[test]
fn jt_topological_dual_mesh_reconstructs_closed_tetrahedron() {
    let polygons = super::decode(
        [&[3, 3, 3], &[3], &[], &[], &[], &[], &[], &[]],
        &[3, 3, 3, 3],
        &[10, 12, 11, 13],
        &[0, 0, 0, 0],
        &[],
        &[],
        super::AttributeMaskLanes {
            small: [&[], &[1, 1, 1, 1], &[], &[], &[], &[], &[], &[]],
            context_7_next_30: &[],
            context_7_upper_4: &[],
            large_words: &[],
        },
    )
    .expect("valid closed dual mesh");

    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.vertex_indices.as_slice())
            .collect::<Vec<_>>(),
        vec![&[0, 1, 2], &[2, 1, 3], &[2, 3, 0], &[3, 1, 0]]
    );
    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.group)
            .collect::<Vec<_>>(),
        vec![10, 12, 11, 13]
    );
    assert_eq!(
        polygons[0].attribute_indices,
        vec![Some(0), Some(1), Some(2)]
    );
}


