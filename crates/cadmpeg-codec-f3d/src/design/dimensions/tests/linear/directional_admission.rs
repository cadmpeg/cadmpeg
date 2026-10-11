// SPDX-License-Identifier: Apache-2.0
use super::{
    directional_point_dimension, Point2, SketchConstraintDefinitionInput, SketchEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::features::ParameterId;

fn entity(name: &str, geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:directional-entity#{name}")).unwrap(),
        SketchId::mint("synthetic:test:sketch#directional-admission").unwrap(),
        SketchGeometry::try_from(geometry).unwrap(),
    )
}

fn point(name: &str, u: f64, v: f64) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Point {
            position: Point2::new(u, v),
        },
    )
}

fn parameter() -> ParameterId {
    ParameterId::mint("synthetic:test:parameter#directional-admission").unwrap()
}

fn rejected(entities: &[&SketchEntity], expected: f64) {
    let parameter = parameter();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(directional_point_dimension(
            ctx,
            entities,
            expected,
            (&parameter, "f3d directional point dimension parameter id"),
            0.0,
        )
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn directional_rejected_measurement_does_not_copy_ids() {
    let first = point("first", 4.0, 16.0);
    let second = point("second", 4.0, 14.0);
    rejected(&[&first, &second], 3.0);
}

#[test]
fn directional_ambiguous_axes_do_not_copy_ids() {
    let first = point("first", 4.0, 16.0);
    let second = point("second", 6.0, 18.0);
    rejected(&[&first, &second], 2.0);
}

#[test]
fn directional_nonpoint_geometry_does_not_copy_ids() {
    let first = point("first", 4.0, 16.0);
    let second = entity(
        "line",
        SketchGeometryDefinition::Line {
            start: Point2::new(4.0, 16.0),
            end: Point2::new(4.0, 14.0),
        },
    );
    rejected(&[&first, &second], 2.0);
}

#[test]
fn directional_wrong_cardinality_does_not_copy_ids() {
    let first = point("first", 4.0, 16.0);
    rejected(&[&first], 2.0);
}

fn selected(horizontal: bool) {
    let first = point("first", 4.0, 16.0);
    let second = if horizontal {
        point("second", 6.0, 16.0)
    } else {
        point("second", 4.0, 14.0)
    };
    let parameter = parameter();
    let bytes = u64::try_from(
        first.id().as_str().len() + second.id().as_str().len() + parameter.as_str().len(),
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = bytes;
    policy.limits.max_materialized_bytes = bytes;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = directional_point_dimension(
            ctx,
            &[&first, &second],
            2.0,
            (&parameter, "f3d directional point dimension parameter id"),
            0.0,
        )
        .transpose()
        .unwrap()
        .unwrap();
        let (actual_first, actual_second, actual_parameter) = match definition {
            SketchConstraintDefinitionInput::HorizontalDistance {
                first,
                second,
                parameter,
            } if horizontal => (first, second, parameter),
            SketchConstraintDefinitionInput::VerticalDistance {
                first,
                second,
                parameter,
            } if !horizontal => (first, second, parameter),
            _ => panic!("expected the uniquely measured direction"),
        };
        assert!(
            matches!(actual_first, cadmpeg_ir::sketches::SketchLocus::Entity(ref id) if id == first.id())
        );
        assert!(
            matches!(actual_second, cadmpeg_ir::sketches::SketchLocus::Entity(ref id) if id == second.id())
        );
        assert_eq!(actual_parameter, parameter);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn directional_horizontal_copies_only_the_selected_ids() {
    selected(true);
}

#[test]
fn directional_vertical_copies_only_the_selected_ids() {
    selected(false);
}
