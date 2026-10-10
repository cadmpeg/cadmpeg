use super::*;

#[test]
fn port_star_keeps_dependencies_and_options_across_branch_frames() {
    let measure = |count: u32| {
        crate::test_support::with_depth_limit(256, |ctx| {
            let mut ports = vec![[0, 0]];
            ports.extend((1..=count).map(|port| [0, port]));
            let mut candidates = vec![vec![[0, 0]]];
            candidates.extend(
                (1..=usize::try_from(count).expect("small count"))
                    .map(|port| vec![[0, 2 * port - 1], [0, 2 * port]]),
            );
            assert_eq!(
                bind_edge_port_candidates(ctx, &ports, &candidates).expect("star search"),
                None
            );
            let CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure performed work")
                .expect_err("measure")
            else {
                panic!("work refusal");
            };
            limit.used
        })
    };
    let small = measure(64);
    let large = measure(128);
    assert!(
        large <= 3 * small,
        "branch setup must grow below quadratic: {small}, {large}"
    );
}
