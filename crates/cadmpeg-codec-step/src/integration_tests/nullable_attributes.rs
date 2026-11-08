// SPDX-License-Identifier: Apache-2.0
//! STEP names each omitted source attribute and every unreadable value position.

use std::io::Cursor;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::loss::StepLossCode;
use crate::StepCodec;

#[test]
fn document_attribute_omissions_name_unrepresentable_values_and_preserve_readable_dri_vri() {
    // #209 §5.2: a representable document attribute is written as CAx-IF
    // DRI/VRI; an attribute whose value shape cannot be held in that form is
    // degraded and named.
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.attributes.extend([
        SourceAttribute {
            id: "test:model:attribute#z-holes".try_into().unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "UnavailableMemo".into(),
            values: vec![
                Some(AttributeValue::String("first source entry".into())),
                None,
                Some(AttributeValue::String("second source entry".into())),
                None,
            ],
        },
        SourceAttribute {
            id: "test:model:attribute#a-readable".try_into().unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "ReadableMemo".into(),
            values: vec![Some(AttributeValue::String("readable source entry".into()))],
        },
    ]);
    let plan = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(
                crate::options::StepSchema::Ap242Edition3
                    .descriptor()
                    .id
                    .as_str(),
            ),
        )
        .unwrap();
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::SourceAttributeNotWritten.kind())
        .collect::<Vec<_>>();
    assert_eq!(omitted.len(), 1);
    assert_eq!(
        omitted[0].message,
        "1 source attribute record(s) were not written to STEP; None positions (zero-based) [1, 3]: test:model:attribute#z-holes"
    );
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DocumentAttributeValueDegraded.kind()
            && loss.message.contains("UnavailableMemo")
    }));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert!(decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "ReadableMemo"));
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "UnavailableMemo"));
}
