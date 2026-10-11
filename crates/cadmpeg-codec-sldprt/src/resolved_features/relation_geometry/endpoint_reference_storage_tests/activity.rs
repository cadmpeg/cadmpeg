use super::{parameter, planar_fixture, EMPTY_INPUT_WORKSPACE_BYTES};
use crate::records::FeatureInputLane;
use crate::resolved_features::relation_geometry::project_relation_bindings;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, DimensionDisplay, Feature, ParameterValue};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition,
};

fn diameter_fixture(role: &str, diameter: f64) -> (Sketch, Feature, SketchEntity, DesignParameter, FeatureInputLane) {
    let (sketch, feature, lane) = planar_fixture();
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    parameter.display = Some(DimensionDisplay::Diameter);
    parameter.value = Some(ParameterValue::Length(Length::new(diameter).unwrap()));
    parameter.properties.insert(
        cadmpeg_core::nonblank_literal!("sldprt_relation_parameter_role"),
        role.into(),
    );
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:entity#circle").unwrap(),
        sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    )
    .with_geometry_ref(Some(lane.relation_instances[0].id.clone()));
    (sketch, feature, entity, parameter, lane)
}

fn matching_diameter(ctx: &DecodeContext<'_>, role: &str) -> Result<(), CodecError> {
    let (sketch, feature, entity, parameter, lane) = diameter_fixture(role, 2.0);
    let mut constraints = Vec::new();
    let expected = SketchConstraintDefinitionInput::Diameter {
        entity: entity.id().clone(),
        parameter: parameter.id.clone(),
    };
    project_relation_bindings(
        ctx,
        &mut constraints,
        &[sketch],
        &[feature],
        &[entity],
        &[parameter],
        &[lane],
    )?;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].definition.kind(), &expected);
    assert_eq!(constraints[0].active, None);
    Ok(())
}

#[test]
fn accepted_reference_diameter_checks_activity_once() {
    let total = |role| {
        let CodecError::ResourceLimit(limit) =
            crate::test_support::work_refusal_at("completed diameter projection", |ctx| {
                matching_diameter(ctx, role)?;
                ctx.charge_work(1, "completed diameter projection")
            })
        else {
            panic!("completed projection work boundary");
        };
        assert_eq!(limit.additional, 1);
        limit.used
    };
    // These role values have the same byte count. Both publish the same typed
    // constraint and require one activity lookup against the unchanged profile.
    assert_eq!(total("reference"), total("referencf"));
}

#[test]
fn rejected_reference_diameter_releases_tentative_identity_storage() {
    let (sketch, feature, entity, parameter, lane) = diameter_fixture("reference", 3.0);
    let existing = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:constraint#native").unwrap(),
        sketch: sketch.id.clone(),
        definition: SketchConstraintDefinition::try_from(SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::nonblank_literal!("synthetic-native"),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![entity.id().clone()],
            parameter: None,
            operands: Vec::new(),
        }).unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some(lane.relation_instances[0].id.clone()),
    };
    let mut constraints = vec![existing.clone()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = EMPTY_INPUT_WORKSPACE_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        project_relation_bindings(
            &ctx,
            &mut constraints,
            std::slice::from_ref(&sketch),
            std::slice::from_ref(&feature),
            std::slice::from_ref(&entity),
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&lane),
        ).unwrap();
        assert_eq!(constraints, [existing.clone()]);
        ctx.reserve_scoped(EMPTY_INPUT_WORKSPACE_BYTES, "reuse rejected diameter workspace")
            .unwrap();
    }
    ctx.finish_session().unwrap();
}
