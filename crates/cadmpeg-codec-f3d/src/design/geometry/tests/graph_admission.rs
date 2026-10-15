// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::design::geometry::{
    arrangement_line_nurbs_meet_only_at_endpoint, SketchArrangementEdge,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn work_refusal_at<T>(
    operation: &'static str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> CodecError {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |maximum| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = maximum;
            crate::test_support::with_decode_policy(&policy, |ctx| run(ctx))
        },
    )
}

#[test]
fn closed_profile_graph_refuses_source_endpoint_and_component_scans() {
    let (sketch, entities, _, _) = coincident_circle_arc_arrangement();
    for operation in [
        "f3d closed profile entity scan",
        "f3d closed profile sketch comparison",
        "f3d closed profile endpoint edge scan",
        "f3d closed profile parent scan",
        "f3d closed profile endpoint cell scan",
        "f3d closed profile adjacent cell scan",
        "f3d closed profile edge index scan",
        "f3d closed profile adjacency edge scan",
        "f3d closed profile component start scan",
        "f3d closed profile component walk",
        "f3d closed profile adjacent edge scan",
        "f3d closed profile branch degree scan",
        "f3d closed profile next edge search",
        "f3d closed profile completion scan",
    ] {
        let error = work_refusal_at(operation, |ctx| {
            closed_sketch_profiles(ctx, &sketch.id, &entities, EPS_ARRANGEMENT_SESSION)
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
    }
}

#[test]
fn arrangement_construction_refuses_original_sources_and_graph_searches() {
    let (sketch, entities, _, _) = coincident_circle_arc_arrangement();
    for operation in [
        "f3d arrangement source profile scan",
        "f3d arrangement source profile member scan",
        "f3d arrangement candidate scan",
        "f3d arrangement node search",
        "f3d arrangement circle node scan",
        "f3d arrangement circle interval scan",
        "f3d arrangement pending scan",
        "f3d arrangement entity comparison",
        "f3d arrangement split interval scan",
        "f3d arrangement split scan",
        "f3d arrangement coincident edge scan",
        "f3d arrangement tube edge scan",
        "f3d arrangement tube scale scan",
        "f3d arrangement tube bounds scan",
        "f3d arrangement left intersection scan",
        "f3d arrangement right intersection scan",
        "f3d arrangement outgoing edge scan",
        "f3d arrangement outgoing degree scan",
        "f3d arrangement outgoing sort scan",
        "f3d arrangement face start scan",
        "f3d arrangement face walk",
        "f3d arrangement face point scan",
        "f3d arrangement twin search",
    ] {
        let error = work_refusal_at(operation, |ctx| {
            let budget = local_arrangement_budget();
            sketch_arrangement_faces(&sketch, &entities, EPS_ARRANGEMENT_SESSION, &budget, ctx)
                .map(|_| ())
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
    }
}

#[test]
fn line_nurbs_separation_refuses_weight_and_support_scans() {
    let sketch = SketchId::mint("synthetic:test:sketch#separation").unwrap();
    let line_id = SketchEntityId::mint("synthetic:test:entity#line-separation").unwrap();
    let nurbs_id = SketchEntityId::mint("synthetic:test:entity#nurbs-separation").unwrap();
    let entities = [
        SketchEntity::new(
            line_id.clone(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(-1.0, 0.0),
            })
            .unwrap(),
        ),
        SketchEntity::new(
            nurbs_id.clone(),
            sketch,
            SketchGeometry::nurbs(
                PcurveNurbs::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 1.0),
                        Point2::new(2.0, 1.0),
                    ],
                    Some(vec![1.0, 1.0, 1.0]),
                    false,
                )
                .unwrap()
                .unwrap(),
            ),
        ),
    ];
    let nodes = [
        Point2::new(0.0, 0.0),
        Point2::new(-1.0, 0.0),
        Point2::new(2.0, 1.0),
    ];
    let run = |ctx: &DecodeContext<'_>| {
        let edge = |entity, nodes| {
            Ok::<_, CodecError>(SketchArrangementEdge {
                nodes,
                boundary: super::super::ArrangementBoundary {
                    entity,
                    parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0])
                        .unwrap(),
                    reversed: false,
                },
                polyline: Vec::new(),
                _polyline_storage: ctx
                    .reserve_scoped(0, "test arrangement empty polyline storage")?,
            })
        };
        let line = edge(&line_id, [0, 1])?;
        let nurbs = edge(&nurbs_id, [0, 2])?;
        arrangement_line_nurbs_meet_only_at_endpoint(
            &line,
            &nurbs,
            &entities,
            &nodes,
            &[0],
            EPS_ARRANGEMENT_SESSION,
            ctx,
        )
    };
    assert!(crate::test_support::with_decode_context(run).unwrap());
    for operation in [
        "f3d arrangement entity scan",
        "f3d arrangement nurbs weight scan",
        "f3d arrangement first support scan",
        "f3d arrangement remaining support scan",
    ] {
        let error = work_refusal_at(operation, run);
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
    }
}

#[test]
fn rejected_branched_walks_do_not_copy_profile_ids() {
    let sketch = SketchId::mint("synthetic:test:sketch#zero-area").unwrap();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(2.0, 0.0),
    ];
    let entities: Vec<_> = (0..3)
        .map(|index| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#zero-area-{index}")).unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[index],
                    end: points[(index + 1) % 3],
                })
                .unwrap(),
            )
        })
        .collect();
    let edges: Vec<_> = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity, [points[index], points[(index + 1) % 3]]))
        .collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let profiles = super::super::branched_line_profiles(
            ctx,
            &[0, 1, 2],
            &edges,
            &[[0, 1], [1, 2], [2, 0]],
            EPS_ARRANGEMENT_SESSION,
        )
        .unwrap();
        assert!(profiles.is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}

fn tangent_notch_entities() -> (Vec<SketchEntity>, Vec<[usize; 2]>, [Point2; 7]) {
    let sketch = SketchId::mint("synthetic:test:sketch#tangent-notch").unwrap();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(0.0, 4.0),
        Point2::new(0.0, 1.0),
        Point2::new(1.0, 1.0),
        Point2::new(1.0, 0.0),
    ];
    let nodes = vec![
        [0, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [0, 4],
        [4, 5],
        [5, 6],
        [6, 0],
    ];
    let entities = nodes
        .iter()
        .enumerate()
        .map(|(index, [start, end])| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#tangent-notch-{index}"))
                    .unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[*start],
                    end: points[*end],
                })
                .unwrap(),
            )
        })
        .collect();
    (entities, nodes, points)
}

#[test]
fn tangent_cycle_storage_releases_after_profile_projection() {
    let (entities, nodes, points) = tangent_notch_entities();
    let edges: Vec<_> = entities
        .iter()
        .zip(&nodes)
        .map(|(entity, [start, end])| (entity, [points[*start], points[*end]]))
        .collect();
    let mut adjacency = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for (index, ends) in nodes.iter().enumerate() {
        for node in ends {
            adjacency.entry(*node).or_default().push(index);
        }
    }
    let component: Vec<_> = (0..edges.len()).collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1_048_576;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let (profile, output_storage) =
            ctx.with_scoped_storage("test tangent profile output", || {
                super::super::tangent_nested_line_profile(
                    ctx,
                    &component,
                    &edges,
                    &nodes,
                    &adjacency,
                    EPS_ARRANGEMENT_SESSION,
                )
            })?;
        let profile = profile.expect("two tangent cycles with opposite orientation");
        assert_eq!(profile.len(), 8);
        for (use_, entity) in profile.iter().zip(&entities) {
            assert_eq!(&use_.entity, entity.id());
            assert!(!use_.reversed);
        }
        drop((profile, output_storage));
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test tangent released storage",
        )?;
        assert!(ctx.resource_refusal().is_none());
        Ok::<_, CodecError>(())
    })
    .unwrap();
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "f3d tangent profile cycle edge",
        ),
        (
            ResourceDimension::CollectionItems,
            "f3d tangent profile cycle",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d tangent profile duplicate scan",
        ),
        (
            ResourceDimension::WorkUnits,
            "f3d tangent profile cycle comparison",
        ),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!(),
            }
            crate::test_support::with_decode_policy(&policy, |ctx| {
                super::super::tangent_nested_line_profile(
                    ctx,
                    &component,
                    &edges,
                    &nodes,
                    &adjacency,
                    EPS_ARRANGEMENT_SESSION,
                )
            })
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == dimension));
    }
}
