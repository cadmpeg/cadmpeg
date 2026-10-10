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

#[test]
fn triple_occurrence_index_separates_common_two_handle_prefixes() {
    let measure = |count: u32| {
        crate::test_support::with_service_context(|ctx| {
            let rows = (2..count + 2)
                .map(|handle| {
                    EdgeRow::new(
                        1,
                        vec![0, 1, handle],
                        EdgeBoundaryLayout::CompleteBoundaryRun,
                    )
                    .expect("triple")
                })
                .collect::<Vec<_>>();
            let cycles = (2..count + 2)
                .map(|handle| vec![vec![0, 1, handle]])
                .collect::<Vec<_>>();
            let occurrences = mesh_edge_occurrences(ctx, &rows, &cycles)
                .expect("indexed triples")
                .expect("unique occurrences");
            for (edge, runs) in occurrences.iter().enumerate() {
                assert_eq!(runs.len(), 1);
                assert_eq!(
                    (
                        runs[0].face,
                        runs[0].cycle,
                        runs[0].start,
                        runs[0].segment_count,
                        runs[0].reversed
                    ),
                    (edge, 0, 0, 2, false)
                );
            }
            let CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure performed work")
                .expect_err("measure")
            else {
                panic!("work refusal");
            };
            limit.used
        })
    };
    let small = measure(1024);
    let large = measure(2048);
    assert!(
        2 * large <= 5 * small,
        "indexed work must grow below quadratic: {small}, {large}"
    );
}

#[test]
fn repeated_long_occurrence_patterns_preserve_forward_precedence() {
    let rows = (0..16)
        .map(|_| {
            EdgeRow::new(
                1,
                vec![0, 1, 0, 1, 0],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("closed pattern")
        })
        .collect::<Vec<_>>();
    let cycles = [vec![vec![0, 1]]];
    crate::test_support::with_service_context(|ctx| {
        let occurrences = mesh_edge_occurrences(ctx, &rows, &cycles)
            .expect("shared long pattern")
            .expect("one occurrence per row");
        assert_eq!(occurrences.len(), rows.len());
        for runs in occurrences {
            assert_eq!(runs.len(), 1);
            assert_eq!(
                (
                    runs[0].face,
                    runs[0].start,
                    runs[0].segment_count,
                    runs[0].reversed
                ),
                (0, 0, 4, false)
            );
        }
    });
}

#[test]
fn repeated_long_occurrence_queries_share_common_prefix_probes() {
    let measure = |count: u32| {
        crate::test_support::with_service_context(|ctx| {
            let rows = (0..count)
                .map(|_| {
                    EdgeRow::new(1, vec![0, 1, 2, 3], EdgeBoundaryLayout::CompleteBoundaryRun)
                        .expect("long pattern")
                })
                .collect::<Vec<_>>();
            let mut cycles = (4..count + 4)
                .map(|handle| vec![vec![0, 1, 2, handle]])
                .collect::<Vec<_>>();
            cycles.push(vec![vec![0, 1, 2, 3]]);
            let occurrences = mesh_edge_occurrences(ctx, &rows, &cycles)
                .expect("shared probes")
                .expect("one occurrence per row");
            assert_eq!(occurrences.len(), rows.len());
            for runs in occurrences {
                assert_eq!(runs.len(), 1);
                assert_eq!(
                    (
                        runs[0].face,
                        runs[0].start,
                        runs[0].segment_count,
                        runs[0].reversed
                    ),
                    (usize::try_from(count).expect("small count"), 0, 3, false)
                );
            }
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
        "repeated long lookup must grow below quadratic: {small}, {large}"
    );
}
