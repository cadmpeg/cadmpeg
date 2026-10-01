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
fn jt_vertex_face_slots_refuse_collection_limit() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 2;
        },
        |ctx| {
            let error = super::decode(
                ctx,
                [&[3], &[], &[], &[], &[], &[], &[], &[]],
                &[3],
                &[10],
                &[0],
                super::SplitLanes {
                    faces: &[],
                    positions: &[],
                },
                super::AttributeMaskLanes {
                    small: [&[]; 8],
                    context_7_next_30: &[],
                    context_7_upper_4: &[],
                    large_words: &[],
                },
            )
            .expect_err("three vertex face slots exceed two items");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && limit.used == 0
                        && limit.additional == 3
            ));
        },
    );
}

#[test]
fn jt_face_vertex_slots_refuse_through_decode() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 10;
        },
        |ctx| {
            let error = super::decode(
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
            .expect_err("three face slots exceed remaining collection items");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && limit.used == 8
                        && limit.additional == 3
            ));
        },
    );
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

#[test]
fn jt_reconstruction_refuses_ring_work_before_traversal() {
    crate::test_support::with_decode_context_over(&[], |policy| policy.limits.max_work_units = 0, |ctx| {
        let error = super::decode(ctx, [&[3, 3, 3], &[3], &[], &[], &[], &[], &[], &[]], &[3, 3, 3, 3], &[10, 12, 11, 13], &[0; 4], super::SplitLanes { faces: &[], positions: &[] }, super::AttributeMaskLanes { small: [&[], &[1; 4], &[], &[], &[], &[], &[], &[]], context_7_next_30: &[], context_7_upper_4: &[], large_words: &[] }).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "scan JT vertex face context" && limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    });
}
