// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, unique_point_class_dimension_definition,
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("f3d:model:sketch#point-classes").unwrap();
    let point = |name: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let lower = point("lower", -53.0, -20.875);
    let lower_duplicate = point("lower-duplicate", -53.0, -20.875 + 5.0e-7);
    let upper = point("upper", -53.0, -7.875);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "13 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d19",
        1.3,
    ))
    .expect("linear parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#point-classes")
            .expect("identity grammar");

    let separated = point("separated", 0.0, 1.5 * EPS_REFUSAL_LINEAR);
    let origin = point("origin", 0.0, 0.0);
    let bridge = point("bridge", 0.0, 0.75 * EPS_REFUSAL_LINEAR);
    let selected = if operation == "f3d point class merged member" {
        vec![origin, separated, bridge, upper.clone()]
    } else {
        vec![lower, lower_duplicate, upper]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        unique_point_class_dimension_definition(
            ctx,
            &selected,
            &sketch,
            &parameter,
            &parameter_id,
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn point_class_candidate_refuses_collection_limit() {
    fixture(
        "f3d point class candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn point_class_match_refuses_collection_limit() {
    fixture("f3d point class match", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_member_refuses_collection_limit() {
    fixture("f3d point class member", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_refuses_collection_limit() {
    fixture("f3d point class", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_merged_member_refuses_collection_limit() {
    fixture(
        "f3d point class merged member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn unique_point_class_first_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_point_class_second_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_point_class_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn point_class_merges_preserve_first_representatives_across_retired_slots() {
    let sketch = SketchId::mint("synthetic:test:sketch#merged-classes").unwrap();
    let point = |index: u32, u: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:sketch-entity#{index}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
    };
    let entities = [
        point(0, 0.0),
        point(1, 1.5 * EPS_REFUSAL_LINEAR),
        point(2, 0.75 * EPS_REFUSAL_LINEAR),
        point(3, 10.0),
        point(4, 10.0 + 1.5 * EPS_REFUSAL_LINEAR),
        point(5, 10.0 + 0.75 * EPS_REFUSAL_LINEAR),
    ];
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "10 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        1.0,
    ))
    .unwrap();
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#merged-classes").unwrap();
    let definition = crate::test_support::with_decode_context(|ctx| {
        unique_point_class_dimension_definition(
            ctx,
            &entities,
            &sketch,
            &parameter,
            &parameter_id,
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
    })
    .unwrap()
    .unwrap();
    assert!(matches!(definition,
        cadmpeg_ir::sketches::SketchConstraintDefinitionInput::HorizontalDistance {
            first: cadmpeg_ir::sketches::SketchLocus::Entity(first),
            second: cadmpeg_ir::sketches::SketchLocus::Entity(second),
            parameter,
        } if first == *entities[0].id() && second == *entities[3].id() && parameter == parameter_id));
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(unique_point_class_dimension_definition(
            ctx,
            &entities[..3],
            &sketch,
            &parameter,
            &parameter_id,
            EPS_REFUSAL_LINEAR,
        )
        .is_none());
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released point classes",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn point_class_queries_refuse_actual_source_work() {
    for operation in [
        "f3d point class source",
        "f3d point class sketch identity",
        "f3d point class candidate source",
        "f3d point class match source",
        "f3d point class member search",
        "f3d point class pair source",
    ] {
        fixture(operation, ResourceDimension::WorkUnits);
    }
}
