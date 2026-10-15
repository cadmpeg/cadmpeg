use super::*;
use std::{collections::HashSet, sync::Arc};

#[test]
fn forced_coordinate_components_reject_unsupported_degree_one_points() {
    crate::test_support::with_service_context(|ctx| {
        let mut quotient = MeshQuotient::new(
            (0..4)
                .map(|point| Arc::new(HashSet::from([point])))
                .collect(),
        );
        let candidates = [vec![[0, 1]], vec![[2, 3]]];
        let boundaries = [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![(0..2)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ])];
        assert!(quotient
            .close_coordinate_roots_for_incidence_with_budget(
                ctx,
                4,
                &candidates,
                MeshIncidenceBoundary {
                    edge_faces: &[[0, 0]; 2],
                    face_count: 1,
                    domains: &boundaries
                },
                None,
            )
            .expect("component closure")
            .is_none());
    });
}

#[test]
fn forced_polygon_coordinate_closure_validates_each_face_once() {
    let measure = |count| {
        crate::test_support::with_service_context(|ctx| {
            let mut quotient = MeshQuotient::new(
                (0..count)
                    .flat_map(|point| [point, (point + 1) % count])
                    .map(|point| Arc::new(HashSet::from([point])))
                    .collect(),
            );
            let candidates = (0..count)
                .map(|point| vec![[point, (point + 1) % count]])
                .collect::<Vec<_>>();
            let faces = vec![[0, 1]; count];
            let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: Some(reversed),
            };
            let boundaries = [
                MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![(0..count).map(|edge| use_(edge, false)).collect()],
                }]),
                MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
                    boundaries: vec![(0..count).rev().map(|edge| use_(edge, true)).collect()],
                }]),
            ];
            let result = quotient
                .close_coordinate_roots_for_incidence_with_budget(
                    ctx,
                    count,
                    &candidates,
                    MeshIncidenceBoundary {
                        edge_faces: &faces,
                        face_count: 2,
                        domains: &boundaries,
                    },
                    None,
                )
                .expect("forced coordinate closure")
                .expect("closed polygon");
            assert_eq!(result.len(), count);
            for edge in 0..count {
                assert_eq!(
                    result[&quotient.root(ctx, 2 * edge).expect("left root")],
                    edge
                );
                assert_eq!(
                    result[&quotient.root(ctx, 2 * edge + 1).expect("right root")],
                    (edge + 1) % count
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
        "forced closure must grow below quadratic: {small}, {large}"
    );
}

#[test]
fn forced_compact_polygon_coordinate_closure_validates_each_face_once() {
    let measure = |count| {
        crate::test_support::with_service_context(|ctx| {
            let mut quotient = MeshQuotient::new(
                (0..count)
                    .flat_map(|point| [point, (point + 1) % count])
                    .map(|point| Arc::new(HashSet::from([point])))
                    .collect(),
            );
            let candidates = (0..count)
                .map(|point| vec![[point, (point + 1) % count]])
                .collect::<Vec<_>>();
            let faces = vec![[0, 1]; count];
            let boundaries = [
                MeshFaceBoundaryDomain::UnorderedFullCycle((0..count).collect()),
                MeshFaceBoundaryDomain::UnorderedFullCycle((0..count).collect()),
            ];
            let result = quotient
                .close_coordinate_roots_for_incidence_with_budget(
                    ctx,
                    count,
                    &candidates,
                    MeshIncidenceBoundary {
                        edge_faces: &faces,
                        face_count: 2,
                        domains: &boundaries,
                    },
                    None,
                )
                .expect("forced coordinate closure")
                .expect("closed polygon");
            assert_eq!(result.len(), count);
            for edge in 0..count {
                assert_eq!(
                    result[&quotient.root(ctx, 2 * edge).expect("left root")],
                    edge
                );
                assert_eq!(
                    result[&quotient.root(ctx, 2 * edge + 1).expect("right root")],
                    (edge + 1) % count
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
        "forced closure must grow below quadratic: {small}, {large}"
    );
}

#[test]
fn coordinate_degree_probe_rejects_saturated_first_face_without_admitting_suffix() {
    const TRIANGLES: usize = 13;
    let mut endpoints = Vec::new();
    let mut candidates = Vec::new();
    let mut faces = Vec::new();
    let mut boundaries = Vec::new();
    for face in 0..TRIANGLES {
        let a = 2 + 2 * face;
        let b = a + 1;
        let first = candidates.len();
        endpoints.extend([[1, a], [a, b], [b, 1]]);
        candidates.extend([vec![[0, a], [1, a]], vec![[a, b]], vec![[0, b], [1, b]]]);
        faces.extend([[face, face]; 3]);
        boundaries.push(MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![(first..first + 3)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ]));
    }
    let loop_edge = candidates.len();
    endpoints.push([0, 0]);
    candidates.push(vec![[0, 0]]);
    faces.push([0, 0]);
    let MeshFaceBoundaryDomain::Ordered(assignments) = &mut boundaries[0] else {
        panic!("ordered face");
    };
    assignments[0]
        .boundaries
        .push(vec![MeshBoundaryEdgeCandidate {
            edge: loop_edge,
            start: 0,
            end: 0,
            reversed: None,
        }]);
    let run = |ctx: &DecodeContext<'_>| {
        let mut quotient = MeshQuotient::new(
            endpoints
                .iter()
                .flatten()
                .map(|&point| {
                    Arc::new(if point == 1 {
                        HashSet::from([0, 1])
                    } else {
                        HashSet::from([point])
                    })
                })
                .collect(),
        );
        let mut first_nodes = HashMap::new();
        for (node, &point) in endpoints.iter().flatten().enumerate() {
            if let Some(&first) = first_nodes.get(&point) {
                quotient
                    .merge_charged(ctx, first, node)?
                    .expect("same root");
            } else {
                first_nodes.insert(point, node);
            }
        }
        let solution = quotient
            .close_coordinate_roots_for_incidence_with_budget(
                ctx,
                2 + 2 * TRIANGLES,
                &candidates,
                MeshIncidenceBoundary {
                    edge_faces: &faces,
                    face_count: TRIANGLES,
                    domains: &boundaries,
                },
                None,
            )?
            .expect("each triangle closes at point one");
        for (&point, &node) in &first_nodes {
            assert_eq!(solution[&quotient.root(ctx, node)?], point);
        }
        Ok::<_, CodecError>(())
    };
    // The shared root completes two edges per triangle. Each edge adds two
    // degrees, so the rejected candidate has 4 * TRIANGLES increment keys.
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "catia_coordinate_closure_probe_degrees",
            Some(4 * cadmpeg_core::decode::u64_from_index(TRIANGLES)),
        );
        run(ctx).expect("no bulk increment-row admission");
        assert_eq!(ctx.resource_refusal(), None);
    });
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "catia_coordinate_closure_affected_roots",
            Some(2 + 2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>())),
        );
        run(ctx).expect("fixed root pairs have no copy-work charge");
        assert_eq!(ctx.resource_refusal(), None);
    });
    let result =
        crate::test_support::with_work_refusal("catia_coordinate_closure_degree_support", run);
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_coordinate_closure_degree_support"));
}

#[test]
fn coordinate_degree_support_rejects_first_point_without_admitting_suffix() {
    const BOUNDARIES: usize = 13;
    let alternate = BOUNDARIES + 2;
    let mut endpoints = Vec::new();
    let mut candidates = Vec::new();
    let mut faces = Vec::new();
    let mut boundaries = Vec::new();
    for face in 0..BOUNDARIES {
        let neighbor = face + 2;
        let first = candidates.len();
        endpoints.extend([[1, neighbor], [neighbor, alternate]]);
        candidates.extend([
            vec![[0, neighbor], [1, neighbor]],
            vec![[1, neighbor], [neighbor, alternate]],
        ]);
        faces.extend([[face, face]; 2]);
        boundaries.push(MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![(first..first + 2)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ]));
    }
    for point in [0, alternate] {
        let edge = candidates.len();
        let face = boundaries.len();
        endpoints.push([point, point]);
        candidates.push(vec![[point, point]]);
        faces.push([face, face]);
        boundaries.push(MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![vec![MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                }]],
            },
        ]));
    }
    let run = |ctx: &DecodeContext<'_>| {
        let mut quotient = MeshQuotient::new(
            endpoints
                .iter()
                .enumerate()
                .flat_map(|(edge, pair)| {
                    pair.iter().enumerate().map(move |(port, &point)| {
                        Arc::new(if edge < 2 * BOUNDARIES && edge % 2 == 1 && port == 1 {
                            HashSet::from([1, alternate])
                        } else if point == 1 {
                            HashSet::from([0, 1])
                        } else {
                            HashSet::from([point])
                        })
                    })
                })
                .collect(),
        );
        let mut shared_nodes = HashMap::new();
        for (edge, pair) in endpoints.iter().enumerate() {
            for (port, &point) in pair.iter().enumerate() {
                if edge < 2 * BOUNDARIES && edge % 2 == 1 && port == 1 {
                    continue;
                }
                let node = 2 * edge + port;
                if let Some(&first) = shared_nodes.get(&point) {
                    quotient
                        .merge_charged(ctx, first, node)?
                        .expect("shared endpoint");
                } else {
                    shared_nodes.insert(point, node);
                }
            }
        }
        let solution = quotient
            .close_coordinate_roots_for_incidence_with_budget(
                ctx,
                BOUNDARIES + 3,
                &candidates,
                MeshIncidenceBoundary {
                    edge_faces: &faces,
                    face_count: boundaries.len(),
                    domains: &boundaries,
                },
                None,
            )?
            .expect("each two-edge boundary closes at point one");
        for (edge, pair) in endpoints.iter().enumerate() {
            for (port, &point) in pair.iter().enumerate() {
                let expected = if edge < 2 * BOUNDARIES && edge % 2 == 1 && port == 1 {
                    1
                } else {
                    point
                };
                assert_eq!(solution[&quotient.root(ctx, 2 * edge + port)?], expected);
            }
        }
        Ok::<_, CodecError>(())
    };
    // Each shared-root edge touches one face and the domains {0,1} and
    // {neighbor}. The affected-point set has 3 * BOUNDARIES keys. Point 0
    // has degree one on face 0 and no incomplete supporting edge.
    crate::test_support::with_work_limit(u64::MAX, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "catia_coordinate_closure_degree_support",
            Some(3 * cadmpeg_core::decode::u64_from_index(BOUNDARIES)),
        );
        run(ctx).expect("no bulk affected-point admission");
        assert_eq!(ctx.resource_refusal(), None);
    });
}
