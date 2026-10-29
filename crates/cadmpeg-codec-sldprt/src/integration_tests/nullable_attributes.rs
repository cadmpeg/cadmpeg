// SPDX-License-Identifier: Apache-2.0
//! Scalar attribute omissions across public SLDPRT planning and round trips.

use crate::{loss::SldprtLossCode, SldprtCodec};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

fn cube() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.bodies[0].name = None;
    for face in &mut ir.model.faces {
        face.name = None;
    }
    for edge in &mut ir.model.edges {
        edge.set_param_range(None);
    }
    ir
}

#[test]
fn scalar_holes_and_structured_values_have_one_named_omission_per_record() {
    use cadmpeg_ir::attributes::{
        AttributeMapEntry, AttributeTarget, AttributeValue, SourceAttribute,
    };
    let mut ir = cube();
    ir.model.attributes.extend([
        SourceAttribute {
            id: "test:model:attribute#holes".try_into().unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "GapMemo".into(),
            values: vec![
                Some(AttributeValue::String("first".into())),
                None,
                Some(AttributeValue::String("third".into())),
                None,
            ],
        },
        SourceAttribute {
            id: "sldprt:model:attribute#structured".try_into().unwrap(),
            native_ref: None,
            target: AttributeTarget::Document,
            name: "MapMemo".into(),
            values: vec![Some(AttributeValue::Map(vec![AttributeMapEntry {
                key: "a".into(),
                value: "1".into(),
            }]))],
        },
    ]);
    ir.model
        .attributes
        .sort_by(|left, right| left.id.cmp(&right.id));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterAttributesOmitted.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 2);
    assert_eq!(
        losses.iter().map(|loss| loss.message.as_str()).collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "1 attribute record(s) omitted from scalar SLDPRT fields; None positions (zero-based) [1, 3]: test:model:attribute#holes",
            "1 attribute record(s) omitted from scalar SLDPRT fields; None positions (zero-based) []: sldprt:model:attribute#structured",
        ])
    );
    assert!(losses
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Warning));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| matches!(attribute.name.as_str(), "GapMemo" | "MapMemo")));
}

#[test]
fn a_given_up_custom_property_slot_cannot_replay_its_old_string() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let mut authored = cube();
    authored.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#memo".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "Memo".into(),
        values: vec![Some(AttributeValue::String("readable source memo".into()))],
    });
    let initial = SldprtCodec
        .plan(EncodeInput::new(&authored, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    initial.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, fidelity) = decoded.into_parts();
    let memo = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Memo")
        .unwrap();
    assert_eq!(
        memo.values,
        vec![Some(AttributeValue::String("readable source memo".into()))]
    );
    let id = memo.id.clone();
    memo.values.push(None);
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let losses = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterAttributesOmitted.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message, format!("1 attribute record(s) omitted from scalar SLDPRT fields; None positions (zero-based) [1]: {id}"));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let decoded = SldprtCodec
        .decode(&mut std::io::Cursor::new(output), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Memo"));
}
