// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_core::CodecError;

fn triangle_topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 40,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(0.0, 1.0, 0.0),
            },
        ],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 20,
            carrier: Some(60),
        }],
        coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 12,
            carrier: Some(70),
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3],
        }],
        shell_faces: vec![AsmHistoricalRelation {
            owner_ref: 3,
            member_refs: vec![10],
        }],
        ..AsmHistoricalTopology::default()
    }
}

fn assert_historical_refusal(kind: AsmHistoricalEntityKind, id: i64, operation: &'static str) {
    let topology = triangle_topology();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                super::super::historical_entity_positions(kind, id, &topology, ctx).map(|_| ())
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation
            && failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

macro_rules! historical_refusal {
    ($name:ident, $kind:expr, $id:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert_historical_refusal($kind, $id, $operation);
        }
    };
}

historical_refusal!(
    historical_edge_ref_refuses_limit,
    AsmHistoricalEntityKind::Edge,
    20,
    "f3d historical direct edge reference"
);
historical_refusal!(
    historical_coedge_ref_refuses_limit,
    AsmHistoricalEntityKind::Coedge,
    12,
    "f3d historical coedge edge reference"
);
historical_refusal!(
    historical_curve_ref_refuses_limit,
    AsmHistoricalEntityKind::Curve,
    60,
    "f3d historical curve edge reference"
);
historical_refusal!(
    historical_loop_ref_refuses_limit,
    AsmHistoricalEntityKind::Loop,
    11,
    "f3d historical loop edge reference"
);
historical_refusal!(
    historical_pcurve_ref_refuses_limit,
    AsmHistoricalEntityKind::Pcurve,
    70,
    "f3d historical pcurve edge reference"
);
historical_refusal!(
    historical_vertex_position_refuses_limit,
    AsmHistoricalEntityKind::Vertex,
    30,
    "f3d historical vertex position"
);
historical_refusal!(
    historical_point_position_refuses_limit,
    AsmHistoricalEntityKind::Point,
    40,
    "f3d historical point position"
);
historical_refusal!(
    historical_edge_position_refuses_limit,
    AsmHistoricalEntityKind::Edge,
    20,
    "f3d historical edge position"
);
historical_refusal!(
    historical_face_position_refuses_limit,
    AsmHistoricalEntityKind::Face,
    10,
    "f3d historical face point"
);
historical_refusal!(
    historical_surface_position_refuses_limit,
    AsmHistoricalEntityKind::Surface,
    50,
    "f3d historical surface position"
);
historical_refusal!(
    historical_owned_face_refuses_limit,
    AsmHistoricalEntityKind::Body,
    1,
    "f3d historical owned face"
);
historical_refusal!(
    historical_owned_face_position_refuses_limit,
    AsmHistoricalEntityKind::Body,
    1,
    "f3d historical owned face position"
);

#[test]
fn historical_face_point_refuses_collection_limit() {
    let topology = triangle_topology();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "f3d historical face point",
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                crate::design::profile_select::historical_face_points(10, &topology, ctx)
                    .map(|_| ())
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == "f3d historical face point"
            && failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn historical_vertex_index_preserves_binding_order_and_first_point() {
    let mut topology = triangle_topology();
    topology.point_positions.push(AsmHistoricalPoint {
        point: 40,
        position: Point3::new(9.0, 9.0, 9.0),
    });
    topology.vertex_points.push(AsmHistoricalCarrierBinding {
        entity: 30,
        carrier: 42,
    });
    topology.vertex_points.push(AsmHistoricalCarrierBinding {
        entity: 30,
        carrier: 999,
    });
    crate::test_support::with_decode_context(|ctx| {
        let vertex = super::super::historical_entity_positions(
            AsmHistoricalEntityKind::Vertex,
            30,
            &topology,
            ctx,
        )?;
        assert_eq!(
            vertex.as_ref().map(AsRef::as_ref),
            Some([Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)].as_slice())
        );
        let edge = super::super::historical_entity_positions(
            AsmHistoricalEntityKind::Edge,
            20,
            &topology,
            ctx,
        )?;
        assert_eq!(
            edge.as_ref().map(AsRef::as_ref),
            Some(
                [
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0)
                ]
                .as_slice()
            )
        );
        let face = crate::design::profile_select::historical_face_points(10, &topology, ctx)?;
        assert_eq!(
            face.as_ref().map(AsRef::as_ref),
            Some(
                [
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(2.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0)
                ]
                .as_slice()
            )
        );
        Ok::<_, CodecError>(())
    })
    .unwrap();
}

#[test]
fn historical_queries_release_scratch_with_no_retained_allowance() {
    let mut topology = triangle_topology();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1_048_576;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let points = super::super::historical_entity_positions(
            AsmHistoricalEntityKind::Body,
            1,
            &topology,
            ctx,
        )?;
        assert!(points.is_some());
        drop(points);
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "historical test released storage",
        )?;
        Ok::<_, CodecError>(())
    })
    .unwrap();
    topology.point_positions.pop();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(super::super::historical_entity_positions(
            AsmHistoricalEntityKind::Body,
            1,
            &topology,
            ctx,
        )?
        .is_none());
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "historical test incomplete storage",
        )?;
        Ok::<_, CodecError>(())
    })
    .unwrap();
}

#[test]
fn historical_queries_refuse_source_index_and_early_search_work() {
    let topology = triangle_topology();
    for (kind, id, operations) in [
        (
            AsmHistoricalEntityKind::Edge,
            20,
            &[
                "f3d historical point index",
                "f3d historical vertex binding scan",
                "f3d historical point lookup",
                "f3d historical vertex index entry",
                "f3d historical edge index",
                "f3d historical edge reference scan",
                "f3d historical edge lookup",
                "f3d historical start vertex lookup",
                "f3d historical end vertex lookup",
                "f3d historical edge position scan",
            ][..],
        ),
        (
            AsmHistoricalEntityKind::Body,
            1,
            &[
                "f3d historical owner relation search",
                "f3d historical duplicate relation search",
                "f3d historical body region scan",
                "f3d historical region shell scan",
                "f3d historical shell face scan",
                "f3d historical owned face scan",
                "f3d historical face loop search",
                "f3d historical loop coedge index",
                "f3d historical coedge index",
                "f3d historical face edge index",
                "f3d historical face vertex index",
                "f3d historical face point index",
                "f3d historical face loop scan",
                "f3d historical loop coedge lookup",
                "f3d historical face coedge scan",
                "f3d historical coedge lookup",
                "f3d historical face edge lookup",
                "f3d historical face vertex lookup",
                "f3d historical face point lookup",
                "f3d historical face point duplicate search",
                "f3d historical owned position scan",
            ][..],
        ),
    ] {
        for operation in operations {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                |limit| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = limit;
                    crate::test_support::with_decode_policy(&policy, |ctx| {
                        super::super::historical_entity_positions(kind, id, &topology, ctx)
                            .map(|_| ())
                    })
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(failure)
                if failure.operation == *operation
                    && failure.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    }
}

#[test]
fn historical_point_run_keeps_storage_until_drop() {
    let topology = triangle_topology();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1_048_576;
    for (kind, id) in [
        (AsmHistoricalEntityKind::Face, 10),
        (AsmHistoricalEntityKind::Body, 1),
    ] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let points = super::super::historical_entity_positions(kind, id, &topology, ctx)?
                .expect("complete triangle topology");
            assert_eq!(points.as_ref(), [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)]);
            let error = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
                "historical live point storage").expect_err("point buffer is live");
            assert!(matches!(error, CodecError::ResourceLimit(failure)
                if failure.operation == "historical live point storage"
                    && failure.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
            Ok::<_, CodecError>(())
        }).unwrap();
    }
}

#[test]
fn historical_missing_vertex_and_edge_skip_the_point_index() {
    let topology = triangle_topology();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for kind in [AsmHistoricalEntityKind::Vertex, AsmHistoricalEntityKind::Edge] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "f3d historical point index",
                None,
            );
            let points = super::super::historical_entity_positions(kind, 999, &topology, ctx)?;
            assert!(points.is_none());
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, CodecError>(())
        })
        .unwrap();
    }
}
