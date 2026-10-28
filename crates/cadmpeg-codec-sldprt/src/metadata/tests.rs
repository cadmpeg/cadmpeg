// SPDX-License-Identifier: Apache-2.0
//! `SWObjects` document-metadata decode and write-back tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::outer_header;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::container::sldprt_with_body_and_envelope;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn metadata_from_nameless_block_keeps_annotation_owner() {
    let payload = br"<swSolidWorks><SW_UnitsLinear>1</SW_UnitsLinear></swSolidWorks>";
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let attributes = super::attributes(&ctx, &scan, &mut annotations, &mut Vec::new()).unwrap();

    assert!(attributes
        .iter()
        .any(|attribute| attribute.name == "source_linear_unit_code"));
    assert!(annotations
        .provenance
        .values()
        .any(|provenance| provenance.stream() == "block@8"));
}

#[test]
fn transformed_reference_enclosure_requires_complete_direct_declaration() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = b"moTransRefPlaneData_c".to_vec();
    payload.extend_from_slice(&[0; 8]);
    for value in [0.01f64, 0.02, 0.03, 0.1, 0.2, 1.0, 0.0, -1.0, 0.5] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    source.extend(make_block(0x43, "SWObjects", &payload));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "transformed_reference_enclosure"));
}

#[test]
fn semantic_writer_preserves_transformed_reference_enclosure_prefix() {
    use cadmpeg_ir::attributes::AttributeValue;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let transformed = ir
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == "transformed_reference_enclosure")
            .unwrap();
        let AttributeValue::Vector(center) = &mut transformed.values[0] else {
            panic!("transformed plane center");
        };
        center[0] = cadmpeg_ir::scalar::FiniteReal::new(25.0).unwrap();
    }

    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();

    let scan = crate::test_support::container::scan(&written);
    let payload = scan
        .blocks
        .iter()
        .find(|block| {
            block
                .payload
                .windows(b"moTransRefPlaneData_c".len())
                .any(|bytes| bytes == b"moTransRefPlaneData_c")
        })
        .map(|block| block.payload.as_slice())
        .unwrap();
    let token = b"moTransRefPlaneData_c";
    let offset = payload
        .windows(token.len())
        .position(|bytes| bytes == token)
        .unwrap()
        + token.len();
    assert_eq!(
        &payload[offset..offset + 25],
        &[1, 0, 0, 0, 1, 0, 0, 0, 0xff, 0x80, 0, 0, 0x80, 0x40, 0, 0, 95, 0, 0, 0, 0, 0, 0, 0, 1]
    );
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let transformed = regenerated
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_enclosure")
        .unwrap();
    assert!(transformed.id.as_str().ends_with(":153"));
}

#[test]
fn decode_does_not_scan_past_unit_name_record_start() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = b"moLengthUserUnits_c".to_vec();
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 4, b'I', 0, b'N', 0]);
    source.extend(make_block(0x43, "SWObjects", &payload));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "source_linear_unit_name"));
}

#[test]
fn semantic_writer_preserves_document_metadata() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let moved = decoded.ir_mut().model.points[0].position().get();
    decoded.ir_mut().model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x,
            moved.y,
            moved.z + 1.0,
        ))
        .expect("a finite position is a point"),
    );

    let expected = decoded
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let actual = regenerated
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(actual, expected);
}

#[test]
fn decode_extracts_document_envelope() {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut cur = Cursor::new(sldprt_with_body_and_envelope(&triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let envelope = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "bounding_envelope")
        .expect("envelope");
    let AttributeValue::Vector(values) = &envelope.values[0] else {
        panic!("vector")
    };
    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_lane(values),
        [10.0, 20.0, -30.0, 40.0]
    );
    let plane = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "default_reference_plane")
        .expect("reference plane");
    let AttributeValue::Vector(origin) = &plane.values[0] else {
        panic!("origin")
    };
    let AttributeValue::Vector(frame) = &plane.values[1] else {
        panic!("frame")
    };
    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_lane(origin),
        [1.0, 2.0, 3.0]
    );
    assert_eq!(frame[2].get(), 1.0);
    let transformed = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_enclosure")
        .expect("transformed reference plane");
    assert!(transformed.id.as_str().ends_with(":153"));
    assert_eq!(
        transformed.values,
        vec![
            AttributeValue::vector([10.0, 20.0, 30.0]).unwrap(),
            AttributeValue::vector([100.0, 200.0, 300.0]).unwrap(),
            AttributeValue::vector([-100.0, -200.0, -300.0]).unwrap(),
            AttributeValue::float(500.0).unwrap(),
        ]
    );
    let part = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "part_record")
        .unwrap();
    assert_eq!(
        part.values,
        vec![AttributeValue::Integer(42), AttributeValue::Integer(2026)]
    );
    assert!(!result
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "configuration_manager"));
    let units = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_code")
        .unwrap();
    assert_eq!(units.values, vec![AttributeValue::Integer(0)]);
    let unit_name = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_name")
        .unwrap();
    assert_eq!(unit_name.values, vec![AttributeValue::String("IN".into())]);
}

/// The metadata attributes a single `SWObjects` block holding `payload` yields.
fn scanned_metadata(payload: &[u8]) -> Vec<cadmpeg_ir::attributes::SourceAttribute> {
    let mut source = outer_header();
    source.extend(make_block(0x43, "SWObjects", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    super::attributes(
        &ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
        &mut Vec::new(),
    )
    .unwrap()
}

#[test]
fn a_bounding_envelope_that_overflows_in_millimetres_is_not_admitted() {
    let mut payload = b"moBBoxCenterData_c".to_vec();
    payload.extend_from_slice(&1u32.to_le_bytes());
    for value in [1.0e306f64, 0.02, -0.03, 0.04] {
        payload.extend_from_slice(&value.to_le_bytes());
    }

    let attributes = scanned_metadata(&payload);
    assert!(
        !attributes
            .iter()
            .any(|attribute| attribute.name == "bounding_envelope"),
        "{attributes:?}"
    );
}

#[test]
fn a_reference_plane_origin_that_overflows_in_millimetres_is_not_admitted() {
    let mut payload = b"moDefaultRefPlnData_c".to_vec();
    for value in [0.001f64, -1.0e306, 0.003, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }

    let attributes = scanned_metadata(&payload);
    assert!(
        !attributes
            .iter()
            .any(|attribute| attribute.name == "default_reference_plane"),
        "{attributes:?}"
    );
}

fn replacement_unit_source() -> Vec<u8> {
    let mut payload = b"moLengthUserUnits_c".to_vec();
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 2, 0, 0xd8]);
    let mut source = outer_header();
    source.extend(make_block(0x43, "SWObjects", &payload));
    source
}

#[test]
fn unit_name_replacement_decodes_replacement_character() {
    let source = replacement_unit_source();
    let scan = crate::test_support::container::scan(&source);
    let section = scan.sections().next().expect("unit-name section");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut attributes = Vec::new();
    super::scan_length_user_units(
        &ctx,
        section,
        &mut attributes,
        &mut cadmpeg_ir::annotations::Annotations::default(),
    )
    .expect("replacement unit name");
    assert_eq!(
        attributes[0].values,
        vec![cadmpeg_ir::attributes::AttributeValue::String("�".into())]
    );
}

fn authored_enclosure(flag: u8, opaque: u8, values: [f64; 10]) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 1, 0, 21, 0];
    bytes.extend(b"moTransRefPlaneData_c");
    bytes.extend([opaque; 24]);
    bytes.push(flag);
    if flag == 1 {
        for value in values {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn transformed_enclosure_preserves_asymmetric_center_and_radius() {
    let values = [0.01, 0.02, 0.03, 0.2, 0.3, 0.4, -0.1, -0.2, -0.3, 0.6];
    for opaque in [0, 0xff, 0x53] {
        let bytes = authored_enclosure(1, opaque, values);
        assert_eq!(super::transformed_enclosure(&bytes, 6), Some(values));
    }
}

#[test]
fn absent_enclosure_never_backfills_from_a_later_extent() {
    let values = [0., 0., 0., 1., 1., 1., -1., -1., -1., 2.];
    let mut bytes = authored_enclosure(0, 0xff, values);
    let boundary = bytes.len();
    bytes.extend(authored_enclosure(1, 0, values));
    assert_eq!(super::transformed_enclosure(&bytes, 6), None);
    assert_eq!(
        super::transformed_enclosure(&bytes, boundary + 6),
        Some(values)
    );
}

#[test]
fn transformed_enclosure_requires_the_complete_optional_extent() {
    let values = [0., 0., 0., 1., 1., 1., -1., -1., -1., 2.];
    let bytes = authored_enclosure(1, 0, values);
    for end in 27..bytes.len() {
        assert_eq!(super::transformed_enclosure(&bytes[..end], 6), None);
    }
    assert_eq!(super::transformed_enclosure(&bytes, 6), Some(values));
}

#[test]
fn transformed_enclosure_rejects_wrong_declaration_and_presence_domain() {
    let values = [0., 0., 0., 1., 1., 1., -1., -1., -1., 2.];
    for at in [0, 2, 4, 6 + b"moTransRefPlaneData_c".len() + 24] {
        let mut bytes = authored_enclosure(1, 0, values);
        bytes[at] ^= 2;
        assert_eq!(super::transformed_enclosure(&bytes, 6), None);
    }
}

#[test]
fn transformed_enclosure_requires_finite_ordered_stored_values() {
    let values = [0., 0., 0., 1., 1., 1., -1., -1., -1., 2.];
    for (at, value) in [(0, f64::NAN), (3, -2.), (6, 2.), (9, -1.)] {
        let mut changed = values;
        changed[at] = value;
        assert_eq!(
            super::transformed_enclosure(&authored_enclosure(1, 0, changed), 6),
            None
        );
    }
}

#[test]
fn enclosure_metadata_edit_preserves_config_owner_and_opaque_prologue() {
    use cadmpeg_ir::attributes::AttributeValue;
    let values = [
        0.01,
        0.02,
        0.03,
        0.2,
        0.3,
        0.4,
        -0.1,
        -0.2,
        -0.3,
        0.473_913_743_529_674_7,
    ];
    let payload = authored_enclosure(1, 0x53, values);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x43, "Contents/Config-7", &payload));
    let source = crate::test_support::container::native_envelope_fixture(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir = decoded.ir_mut();
        let attribute = ir
            .model
            .attributes
            .iter_mut()
            .find(|a| a.name == "transformed_reference_enclosure")
            .unwrap();
        let AttributeValue::Vector(center) = &mut attribute.values[0] else {
            panic!("center");
        };
        center[0] = cadmpeg_ir::scalar::FiniteReal::new(25.).unwrap();
    }
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&written);
    let members: Vec<_> = scan
        .blocks
        .iter()
        .filter(|b| b.section.name() == Some("Contents/Config-7"))
        .collect();
    assert_eq!(members.len(), 1);
    let bytes = &members[0].payload;
    assert_eq!(&bytes[..52], &payload[..52]);
    assert_eq!(super::transformed_enclosure(bytes, 6).unwrap()[0], 0.025);
    assert_eq!(&bytes[60..], &payload[60..]);
}

fn authored_part_root(words: &[u32]) -> Vec<u8> {
    let mut bytes = vec![255, 255, 1, 0, 8, 0];
    bytes.extend_from_slice(b"moPart_c");
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    let successor: &[u8] = if words.len() == 1 {
        b"moHeader_c"
    } else {
        b"moNodeName_c"
    };
    bytes.extend([255, 255, 1, 0]);
    bytes.extend(u16::try_from(successor.len()).unwrap().to_le_bytes());
    bytes.extend(successor);
    bytes.extend_from_slice(b"authored continuation");
    bytes
}

#[test]
fn unsupported_part_roots_report_loss_and_embedded_tokens_do_not_create_metadata() {
    for (payload, unsupported) in [
        (authored_part_root(&[7, 3, 11]), true),
        (
            b"unqlite\xa5class\xa8moPart_c\0\0\0\0\0\0\0\0\0\0\0\0".to_vec(),
            false,
        ),
        (
            [
                b"\xff\xff\x01\x00\x05\x00Other".as_slice(),
                &authored_part_root(&[7, 0, 11]),
            ]
            .concat(),
            false,
        ),
        (b"moPart_c\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec(), false),
    ] {
        let mut source = outer_header();
        source.extend(make_block(0x43, "AuthoredMetadata", &payload));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
        let mut losses = Vec::new();
        let attributes = super::attributes(
            &ctx,
            &scan,
            &mut cadmpeg_ir::annotations::Annotations::default(),
            &mut losses,
        )
        .unwrap();
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.name == "part_record"));
        assert_eq!(losses.len(), usize::from(unsupported));
        if unsupported {
            assert_eq!(
                losses[0].code.local_code(),
                "metadata.part-root-unsupported"
            );
        }
    }
}

#[test]
fn retained_part_roots_preserve_scalar_form_and_all_other_owned_bytes() {
    use cadmpeg_ir::attributes::AttributeValue;
    for words in [vec![17], vec![29, 0, 37]] {
        let payload = authored_part_root(&words);
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(0x43, "AuthoredPartRoot", &payload));
        let source = crate::test_support::container::native_envelope_fixture(&source);
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        let attribute = decoded
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "part_record")
            .unwrap();
        let mut expected_values = vec![AttributeValue::Integer(i64::from(words[0]))];
        if let Some(last) = words.get(2) {
            expected_values.push(AttributeValue::Integer(i64::from(*last)));
        }
        assert_eq!(attribute.values, expected_values);
        let mut unchanged = Vec::new();
        crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut unchanged,
        )
        .unwrap();
        assert_eq!(unchanged, source);
        let mut edited = decoded.ir_mut();
        let attribute = edited
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == "part_record")
            .unwrap();
        attribute.values[0] = AttributeValue::Integer(73);
        if attribute.values.len() == 2 {
            attribute.values[1] = AttributeValue::Integer(89);
        }
        drop(edited);
        let mut encoded = Vec::new();
        crate::test_support::plan_inherited_write(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut encoded,
        )
        .unwrap();
        let scan = crate::test_support::container::scan(&encoded);
        let block = scan
            .blocks
            .iter()
            .find(|block| block.section.name() == Some("AuthoredPartRoot"))
            .unwrap();
        let mut expected = payload;
        expected[14..18].copy_from_slice(&73u32.to_le_bytes());
        if words.len() == 3 {
            expected[22..26].copy_from_slice(&89u32.to_le_bytes());
        }
        assert_eq!(block.payload, expected);
        let reread = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let values = &reread
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "part_record")
            .unwrap()
            .values;
        assert_eq!(values[0], AttributeValue::Integer(73));
        assert_eq!(values.len(), if words.len() == 1 { 1 } else { 2 });
    }
}

#[test]
fn configuration_manager_roots_are_retained_without_invented_scalar_roles() {
    for width in [113, 139] {
        let mut payload = vec![255, 255, 1, 0, 20, 0];
        payload.extend(b"moConfigurationMgr_c");
        payload.extend(vec![0x35; width]);
        payload.extend([255, 255, 1, 0, 21, 0]);
        payload.extend(b"moPartConfiguration_c");
        payload.extend([0xde, 0xad, 0xbe, 0xef]);
        let mut source = outer_header();
        source.extend(make_block(0x43, "Contents/CMgr", &payload));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut losses = Vec::new();
        let attributes = super::attributes(&ctx, &scan, &mut annotations, &mut losses).unwrap();
        assert!(!attributes.iter().any(|a| a.name == "configuration_manager"));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::SldprtLossCode::MetadataConfigurationManagerOpaque
                .note("")
                .code
        );
        assert!(!annotations
            .provenance
            .keys()
            .any(|id| id.contains(":configuration_manager#")));
        assert_eq!(scan.sections().next().unwrap().payload(), payload);
    }
}

#[test]
fn embedded_or_truncated_manager_tokens_do_not_establish_an_mfc_root() {
    for payload in [
        b"unqlite\xa5class\xb4moConfigurationMgr_c".to_vec(),
        b"moConfigurationMgr_c".to_vec(),
        b"\xff\xff\x01\x00\x14\x00moConfigurationMgr".to_vec(),
    ] {
        let mut source = outer_header();
        source.extend(make_block(0x43, "Opaque", &payload));
        let scan = crate::test_support::container::scan(&source);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut losses = Vec::new();
        let attributes = super::attributes(&ctx, &scan, &mut annotations, &mut losses).unwrap();
        assert!(!attributes.iter().any(|a| a.name == "configuration_manager"));
        assert!(losses.is_empty());
    }
}
