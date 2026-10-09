//! Resource admission for transform votes and indexed marker lookup.

use super::super::{compatible_marker_transform_candidates, Axes, MarkerTransform, Sign};
use crate::resolved_features::grid::GridPoint;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::{HashMap, HashSet};

#[test]
fn transform_votes_release_scratch_and_retain_only_the_winner() {
    let loci = (0i64..64)
        .map(|i| GridPoint::from((i + 7, i * i + 11)))
        .collect::<HashSet<_>>();
    let compatible = (0i64..64)
        .map(|i| (GridPoint::from((i, i * i)), &loci))
        .collect::<HashMap<_, _>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let expected = MarkerTransform {
        axes: Axes::Aligned {
            swap: false,
            u: Sign::Positive,
            v: Sign::Positive,
        },
        translation: (7, 11),
    };
    // Repeated calls must not accumulate maps that the previous call dropped.
    for _ in 0..4 {
        assert_eq!(
            compatible_marker_transform_candidates(&ctx, &compatible).unwrap(),
            vec![expected]
        );
    }
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn transform_votes_preserve_scratch_refusal() {
    let compatible = HashMap::from([
        (
            GridPoint::from((0, 0)),
            HashSet::from([GridPoint::from((1, 1))]),
        ),
        (
            GridPoint::from((1, 0)),
            HashSet::from([GridPoint::from((2, 1))]),
        ),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
        compatible_marker_transform_candidates(&ctx, &compatible)
    else {
        panic!("vote storage requires scratch admission");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
