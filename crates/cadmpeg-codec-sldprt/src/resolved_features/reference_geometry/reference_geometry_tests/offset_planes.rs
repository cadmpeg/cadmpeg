use super::super::*;
use super::*;
use crate::resolved_features::curves::sketch_plane_frames;
use std::collections::HashSet;

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

    assert_eq!(structured_offset_plane_sources(&payload), [3]);
    payload[80] ^= 1;
    assert!(structured_offset_plane_sources(&payload).is_empty());
}

#[test]
fn classed_offset_plane_source_requires_exact_length_delimited_type() {
    let mut payload = 4u32.to_le_bytes().to_vec();
    payload.extend(b"\xff\xff\x01\x00\x1b\x00moFromSktEnt3IntSurfIdRep_c\x00\x00");

    assert_eq!(classed_offset_plane_sources(&payload), [4]);
    payload[8] = 0;
    assert!(classed_offset_plane_sources(&payload).is_empty());
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
        Some("same".into())
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
        Some("source".into())
    );
    assert_eq!(
        select_reference_plane_frame_source(["first", "second"].into_iter()),
        None
    );
}

#[test]
fn offset_plane_frame_translates_its_reference_frame() {
    use cadmpeg_ir::features::Feature as NeutralFeature;

    let native = |id: &str, source: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source.into()),
        parent_source_id: None,
        ordinal: source.parse().expect("required invariant"),
        name: id.into(),
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
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition,
        native_ref: Some(native_ref.into()),
    };
    let features = vec![
        neutral(
            "plane",
            "plane-native",
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
        ),
        neutral(
            "offset",
            "offset-native",
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(cadmpeg_ir::features::DatumPlaneReference::Feature(
                    FeatureId("plane".into()),
                )),
                distance: Length(3.0),
            },
        ),
    ];
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![native("plane-native", "3"), native("offset-native", "549")],
    };

    assert_eq!(
        sketch_plane_frames(&features, &[history]).get(&549),
        Some(&crate::resolved_features::curves::SketchPlaneFrame {
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            u_axis_source: SketchPlaneUAxisSource::Native,
        })
    );
}
