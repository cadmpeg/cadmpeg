//! Tests for the `axes` module.

use super::super::curves::compact_bounded_curve_tangent;
use super::super::endpoints::roster_curve_endpoint_markers;
use super::super::{
    CLASS_MARKER, LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER,
};
use super::*;
use crate::layout::temporary_axis_reference_nine_scalar as temporary_axis;
use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, SketchInputEntity,
    SketchInputKind, SketchRelationKind,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId};
use std::collections::{BTreeMap, HashSet};

#[test]
fn compact_line_reference_rejects_conflicting_eight_and_nine_scalar_directions() {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let mut payload = vec![0; 112];
    payload[..8].copy_from_slice(&HANDLES);
    payload[12..16].copy_from_slice(&7000u32.to_le_bytes());
    for (offset, value) in [
        (24, 0.1f64),
        (32, 0.2),
        (40, 0.3),
        (48, 0.4),
        (56, 0.5),
        (64, 1.0),
        (72, 0.0),
        (80, 0.0),
        (88, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert!(compact_line_reference_directions(&payload, 0, payload.len(), &[]).is_empty());
}

#[test]
fn compact_line_reference_rejects_conflicting_layout_candidates() {
    let mut payload = vec![0; 136];
    let handles = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    payload[..8].copy_from_slice(&handles);
    payload[12..16].copy_from_slice(&7000u32.to_le_bytes());
    for (offset, value) in [
        (56, 1.0f64),
        (64, 0.0),
        (72, 0.0),
        (80, 0.0),
        (88, f64::from_bits(0x3ff0_0000_0000_0001)),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[96..104].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    payload[116..118].copy_from_slice(&0x8200u16.to_le_bytes());
    payload[134..136].copy_from_slice(&[0xff; 2]);

    assert!(compact_line_reference_directions(&payload, 0, payload.len(), &[]).is_empty());
}

#[test]
fn revolution_line_reference_inputs_decode_profile_owner_and_placed_axis() {
    let mut payload = vec![0; 240];
    let handles = 96;
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x5919_4a35u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x81dbu16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 16 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 64..handles + 68].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 1.0, 0.0)
        ))
    );

    payload[handles + 8..handles + 12].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&[0; 4]);
    payload[handles + 16..handles + 20].copy_from_slice(&7000u32.to_le_bytes());
    payload[handles + 64..handles + 68].copy_from_slice(&[0; 4]);
    for (index, value) in [0.012, -0.034, 0.056, 0.1, 0.2, 0.3, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 20 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 92..handles + 96].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 0.0, -1.0)
        ))
    );

    payload.fill(0);
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x5919_4a35u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x81dbu16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.1, 0.2, 0.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 16 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 80..handles + 84].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 1.0, 0.0)
        ))
    );
}

#[test]
fn revolution_line_reference_inputs_decode_repeated_instance_frame() {
    let mut payload = vec![0; 240];
    let handles = 96;
    payload[64..68].copy_from_slice(&42u32.to_le_bytes());
    payload[68..72].copy_from_slice(&0x536b_2f76u32.to_le_bytes());
    payload[72..74].copy_from_slice(&0x8127u16.to_le_bytes());
    payload[76..80].copy_from_slice(&[0xff; 4]);
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 12..handles + 16].copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.012, -0.034, 0.056, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 75..handles + 77].copy_from_slice(&0x81bau16.to_le_bytes());

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(12.0, -34.0, 56.0),
            Vector3::new(0.0, 0.0, 1.0)
        ))
    );
}

#[test]
fn revolution_line_reference_inputs_decode_declared_pre_handle_address() {
    let mut payload = vec![0; 240];
    let handles = 108;
    let source = handles - 44;
    payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x4901_2c88u32.to_le_bytes());
    payload[source + 8..source + 10].copy_from_slice(&0x810fu16.to_le_bytes());
    payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    payload[source + 16..source + 20].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 28..source + 32].copy_from_slice(&122u32.to_le_bytes());
    payload[handles..handles + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    payload[handles + 4..handles + 8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    for (index, value) in [1.0, 0.0, 0.060_285_851_239_7, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 72..handles + 76].copy_from_slice(CLASS_MARKER);

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(1000.0, 0.0, 0.060_285_851_239_7 * 1000.0),
            Vector3::new(0.0, 0.0, -1.0)
        ))
    );
}

#[test]
fn revolution_line_reference_inputs_decode_declared_three_handle_layouts() {
    let make_payload = |addressed: bool| {
        let mut payload = vec![0; 320];
        let handles = 128;
        let source = handles - 48;
        payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
        payload[source + 4..source + 8].copy_from_slice(&0x3e34_ce43u32.to_le_bytes());
        payload[source + 8..source + 10].copy_from_slice(&0x8101u16.to_le_bytes());
        payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
        payload[source + 20..source + 24]
            .copy_from_slice(&(if addressed { 4u32 } else { 10 }).to_le_bytes());
        payload[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
        payload[source + 32..source + 36].copy_from_slice(&274u32.to_le_bytes());
        for offset in [handles, handles + 4, handles + 8] {
            payload[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
        }
        let handles_end = handles + 12;
        let (frame, values, marker) = if addressed {
            payload[handles_end + 4..handles_end + 8].copy_from_slice(&9000u32.to_le_bytes());
            payload[handles_end + 20..handles_end + 24].copy_from_slice(&[0xff; 4]);
            (
                handles_end + 24,
                vec![0.0, 0.015, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                handles_end + 89,
            )
        } else {
            (
                handles_end + 4,
                vec![0.0, 0.0, 0.0, 0.052, 0.0, 0.0, 0.0, 0.0, 1.0],
                handles_end + 85,
            )
        };
        for (index, value) in values.into_iter().enumerate() {
            let offset = frame + index * 8;
            payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
        }
        payload[marker..marker + 4].copy_from_slice(CLASS_MARKER);
        payload
    };

    assert_eq!(
        revolution_line_reference_inputs(&make_payload(true), 32, 320, &HashSet::from([42])),
        Some((42, Point3::new(0.0, 15.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );
    assert_eq!(
        revolution_line_reference_inputs(&make_payload(false), 32, 320, &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );
}

#[test]
fn revolution_line_reference_inputs_decode_extended_two_handle_layouts() {
    let mut declared = vec![0; 280];
    let handles = 112;
    let source = handles - 48;
    declared[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    declared[source + 4..source + 8].copy_from_slice(&0x49ab_4bc9u32.to_le_bytes());
    declared[source + 8..source + 10].copy_from_slice(&0x8120u16.to_le_bytes());
    declared[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    declared[source + 20..source + 24].copy_from_slice(&2u32.to_le_bytes());
    declared[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
    declared[source + 32..source + 36].copy_from_slice(&308u32.to_le_bytes());
    for offset in [handles, handles + 4] {
        declared[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    for (index, value) in [0.0, 0.0, 0.0, 0.064, 0.0, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 8 + index * 8;
        declared[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    declared[handles + 89..handles + 93].copy_from_slice(CLASS_MARKER);
    assert_eq!(
        revolution_line_reference_inputs(&declared, 32, declared.len(), &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)))
    );

    let mut nested = vec![0; 300];
    let handles = 132;
    let source = handles - 44;
    nested[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    nested[source + 4..source + 8].copy_from_slice(&0x4890_6465u32.to_le_bytes());
    nested[source + 8..source + 10].copy_from_slice(&0x80b6u16.to_le_bytes());
    nested[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    nested[source + 16..source + 20].copy_from_slice(&7u32.to_le_bytes());
    nested[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    nested[source + 28..source + 32].copy_from_slice(&126u32.to_le_bytes());
    for offset in [handles, handles + 4] {
        nested[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    nested[handles + 12..handles + 16].copy_from_slice(&3800u32.to_le_bytes());
    for (index, value) in [0.056, -0.051, -0.008, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        nested[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    nested[handles + 85..handles + 89].copy_from_slice(&103u32.to_le_bytes());
    assert_eq!(
        revolution_line_reference_inputs(&nested, 32, nested.len(), &HashSet::from([42])),
        Some((
            42,
            Point3::new(56.0, -51.0, -8.0),
            Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}

#[test]
fn revolution_line_reference_inputs_decode_declared_post_handle_address() {
    let mut payload = vec![0; 300];
    let handles = 128;
    let source = handles - 48;
    payload[source..source + 4].copy_from_slice(&42u32.to_le_bytes());
    payload[source + 4..source + 8].copy_from_slice(&0x5976_e99cu32.to_le_bytes());
    payload[source + 8..source + 10].copy_from_slice(&0x81e4u16.to_le_bytes());
    payload[source + 12..source + 16].copy_from_slice(&[0xff; 4]);
    payload[source + 20..source + 24].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 24..source + 28].copy_from_slice(&1u32.to_le_bytes());
    payload[source + 32..source + 36].copy_from_slice(&151u32.to_le_bytes());
    for offset in [handles, handles + 4, handles + 8] {
        payload[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[handles + 16..handles + 20].copy_from_slice(&8000u32.to_le_bytes());
    for (index, value) in [0.0, 0.0, 0.006, 0.006, 0.0, 0.0, -1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 24 + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[handles + 97..handles + 101].copy_from_slice(CLASS_MARKER);

    assert_eq!(
        revolution_line_reference_inputs(&payload, 32, payload.len(), &HashSet::from([42])),
        Some((42, Point3::new(0.0, 0.0, 6.0), Vector3::new(-1.0, 0.0, 0.0)))
    );
}

#[test]
fn temporary_axis_reference_decodes_placed_axis_record() {
    let mut payload = vec![0; 400];
    let declaration = 40;
    payload[declaration + temporary_axis::CLASS_MARKER
        ..declaration + temporary_axis::CLASS_MARKER + temporary_axis::CLASS_MARKER_VALUE.len()]
        .copy_from_slice(&temporary_axis::CLASS_MARKER_VALUE);
    payload
        [declaration + temporary_axis::NAME_LENGTH..declaration + temporary_axis::NAME_LENGTH + 2]
        .copy_from_slice(&temporary_axis::NAME_LENGTH_VALUE.to_le_bytes());
    payload[declaration + temporary_axis::NAME
        ..declaration + temporary_axis::NAME + temporary_axis::NAME_VALUE.len()]
        .copy_from_slice(&temporary_axis::NAME_VALUE);
    payload[declaration + temporary_axis::HANDLES
        ..declaration + temporary_axis::HANDLES + temporary_axis::HANDLES_VALUE.len()]
        .copy_from_slice(&temporary_axis::HANDLES_VALUE);
    payload[declaration + temporary_axis::STREAM_ADDRESS
        ..declaration + temporary_axis::STREAM_ADDRESS + 4]
        .copy_from_slice(&5000u32.to_le_bytes());
    for (index, value) in [0.0, 0.0, 0.03, 0.0, 0.0, 0.072, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = declaration + temporary_axis::AXIS_FRAME + index * 8;
        payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    payload[declaration + temporary_axis::NEXT_CLASS_MARKER
        ..declaration
            + temporary_axis::NEXT_CLASS_MARKER
            + temporary_axis::NEXT_CLASS_MARKER_VALUE.len()]
        .copy_from_slice(&temporary_axis::NEXT_CLASS_MARKER_VALUE);

    assert_eq!(
        temporary_axis_reference(&payload, 32, payload.len()),
        Some((Point3::new(0.0, 0.0, 30.0), Vector3::new(0.0, 0.0, -1.0)))
    );
}

#[test]
fn temporary_axis_reference_rejects_conflicting_records() {
    let mut payload = vec![0; 720];
    let write_record = |payload: &mut [u8], declaration: usize, origin: [f64; 3]| {
        payload[declaration + temporary_axis::CLASS_MARKER
            ..declaration
                + temporary_axis::CLASS_MARKER
                + temporary_axis::CLASS_MARKER_VALUE.len()]
            .copy_from_slice(&temporary_axis::CLASS_MARKER_VALUE);
        payload[declaration + temporary_axis::NAME_LENGTH
            ..declaration + temporary_axis::NAME_LENGTH + 2]
            .copy_from_slice(&temporary_axis::NAME_LENGTH_VALUE.to_le_bytes());
        payload[declaration + temporary_axis::NAME
            ..declaration + temporary_axis::NAME + temporary_axis::NAME_VALUE.len()]
            .copy_from_slice(&temporary_axis::NAME_VALUE);
        payload[declaration + temporary_axis::HANDLES
            ..declaration + temporary_axis::HANDLES + temporary_axis::HANDLES_VALUE.len()]
            .copy_from_slice(&temporary_axis::HANDLES_VALUE);
        payload[declaration + temporary_axis::STREAM_ADDRESS
            ..declaration + temporary_axis::STREAM_ADDRESS + 4]
            .copy_from_slice(&5000u32.to_le_bytes());
        for (index, value) in [
            origin[0], origin[1], origin[2], 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = declaration + temporary_axis::AXIS_FRAME + index * 8;
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        payload[declaration + temporary_axis::NEXT_CLASS_MARKER
            ..declaration
                + temporary_axis::NEXT_CLASS_MARKER
                + temporary_axis::NEXT_CLASS_MARKER_VALUE.len()]
            .copy_from_slice(&temporary_axis::NEXT_CLASS_MARKER_VALUE);
    };
    write_record(&mut payload, 40, [0.0, 0.0, 0.01]);
    write_record(&mut payload, 380, [0.0, 0.0, 0.02]);

    assert_eq!(temporary_axis_reference(&payload, 32, payload.len()), None);
}

#[test]
fn indexed_profile_construction_line_places_a_revolution_axis() {
    let mut payload = vec![0; 300];
    for offset in [0, 100, 200] {
        payload[offset..offset + 5].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    }
    payload[217..221].copy_from_slice(&2u32.to_le_bytes());
    payload[256..258].copy_from_slice(&0u16.to_le_bytes());
    payload[258..260].copy_from_slice(&2u16.to_le_bytes());
    payload[260..264].copy_from_slice(&[1, 0, 0, 0]);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(3),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
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
            marker("first", 0, Some(1), Some([0.0, 0.0195])),
            marker("relation", 50, None, None),
            marker("second", 100, Some(2), Some([0.008, 0.0195])),
            marker("axis", 200, None, None),
        ],
    };
    lane.sketch_entities[1].kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    lane.sketch_entities[3].kind = SketchInputKind::LineOrCircle;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert_eq!(
        roster_curve_endpoint_markers(&lane.native_payload, &lane.sketch_entities[3], &markers,)
            .into_iter()
            .map(|marker| marker.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );

    lane.native_payload.resize(400, 0);
    lane.native_payload[200..292].fill(0);
    lane.native_payload[200..205].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&4u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[292..297].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );

    lane.native_payload[200..292].fill(0);
    lane.native_payload[200..205].copy_from_slice(SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&5u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[256..258].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[258..260].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[264..272].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[284..289].copy_from_slice(SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Vertical);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );

    lane.native_payload[200..312].fill(0);
    lane.native_payload[200..205].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[205..213].fill(0xff);
    lane.native_payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[217..221].copy_from_slice(&4u32.to_le_bytes());
    lane.native_payload[223..227].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[227..231].copy_from_slice(&[1, 0, 1, 0]);
    lane.native_payload[231..239]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    lane.native_payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[268..272].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[280..284].copy_from_slice(&u32::MAX.to_le_bytes());
    lane.native_payload[284..286].copy_from_slice(&1u16.to_le_bytes());
    for offset in (286..302).step_by(4) {
        lane.native_payload[offset..offset + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    lane.native_payload[312..317].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.sketch_entities[3].kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
}

#[test]
fn compact_profile_construction_role_places_a_revolution_axis() {
    let mut payload = vec![0; 300];
    for offset in [0, 100, 200] {
        payload[offset..offset + 5].copy_from_slice(LEGACY_SKETCH_MARKER);
    }
    payload[205..213].fill(0xff);
    payload[213..217].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[217..221].copy_from_slice(&0u32.to_le_bytes());
    payload[223..227].copy_from_slice(&[0x05, 0x00, 0x01, 0x00]);
    payload[227..229].copy_from_slice(&2u16.to_le_bytes());
    payload[231..239].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0d, 0x00]);
    payload[248..256].copy_from_slice(&1.0f64.to_le_bytes());
    payload[264..266].copy_from_slice(&0u16.to_le_bytes());
    payload[266..268].copy_from_slice(&1u16.to_le_bytes());
    payload[272..280].copy_from_slice(&(-1.0f64).to_le_bytes());
    payload[292..297].copy_from_slice(LEGACY_SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(3),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
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
            marker("first", 0, Some(1), Some([0.0, 0.0195])),
            marker("second", 100, Some(2), Some([0.008, 0.0195])),
            marker("axis", 200, None, None),
        ],
    };
    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 19.5),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    lane.sketch_entities[0].kind = SketchInputKind::Arc;
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());
}

#[test]
fn bounded_profile_chords_place_implicit_revolution_axes() {
    let curve = 300;
    let mut payload = vec![0; curve + 180];
    payload[curve..curve + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    payload[curve + 5..curve + 13].fill(0xff);
    payload[curve + 13..curve + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[curve + 17..curve + 21].copy_from_slice(&2u32.to_le_bytes());
    payload[curve + 23..curve + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    payload[curve + 27..curve + 29].copy_from_slice(&1u16.to_le_bytes());
    payload[curve + 31..curve + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[curve + 48..curve + 56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[curve + 56..curve + 58].copy_from_slice(&0u16.to_le_bytes());
    payload[curve + 58..curve + 60].copy_from_slice(&1u16.to_le_bytes());
    payload[curve + 84..curve + 84 + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    let marker = |id: &str,
                  offset: u64,
                  object_index: Option<u32>,
                  coordinates_m: Option<[f64; 2]>| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: object_index.unwrap_or(4),
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
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
            marker("first", 0, Some(1), Some([0.0, 0.0])),
            marker("second", 100, Some(2), Some([0.0, 0.02])),
            marker("profile-point", 200, Some(3), Some([-0.01, 0.01])),
            marker("axis-chord", curve as u64, None, None),
        ],
    };
    lane.sketch_entities[3].kind = SketchInputKind::Arc;
    let sketch = Sketch {
        id: SketchId("sketch".into()),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, -1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, -1.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };

    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.sketch_entities.push(marker(
        "opposite-profile-point",
        250,
        Some(4),
        Some([0.01, 0.01]),
    ));
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert!(!bounded_profile_axis_endpoints(
        "profile-native",
        &markers,
        &HashSet::from(["profile-point", "opposite-profile-point"]),
        [&lane.sketch_entities[0], &lane.sketch_entities[1]],
    ));
    lane.sketch_entities[4].object_index = None;
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    assert!(bounded_profile_axis_endpoints(
        "profile-native",
        &markers,
        &HashSet::from(["profile-point", "opposite-profile-point"]),
        [&lane.sketch_entities[0], &lane.sketch_entities[1]],
    ));
    lane.sketch_entities.pop();

    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.sketch_entities[2].kind = SketchInputKind::Point;
    lane.sketch_entities[2].coordinates_m = Some([-0.01, 0.01]);

    lane.native_payload[curve + 56..curve + 60].fill(0);
    lane.native_payload[curve + 64..curve + 66].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[curve + 66..curve + 68].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[curve + 68..curve + 72].copy_from_slice(&[1, 0, 0, 0]);
    lane.native_payload[curve + 72..curve + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[curve + 84..curve + 92].fill(0);
    lane.native_payload[curve + 92..curve + 92 + SKETCH_MARKER.len()]
        .copy_from_slice(SKETCH_MARKER);
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&0u32.to_le_bytes());
    lane.native_payload[curve + 56..curve + 58].copy_from_slice(&0u16.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&1u16.to_le_bytes());
    lane.native_payload[curve + 60..curve + 64].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 64..curve + 80].fill(0);
    let detail = curve + 84;
    lane.native_payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[detail + 5..detail + 13]
        .copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0xff, 0xff]);
    lane.native_payload[detail + 13..detail + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    lane.native_payload[detail + 23..detail + 27].copy_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    lane.native_payload[detail + 27..detail + 29].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[detail + 31..detail + 39]
        .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x0c, 0x00]);
    lane.native_payload[detail + 48..detail + 56].copy_from_slice(&1.0f64.to_le_bytes());
    lane.native_payload[detail + 64..detail + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    lane.native_payload[detail + 72..detail + 80].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(
        compact_bounded_curve_tangent(&lane.native_payload, curve),
        Some([-1.0, 0.0])
    );
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    lane.native_payload[detail..detail + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve..curve + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[detail..detail + LEGACY_EXTENDED_SKETCH_MARKER.len()]
        .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&2u32.to_le_bytes());
    assert!(profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]).is_some());

    lane.native_payload[curve + 17..curve + 21].copy_from_slice(&1u32.to_le_bytes());
    lane.sketch_entities[0].coordinates_m = Some([-0.01, 0.0]);
    lane.sketch_entities[1].coordinates_m = Some([-0.01, 0.02]);
    lane.sketch_entities[2].kind = SketchInputKind::LineOrCircle;
    lane.sketch_entities
        .push(marker("axis-start", 450, None, Some([0.0, 0.0])));
    lane.sketch_entities
        .push(marker("axis-end", 460, None, Some([0.0, 0.02])));
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.sketch_entities.truncate(4);
    lane.sketch_entities[0].coordinates_m = Some([0.0, 0.0]);
    lane.sketch_entities[1].coordinates_m = Some([-0.01, 0.01]);
    lane.sketch_entities
        .push(marker("selected-axis-end", 50, None, Some([0.0, 0.02])));
    lane.native_payload[126..130].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );

    lane.native_payload[76..80].copy_from_slice(&1u32.to_le_bytes());
    lane.native_payload[curve + 56..curve + 58].copy_from_slice(&2u16.to_le_bytes());
    lane.native_payload[curve + 58..curve + 60].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        profile_roster_construction_axis(&lane, "profile-native", &sketch, &[]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        })
    );
}

#[test]
fn generated_revolution_axis_requires_multiple_coaxial_surfaces() {
    let cylinder = |id: &str, origin: Point3| Surface {
        id: SurfaceId(id.into()),
        geometry: SurfaceGeometry::Cylinder {
            origin,
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    };
    let first = cylinder("first", Point3::new(0.0, 0.0, 0.0));
    let second = cylinder("second", Point3::new(10.0, 0.0, 0.0));

    assert_eq!(
        common_generated_surface_axis(std::slice::from_ref(&first)),
        None
    );
    assert_eq!(
        common_generated_surface_axis(&[first.clone(), second]),
        Some(cadmpeg_ir::features::RevolutionAxis {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        })
    );
    assert_eq!(
        common_generated_surface_axis(&[first, cylinder("offset", Point3::new(0.0, 1.0, 0.0)),]),
        None
    );
}

#[test]
fn omitted_origin_and_principal_axes_use_unique_maximum_incidence_support_lines() {
    let mut payload = vec![0; 700];
    let curve = |payload: &mut [u8], offset: usize, start: u16, end: u16| {
        payload[offset..offset + SKETCH_MARKER.len()].copy_from_slice(SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0, 0, 0x80, 0xbf]);
        payload[offset + 23..offset + 27].copy_from_slice(&[4, 0, 2, 0]);
        payload[offset + 27..offset + 29].copy_from_slice(&1u16.to_le_bytes());
        payload[offset + 31..offset + 39].copy_from_slice(&[0, 0, 0x80, 0xbf, 0, 0, 4, 0]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 56..offset + 58].copy_from_slice(&start.to_le_bytes());
        payload[offset + 58..offset + 60].copy_from_slice(&end.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
    };
    curve(&mut payload, 400, 0, 1);
    curve(&mut payload, 484, 1, 2);
    curve(&mut payload, 568, 2, 0);
    let marker = |id: &str, offset, object_index, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("profile-native".into()),
        ordinal: offset as u32,
        offset,
        object_index,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let mut entities = vec![
        marker("vertical-near", 0, Some(1), Some([0.0, 0.01])),
        marker("vertical-far", 100, Some(2), Some([0.0, 0.02])),
        marker("tangent", 200, Some(3), Some([-0.01, 0.01])),
        marker("origin", 300, Some(4), Some([0.0, 0.0])),
        marker("curve-a", 400, None, None),
        marker("curve-b", 484, None, None),
        marker("curve-c", 568, None, None),
    ];
    for entity in &mut entities[4..] {
        entity.kind = SketchInputKind::LineOrCircle;
    }
    let lane = FeatureInputLane {
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
        sketch_entities: entities,
    };
    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();

    assert_eq!(
        profile_roster_origin_axis_endpoints(&lane, "profile-native", &markers),
        Some([[0.0, 0.0], [0.0, 0.01]])
    );
    assert_eq!(
        profile_roster_principal_axis_endpoints(&lane, "profile-native", &markers),
        Some([[0.0, 0.0], [0.0, 1.0]])
    );
}

#[test]
fn revolution_consumes_the_preceding_profile_object() {
    let feature = |id: &str, source: &str, class: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: 0,
        name: id.into(),
        kind: String::new(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("profile", "23", "moProfileFeature_c"),
            feature("revolution", "28", "moRevolution_c"),
            feature("cut-profile", "29", "moProfileFeature_c"),
            feature("cut", "30", "moRevCut_c"),
        ],
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 256],
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "profile-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 100,
                object_id: Some(23),
                value: "profile".into(),
            },
            FeatureInputName {
                id: "revolution-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 200,
                object_id: Some(28),
                value: "revolution".into(),
            },
            FeatureInputName {
                id: "cut-profile-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: 220,
                object_id: Some(29),
                value: "cut-profile".into(),
            },
            FeatureInputName {
                id: "cut-name".into(),
                parent: "lane".into(),
                ordinal: 3,
                offset: 240,
                object_id: Some(30),
                value: "cut".into(),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };

    enrich_history_revolution_inputs(&mut histories, std::slice::from_ref(&lane));

    assert_eq!(
        histories[0].features[1].properties.get("Profile"),
        Some(&"23".into())
    );
    assert_eq!(
        histories[0].features[3].properties.get("Profile"),
        Some(&"29".into())
    );

    for feature in &mut histories[0].features {
        feature.source_id = None;
        feature.properties.clear();
    }
    enrich_history_revolution_inputs(&mut histories, &[lane]);
    assert_eq!(
        histories[0].features[1].properties.get("Profile"),
        Some(&"23".into())
    );
    assert_eq!(
        histories[0].features[3].properties.get("Profile"),
        Some(&"29".into())
    );
}
