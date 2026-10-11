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
