// SPDX-License-Identifier: Apache-2.0
//! Unavailable property bodies do not supply native-operation operands.
use super::*;
use cadmpeg_ir::features::{FeatureOperation, ParameterValue};
use cadmpeg_ir::CadIr;

fn assert_held_operations(ir: &CadIr) {
    for (name, kind, width) in [
        ("Pad", "PartDesign::Pad", 4.0),
        ("Peer", "Vendor::Peer", 7.0),
    ] {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap();
        let FeatureOperation::Native {
            kind: actual,
            parameters,
        } = feature.definition().operation()
        else {
            panic!("partial operation remains native")
        };
        assert_eq!(actual.as_str(), kind);
        assert!(!parameters.contains_key("Length"));
        let cadmpeg_ir::Dimension::Parameter(id) = &parameters["Width"] else {
            panic!("readable width keeps its canonical parameter")
        };
        let held = ir
            .model
            .parameters
            .iter()
            .find(|parameter| &parameter.id == id)
            .unwrap();
        assert_eq!(held.owner.as_ref(), Some(&feature.id));
        assert!(matches!(&held.value, Some(ParameterValue::Length(value)) if value.get() == width));
    }
}

fn exercise(legacy: bool) {
    let other = if legacy {
        "<Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='0'/></Property>"
    } else {
        ""
    };
    let count = if legacy { 3 } else { 2 };
    let xml = format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='0'/><Objects Count='2'><Object name='Pad' type='PartDesign::Pad'/><Object name='Peer' type='Vendor::Peer'/></Objects><ObjectData Count='2'><Object name='Pad'><Properties Count='{count}'><Property name='Length' type='App::PropertyLength'><Float value='not-a-number'/></Property><Property name='Width' type='App::PropertyLength'><Float value='4'/></Property>{other}</Properties></Object><Object name='Peer'><Properties Count='1'><Property name='Width' type='App::PropertyLength'><Float value='7'/></Property></Properties></Object></ObjectData></Document>");
    let decoded = decode(archive(&xml));
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let bad = properties
        .iter()
        .find(|property| property.owner == "fcstd:native:object#Pad" && property.name == "Length")
        .unwrap();
    if legacy {
        assert!(matches!(
            &bad.body,
            Some(crate::native::PropertyBody::Unreadable(_))
        ));
    } else {
        assert!(bad.body.is_none());
    }
    assert_held_operations(decoded.ir());
    let model = decoded.ir().model.clone();
    let native = decoded.ir().native.clone();
    let plan = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let omission =
        cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(String::new());
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == omission.code && loss.message.contains(&bad.id)));
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("fcstd", "properties", &bad.id),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = decode(bytes);
    assert_held_operations(written.ir());
    let written_properties = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(!written_properties
        .iter()
        .any(|property| property.owner == "fcstd:native:object#Pad" && property.name == "Length"));
    assert_eq!(decoded.ir().model, model);
    assert_eq!(decoded.ir().native, native);
}

#[test]
fn an_absent_body_keeps_native_pad_without_length_and_source_free_writes_its_readable_peer() {
    exercise(false);
}

#[test]
fn an_unreadable_body_cannot_recover_a_native_operand_from_source_and_keeps_its_peer() {
    exercise(true);
}
