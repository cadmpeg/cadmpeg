//! Tests for the `bindings` module.

use super::super::LEGACY_SKETCH_MARKER;
use super::{bind_resolved_curve_vertices, normalize_indexed_curve_entities};
use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
#[test]
fn indexed_curve_vertex_binding_follows_the_resolved_coordinate_roster() {
    let mut payload = vec![0; 104 + LEGACY_SKETCH_MARKER.len()];
    payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&1u16.to_le_bytes());
    payload[58..60].copy_from_slice(&3u16.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());
    payload[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[72..76].copy_from_slice(&1i32.to_le_bytes());
    payload[76..78].copy_from_slice(&2u16.to_le_bytes());
    for start in (78..94).step_by(4) {
        payload[start..start + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    payload[104..].copy_from_slice(LEGACY_SKETCH_MARKER);
    let entity = |id: &str, offset, object_index, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile".into()),
        ordinal: 0,
        offset,
        object_index,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![
            entity("curve", 0, Some(1), SketchInputKind::Arc, None),
            entity("handle", 1, None, SketchInputKind::Point, Some([-1.0, 0.0])),
            entity(
                "start",
                2,
                Some(2),
                SketchInputKind::Point,
                Some([0.0, 0.0]),
            ),
            entity("center", 3, None, SketchInputKind::Point, Some([0.5, 0.5])),
            entity(
                "end",
                4,
                Some(3),
                SketchInputKind::LineOrCircle,
                Some([1.0, 0.0]),
            ),
        ],
    };

    normalize_indexed_curve_entities(&mut lane);
    bind_resolved_curve_vertices(&mut lane);

    assert_eq!(lane.sketch_entities[4].kind, SketchInputKind::Point);
}
