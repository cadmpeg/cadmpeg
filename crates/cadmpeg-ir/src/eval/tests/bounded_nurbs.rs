// SPDX-License-Identifier: Apache-2.0

#[test]
fn bounded_nurbs_interval_search_keeps_a_fixed_working_set() {
    let boundaries = (0..=10_000)
        .map(|index| crate::scalar::FiniteReal::from_index(index).expect("test index is exact"))
        .collect::<Vec<_>>();
    let seed = crate::scalar::FiniteReal::new(5_000.5).expect("finite seed");
    let intervals = super::super::bounded_nearest_intervals(&boundaries, seed)
        .expect("resource allocation did not fail");

    assert_eq!(intervals.len(), 512);
    assert!(intervals
        .iter()
        .any(|interval| interval.map(crate::scalar::FiniteReal::get) == [5_000.0, 5_001.0]));
}

#[test]
fn bounded_nurbs_containment_search_keeps_the_final_valid_spans() {
    let boundaries = [0.0, 1.0, 1.0, 2.0, 3.0];

    assert_eq!(
        super::super::bounded_tail_intervals(&boundaries)
            .expect("resource allocation did not fail"),
        (vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]], false)
    );

    let many_boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let (intervals, truncated) = super::super::bounded_tail_intervals(&many_boundaries)
        .expect("resource allocation did not fail");
    assert_eq!(intervals.len(), 512);
    assert!(truncated);
}

#[test]
fn bounded_nurbs_boundary_witness_preserves_seed_priority() {
    let boundaries = [0, 1, 2]
        .map(|index| crate::scalar::FiniteReal::from_index(index).expect("test index is exact"));
    let seed = crate::scalar::FiniteReal::new(1.4).expect("finite seed");

    assert_eq!(
        super::super::nearest_boundary_witness(&boundaries, seed, 0.0, |_| Ok(Some(0.0)))
            .expect("resource allocation did not fail"),
        super::super::BoundaryWitness::Found(crate::scalar::FiniteReal::ONE)
    );
}
