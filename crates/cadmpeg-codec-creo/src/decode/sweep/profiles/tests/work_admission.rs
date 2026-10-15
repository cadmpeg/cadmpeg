// SPDX-License-Identifier: Apache-2.0
use super::{
    sketch, CadIr, Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};

fn fixture() -> (CadIr, SketchId) {
    let sketch_id = SketchId::mint("creo:model:sketch#73").expect("sketch ID");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#73:1").expect("entity ID");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(1.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(2.0).expect("radius"),
    })
    .expect("circle");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model
        .sketch_entities
        .push(SketchEntity::new(entity_id, sketch_id.clone(), geometry));
    (ir, sketch_id)
}

#[test]
fn closed_circle_sketch_resolves_one_profile() {
    let (ir, sketch_id) = fixture();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::decode::with_service_context(|ctx| {
        super::super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1)
    })
    .expect("resolved profiles");
    assert_eq!(profiles.expect("closed circle").len(), 1);
}

#[test]
fn closed_circle_sketch_projects_connected_vertices() {
    let (ir, sketch_id) = fixture();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::decode::with_service_context(|ctx| {
        super::super::connected_sketch_profile_vertices(ctx, &ir, &carriers, &sketch_id)
            .map(Iterator::collect::<Vec<_>>)
    })
    .expect("connected profile vertices");
    assert_eq!(profiles, vec![(0, vec![[3.0, 0.0]])]);
}
