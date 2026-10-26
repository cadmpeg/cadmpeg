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
    let attributes = super::attributes(&ctx, &scan, &mut annotations).unwrap();

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
        let Some(AttributeValue::Vector(center)) = &mut transformed.values[0] else {
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
    let Some(AttributeValue::Vector(values)) = &envelope.values[0] else {
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
    let Some(AttributeValue::Vector(origin)) = &plane.values[0] else {
        panic!("origin")
    };
    let Some(AttributeValue::Vector(frame)) = &plane.values[1] else {
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
            Some(AttributeValue::vector([10.0, 20.0, 30.0]).unwrap()),
            Some(AttributeValue::vector([100.0, 200.0, 300.0]).unwrap()),
            Some(AttributeValue::vector([-100.0, -200.0, -300.0]).unwrap()),
            Some(AttributeValue::float(500.0).unwrap()),
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
        vec![
            Some(AttributeValue::Integer(42)),
            Some(AttributeValue::Integer(2026))
        ]
    );
    let configuration = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "configuration_manager")
        .unwrap();
    assert_eq!(configuration.values[1], Some(AttributeValue::Integer(3)));
    let units = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_code")
        .unwrap();
    assert_eq!(units.values, vec![Some(AttributeValue::Integer(0))]);
    let unit_name = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_name")
        .unwrap();
    assert_eq!(
        unit_name.values,
        vec![Some(AttributeValue::String("IN".into()))]
    );
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
        vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
            "�".into()
        ))]
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
        let Some(AttributeValue::Vector(center)) = &mut attribute.values[0] else {
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
