use super::super::{
    classed_offset_plane_sources, legacy_offset_plane_face_alias, offset_plane_reference_source,
    select_reference_plane_frame_source, structured_offset_plane_sources,
};
use crate::records::{Feature, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use crate::resolved_features::curves::{sketch_plane_frames, SketchPlaneUAxisSource};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::Length;
use std::collections::{BTreeMap, HashSet};

#[test]
fn legacy_offset_plane_face_alias_requires_the_complete_nested_record() {
    let mut body = vec![0; 115];
    body[..2].copy_from_slice(&0x802d_u16.to_le_bytes());
    body[2..6].copy_from_slice(&2u32.to_le_bytes());
    body[45..61].fill(0xff);
    body[69..73].copy_from_slice(&2u32.to_le_bytes());
    body[73..77].copy_from_slice(&0x4c41_ac95_u32.to_le_bytes());
    body[77..83].copy_from_slice(&[0, 0, 3, 0, 0, 0]);
    body[83..87].copy_from_slice(&1u32.to_le_bytes());
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[99..103].copy_from_slice(&3u32.to_le_bytes());
    body[107..115].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(legacy_offset_plane_face_alias(&body), Some((0, 175)));
    body[91..95].fill(0);
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
    body[91..95].copy_from_slice(&175u32.to_le_bytes());
    body[83] = 2;
    assert_eq!(legacy_offset_plane_face_alias(&body), None);
}

#[test]
fn structured_offset_plane_source_requires_repeated_identities_and_terminator() {
    let mut payload = vec![0; 140];
    let header = 0x8323u32.to_le_bytes();
    let identity = [
        0xd7, 0x81, 0x26, 0x03, 0x1d, 0x00, 0x00, 0x00, 0x5e, 0x2c, 0xdb, 0x54,
    ];
    let link = 0x81dcu32.to_le_bytes();
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&header);
    for offset in [8, 32, 52, 76] {
        payload[offset..offset + 12].copy_from_slice(&identity);
    }
    payload[28..32].copy_from_slice(&link);
    payload[44..48].copy_from_slice(&3u32.to_le_bytes());
    payload[48..52].copy_from_slice(&header);
    for offset in [64, 88, 108] {
        payload[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    payload[72..76].copy_from_slice(&link);
    payload[116..120].copy_from_slice(&2600u32.to_le_bytes());
    payload[132..140].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);

    assert_eq!(
        structured_offset_plane_sources(&payload).collect::<Vec<_>>(),
        [3]
    );
    payload[80] ^= 1;
    assert!(structured_offset_plane_sources(&payload).next().is_none());
}

#[test]
fn classed_offset_plane_source_requires_exact_length_delimited_type() {
    let mut payload = 4u32.to_le_bytes().to_vec();
    payload.extend(b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00");

    assert_eq!(
        classed_offset_plane_sources(&payload).collect::<Vec<_>>(),
        [4]
    );
    payload[8] = 0;
    assert!(classed_offset_plane_sources(&payload).next().is_none());
}

#[test]
fn typed_offset_plane_reference_requires_one_known_plane_target() {
    let record = |source: u32, signature: [u8; 4], selector: u32| {
        let mut bytes = Vec::new();
        bytes.extend(source.to_le_bytes());
        bytes.extend(signature);
        bytes.extend([0; 2]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend([0; 4]);
        bytes.extend(247u32.to_le_bytes());
        bytes.extend([0; 12]);
        bytes.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
        bytes
    };
    let known = HashSet::from([3, 225]);
    let principal = record(3, [0x43, 0xf6, 0x8a, 0x4d], 3);
    assert_eq!(
        offset_plane_reference_source(&principal, &known, &known, None),
        Some(3)
    );
    let feature = record(225, [0x30, 0x92, 0xab, 0x53], 0);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, None),
        Some(225)
    );
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &known, Some(225)),
        None
    );

    let mut ambiguous = principal.clone();
    ambiguous.extend_from_slice(&feature);
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        None
    );
    let mut repeated = principal.clone();
    repeated.extend_from_slice(&principal);
    assert_eq!(
        offset_plane_reference_source(&repeated, &known, &known, None),
        Some(3)
    );
    ambiguous[38] ^= 1;
    assert_eq!(
        offset_plane_reference_source(&ambiguous, &known, &known, None),
        Some(225)
    );
    let mut malformed = record(3, [0; 4], 2);
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        None
    );
    malformed[4..8].copy_from_slice(&[1, 2, 3, 4]);
    malformed[10..14].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        offset_plane_reference_source(&malformed, &known, &known, None),
        Some(3)
    );
    let principal_only = HashSet::from([3]);
    assert_eq!(
        offset_plane_reference_source(&feature, &known, &principal_only, None),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_requires_one_unique_source() {
    assert_eq!(
        select_reference_plane_frame_source(["derived", "principal", "older"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["same", "same"].into_iter()),
        Some("same")
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn frame_only_offset_plane_reference_does_not_use_feature_order() {
    assert_eq!(
        select_reference_plane_frame_source(["older", "latest", "latest"].into_iter(),),
        None
    );
    assert_eq!(
        select_reference_plane_frame_source(["source", "source"].into_iter()),
        Some("source")
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

/// The sketch-plane frame of an offset plane 3 mm above the Top plane that
/// stores `stored` as its own `Normal` and `UAxis`.
fn top_offset_plane_frame(
    stored: Option<(&str, &str)>,
) -> Option<crate::resolved_features::curves::SketchPlaneFrame> {
    use cadmpeg_ir::features::Feature as NeutralFeature;

    let native = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(FeatureSource::try_from(source).expect("test feature source id")),
        ordinal: source.parse().expect("required invariant"),
        name: id.to_string(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let neutral = |id: &str, native_ref: &str, definition| NeutralFeature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::new(
            definition,
            cadmpeg_ir::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        ),
        native_ref: Some(native_ref.into()),
    };
    let mut offset = neutral(
        "synthetic:test:id#offset",
        "offset-native",
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(cadmpeg_ir::features::datums::DatumPlaneReference::Feature {
                feature: FeatureId::mint("synthetic:test:id#plane").expect("identity grammar"),
            }),
            distance: Length::new(3.0).unwrap(),
        }),
    );
    if let Some((normal, u_axis)) = stored {
        offset
            .source_properties
            .insert(cadmpeg_core::nonblank_literal!("Normal"), normal.into());
        offset
            .source_properties
            .insert(cadmpeg_core::nonblank_literal!("UAxis"), u_axis.into());
    }
    let features = vec![
        neutral(
            "synthetic:test:id#plane",
            "plane-native",
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            }),
        ),
        offset,
    ];
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("plane-native", "3"), native("offset-native", "549")],
    };

    sketch_plane_frames(&features, &[history])
        .get(&549)
        .copied()
}

fn top_offset_frame_with_u_axis(
    u_axis: cadmpeg_ir::math::Vector3,
) -> crate::resolved_features::curves::SketchPlaneFrame {
    crate::resolved_features::curves::SketchPlaneFrame {
        origin: Point3::new(0.0, 3.0, 0.0),
        normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        u_axis,
        u_axis_source: SketchPlaneUAxisSource::Native,
        principal: None,
    }
}

#[test]
fn offset_plane_frame_translates_its_reference_frame() {
    assert_eq!(
        top_offset_plane_frame(None),
        Some(top_offset_frame_with_u_axis(
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
        ))
    );
}

#[test]
fn offset_plane_frame_takes_its_own_stored_u_axis() {
    assert_eq!(
        top_offset_plane_frame(Some(("0,1,0", "0,0,1"))),
        Some(top_offset_frame_with_u_axis(
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        ))
    );
}

#[test]
fn offset_plane_frame_keeps_the_source_u_axis_for_an_opposite_or_tilted_stored_frame() {
    for stored in [("0,-1,0", "0,0,1"), ("0,1,0", "0,0.5,1")] {
        assert_eq!(
            top_offset_plane_frame(Some(stored)),
            Some(top_offset_frame_with_u_axis(
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            ))
        );
    }
}

/// A name record with `token`, the UTF-16 `name`, and its object identifier
/// field left to the caller.
fn name_record(token: u16, name: &str) -> Vec<u8> {
    let mut record = token.to_le_bytes().to_vec();
    record.extend_from_slice(&[0xff, 0xfe, 0xff]);
    record.push(u8::try_from(name.encode_utf16().count()).unwrap());
    for unit in name.encode_utf16() {
        record.extend_from_slice(&unit.to_le_bytes());
    }
    record
}

/// An offset reference-plane object named `Plane9`: the plane's own bytes
/// end with ten zero bytes, a plane-data class token and the data head with
/// `point` (metres), the Front normal and flag `0`; the plane's `D1`
/// parameter object stores `stored` metres behind the 18-byte header, where
/// `stored` is given.
fn offset_plane_lane(point: [f64; 3], stored: Option<f64>) -> FeatureInputLane {
    let mut payload = name_record(0x8040, "Plane9");
    payload.extend_from_slice(&[0x11; 40]);
    payload.extend_from_slice(&[0; 10]);
    payload.extend_from_slice(&0x8123_u16.to_le_bytes());
    for value in point.into_iter().chain([0.0, 0.0, 1.0]) {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(0);
    payload.extend_from_slice(&[0x22; 64]);
    let d1 = payload.len();
    let mut names = vec![FeatureInputName {
        id: "plane".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: ObjectId::from_value(9),
        value: "Plane9".into(),
    }];
    if let Some(stored) = stored {
        payload.extend_from_slice(&name_record(0x8041, "D1"));
        payload.extend_from_slice(crate::resolved_features::COMPACT_SCALAR_HEADER);
        payload.extend_from_slice(&stored.to_le_bytes());
        payload.extend_from_slice(&[0, 0, 0, 0x37, 0x01, 0, 0]);
        names.push(FeatureInputName {
            id: "d1".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: u64::try_from(d1).unwrap(),
            object_id: None,
            value: "D1".into(),
        });
    }
    payload.extend_from_slice(&[0; 32]);
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: Vec::new(),
        names,
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

/// Enriches one `Plane9` record whose `D1` displays `display`, and returns
/// its parameters and properties.
fn enriched_offset_plane(
    lane: FeatureInputLane,
    display: &str,
    unit: Option<cadmpeg_ir::design::units::UnitSymbol>,
) -> (
    BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    let mut plane = Feature {
        id: "plane".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(9),
        ordinal: 4,
        name: "Plane9".into(),
        kind: "Plane".into(),
        input_class: Some("moRefPlane_c".into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    plane
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), display.into());
    let mut histories = vec![crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![plane],
    }];
    super::super::enrich_history_reference_planes(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut histories,
        &[lane],
        unit,
    )
    .unwrap();
    let plane = histories.remove(0).features.remove(0);
    (plane.parameters, plane.properties)
}

fn distance_mm(parameters: &BTreeMap<cadmpeg_core::text::NonBlankString, String>) -> f64 {
    crate::history::literals::parse_dimension_length_mm(&parameters["D1"])
        .unwrap()
        .get()
}

/// The stored metres of the `D1` parameter object give the distance in an
/// inch document, whose display is an inch number, and in a millimetre
/// document whose display is rounded; the data head's point and normal are
/// kept.
#[test]
fn offset_plane_takes_the_distance_its_d1_object_stores() {
    use cadmpeg_ir::design::units::UnitSymbol;
    for (display, unit) in [("0.49", UnitSymbol::In), ("12.35", UnitSymbol::Mm)] {
        let lane = offset_plane_lane([0.001, 0.002, -0.012_345], Some(0.012_345));
        let (parameters, properties) = enriched_offset_plane(lane, display, Some(unit));
        assert!(
            (distance_mm(&parameters) - 12.345).abs() <= 1e-9,
            "{display}"
        );
        assert!(!properties.contains_key("DistanceDisplayed"));
        let point = crate::history::literals::parse_point3_mm(&properties["PlanePoint"])
            .unwrap()
            .get();
        assert!((point.z + 12.345).abs() <= 1e-9 && (point.x - 1.0).abs() <= 1e-9);
        assert_eq!(properties.get("PlaneNormal"), Some(&"0,0,1".to_string()));
    }
}

/// Without a `D1` parameter object the bare display is read in the
/// document's length unit, and the plane keeps the display it was read from.
#[test]
fn offset_plane_without_a_stored_distance_reads_its_display_in_the_document_unit() {
    let lane = offset_plane_lane([0.0, 0.0, 0.0127], None);
    let (parameters, properties) =
        enriched_offset_plane(lane, "0.5", Some(cadmpeg_ir::design::units::UnitSymbol::In));
    assert!((distance_mm(&parameters) - 12.7).abs() <= 1e-9);
    assert_eq!(
        properties.get("DistanceDisplayed"),
        Some(&"0.5".to_string())
    );
}

/// A value the `D1` parameter object holds that the display does not show is
/// not the plane's distance: the plane reads its display and keeps it.
#[test]
fn offset_plane_rejects_a_stored_value_its_display_does_not_show() {
    let lane = offset_plane_lane([0.0, 0.0, 0.006], Some(0.02));
    let (parameters, properties) =
        enriched_offset_plane(lane, "6", Some(cadmpeg_ir::design::units::UnitSymbol::Mm));
    assert!((distance_mm(&parameters) - 6.0).abs() <= 1e-9);
    assert_eq!(properties.get("DistanceDisplayed"), Some(&"6".to_string()));
}
