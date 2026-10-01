// SPDX-License-Identifier: Apache-2.0
use super::{sketch, CadIr, Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId};

fn fixture() -> (CadIr, SketchId) {
    let sketch_id = SketchId::mint("creo:model:sketch#73").expect("sketch ID");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#73:1").expect("entity ID");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle { center: Point2::new(1.0, 0.0), radius: cadmpeg_ir::scalar::Length::new(2.0).expect("radius") }).expect("circle");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.push(SketchEntity::new(entity_id, sketch_id.clone(), geometry));
    (ir, sketch_id)
}

#[test]
fn resolved_profile_lookups_scale_and_closure_refuse_work() {
    let (ir, sketch_id) = fixture();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::test_support::assert_work_boundaries(&["creo profile sketch lookup", "creo profile entity lookup", "creo resolved profile row scan", "creo resolved profile scale", "creo resolved profile closure"], |ctx| super::super::resolved_sketch_profiles(ctx, &ir, &carriers, &sketch_id, 1));
    assert_eq!(profiles.expect("closed circle").len(), 1);
}

#[test]
fn connected_profile_lookups_connectivity_and_projection_refuse_work() {
    let (ir, sketch_id) = fixture();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let profiles = crate::test_support::assert_work_boundaries(&["creo profile sketch lookup", "creo profile entity lookup", "creo connected profile row scan", "creo connected profile scale", "creo connected profile closure", "creo connected profile projection"], |ctx| super::super::connected_sketch_profile_vertices(ctx, &ir, &carriers, &sketch_id).map(Iterator::collect::<Vec<_>>));
    assert_eq!(profiles, vec![(0, vec![[3.0, 0.0]])]);
}
