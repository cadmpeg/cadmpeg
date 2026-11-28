// SPDX-License-Identifier: Apache-2.0
use super::{decode, make_block, sldprt_with_body_and_envelope, triangle_body};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_test_support::EditableDecodeResult;

use crate::writer::Held;

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
fn write(decoded: &EditableDecodeResult) -> Result<Vec<u8>, cadmpeg_ir::codec::write::EncodeError> {
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
    let before = decoded.source_fidelity().codec_digests["sldprt_swobjects_local_sha256"].clone();
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
    let Some(AttributeValue::Vector(values)) = &mut bbox.values[0] else {
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
    assert_eq!(
        saved.values,
        vec![Some(AttributeValue::String(String::new()))]
    );
    assert_ne!(
        readback.source_fidelity().codec_digests["sldprt_swobjects_local_sha256"],
        before
    );
}
/// The one saved-property carrier in `bytes`.
fn carrier(bytes: &[u8]) -> Vec<u8> {
    let scan = crate::test_support::container::scan(bytes);
    let members = scan
        .sections()
        .filter(|s| s.name() == Some(MEMBER))
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 1);
    members[0].payload().to_vec()
}

/// Every saved-property edit writes the part with the carrier as the source
/// stored it. `export:edit.dropped` names an edited or removed property, and
/// `WriterAttributesOmitted` an added one.
#[test]
fn inherited_writer_keeps_the_carrier_and_names_every_saved_property_edit() {
    for mutation in 0..7 {
        let mut decoded = fixture();
        let mut ir = decoded.ir_mut();
        let i = ir
            .model
            .attributes
            .iter()
            .position(crate::metadata::default_properties::is_attribute)
            .unwrap();
        let mut edited = ir.model.attributes[i].id.as_str().to_owned();
        let mut held = Held::Source;
        match mutation {
            0 => {
                ir.model.attributes.remove(i);
            }
            1 => {
                let mut row = ir.model.attributes[i].clone();
                row.id = AttributeId::mint("sldprt:metadata:default_property#8:4:Added").unwrap();
                edited = row.id.as_str().to_owned();
                held = Held::Nothing;
                ir.model.attributes.push(row);
            }
            2 => ir.model.attributes[i].name = "Renamed".into(),
            3 => ir.model.attributes[i].values = vec![Some(AttributeValue::Integer(7))],
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
                edited = row.id.as_str().to_owned();
                row.values = vec![Some(AttributeValue::float(1.0).unwrap())];
            }
            _ => unreachable!(),
        }
        drop(ir);
        let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
            decoded.ir(),
            decoded.source_fidelity(),
        )
        .unwrap_or_else(|error| panic!("mutation {mutation}: {error}"));
        assert_eq!(carrier(&bytes), XML, "mutation {mutation}");
        losses.assert_charged(held, "saved properties", &edited);
    }
}
#[test]
fn saved_negative_zero_and_zero_are_one_property_value() {
    let mut decoded = fixture();
    let mut ir = decoded.ir_mut();
    let row = ir
        .model
        .attributes
        .iter_mut()
        .filter(is_property)
        .find(|row| row.name == "SavedName")
        .unwrap();
    let [Some(AttributeValue::Float(saved))] = row.values.as_slice() else {
        panic!("SavedName decodes as one float")
    };
    assert_eq!(saved.get().to_bits(), (-0.0_f64).to_bits());
    row.values = vec![Some(AttributeValue::float(0.0).unwrap())];
    drop(ir);
    let bytes = write(&decoded).unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let members = scan
        .sections()
        .filter(|s| s.name() == Some(MEMBER))
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].payload(), XML);
}
/// A unit mirror the default carrier owns is a saved property: the carrier
/// keeps the stored code, and `export:edit.dropped` names the mirror.
#[test]
fn inherited_writer_keeps_the_saved_carrier_and_names_a_changed_unit_mirror() {
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
    mirror.values = vec![Some(AttributeValue::Integer(3))];
    let mirror = mirror.id.as_str().to_owned();
    drop(ir);
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(carrier(&bytes), XML);
    losses.assert_charged(Held::Source, "saved properties", &mirror);
}

/// A document with no source states saved properties no carrier holds: the
/// part is written without them, and the loss names each.
#[test]
fn source_less_writer_writes_the_part_and_names_its_saved_properties() {
    let mut ir = crate::test_support::ir::source_less_cube();
    let id = "sldprt:metadata:default_property#8:2:Saved";
    ir.model.attributes.push(SourceAttribute {
        id: AttributeId::mint(id).unwrap(),
        target: AttributeTarget::Document.into(),
        name: "Saved".into(),
        values: vec![Some(AttributeValue::Integer(7))],
        native_ref: None,
    });
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        cadmpeg_ir::codec::write::EncodeInput::new(&ir, None),
        cadmpeg_ir::codec::write::target::TargetRequest::Unnamed,
    )
    .unwrap();
    crate::test_support::UnwrittenLosses::of(&plan.report().losses).assert_charged(
        Held::Nothing,
        "saved properties",
        id,
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    assert!(!bytes.is_empty());
}

/// A carrier whose retained bytes the fidelity does not hold is not written, and every saved
/// property the document states is named.
#[test]
fn a_carrier_without_retained_bytes_writes_the_part_and_names_every_property() {
    let mut decoded = fixture();
    let carrier_id = decoded
        .source_fidelity()
        .annotations
        .provenance
        .iter()
        .find(|(id, provenance)| {
            id.as_str().starts_with("sldprt:file:block#") && provenance.stream() == MEMBER
        })
        .map(|(id, _)| id.to_string())
        .unwrap();
    crate::test_support::make_retained_record_unavailable(
        decoded.source_fidelity_mut(),
        &carrier_id,
    );
    let saved = {
        let mut ir = decoded.ir_mut();
        let row = ir
            .model
            .attributes
            .iter_mut()
            .filter(is_property)
            .find(|row| row.name == "SavedName")
            .unwrap();
        row.values = vec![Some(AttributeValue::float(1.0).unwrap())];
        row.id.as_str().to_owned()
    };
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    assert!(scan
        .sections()
        .all(|section| section.name() != Some(MEMBER)));
    losses.assert_charged(Held::Nothing, "saved properties", &saved);
}

/// The ids of the saved properties `decoded` states.
fn property_ids(decoded: &EditableDecodeResult) -> Vec<String> {
    decoded
        .ir()
        .model
        .attributes
        .iter()
        .filter(|row| crate::metadata::default_properties::is_attribute(row))
        .map(|row| row.id.as_str().to_owned())
        .collect()
}

/// With a digest-only source image, the carrier's block record still holds
/// its bytes: the write emits the carrier as stored, and an unedited
/// property is written.
#[test]
fn imageless_write_emits_the_carrier_and_charges_no_unedited_property() {
    let mut decoded = fixture();
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let ids = property_ids(&decoded);
    assert_eq!(ids.len(), 2);
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(carrier(&bytes), XML);
    for id in &ids {
        losses.assert_uncharged(id);
    }
}

/// With a digest-only source image, a property removed from the document is
/// still in the emitted carrier, and `export:edit.dropped` names it.
#[test]
fn imageless_write_charges_a_removed_property_the_carrier_keeps() {
    let mut decoded = fixture();
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let ids = property_ids(&decoded);
    decoded
        .ir_mut()
        .model
        .attributes
        .retain(|row| row.id.as_str() != ids[0]);
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(carrier(&bytes), XML);
    losses.assert_charged(Held::Source, "saved properties", &ids[0]);
    losses.assert_uncharged(&ids[1]);
}

const CONFIG_MEMBER: &str = "docProps/Config-0-Properties.xml";
const CONFIG_XML: &[u8] = br#"<ConfigProperties xmlns="http://www.solidworks.com/config-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes" SW_UnitsLinear="0"><propertySection xmlns="" fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"><property name="Finish" pid="3" IsEquation="False"><v:lpstr>Anodized</v:lpstr></property><propertyNameDictionaryElement name="Finish" pid="3"/></propertySection></ConfigProperties>"#;
const CONFIG_PROPERTY: &str = "sldprt:metadata:configuration_property#";
const UNIT_MIRROR: &str = "sldprt:metadata:source_linear_unit_code#";

/// The envelope fixture with a configuration carrier that holds a unit code.
fn configuration_carrier_fixture() -> EditableDecodeResult {
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
    bytes.extend(make_block(0x44, CONFIG_MEMBER, CONFIG_XML));
    decode(crate::test_support::container::native_envelope_fixture(
        &bytes,
    ))
}

fn attribute_ids(decoded: &EditableDecodeResult, prefix: &str) -> Vec<String> {
    decoded
        .ir()
        .model
        .attributes
        .iter()
        .filter(|row| row.id.as_str().starts_with(prefix))
        .map(|row| row.id.as_str().to_owned())
        .collect()
}

fn set_last_value(decoded: &mut EditableDecodeResult, id: &str, value: AttributeValue) {
    let mut ir = decoded.ir_mut();
    let row = ir
        .model
        .attributes
        .iter_mut()
        .find(|row| row.id.as_str() == id)
        .unwrap();
    match row.values.last_mut() {
        Some(last) => *last = Some(value),
        None => row.values.push(Some(value)),
    }
}

/// A configuration carrier is written as stored. Its property and its unit
/// mirror are charged only when edited, also with a digest-only image.
#[test]
fn a_configuration_carrier_charges_only_its_edited_property_and_unit_mirror() {
    let stored = |bytes: &[u8]| {
        crate::test_support::container::scan(bytes)
            .sections()
            .filter(|section| {
                section.name() == Some(CONFIG_MEMBER) && section.payload() == CONFIG_XML
            })
            .count()
    };
    let mut decoded = configuration_carrier_fixture();
    let properties = attribute_ids(&decoded, CONFIG_PROPERTY);
    assert_eq!(properties.len(), 1);
    let owner = properties[0]
        .strip_prefix(CONFIG_PROPERTY)
        .unwrap()
        .split_once(':')
        .unwrap()
        .0
        .to_owned();
    let mirror = attribute_ids(&decoded, UNIT_MIRROR)
        .into_iter()
        .find(|id| id.starts_with(&format!("{UNIT_MIRROR}{owner}:")))
        .unwrap();

    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(stored(&bytes), 1);
    losses.assert_uncharged(&properties[0]);
    losses.assert_uncharged(&mirror);

    set_last_value(
        &mut decoded,
        &properties[0],
        AttributeValue::String("Painted".into()),
    );
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(stored(&bytes), 1);
    losses.assert_charged(Held::Source, "saved properties", &properties[0]);
    losses.assert_uncharged(&mirror);

    let mut decoded = configuration_carrier_fixture();
    set_last_value(&mut decoded, &mirror, AttributeValue::Integer(3));
    let (bytes, losses) = crate::test_support::write_with_unwritten_attributes(
        decoded.ir(),
        decoded.source_fidelity(),
    )
    .unwrap();
    assert_eq!(stored(&bytes), 1);
    losses.assert_charged(Held::Source, "saved properties", &mirror);
    losses.assert_uncharged(&properties[0]);
}

#[test]
fn saved_properties_from_the_retained_image_fit_the_images_input_ceiling() {
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    let mut decoded = fixture();
    let mut ir = decoded.ir_mut();
    let property = ir
        .model
        .attributes
        .iter_mut()
        .find(|row| row.name == "bounding_envelope" && is_property(row))
        .unwrap();
    property.values = vec![Some(AttributeValue::String("edited".into()))];
    drop(ir);
    let image = decoded
        .source_fidelity()
        .retained_record(crate::SOURCE_IMAGE_ID)
        .and_then(|record| record.data())
        .unwrap();
    let limits = cadmpeg_core::decode::ResourceLimits {
        max_input_bytes: cadmpeg_core::decode::u64_from_index(image.len()),
        ..cadmpeg_core::decode::ResourceLimits::desktop()
    };
    let plan = crate::SldprtCodec
        .plan(EncodeInput {
            limits,
            ..EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()))
        })
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    let mut output = Vec::new();
    plan.write_to(&mut output).unwrap();
    let restored = decode(output);
    assert!(restored.ir().model.attributes.iter().any(|row| {
        crate::metadata::default_properties::is_attribute(row)
            && row.name == "bounding_envelope"
            && row.values == [Some(AttributeValue::String(String::new()))]
    }));
}

#[test]
fn a_separately_supplied_property_carrier_consumes_more_input() {
    use cadmpeg_core::decode::{u64_from_index, LimitScope, ResourceDimension, ResourceLimits};
    use cadmpeg_ir::codec::write::{EncodeError, EncodeInput, Encoder};
    use cadmpeg_ir::RetainedSourceRecord;

    let mut decoded = fixture();
    let fidelity = decoded.source_fidelity_mut();
    let image_len = fidelity
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .unwrap()
        .len();
    let image = fidelity
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .unwrap();
    let scan = crate::test_support::container::scan(image);
    let member = scan
        .sections()
        .find(|member| member.name() == Some(MEMBER))
        .unwrap();
    let id = member.native_id();
    let mut distinct = member.payload().to_vec();
    distinct.push(b' ');
    fidelity.remove_retained_record(id.as_str()).unwrap();
    fidelity
        .insert_retained_record(
            id,
            RetainedSourceRecord::whole(
                cadmpeg_ir::stream_name!("docProps/ISolidWorksInformation.xml"),
                distinct,
            ),
        )
        .unwrap();
    let mut ir = decoded.ir_mut();
    let row = ir
        .model
        .attributes
        .iter_mut()
        .find(|row| crate::metadata::default_properties::is_attribute(row))
        .unwrap();
    row.values = vec![Some(AttributeValue::String("edited".into()))];
    drop(ir);
    let plan = crate::SldprtCodec.plan(EncodeInput {
        limits: ResourceLimits {
            max_input_bytes: u64_from_index(image_len),
            ..ResourceLimits::desktop()
        },
        ..EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity()))
    });
    let Err(EncodeError::ResourceLimit(limit)) = plan else {
        panic!("the independently supplied carrier exceeds the remaining input budget");
    };
    assert_eq!(
        (limit.dimension, limit.scope),
        (ResourceDimension::InputBytes, LimitScope::Session)
    );
}
