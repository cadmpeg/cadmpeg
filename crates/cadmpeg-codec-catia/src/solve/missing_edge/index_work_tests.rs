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

#[test]
fn one_use_long_occurrence_queries_keep_no_match_cache() {
    for count in [1, 32] {
        let rows = (3..count + 3)
            .map(|handle| {
                EdgeRow::new(
                    1,
                    vec![0, 1, 2, handle],
                    EdgeBoundaryLayout::CompleteBoundaryRun,
                )
                .expect("long pattern")
            })
            .collect::<Vec<_>>();
        crate::test_support::with_collection_limit(u64::MAX, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                "catia_mesh_occurrence_matches",
                None,
            );
            assert_eq!(
                mesh_edge_occurrences(ctx, &rows, &[])
                    .expect("empty output rows need no match cache"),
                Some(vec![Vec::new(); rows.len()])
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn single_long_occurrence_query_needs_no_query_index() {
    let rows = [
        EdgeRow::new(1, vec![0, 1, 2, 3], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("long pattern"),
    ];
    let cycles = [vec![vec![0, 1, 2, 3]]];
    crate::test_support::with_collection_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "catia_mesh_occurrence_queries",
            None,
        );
        assert_eq!(
            mesh_edge_occurrences(ctx, &rows, &cycles).expect("one query"),
            Some(vec![vec![MeshEdgeRun {
                edge: 0,
                face: 0,
                cycle: 0,
                start: 0,
                segment_count: 3,
                reversed: false
            }]])
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn repeated_long_occurrences_reapply_each_boundary_layout() {
    for first_layout in [
        EdgeBoundaryLayout::CompleteBoundaryRun,
        EdgeBoundaryLayout::InteriorWithFlankingCorners,
    ] {
        let layouts = [
            first_layout,
            EdgeBoundaryLayout::CompleteBoundaryRun,
            EdgeBoundaryLayout::InteriorWithFlankingCorners,
        ];
        let rows = layouts.map(|layout| {
            let handles = match layout {
                EdgeBoundaryLayout::CompleteBoundaryRun => vec![0, 1, 2, 3],
                EdgeBoundaryLayout::InteriorWithFlankingCorners => vec![9, 0, 1, 2, 3, 8],
            };
            EdgeRow::new(1, handles, layout).expect("long pattern")
        });
        let cycles = [vec![vec![0, 1, 2, 3]]];
        crate::test_support::with_service_context(|ctx| {
            let result = mesh_edge_occurrences(ctx, &rows, &cycles)
                .expect("shared query")
                .expect("one occurrence per cycle");
            assert_eq!(result.len(), rows.len());
            for (edge, runs) in result.iter().enumerate() {
                let (start, segment_count) = match layouts[edge] {
                    EdgeBoundaryLayout::CompleteBoundaryRun => (0, 3),
                    EdgeBoundaryLayout::InteriorWithFlankingCorners => (3, 5),
                };
                assert_eq!(
                    runs,
                    &[MeshEdgeRun {
                        edge,
                        face: 0,
                        cycle: 0,
                        start,
                        segment_count,
                        reversed: false,
                    }]
                );
            }
        });
    }
}

#[test]
fn motif_quad_scan_admits_its_end_probe() {
    let mut bytes = Vec::new();
    for handles in [[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11]] {
        bytes.extend_from_slice(&[0x01, 0x4a, 1, 0xff, 4, 0, 0, 0]);
        for component in [0.0_f32, 0.0, 1.0] {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        bytes.push(4);
        bytes.extend_from_slice(&handles);
    }
    let trims = crate::test_support::with_service_context(|ctx| {
        parse_trim_chain(ctx, &bytes, bytes.len(), 3, 1)
    })
    .expect("trim packets")
    .expect("complete handle partitions");
    let run = |ctx: &DecodeContext<'_>| motif_port_points(ctx, &trims, 8);
    let expected = [8, 9, 2, 3, 0, 1, 10, 11]
        .into_iter()
        .enumerate()
        .map(|(point, handle)| (handle, point))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        crate::test_support::with_service_context(run).expect("motif identities"),
        Some(expected)
    );
    let refused = crate::test_support::with_work_refusal("catia_missing_edge_iteration", run);
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_missing_edge_iteration"));
}

#[test]
fn endpoint_trail_selection_admits_the_next_trail() {
    let rows = [vec![10, 11], vec![12, 13], vec![1, 0], vec![3, 2]].map(|handles| {
        EdgeRow::new(1, handles, EdgeBoundaryLayout::CompleteBoundaryRun).expect("edge row")
    });
    let runs = [0, 1].map(|cycle| MeshEdgeRun {
        edge: cycle + 2,
        face: 0,
        cycle,
        start: 1,
        segment_count: 1,
        reversed: false,
    });
    let context = StandardMeshBoundaryContext {
        analysis: Arc::new(StandardMeshAnalysis {
            edge_rows: rows.into(),
            cycles: vec![vec![vec![0, 1], vec![2, 3]]],
            occurrences: vec![vec![], vec![], vec![runs[0]], vec![runs[1]]],
            fixed_complete_row_spans: true,
        }),
        coverage: vec![MeshFaceCoverage {
            face: 0,
            gaps: [0, 1]
                .map(|cycle| MeshBoundaryGap {
                    cycle,
                    start: 0,
                    length: 1,
                })
                .into(),
            missing_edges: vec![0, 1],
        }],
        edge_ports: Arc::new(vec![[0, 1], [2, 3], [4, 5], [6, 7]]),
        edge_runs: Arc::new(runs.into()),
        cycle_lengths: Arc::new(vec![vec![2, 2]]),
    };
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 1]], vec![[2, 3]]];
    let run = |ctx: &DecodeContext<'_>| {
        standard_mesh_missing_edge_assignment_domains(ctx, &context, Some(&candidates), true, false)
    };
    let placements = [0, 1].map(|edge| MeshEdgePlacementCandidate {
        edge,
        face: 0,
        cycle: edge,
        start: 0,
        segment_count: 1,
    });
    assert_eq!(
        crate::test_support::with_service_context(run).expect("two gap placements"),
        Some((
            vec![MeshFaceAssignmentDomain::Ordered(vec![placements.into()])],
            runs.into()
        ))
    );
    let refused = crate::test_support::with_work_refusal("catia_missing_edge_iteration", run);
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_missing_edge_iteration"));
}
