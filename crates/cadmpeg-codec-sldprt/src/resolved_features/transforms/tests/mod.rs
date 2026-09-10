//! Tests for the `transforms` module.

use super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
    {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = crate::records::SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            0,
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature-native".into());
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
    }
}

mod frames;
mod join;
mod patterns;
mod profile;
mod relation_geometry;
mod relation_links;
mod relation_operands;
mod selection;

fn sketch_entity(geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("synthetic:test:id#entity").unwrap(),
        cadmpeg_ir::sketches::SketchId::mint("synthetic:test:id#sketch").unwrap(),
        cadmpeg_ir::sketches::SketchGeometry::try_from(geometry).unwrap(),
    )
}

#[test]
fn a_sketch_entity_without_marker_loci_has_no_marker_kind() {
    let text = sketch_entity(SketchGeometryDefinition::Text {
        text: cadmpeg_ir::products::NonEmptyString::new("cadmpeg").unwrap(),
        font_family: cadmpeg_ir::products::NonEmptyString::new("sans").unwrap(),
        font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
        height: cadmpeg_ir::scalar::Length::new(4.0).unwrap(),
        width_factor: None,
        placement: None,
        horizontal_alignment: None,
        vertical_alignment: None,
    });
    assert!(sketch_entity_marker_loci(&text).is_none());
    assert!(sketch_entity_loci(&text).is_empty());

    let reference_line = sketch_entity(SketchGeometryDefinition::ReferenceLine {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    });
    assert!(sketch_entity_marker_loci(&reference_line).is_none());

    let point = sketch_entity(SketchGeometryDefinition::Point {
        position: Point2::new(1.0, 2.0),
    });
    let markers = sketch_entity_marker_loci(&point).expect("a point has one marker locus");
    assert_eq!(markers.kind(), SketchInputKind::Point);
    assert_eq!(markers.loci().len(), 1);
}
