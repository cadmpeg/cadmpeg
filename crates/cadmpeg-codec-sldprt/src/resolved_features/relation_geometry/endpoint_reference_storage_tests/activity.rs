use super::{parameter, planar_fixture};
use crate::resolved_features::relation_geometry::project_relation_bindings;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DimensionDisplay, ParameterValue};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition,
};

fn matching_diameter(ctx: &DecodeContext<'_>, role: &str) -> Result<(), CodecError> {
    let (sketch, feature, lane) = planar_fixture();
    let mut parameter = parameter();
    parameter.owner = Some(feature.id.clone());
    parameter.display = Some(DimensionDisplay::Diameter);
    parameter.value = Some(ParameterValue::Length(Length::new(2.0).unwrap()));
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
