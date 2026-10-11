use super::{dynamic_relation, length_parameter, typed_relation_definition};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{FeatureInputOperandKind, FeatureInputRelationFamily};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use std::collections::HashMap;

#[test]
fn solver_point_definition_retains_only_published_identities() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);
    for operand in &mut relation.operands {
        operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::TAG_8100);
    }
    let parameter = length_parameter(7.0);
    let entities = [
        ("synthetic:test:id#first", 0, 0.0),
        ("synthetic:test:id#second", 1, 7.0),
    ]
    .map(|(id, operand, u)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, 0.0),
            })
            .unwrap(),
        )
        .with_geometry_ref(Some(format!("relation:operand:{operand}")))
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = u64_from_index(
        parameter.id.as_str().len()
            + entities[0].id().as_str().len()
            + entities[1].id().as_str().len(),
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let definition = typed_relation_definition(
        &ctx,
        &relation,
        Some(&parameter),
        &sketch,
        &entities,
        &HashMap::new(),
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(
        definition,
        Some(SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Entity(entities[0].id().clone()),
            second: SketchLocus::Entity(entities[1].id().clone()),
            parameter: parameter.id.clone(),
        })
    );
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "reuse solver definition scratch",
    )
    .unwrap();
    ctx.finish_session().unwrap();
}
