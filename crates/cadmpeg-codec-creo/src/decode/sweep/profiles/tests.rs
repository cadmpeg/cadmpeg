// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    SketchPlacement,
};

fn sketch(id: &SketchId, entity: &SketchEntityId) -> Sketch {
    Sketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: vec![vec![SketchEntityUse {
            entity: entity.clone(),
            reversed: false,
        }]],
        native_ref: None,
    }
}

fn line_entity(id: &SketchEntityId, sketch: &SketchId, end: [f64; 2]) -> SketchEntity {
    SketchEntity::new(
        id.clone(),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(end[0], end[1]),
        },
    )
}

#[test]
fn profile_joins_reject_duplicate_sketch_ids() {
    let sketch_id = SketchId("creo:model:sketch#7".to_string());
    let entity_id = SketchEntityId("creo:featdefs:sketch_entity#7:1".to_string());
    let mut ir = CadIr::empty();
    ir.model.sketches.extend([
        sketch(&sketch_id, &entity_id),
        sketch(&sketch_id, &entity_id),
    ]);
    ir.model
        .sketch_entities
        .push(line_entity(&entity_id, &sketch_id, [1.0, 0.0]));

    assert!(super::connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
    assert!(super::resolved_sketch_profiles(&ir, &sketch_id, 1).is_none());
}

#[test]
fn profile_joins_reject_duplicate_sketch_entity_ids() {
    let sketch_id = SketchId("creo:model:sketch#7".to_string());
    let entity_id = SketchEntityId("creo:featdefs:sketch_entity#7:1".to_string());
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.extend([
        line_entity(&entity_id, &sketch_id, [1.0, 0.0]),
        line_entity(&entity_id, &sketch_id, [0.0, 1.0]),
    ]);

    assert!(super::connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
    assert!(super::resolved_sketch_profiles(&ir, &sketch_id, 1).is_none());
}
