// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignOccurrenceAttributeValue,
};

fn frame(attributes: &[(&str, &str, &str)], separator: bool) -> Vec<u8> {
    let plain = attributed(OCCURRENCE, 0, 1);
    let mut bytes = plain[..40].to_vec();
    bytes.extend_from_slice(&u32::try_from(attributes.len()).unwrap().to_le_bytes());
    for (name, kind, value) in attributes {
        push_ascii(&mut bytes, name);
        push_ascii(&mut bytes, kind);
        if *kind == "IntrinsicMetaTypebool" {
            bytes.push(u8::from(*value == "true"));
        } else {
            push_utf16(&mut bytes, value);
        }
    }
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    if separator {
        bytes.push(1);
    }
    let original_guid = plain
        .windows(4)
        .position(|b| b == 36_u32.to_le_bytes())
        .unwrap();
    bytes.extend_from_slice(&plain[original_guid..plain.len() - 11]);
    bytes
}

fn decode(bytes: &[u8]) -> Option<DesignComponentOccurrence> {
    crate::test_support::with_decode_context(|ctx| {
        super::super::attributed_component_occurrence_frame(
            ctx,
            bytes,
            0,
            bytes.len(),
            "f3d:Design/BulkStream.dat",
        )
    })
}

#[test]
fn empty_attributes_retain_the_identity_namespace_and_closing_pose() {
    let bytes = frame(&[], false);
    let occurrence = decode(&bytes).unwrap();
    assert_eq!(occurrence.component_record_index, 187);
    assert_eq!(occurrence.owner_entity, 6);
    assert_eq!(occurrence.occurrence_ordinal(), 1);
    assert_eq!(occurrence.transform().unwrap().value.rows()[0][3], 2.0);
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["attributed"]["attributes"], serde_json::json!([]));
    assert_eq!(wire["attributed"]["key_record_index"], 139);
    assert_eq!(wire["attributed"]["key_segment_index"], 1);
}

#[test]
fn three_attributes_and_guid_separator_retain_the_component_pose() {
    let bytes = frame(
        &[
            ("GatedByParent", "IntrinsicMetaTypebool", "false"),
            ("ResolutionTime", "IntrinsicMetaTypeHString", ""),
            ("SelfGated", "IntrinsicMetaTypebool", "false"),
        ],
        true,
    );
    let occurrence = decode(&bytes).unwrap();
    assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
    assert_eq!(occurrence.occurrence_guid.as_str(), OCCURRENCE);
    assert_eq!(occurrence.occurrence_ordinal(), 1);
    assert_eq!(occurrence.transform().unwrap().value.rows()[2][3], 4.0);
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["attributed"]["guid_marker"], 1);
    assert_eq!(
        wire["attributed"]["attributes"].as_array().unwrap().len(),
        3
    );
    let restored: DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, occurrence);
    for len in 0..bytes.len() {
        assert!(decode(&bytes[..len]).is_none(), "truncated at {len}");
    }
}

#[test]
fn interned_text_keeps_its_type_and_full_record_extent() {
    let text = "a".repeat(512) + "\u{3}\0\u{3833}2\0\0";
    let bytes = frame(
        &[("FusionArrangeProperties", "IntrinsicMetaTypeIString", &text)],
        false,
    );
    let occurrence = decode(&bytes).unwrap();
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(
        wire["attributed"]["attributes"][0]["value"],
        serde_json::to_value(DesignOccurrenceAttributeValue::InternedText(text)).unwrap()
    );
    let restored: DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, occurrence);
}

#[test]
fn an_unknown_attribute_type_or_different_component_stays_native() {
    let mut bytes = frame(&[("Property", "UnknownType", "value")], true);
    assert!(decode(&bytes).is_none());
    bytes = frame(&[], true);
    let repeated = bytes
        .windows(9)
        .rposition(|b| b[0] == 1 && b[1..] == 187_u64.to_le_bytes())
        .unwrap();
    bytes[repeated + 1..repeated + 9].copy_from_slice(&188_u64.to_le_bytes());
    assert!(decode(&bytes).is_none());
}

#[test]
fn a_single_prefix_byte_and_null_identity_reference_close_exactly() {
    let mut bytes = frame(&[], false);
    bytes.remove(44);
    let guid_end = 49 + 76 + 76;
    let mut at = guid_end;
    crate::bytes::take_owned_reference(&bytes, &mut at).unwrap();
    bytes.splice(guid_end..=at, [0]);
    let occurrence = decode(&bytes).unwrap();
    assert_eq!(occurrence.occurrence_ordinal(), 1);
    assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
    assert_eq!(occurrence.transform().unwrap().value.rows()[1][3], 3.0);
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["attributed"]["ordinal_prefix"], 0);
    assert_eq!(wire["attributed"]["key_record_index"], 0);
    assert_eq!(wire["attributed"]["key_segment_index"], 0);
    let restored: DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, occurrence);
}

#[test]
fn property_prelude_retains_names_key_and_equal_component_references() {
    let mut bytes = frame(&[], false);
    bytes.remove(44);
    let mut prelude = vec![0];
    let names = [
        "Appearance",
        "Geometry",
        "PhysicalMaterial",
        "Properties",
        "StructureChange",
        "Transform",
    ];
    prelude.extend_from_slice(&6_u32.to_le_bytes());
    for name in names {
        push_ascii(&mut prelude, name);
    }
    prelude.push(0);
    prelude.push(1);
    prelude.extend_from_slice(&20_u64.to_le_bytes());
    prelude.extend_from_slice(&[0; 2]);
    prelude.push(0);
    prelude.push(1);
    prelude.extend_from_slice(&187_u64.to_le_bytes());
    prelude.extend_from_slice(&[0; 2]);
    push_utf16(&mut prelude, "1_20");
    bytes.splice(44..44, prelude);
    let occurrence = decode(&bytes).unwrap();
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(
        wire["attributed"]["property_names"],
        serde_json::json!(names)
    );
    assert_eq!(wire["attributed"]["persistent_key"], "1_20");
    assert_eq!(occurrence.occurrence_ordinal(), 1);
    assert_eq!(occurrence.transform().unwrap().value.rows()[0][3], 2.0);
    let restored: DesignComponentOccurrence = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, occurrence);
    bytes[24 + 1..24 + 9].copy_from_slice(&188_u64.to_le_bytes());
    assert!(decode(&bytes).is_none());
}
