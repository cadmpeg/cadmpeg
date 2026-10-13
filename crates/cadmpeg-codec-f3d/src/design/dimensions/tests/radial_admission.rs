// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    owner_scoped_radial_dimension_definition, radial_locus_dimension_definition,
};
use crate::design::test_support::parameter_record;

fn entity(index: u32, circle: bool) -> SketchEntity {
    let position = Point2::new(2.0, 3.0);
    let definition = if circle {
        SketchGeometryDefinition::Circle {
            center: position,
            radius: Length::new(5.0).unwrap(),
        }
    } else {
        SketchGeometryDefinition::Point { position }
    };
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:radial-entity#{index}")).unwrap(),
        SketchId::mint("synthetic:test:sketch#radial-admission").unwrap(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}

fn parameter() -> ParameterId {
    ParameterId::mint("synthetic:test:parameter#radial-admission").unwrap()
}

fn rejected_owner(circle: bool, evaluated: f64) {
    let entity = entity(0, circle);
    let parameter_id = parameter();
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "5 mm",
        "Radius Dimension-2",
        Some("mm"),
        "d1",
        evaluated,
    ))
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(owner_scoped_radial_dimension_definition(
            ctx,
            std::slice::from_ref(&entity),
            &entity.sketch,
            &parameter,
            &parameter_id,
            0.0,
        )
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn owner_radial_rejected_measurement_does_not_copy_ids() {
    rejected_owner(true, 0.7);
}

#[test]
fn owner_radial_unsupported_geometry_does_not_copy_ids() {
    rejected_owner(false, 0.5);
}

fn rejected_locus(circle: bool, evaluated: f64) {
    let entity = entity(0, circle);
    let parameter = parameter();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(radial_locus_dimension_definition(
            ctx,
            &[&entity],
            &[],
            "Radius Dimension-2",
            evaluated,
            &parameter,
        )
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn radial_locus_rejected_measurement_does_not_copy_ids() {
    rejected_locus(true, 0.7);
}

#[test]
fn radial_locus_unsupported_geometry_does_not_copy_ids() {
    rejected_locus(false, 0.5);
}

#[test]
fn radial_locus_duplicate_ids_release_scratch_without_retained_copies() {
    let entity = entity(0, true);
    let parameter = parameter();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1 << 20;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(radial_locus_dimension_definition(
            ctx,
            &[&entity, &entity],
            &[],
            "Radius Dimension-2",
            0.5,
            &parameter,
        )
        .is_none());
        let _released = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "released radial scratch",
            )
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn radial_center_ambiguity_stops_at_second_match_without_copying_ids() {
    let center = entity(0, false);
    let entities = [entity(1, true), entity(2, true), entity(3, true)];
    let parameter = parameter();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d radial center witness source",
        1,
        |ctx| {
            radial_locus_dimension_definition(
                ctx,
                &[&center],
                &entities,
                "Radius Dimension-2",
                0.5,
                &parameter,
            )
            .transpose()
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected work refusal");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = limit.used + limit.additional;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(radial_locus_dimension_definition(
            ctx,
            &[&center],
            &entities,
            "Radius Dimension-2",
            0.5,
            &parameter,
        )
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn radial_locus_repeated_definition_copies_one_parameter() {
    let entities = [entity(0, true), entity(1, true)];
    let parameter = parameter();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(
        parameter.as_str().len()
        + entities[0].id().as_str().len()
        + entities[1].id().as_str().len()
        // Core amortized growth keeps four initial slots for this ID width.
        + 4 * std::mem::size_of::<SketchEntityId>(),
    )
    .unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = radial_locus_dimension_definition(
            ctx,
            &[&entities[0], &entities[1]],
            &[],
            "Radius Dimension-2",
            0.5,
            &parameter,
        )
        .transpose()
        .unwrap()
        .unwrap();
        assert!(
            matches!(definition, SketchConstraintDefinitionInput::RepeatedRadius {
            entities: ref actual, parameter: ref actual_parameter,
        } if actual == &vec![entities[0].id().clone(), entities[1].id().clone()]
            && actual_parameter == &parameter)
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn owner_radial_repeated_definition_copies_one_parameter() {
    let entities = [entity(0, true), entity(1, true)];
    let parameter_id = parameter();
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "5 mm",
        "Radius Dimension-2",
        Some("mm"),
        "d1",
        0.5,
    ))
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(
        parameter_id.as_str().len()
        + entities[0].id().as_str().len()
        + entities[1].id().as_str().len()
        // Core amortized growth keeps four initial slots for this ID width.
        + 4 * std::mem::size_of::<SketchEntityId>(),
    )
    .unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = owner_scoped_radial_dimension_definition(
            ctx,
            &entities,
            &entities[0].sketch,
            &parameter,
            &parameter_id,
            0.0,
        )
        .transpose()
        .unwrap()
        .unwrap();
        assert!(
            matches!(definition, SketchConstraintDefinitionInput::RepeatedRadius {
            entities: ref actual, parameter: ref actual_parameter,
        } if actual == &vec![entities[0].id().clone(), entities[1].id().clone()]
            && actual_parameter == &parameter_id)
        );
        assert!(ctx.resource_refusal().is_none());
    });
}
