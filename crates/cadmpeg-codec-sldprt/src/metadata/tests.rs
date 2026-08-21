// SPDX-License-Identifier: Apache-2.0
//! `SWObjects` document-metadata decode and write-back tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn transformed_reference_plane_requires_fixed_prefix() {
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
        .any(|attribute| attribute.name == "transformed_reference_plane"));
}

#[test]
fn semantic_writer_preserves_transformed_reference_plane_prefix() {
    use cadmpeg_ir::attributes::AttributeValue;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir = decoded.ir_mut();
        let transformed = ir
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == "transformed_reference_plane")
            .unwrap();
        let AttributeValue::Vector(center) = &mut transformed.values[0] else {
            panic!("transformed plane center");
        };
        center[0] = 25.0;
    }

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();

    let scan = container::scan_bytes(&written);
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
    assert_eq!(&payload[offset..offset + 8], &[0xff; 8]);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let transformed = regenerated
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_plane")
        .unwrap();
    assert!(transformed.id.0.ends_with(":147"));
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
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.points[0].position.z += 1.0;

    let expected = decoded
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    assert_eq!(values, &[10.0, 20.0, -30.0, 40.0]);
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
    assert_eq!(origin, &[1.0, 2.0, 3.0]);
    assert_eq!(frame[2], 1.0);
    let transformed = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_plane")
        .expect("transformed reference plane");
    assert!(transformed.id.0.ends_with(":147"));
    assert_eq!(
        transformed.values,
        vec![
            AttributeValue::Vector(vec![10.0, 20.0, 30.0]),
            AttributeValue::Vector(vec![100.0, 200.0]),
            AttributeValue::Vector(vec![1.0, 0.0, -1.0]),
            AttributeValue::Float(500.0),
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
    let configuration = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "configuration_manager")
        .unwrap();
    assert_eq!(configuration.values[1], AttributeValue::Integer(3));
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
