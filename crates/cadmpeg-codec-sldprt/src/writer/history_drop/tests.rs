// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::HistoryDrop;

fn sketch_with_a_constrained_line() -> (cadmpeg_ir::CadIr, SketchEntityId, SketchConstraintId) {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch = SketchId::mint("other:model:sketch#profile").unwrap();
    let entity = SketchEntityId::mint("other:model:sketch-entity#line").unwrap();
    let constraint = SketchConstraintId::mint("other:model:constraint#horizontal").unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
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
            entity: entity.clone(),
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: constraint.clone(),
        sketch,
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Horizontal {
                entity: entity.clone(),
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    (ir, entity, constraint)
}

#[test]
fn a_failure_that_names_a_constraint_leaves_out_that_constraint_and_keeps_its_operands() {
    let (mut ir, entity, constraint) = sketch_with_a_constrained_line();
    let mut drop = HistoryDrop::default();
    drop.degrade(
        &mut ir,
        CodecError::NotImplemented(format!(
            "relation cannot identify entity {} with one unambiguous marker locus: sketch \
             constraint {}",
            entity.as_str(),
            constraint.as_str()
        )),
    )
    .unwrap();
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert!(ir.model.sketch_constraints.is_empty());
    let losses = drop.finish();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "writer.sketch-constraints-omitted"
    );
}
