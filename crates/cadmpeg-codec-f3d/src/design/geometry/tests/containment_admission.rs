// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::design::geometry::profile_loops_are_independent;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

const EPS_CONTAINMENT_ADMISSION: f64 = 1.0e-6;

fn square(size: f64) -> Vec<Point2> {
    vec![
        Point2::new(-size, -size),
        Point2::new(size, -size),
        Point2::new(size, size),
        Point2::new(-size, size),
    ]
}

#[test]
fn containment_predicates_refuse_before_ray_and_intersection_visits() {
    let outer = ProfileBoundary::Polygon(square(3.0));
    let inner = ProfileBoundary::Polygon(square(1.0));
    for operation in [
        "f3d polygon inner point scan",
        "f3d polygon ray edge scan",
        "f3d polygon outer intersection scan",
        "f3d polygon inner intersection scan",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| outer.strictly_contains(&inner, ctx),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
    }
}

#[test]
fn containment_rejection_stops_before_intersection_work() {
    let outer = ProfileBoundary::Polygon(square(1.0));
    let inner = ProfileBoundary::Polygon(square(3.0));
    let point = Point2::new(-3.0, -3.0);
    let mut policy = DecodePolicy::service();
    // One point visit, four polygon edges, and the final iterator probe decide the rejection.
    policy.limits.max_work_units = 6;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!outer.strictly_contains(&inner, ctx).unwrap());
        assert!(ctx.resource_refusal().is_none());
    });
    assert!(!crate::test_support::with_decode_context(|ctx| {
        super::super::point_in_polygon(point, &square(1.0), ctx)
    })
    .unwrap());
}

fn circle_fixture() -> (Sketch, Vec<SketchEntity>) {
    let sketch_id = SketchId::mint("synthetic:test:sketch#containment-budget").unwrap();
    let entity_id = SketchEntityId::mint("synthetic:test:entity#containment-budget").unwrap();
    let entity = SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    (sketch, vec![entity])
}

#[test]
fn region_selection_refuses_at_source_and_key_scans() {
    let (sketch, entities) = circle_fixture();
    for operation in [
        "f3d profile boundary scan",
        "f3d profile entity scan",
        "f3d profile entity comparison",
        "f3d profile outer boundary scan",
        "f3d profile inner boundary scan",
        "f3d profile source point scan",
        "f3d profile incidence point scan",
        "f3d profile incidence profile scan",
        "f3d profile incidence use scan",
        "f3d profile incidence entity scan",
        "f3d profile incidence entity comparison",
        "f3d profile closure boundary scan",
        "f3d immediate hole candidate scan",
        "f3d region closure point scan",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                region_containing_points(
                    &sketch,
                    &entities,
                    &[Point3::new(0.5, 0.0, 0.0)],
                    EPS_CONTAINMENT_ADMISSION,
                    ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == operation && limit.dimension == ResourceDimension::WorkUnits));
    }
}

#[test]
fn independent_profile_boundaries_use_only_scoped_storage() {
    let (sketch, entities) = circle_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(crate::test_support::with_decode_policy(&policy, |ctx| {
        profile_loops_are_independent(&sketch, &entities, &[0], EPS_CONTAINMENT_ADMISSION, ctx)
    })
    .unwrap());
    assert!(crate::test_support::with_decode_policy(&policy, |ctx| {
        region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(0.5, 0.0, 0.0)],
            EPS_CONTAINMENT_ADMISSION,
            ctx,
        )
    })
    .unwrap()
    .is_some());
}

#[test]
fn rejected_line_boundary_releases_storage_before_arc_fallback() {
    let sketch = SketchId::mint("synthetic:test:sketch#arc-fallback").unwrap();
    let points = [
        Point2::new(0.0, -1.0),
        Point2::new(1.0, -1.0),
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 1.0),
    ];
    let mut entities: Vec<_> = (0..3)
        .map(|index| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#fallback-line-{index}"))
                    .unwrap(),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[index],
                    end: points[index + 1],
                })
                .unwrap(),
            )
        })
        .collect();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:entity#fallback-arc").unwrap(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(3.0 * std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    ));
    let profile: Vec<_> = entities
        .iter()
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        })
        .collect();
    let mut arc_first = profile.clone();
    arc_first.rotate_right(1);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "f3d circular arc profile segment",
        |maximum| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = maximum;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                super::super::profile_boundary(
                    &arc_first,
                    &entities,
                    EPS_CONTAINMENT_ADMISSION,
                    ctx,
                )
                .map(|_| ())
            })
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected arc storage refusal");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = limit.used.checked_add(limit.additional).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let boundary =
            super::super::profile_boundary(&profile, &entities, EPS_CONTAINMENT_ADMISSION, ctx)
                .unwrap()
                .unwrap();
        assert!(
            matches!(&boundary.0, super::super::ProfileBoundary::CircularArcLoop(segments)
            if segments.len() == 4)
        );
        assert!(boundary
            .0
            .contains_point(Point2::new(0.5, 0.0), ctx)
            .unwrap());
        drop(boundary);
        let released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test profile storage release",
            )
            .unwrap();
        drop(released);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn region_ambiguity_stops_after_two_incident_profiles() {
    let (mut sketch, original) = circle_fixture();
    let entities: Vec<_> = (0..3)
        .map(|index| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:entity#incident-circle-{index}"))
                    .unwrap(),
                sketch.id.clone(),
                original[0].geometry.clone(),
            )
        })
        .collect();
    sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(
        entities
            .iter()
            .map(|entity| {
                vec![SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                }]
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let points = [Point3::new(2.0, 0.0, 0.0)];
    let run = |maximum| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = maximum;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            region_containing_points(&sketch, &entities, &points, EPS_CONTAINMENT_ADMISSION, ctx)
        })
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "f3d profile closure match",
        run,
    );
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected first candidate refusal");
    };
    let error = run(first.used.checked_add(first.additional).unwrap()).unwrap_err();
    let CodecError::ResourceLimit(second) = error else {
        panic!("expected second candidate refusal");
    };
    assert_eq!(second.dimension, ResourceDimension::CollectionItems);
    assert_eq!(second.operation, "f3d profile closure match");
    assert!(run(second.used.checked_add(second.additional).unwrap())
        .unwrap()
        .is_none());
}
