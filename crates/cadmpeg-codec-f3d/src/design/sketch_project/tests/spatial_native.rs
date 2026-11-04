// SPDX-License-Identifier: Apache-2.0
use super::*;

pub(super) fn assert_native_fallback(
    placement: &DesignSketchPlacement,
    relations: &[SketchRelation],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) {
    let mut native_relation = relations[0].clone();
    native_relation.definition =
        crate::records::sketch_relations::SketchRelationDefinition::new(0, None)
            .expect("valid relation definition");
    let native_constraints = project_spatial_sketch_constraints(
        std::slice::from_ref(placement),
        &[native_relation],
        points,
        curves,
        surfaces,
        entities,
    );
    assert!(matches!(
        native_constraints[0].definition.kind(),
        cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Native {
            native_kind, native_state: Some(0), operands, ..
        } if native_kind.as_str() == "coincident" && operands.len() == 4
    ));
}
