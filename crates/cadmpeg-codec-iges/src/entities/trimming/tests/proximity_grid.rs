// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::scalar::PositiveReal;

fn cluster_error(error: BoundaryVertexCreationError) -> CodecError {
    match error {
        BoundaryVertexCreationError::Resource(error) => error,
        error => panic!("unexpected cluster error: {error:?}"),
    }
}

#[test]
fn boundary_proximity_grid_charges_cell_insertion_and_candidate_comparison() {
    let points = [Point3::new(-0.25, 0.0, 0.0), Point3::new(0.25, 0.0, 0.0)]
        .map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap());
    for operation in ["iges boundary proximity cell insertion", "iges boundary clustering comparisons"] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, operation, |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    cluster_boundary_positions(&points, PositiveReal::ONE, ctx)
                        .map_err(cluster_error)
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.additional == 1));
    }
}

#[test]
fn boundary_proximity_grid_refuses_index_storage_before_allocation() {
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)]
        .map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap());
    for operation in ["iges boundary proximity links", "iges boundary proximity cells"] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, operation, |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    cluster_boundary_positions(&points, PositiveReal::ONE, ctx)
                        .map_err(cluster_error)
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.additional == if operation == "iges boundary proximity links" {
                u64::try_from(2 * std::mem::size_of::<Option<usize>>()).unwrap()
            } else {
                // Core's growth bound for four entries uses eight buckets.
                u64::try_from(8 * (std::mem::size_of::<([i64; 3], usize)>() + 1) + 31).unwrap()
            }));
    }
}

fn polygon_clusters(count: u32) -> u64 {
    let vertices: Vec<_> = (0..count).map(|index| {
        let angle = std::f64::consts::TAU * f64::from(index) / f64::from(count);
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(
            f64::from(count) * angle.cos(), f64::from(count) * angle.sin(), 0.0,
        )).unwrap()
    }).collect();
    let endpoints: Vec<_> = (0..vertices.len()).flat_map(|index| {
        [vertices[index], vertices[(index + 1) % vertices.len()]]
    }).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Admit the unchanged tree grouping and sort as well as proximity work.
    policy.limits.max_work_units = 64_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let clusters = cluster_boundary_positions(&endpoints, PositiveReal::ONE, &ctx).unwrap();
    assert_eq!(clusters.len(), vertices.len());
    assert_eq!(clusters[0].members, [0, endpoints.len() - 1]);
    assert_eq!(clusters[0].representative, vertices[0]);
    for (index, cluster) in clusters.iter().enumerate().skip(1) {
        assert_eq!(cluster.members, [2 * index - 1, 2 * index]);
        assert_eq!(cluster.representative, vertices[index]);
    }
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(u64::MAX, "test clustering work total") else {
        panic!("the accounting probe exceeds the work budget");
    };
    limit.used
}

#[test]
fn boundary_proximity_grid_polygon_work_grows_below_quadratic() {
    let small = polygon_clusters(2048);
    let large = polygon_clusters(4096);
    assert!(large < 5 * small / 2, "doubling endpoints: {small} -> {large}");
}

#[test]
fn boundary_proximity_grid_preserves_neighbours_at_numeric_boundaries() {
    for (left, right, tolerance, joined) in [
        (-0.5, 0.5, 1.0, true),
        (0.0, 1.0, 1.0, true),
        (0.0, f64::from_bits(1.0_f64.to_bits() + 1), 1.0, false),
        (f64::MAX, f64::MAX, f64::MIN_POSITIVE, true),
        (-f64::MAX, f64::MAX, f64::MIN_POSITIVE, false),
        (0.0, f64::from_bits(1), f64::from_bits(1), true),
        (4_503_599_627_370_495.0, 4_503_599_627_370_496.0, 1.0, true),
    ] {
        let points = [left, right].map(|x|
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap());
        crate::test_support::with_service_context(&[], |ctx| {
            let clusters = cluster_boundary_positions(&points, PositiveReal::new(tolerance).unwrap(), ctx).unwrap();
            assert_eq!(clusters.len(), if joined { 1 } else { 2 });
            if joined {
                assert_eq!(clusters[0].members, [0, 1]);
                assert_eq!(clusters[0].representative, points[0]);
            }
        });
    }
}
