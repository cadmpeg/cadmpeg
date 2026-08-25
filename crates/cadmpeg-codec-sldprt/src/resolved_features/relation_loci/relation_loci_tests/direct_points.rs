use super::{dynamic_relation, length_parameter, line_entity, marker, typed_relation_definition};
use crate::records::{FeatureInputRelationFamily, SketchInputKind};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::HashMap;

fn point_entity(id: &str, sketch: &SketchId, native_ref: &str, position: Point2) -> SketchEntity {
    let mut entity = SketchEntity::new(
        SketchEntityId(id.into()),
        sketch.clone(),
        SketchGeometry::Point { position },
    );
    entity.native_ref = Some(native_ref.into());
    entity
}

#[test]
fn dynamic_point_distance_uses_direct_point_roster_when_ordinal_pair_misses() {
    let sketch = SketchId("sketch".into());
    let markers = [
        marker(
            "first-marker",
            0,
            0,
            SketchInputKind::Point,
            Some([0.0, 0.0]),
        ),
        marker(
            "wrong-marker",
            1,
            1,
            SketchInputKind::Point,
            Some([0.03, 0.0]),
        ),
        marker(
            "target-marker",
            2,
            2,
            SketchInputKind::Point,
            Some([0.01, 0.0]),
        ),
    ];
    let first = point_entity("first", &sketch, "first-marker", Point2::new(0.0, 0.0));
    let wrong = point_entity("wrong", &sketch, "wrong-marker", Point2::new(30.0, 0.0));
    let target = point_entity("target", &sketch, "target-marker", Point2::new(10.0, 0.0));
    let entities = vec![
        first.clone(),
        wrong,
        target.clone(),
        line_entity(
            "distractor-line",
            &sketch,
            Point2::new(0.0, 10.0),
            Point2::new(10.0, 10.0),
        ),
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let relation = dynamic_relation(FeatureInputRelationFamily::PointPointDistance, [0, 1]);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(10.0)),
            &sketch,
            &entities,
            &markers_by_id,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(first.id),
            second: SketchLocus::Entity(target.id),
            parameter: ParameterId("parameter".into()),
        })
    );
}
