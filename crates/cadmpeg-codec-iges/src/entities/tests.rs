// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

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
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
}
