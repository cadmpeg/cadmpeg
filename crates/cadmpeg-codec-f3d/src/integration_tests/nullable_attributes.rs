// SPDX-License-Identifier: Apache-2.0
//! Omitted sparse attributes retain their identities and source slot positions in losses.

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::CadIr;

#[test]
fn unsupported_sparse_attribute_is_named_with_every_missing_source_position() {
    let mut ir = CadIr::empty();
    let attribute = SourceAttribute {
        id: "test:model:attribute#sparse".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "sparse".into(),
        values: vec![Some(AttributeValue::String("held".into())), None, None],
    };
    let id = attribute.id.to_string();
    ir.model.attributes.push(attribute);
    let plan = crate::F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let code = crate::loss::F3dLossCode::WriterAuxiliaryOmitted
        .note("")
        .code;
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code && loss.message.contains(&id))
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 record(s)"));
    assert!(losses[0]
        .message
        .contains("unavailable value positions [1, 2]"));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn sparse_timestamp_omits_the_whole_native_scalar_while_complete_timestamp_round_trips() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::scalar::FiniteReal;
    use std::io::Cursor;

    let mut complete = cadmpeg_ir::examples::unit_cube().expect("admitted authored cube");
    let target = AttributeTarget::Body(complete.model.bodies[0].id.clone());
    complete.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#timestamp".try_into().unwrap(),
        native_ref: None,
        target: target.clone(),
        name: "Timestamp".into(),
        values: vec![
            Some(AttributeValue::String("Timestamp_attrib_def".into())),
            Some(AttributeValue::Integer(1)),
            Some(AttributeValue::Float(FiniteReal::new(42.0).unwrap())),
        ],
    });
    let mut complete_output = Vec::new();
    crate::F3dCodec
        .plan(EncodeInput::new(&complete, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut complete_output)
        .unwrap();
    let complete_decoded = crate::F3dCodec
        .decode(&mut Cursor::new(complete_output), &DecodeOptions::default())
        .unwrap();
    let complete_native = crate::test_support::native_test::f3d_native(complete_decoded.ir());
    assert_eq!(complete_native.creation_timestamps.len(), 1);
    assert_eq!(
        complete_native.creation_timestamps[0].target.as_str(),
        complete_decoded.ir().model.bodies[0]
            .native_ref
            .as_deref()
            .expect("linked timestamp target"),
    );
    assert_eq!(
        complete_native.creation_timestamps[0]
            .unix_microseconds
            .get(),
        42.0
    );

    let mut sparse = complete.clone();
    sparse.model.attributes[0].values.push(None);
    let expected_sparse_attributes = sparse.model.attributes.clone();
    let plan = crate::F3dCodec
        .plan(EncodeInput::new(&sparse, None), TargetRequest::Inherit)
        .unwrap();
    let code = crate::loss::F3dLossCode::WriterAuxiliaryOmitted
        .note("")
        .code;
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code && loss.message.contains("test:model:attribute#timestamp"))
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("unavailable value positions [3]"));
    let mut sparse_output = Vec::new();
    plan.write_to(&mut sparse_output).unwrap();
    let sparse_decoded = crate::F3dCodec
        .decode(&mut Cursor::new(sparse_output), &DecodeOptions::default())
        .unwrap();
    let sparse_native = crate::test_support::native_test::f3d_native(sparse_decoded.ir());
    assert!(sparse_native.creation_timestamps.is_empty());
    assert_eq!(sparse.model.attributes, expected_sparse_attributes);
}
