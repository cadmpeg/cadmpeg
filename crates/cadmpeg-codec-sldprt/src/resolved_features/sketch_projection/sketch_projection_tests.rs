//! Tests for the `sketch_projection` module.

use super::super::curves::{resolve_connected_marker_arcs, resolve_slot_marker_arcs};
use super::super::LEGACY_EXTENDED_SKETCH_MARKER;
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometry, SketchId};

#[test]
fn indexed_arc_uses_its_consecutive_middle_point_as_center() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, offset: u64, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point { position },
        )
        .with_native_ref(Some(format!("native:{offset}")))
    };
    let mut entities = vec![
        point("start", 100, Point2::new(1.0, 0.0)),
        point("center", 200, Point2::new(0.0, 0.0)),
        point("end", 300, Point2::new(0.0, 1.0)),
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId("arc".into()),
            sketch,
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
        )
        .with_native_ref(Some("native:400".into()))
        .with_endpoint_refs(vec!["native:100".into(), "native:300".into()]),
    ];

    resolve_connected_marker_arcs(&mut entities, 1.0e-9);

    assert_eq!(
        entities[3].geometry,
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        }
    );
}

#[test]
fn slot_cycle_supplies_the_missing_cap_endpoints_and_center() {
    let slot_offset = 500;
    let mut payload = vec![0; slot_offset + 140];
    let declaration = b"\xff\xff\x01\x00\x08\x00sgSlot_c\0\0\0\0\x01\0\0\0";
    payload[slot_offset - declaration.len()..slot_offset].copy_from_slice(declaration);
    payload[slot_offset..slot_offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    payload[slot_offset + 5..slot_offset + 13].fill(0xff);
    payload[slot_offset + 13..slot_offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[slot_offset + 23..slot_offset + 29]
        .copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[slot_offset + 31..slot_offset + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[slot_offset + 48..slot_offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
    for (index, (tag, id)) in [
        (0x8156_u16, 0_u16),
        (0x814c, 3),
        (0x8156, 1),
        (0x8156, 2),
        (0x8294, 0),
        (0x8294, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let start = slot_offset + 64 + index * 12;
        payload[start..start + 2].copy_from_slice(&tag.to_le_bytes());
        payload[start + 2..start + 4].copy_from_slice(&id.to_le_bytes());
        payload[start + 4..start + 8].fill(0xff);
    }

    let input = |id: &str, offset, kind, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let inputs = [
        input("center-left", 100, SketchInputKind::Point, Some([0.0, 0.0])),
        input(
            "center-right",
            110,
            SketchInputKind::Point,
            Some([2.0, 0.0]),
        ),
        input("left-top", 120, SketchInputKind::Point, Some([0.0, 1.0])),
        input("right-top", 130, SketchInputKind::Point, Some([2.0, 1.0])),
        input(
            "left-bottom",
            140,
            SketchInputKind::Point,
            Some([0.0, -1.0]),
        ),
        input(
            "right-bottom",
            150,
            SketchInputKind::Point,
            Some([2.0, -1.0]),
        ),
        input("top", 200, SketchInputKind::LineOrCircle, None),
        input("bottom", 210, SketchInputKind::LineOrCircle, None),
        input("right", 220, SketchInputKind::Arc, None),
        input("left", 230, SketchInputKind::Arc, None),
        input("slot", slot_offset as u64, SketchInputKind::Point, None),
    ];
    let markers = inputs.iter().collect::<Vec<_>>();
    let sketch = SketchId("sketch".into());
    let point = |id: &str, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(format!("model:{id}")),
            sketch.clone(),
            SketchGeometry::Point { position },
        )
        .with_native_ref(Some(id.into()))
    };
    let curve = |id: &str, geometry, endpoint_refs: &[&str]| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId(format!("model:{id}")),
            sketch.clone(),
            geometry,
        )
        .with_native_ref(Some(id.into()))
        .with_endpoint_refs(endpoint_refs.iter().map(|id| (*id).into()).collect())
    };
    let mut entities = vec![
        point("center-left", Point2::new(0.0, 0.0)),
        point("center-right", Point2::new(2.0, 0.0)),
        point("left-top", Point2::new(0.0, 1.0)),
        point("right-top", Point2::new(2.0, 1.0)),
        point("left-bottom", Point2::new(0.0, -1.0)),
        point("right-bottom", Point2::new(2.0, -1.0)),
        curve(
            "top",
            SketchGeometry::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(2.0, 1.0),
            },
            &["left-top", "right-top"],
        ),
        curve(
            "bottom",
            SketchGeometry::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(2.0, -1.0),
            },
            &["left-bottom", "right-bottom"],
        ),
        curve(
            "right",
            SketchGeometry::Arc {
                center: Point2::new(2.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(std::f64::consts::FRAC_PI_2),
                end_angle: Angle(-std::f64::consts::FRAC_PI_2),
            },
            &["right-top", "right-bottom"],
        ),
        curve(
            "left",
            SketchGeometry::Native {
                native_kind: "sldprt:marker-geometry:2".into(),
            },
            &[],
        ),
    ];

    resolve_slot_marker_arcs(&payload, &markers, &mut entities, 1.0e-9);

    assert_eq!(
        entities[9].endpoint_refs,
        ["left-top".to_string(), "left-bottom".to_string()]
    );
    assert!(matches!(
        entities[9].geometry,
        SketchGeometry::Arc {
            center,
            radius: Length(radius),
            ..
        } if center == Point2::new(0.0, 0.0) && radius == 1.0
    ));
}
