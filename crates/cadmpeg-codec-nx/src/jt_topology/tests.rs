// SPDX-License-Identifier: Apache-2.0
//! Unit tests for JT topological dual-mesh reconstruction.

#![allow(clippy::unwrap_used)]

#[test]
fn jt_topological_dual_mesh_reconstructs_closed_tetrahedron() {
    crate::test_support::with_decode_context(|ctx| {
        let polygons = super::decode(
            ctx,
            [&[3, 3, 3], &[3], &[], &[], &[], &[], &[], &[]],
            &[3, 3, 3, 3],
            &[10, 12, 11, 13],
            &[0, 0, 0, 0],
            super::SplitLanes {
                faces: &[],
                positions: &[],
            },
            super::AttributeMaskLanes {
                small: [&[], &[1, 1, 1, 1], &[], &[], &[], &[], &[], &[]],
                context_7_next_30: &[],
                context_7_upper_4: &[],
                large_words: &[],
            },
        )
        .expect("dual mesh decode resources admitted")
        .expect("valid closed dual mesh");

        assert_eq!(
            polygons
                .iter()
                .map(|polygon| polygon
                    .corners
                    .iter()
                    .map(|&(vertex, _)| vertex)
                    .collect::<Vec<_>>())
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
            polygons[0]
                .corners
                .iter()
                .map(|&(_, attribute)| attribute)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
    });
}

#[test]
fn every_face_degree_selects_its_stated_attribute_mask_context() {
    use std::num::NonZeroUsize;

    // A dual face of degree `d` consumes its attribute mask from context
    // `min(7, max(0, d - 2))`. Degrees one and two occur at non-manifold
    // display seams and take context zero; every degree above nine shares
    // context seven. The mapping is total over the degrees the symbol stream
    // can state, and a zero degree is a split reference that never reaches it.
    let expected = |degree: usize| match degree {
        1 | 2 => 0,
        3..=9 => degree - 2,
        _ => 7,
    };
    for degree in 1..=100_usize {
        let context =
            super::AttributeMaskContext::of(NonZeroUsize::new(degree).expect("degree is nonzero"))
                .expect("degree maps to a context");
        assert_eq!(context.lane(), expected(degree), "degree {degree}");
        assert_eq!(
            context == super::AttributeMaskContext::COMBINED,
            degree >= 9,
            "degree {degree}"
        );
    }
    assert_eq!(super::AttributeMaskContext::COMBINED.lane(), 7);
}

fn active_frontier(count: usize) -> super::Decoder<'static> {
    let faces = crate::test_support::with_decode_context(|ctx| {
        (0..count)
            .map(|_| super::Face {
                vertices: super::FaceSlots::new(ctx, 1).unwrap(),
                attribute_mask: Vec::new(),
                attributes: Vec::new(),
            })
            .collect()
    });
    super::Decoder {
        symbols: super::Symbols {
            degrees: [&[]; 8],
            degree_pos: [0; 8],
            valences: &[],
            groups: &[],
            flags: &[],
            split_faces: &[],
            split_positions: &[],
            attribute_masks: super::AttributeMaskLanes {
                small: [&[]; 8],
                context_7_next_30: &[],
                context_7_upper_4: &[],
                large_words: &[],
            },
            attribute_mask_pos: [0; 8],
            large_mask_pos: 0,
            vertex_pos: 0,
            split_pos: 0,
        },
        vertices: Vec::new(),
        faces,
        active: (0..count).collect(),
        removed: vec![false; count],
        slot_count: 0,
        attribute_count: 0,
    }
}

#[test]
fn jt_active_face_window_admits_large_unchanged_frontier() {
    let mut decoder = active_frontier(10_000);
    assert_eq!(decoder.next_active_face(), Some(9_999));
    assert_eq!(decoder.active.len(), 10_000);
}
