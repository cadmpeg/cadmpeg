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
        assert!(matches!(&bad.body, None));
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

#[test]
fn source_free_partial_box_keeps_its_native_dependency_and_only_omits_the_unheld_length() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="Bad" Count="0"/><ObjectDeps Name="Pocket" Count="1"><Dep Name="Bad"/></ObjectDeps><Object type="Part::Box" name="Bad"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects><ObjectData Count="2"><Object name="Bad"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="NaN"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object><Object name="Pocket"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = decode(archive_entries(&[("Document.xml", document.as_bytes())]));
    let before = decoded.ir().clone();
    let namespace = before.native.namespace("fcstd").unwrap();
    let original_objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let bad = original_objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    let pocket = original_objects
        .iter()
        .find(|object| object.name() == "Pocket")
        .unwrap();
    assert_eq!(bad.type_name, "Part::Box");
    assert_eq!(pocket.dependencies, [bad.id().as_str().to_owned()]);
    let feature = before
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(bad.id().as_str()))
        .unwrap();
    let FeatureOperation::Native { kind, parameters } = feature.definition().operation() else {
        panic!("missing Length preserves the native Box operation")
    };
    assert_eq!(kind.as_str(), "Part::Box");
    assert!(!parameters.contains_key("Length"));
    for name in ["Width", "Height"] {
        let cadmpeg_ir::Dimension::Parameter(id) = &parameters[name] else {
            panic!("readable dimensions retain their canonical parameter")
        };
        assert!(matches!(
            &before.model.parameters.iter().find(|parameter| &parameter.id == id).unwrap().value,
            Some(ParameterValue::Length(value)) if value.get() == 1.0
        ));
    }
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let length = properties
        .iter()
        .find(|property| property.owner == bad.id().as_str() && property.name == "Length")
        .unwrap();
    assert!(matches!(&length.body, None));
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&before, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let omitted =
        cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(String::new());
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == omitted.code && loss.message.contains(&length.id) }));
    assert_eq!(
        plan.report()
            .native_dispositions
            .record_disposition("fcstd", "properties", &length.id),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped,
    );
    for object in [bad, pocket] {
        assert_ne!(
            plan.report().native_dispositions.record_disposition(
                "fcstd",
                "objects",
                object.id().as_str(),
            ),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
        );
    }
    for property in properties.iter().filter(|property| {
        property.owner == bad.id().as_str() && matches!(property.name.as_str(), "Width" | "Height")
    }) {
        assert_ne!(
            plan.report().native_dispositions.record_disposition(
                "fcstd",
                "properties",
                &property.id
            ),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = decode(bytes);
    let objects = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let bad_written = objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    let pocket_written = objects
        .iter()
        .find(|object| object.name() == "Pocket")
        .unwrap();
    assert_eq!(bad_written.type_name, "Part::Box");
    assert_eq!(pocket_written.type_name, "PartDesign::Pocket");
    assert_eq!(
        pocket_written.dependencies,
        [bad_written.id().as_str().to_owned()]
    );
    assert!(objects
        .iter()
        .all(|object| object.dependencies.iter().all(|dependency| {
            objects
                .iter()
                .any(|target| target.id().as_str() == dependency)
        })));
    let output_properties = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(!output_properties.iter().any(|property| {
        property.owner == bad_written.id().as_str() && property.name == "Length"
    }));
    for name in ["Width", "Height"] {
        let property = output_properties
            .iter()
            .find(|property| property.owner == bad_written.id().as_str() && property.name == name)
            .unwrap();
        assert_eq!(property.type_name, "App::PropertyLength");
        assert_eq!(
            property.values()[0].attributes["value"]
                .parse::<f64>()
                .unwrap(),
            1.0
        );
    }
    assert_eq!(decoded.ir().model, before.model);
    assert_eq!(decoded.ir().native, before.native);
}
