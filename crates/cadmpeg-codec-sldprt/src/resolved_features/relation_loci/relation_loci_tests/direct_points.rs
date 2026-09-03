use super::{dynamic_relation, length_parameter, line_entity, marker, typed_relation_definition};
use crate::records::{
    FeatureInputOperandKind, FeatureInputRelationFamily, SketchInputKind, SketchInputLink,
};
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
            first: SketchLocus::Entity(first.id().clone()),
            second: SketchLocus::Entity(target.id().clone()),
            parameter: ParameterId("parameter".into()),
        })
    );
}

#[test]
fn dynamic_point_line_uses_roster_line_when_point_is_explicit() {
    let sketch = SketchId("sketch".into());
    let point_marker = marker(
        "point-marker",
        0,
        0,
        SketchInputKind::Point,
        Some([0.0, 0.0]),
    );
    let point = point_entity("point", &sketch, "point-marker", Point2::new(0.0, 0.0));
    let line = line_entity(
        "line",
        &sketch,
        Point2::new(-10.0, 2.0),
        Point2::new(10.0, 2.0),
    );
    let markers_by_id = HashMap::from([(point_marker.id.as_str(), &point_marker)]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointLineDistance, [0, 9]);
    relation.operands[0].entity_ref = Some(point_marker.id.clone());

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(2.0)),
            &sketch,
            &[point.clone(), line.clone()],
            &markers_by_id,
            &HashMap::new(),
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id().clone()),
            second: SketchLocus::Entity(line.id().clone()),
            parameter: ParameterId("parameter".into()),
        })
    );
}

#[test]
fn qualified_point_operand_uses_unique_linked_point_carrier() {
    let sketch = SketchId("sketch".into());
    let point_marker = marker(
        "point-marker",
        0,
        0,
        SketchInputKind::Point,
        Some([0.0, 0.001]),
    );
    let mut arc_marker = marker("arc-marker", 1, 1, SketchInputKind::Arc, None);
    let arc_marker_id = arc_marker.id.clone();
    arc_marker.links.push(SketchInputLink {
        local_id: 15,
        entity_ref: point_marker.id.clone(),
    });
    let line_marker = marker("line-marker", 2, 2, SketchInputKind::LineOrCircle, None);
    let point = point_entity("point", &sketch, &point_marker.id, Point2::new(0.0, 1.0));
    let line = line_entity(
        "line",
        &sketch,
        Point2::new(-10.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let mut qualified_proxy = SketchEntity::new(
        SketchEntityId("qualified-proxy".into()),
        sketch.clone(),
        SketchGeometry::Point {
            position: Point2::new(0.0, 100.0),
        },
    );
    qualified_proxy.geometry_ref = Some(arc_marker_id.clone());
    let markers = [
        point_marker.clone(),
        arc_marker.clone(),
        line_marker.clone(),
    ];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci_by_marker = HashMap::from([
        (
            point_marker.id.clone(),
            vec![SketchLocus::Entity(point.id().clone())],
        ),
        (
            line_marker.id.clone(),
            vec![SketchLocus::Entity(line.id().clone())],
        ),
        (
            super::super::qualified_point_marker_key(&arc_marker_id),
            vec![SketchLocus::Entity(qualified_proxy.id().clone())],
        ),
    ]);
    let mut relation = dynamic_relation(FeatureInputRelationFamily::PointLineDistance, [0, 1]);
    relation.operands[0].kind = FeatureInputOperandKind::Native(0x837b);
    relation.operands[0].entity_ref = Some(arc_marker.id);
    relation.operands[1].kind = FeatureInputOperandKind::Native(0x8386);
    relation.operands[1].entity_ref = Some(line_marker.id);

    assert_eq!(
        typed_relation_definition(
            &relation,
            Some(&length_parameter(1.0)),
            &sketch,
            &[point.clone(), qualified_proxy, line.clone()],
            &markers_by_id,
            &loci_by_marker,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Entity(point.id().clone()),
            second: SketchLocus::Entity(line.id().clone()),
            parameter: ParameterId("parameter".into()),
        })
    );
}
