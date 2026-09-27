// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

#[test]
fn affine_parameter_map_retains_finite_ratio_of_overflowing_span() {
    let (scale, offset) = crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [0.0, 1.0])
        .expect("finite affine map");
    assert!((scale * f64::MAX - 0.5).abs() < 8.0 * f64::EPSILON);
    assert!((offset - 0.5).abs() < 8.0 * f64::EPSILON);
}

#[test]
fn affine_parameter_map_retains_identity_between_overflowing_spans() {
    assert_eq!(
        crate::entities::affine_parameter_map([-f64::MAX, f64::MAX], [-f64::MAX, f64::MAX]),
        Some((1.0, 0.0))
    );
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        None,
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ).unwrap());
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        None,
        |sequence| graph.get(&sequence).into_iter().flatten().copied()
    ).unwrap());
}

#[test]
fn directed_cycle_refuses_stack_and_tree_nodes_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let graph = [(1_u32, vec![2_u32]), (2, Vec::new())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    for (cap, operation) in [
        (0, "iges cycle stack"),
        (1, "iges cycle active"),
        (6, "iges cycle visited"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = crate::entities::directed_cycle(
            1,
            &mut BTreeSet::new(),
            Some(&ctx),
            |sequence| graph.get(&sequence).into_iter().flatten().copied(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut visited = BTreeSet::new();
    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        Some(&ctx),
        |sequence| graph.get(&sequence).into_iter().flatten().copied(),
    ).unwrap());
    assert_eq!(visited, [1, 2].into());
}
