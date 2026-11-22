use super::*;
use crate::resolved_features::sweeps::tests::sketch;

const EPS_TEST: f64 = 1.0e-12;

#[test]
fn planar_path_orders_the_lines_and_arc_by_their_endpoints() {
    let lane = sketch(false);
    let curves = planar(&lane, "source").unwrap();
    assert_eq!(curves.len(), 3);
    assert_eq!(curves[0].start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(curves[2].end, Point3::new(18.0, 22.0, 0.0));
    assert!(
        matches!(curves[1].geometry.definition(), SketchGeometryDefinition::Arc { radius, .. } if (radius.get() - 6.0).abs() < EPS_TEST)
    );
    assert!(coincident(curves[0].end, curves[1].start));
    assert!(coincident(curves[1].end, curves[2].start));
}

#[test]
fn spatial_path_uses_the_same_framed_point_roster() {
    let lane = sketch(true);
    let curves = spatial(&lane, "source").unwrap();
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(curves[1].end, Point3::new(12.0, 14.0, 9.0));
    assert_eq!(curves[0].end, curves[1].start);
}

#[test]
fn a_curve_with_an_out_of_range_endpoint_retains_native_geometry() {
    let mut lane = sketch(true);
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    lane.native_payload[offset + 64..offset + 66].copy_from_slice(&100u16.to_le_bytes());
    assert!(spatial(&lane, "source").is_none());
}

#[test]
fn a_disconnected_path_is_retained() {
    let mut lane = sketch(false);
    lane.sketch_entities.remove(2);
    assert!(planar(&lane, "source").is_none());
}

#[test]
fn spatial_coordinate_mask_keeps_its_low_state_bit() {
    use crate::resolved_features::sketch_lists::{ListedRecord, SketchRecordLists};
    let mut lane = sketch(true);
    let lists = SketchRecordLists::new(&lane.native_payload);
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    let Some(ListedRecord::Curve { sketch, .. }) = lists.record(offset) else {
        panic!("curve")
    };
    let point = sketch.point(2).unwrap();
    lane.native_payload[point + 64..point + 66].copy_from_slice(&0x0fu16.to_le_bytes());
    let curves = spatial(&lane, "source").unwrap();
    assert_eq!(curves[1].end, Point3::new(12.0, 14.0, 9.0));
}

fn unframed_spatial() -> crate::records::FeatureInputLane {
    use crate::records::{SketchInputEntity, SketchInputKind};
    let mut lane = sketch(true);
    let lists = SketchRecordLists::new(&lane.native_payload);
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    let Some(ListedRecord::Curve { sketch, .. }) = lists.record(offset) else {
        panic!("curve")
    };
    let points = (0..3)
        .map(|index| sketch.point(index).unwrap())
        .collect::<Vec<_>>();
    lane.native_payload[32..36].fill(0);
    for marker in &lane.sketch_entities {
        let offset = usize::try_from(marker.offset()).unwrap();
        for field in [64, 66] {
            let index = View::u16_le_at(&lane.native_payload, offset + field).unwrap() + 1;
            lane.native_payload[offset + field..offset + field + 2]
                .copy_from_slice(&index.to_le_bytes());
        }
    }
    for (ordinal, offset) in points.into_iter().enumerate() {
        let mut marker = SketchInputEntity::new(
            format!("point#{offset}"),
            "lane#1",
            u32::try_from(ordinal).unwrap(),
            u64::try_from(offset).unwrap(),
            SketchInputKind::Point,
        );
        marker.feature_ref = Some("source".into());
        lane.sketch_entities.push(marker);
    }
    lane
}

#[test]
fn unframed_spatial_lines_use_one_based_primary_point_positions() {
    let mut lane = unframed_spatial();
    let curves = spatial(&lane, "source").unwrap();
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(curves[1].end, Point3::new(12.0, 14.0, 9.0));
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    lane.native_payload[offset + 64..offset + 66].fill(0);
    assert!(spatial(&lane, "source").is_none());
}

#[test]
fn an_earlier_framed_sketch_keeps_its_point_roster() {
    use crate::records::{SketchInputEntity, SketchInputKind};
    let mut lane = sketch(false);
    let lists = SketchRecordLists::new(&lane.native_payload);
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    let Some(ListedRecord::Curve { sketch, .. }) = lists.record(offset) else {
        panic!("curve")
    };
    let first_point = sketch.point(0).unwrap();
    let mut marker = SketchInputEntity::new(
        "earlier-point",
        "lane#1",
        0,
        u64::try_from(first_point).unwrap(),
        SketchInputKind::Point,
    );
    marker.feature_ref = Some("other".into());
    lane.sketch_entities.push(marker);
    for marker in &mut lane.sketch_entities {
        marker.feature_ref = Some("other".into());
    }
    let shift = lane.native_payload.len();
    let source = unframed_spatial();
    let mut payload = lane.native_payload.to_vec();
    payload.extend_from_slice(&source.native_payload);
    lane.native_payload = payload.into();
    for marker in source.sketch_entities {
        let offset = marker.offset() + u64::try_from(shift).unwrap();
        let mut projected = SketchInputEntity::new(
            format!("source#{offset}"),
            "lane#1",
            0,
            offset,
            marker.kind(),
        );
        projected.feature_ref = Some("source".into());
        lane.sketch_entities.push(projected);
    }
    let curves = spatial(&lane, "source").unwrap();
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(curves[1].end, Point3::new(12.0, 14.0, 9.0));
}

#[test]
fn an_unreadable_primary_point_keeps_its_roster_position() {
    let mut lane = unframed_spatial();
    lane.sketch_entities.remove(0);
    let point = usize::try_from(lane.sketch_entities[1].offset()).unwrap();
    lane.native_payload[point + 64..point + 66].copy_from_slice(&0x0fu16.to_le_bytes());
    assert_eq!(
        spatial(&lane, "source").unwrap()[0].start,
        Point3::new(0.0, 0.0, 0.0)
    );
    lane.native_payload[point + 66..point + 74].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(spatial(&lane, "source").is_none());
}

#[test]
fn an_unadmitted_selected_marker_retains_the_path() {
    use crate::records::{SketchInputEntity, SketchInputKind};
    let mut lane = sketch(false);
    assert!(planar(&lane, "source").is_some());
    // The first point record carries the selected head but is not a listed curve.
    let offset = 32 + 4 + 6 + 4;
    assert!(selected_curve(&lane.native_payload, offset));
    let mut marker = SketchInputEntity::new(
        String::from("marker#point"),
        "lane#1",
        9,
        u64::try_from(offset).unwrap(),
        SketchInputKind::LineOrCircle,
    );
    marker.feature_ref = Some("source".into());
    lane.sketch_entities.push(marker);
    assert!(planar(&lane, "source").is_none());
}
