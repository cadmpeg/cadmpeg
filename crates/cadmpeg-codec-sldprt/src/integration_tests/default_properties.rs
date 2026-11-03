// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::{decode, make_block, sldprt_with_body_and_envelope, triangle_body};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_test_support::EditableDecodeResult;
const MEMBER: &str = "docProps/ISolidWorksInformation.xml";
const XML:&[u8]=br#"<p:Properties xmlns:p="http://schemas.openxmlformats.org/officeDocument/2006/SolidworksInformation-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes" SW_UnitsLinear="0"><propertySection fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"><property pid="2" name="bounding_envelope"><v:lpstr/></property><property pid="3" name="SavedName"><v:r8>-0.0</v:r8></property></propertySection></p:Properties>"#;
fn fixture() -> EditableDecodeResult {
    let original = sldprt_with_body_and_envelope(&triangle_body());
    let scan = crate::test_support::container::scan(&original);
    let mut bytes = crate::test_support::container::outer_header();
    for block in &scan.blocks {
        bytes.extend(make_block(
            block.type_id,
            block.section.name().unwrap(),
            &block.payload,
        ));
    }
    bytes.extend(make_block(0x43, MEMBER, XML));
    decode(crate::test_support::container::native_envelope_fixture(
        &bytes,
    ))
}

#[test]
fn block_appended_after_completed_native_directory_remains_malformed() {
    use cadmpeg_ir::codec::Codec;
    let mut bytes = sldprt_with_body_and_envelope(&triangle_body());
    bytes.extend(make_block(0x43, MEMBER, XML));
    assert!(crate::SldprtCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default()
        )
        .is_err());
}
fn is_property(row: &&mut SourceAttribute) -> bool {
    crate::metadata::default_properties::is_attribute(row)
}
fn write(decoded: &EditableDecodeResult) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut out)?;
    Ok(out)
}
#[test]
fn semantic_metadata_edit_retains_saved_default_bytes_and_aliases() {
    let mut decoded = fixture();
    assert_eq!(
        decoded
            .ir()
            .model
            .attributes
            .iter()
            .filter(|row| crate::metadata::default_properties::is_attribute(row))
            .count(),
        2
    );
    let before = decoded
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .get("sldprt_swobjects_local_sha256")
        .unwrap()
        .clone();
    let mut ir = decoded.ir_mut();
    let bbox = ir
        .model
        .attributes
        .iter_mut()
        .find(|row| {
            row.name == "bounding_envelope"
                && !crate::metadata::default_properties::is_attribute(row)
        })
        .unwrap();
    let AttributeValue::Vector(values) = &mut bbox.values[0] else {
        panic!("bbox")
    };
    values[3] = cadmpeg_ir::scalar::FiniteReal::new(41.0).unwrap();
    drop(ir);
    let bytes = write(&decoded).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let members = scan
        .sections()
        .filter(|s| s.name() == Some(MEMBER))
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].payload(), XML);
    let readback = decode(bytes);
    let saved = readback
        .ir()
        .model
        .attributes
        .iter()
        .find(|row| {
            row.name == "bounding_envelope"
                && crate::metadata::default_properties::is_attribute(row)
        })
        .unwrap();
    assert_eq!(saved.values, vec![AttributeValue::String(String::new())]);
    assert_ne!(
        readback
            .ir()
            .source
            .as_ref()
            .unwrap()
            .attributes
            .get("sldprt_swobjects_local_sha256")
            .unwrap(),
        &before
    );
}
#[test]
fn inherited_writer_refuses_every_saved_property_edit() {
    for mutation in 0..7 {
        let mut decoded = fixture();
        let mut ir = decoded.ir_mut();
        let i = ir
            .model
            .attributes
            .iter()
            .position(crate::metadata::default_properties::is_attribute)
            .unwrap();
        match mutation {
            0 => {
                ir.model.attributes.remove(i);
            }
            1 => {
                let mut row = ir.model.attributes[i].clone();
                row.id = AttributeId::mint("sldprt:metadata:default_property#8:4:Added").unwrap();
                ir.model.attributes.push(row);
            }
            2 => ir.model.attributes[i].name = "Renamed".into(),
            3 => ir.model.attributes[i].values = vec![AttributeValue::Integer(7)],
            4 => ir.model.attributes[i].id = AttributeId::mint("other:metadata:saved#0").unwrap(),
            5 => ir
                .model
                .attributes
                .retain(|row| !crate::metadata::default_properties::is_attribute(row)),
            6 => {
                let row = ir
                    .model
                    .attributes
                    .iter_mut()
                    .filter(is_property)
                    .find(|row| row.name == "SavedName")
                    .unwrap();
                row.values = vec![AttributeValue::float(0.0).unwrap()];
            }
            _ => unreachable!(),
        }
        drop(ir);
        assert!(
            matches!(write(&decoded), Err(CodecError::NotImplemented(message)) if message == "SLDPRT saved-property edits require a retained property serializer"),
            "mutation {mutation}"
        );
    }
}
#[test]
fn inherited_writer_refuses_changed_unit_mirrors_in_the_saved_carrier() {
    let mut decoded = fixture();
    let mut ir = decoded.ir_mut();
    let owner = ir
        .model
        .attributes
        .iter()
        .find(|row| crate::metadata::default_properties::is_attribute(row))
        .unwrap()
        .id
        .as_str()
        .split_once('#')
        .unwrap()
        .1
        .split_once(':')
        .unwrap()
        .0
        .to_owned();
    let prefix = format!("sldprt:metadata:source_linear_unit_code#{owner}:");
    let mirror = ir
        .model
        .attributes
        .iter_mut()
        .find(|row| row.id.as_str().starts_with(&prefix))
        .unwrap();
    mirror.values = vec![AttributeValue::Integer(3)];
    drop(ir);
    assert!(
        matches!(write(&decoded), Err(CodecError::NotImplemented(message)) if message == "SLDPRT saved-property edits require a retained property serializer")
    );
}
#[test]
fn source_less_writer_refuses_saved_properties() {
    let mut ir = crate::test_support::ir::source_less_cube();
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint("sldprt:metadata:default_property#8:2:Saved").unwrap(),
        target: AttributeTarget::Document,
        name: "Saved".into(),
        values: vec![AttributeValue::Integer(7)],
    });
    let mut bytes = Vec::new();
    assert!(
        matches!(crate::writer::write_semantic_with_records(&ir, &cadmpeg_ir::annotations::Annotations::default(), &[], &mut bytes), Err(CodecError::NotImplemented(message)) if message == "SLDPRT saved properties require their retained source carrier")
    );
    assert!(bytes.is_empty());
}
