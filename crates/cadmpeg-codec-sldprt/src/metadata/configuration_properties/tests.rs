// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::test_support::container::{make_block, outer_header};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn xml(records: &str) -> String {
    format!(
        r#"<ConfigProperties xmlns="http://www.solidworks.com/config-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><propertySection xmlns="" fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}">{records}</propertySection></ConfigProperties>"#
    )
}

fn facts(members: &[(&str, &[u8])]) -> (Vec<SourceAttribute>, Annotations, Vec<LossNote>) {
    let mut bytes = outer_header();
    for (name, payload) in members {
        bytes.extend(make_block(0x43, name, payload));
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let (mut out, mut notes, mut losses) = (Vec::new(), Annotations::default(), Vec::new());
    append(&ctx, &scan, &mut out, &mut notes, &mut losses).unwrap();
    (out, notes, losses)
}

const RECORD: &str = r#"<property name="Designer" pid="3" IsEquation="False"><v:lpstr>Merlin</v:lpstr><FPVals><v:lpstr>unproved cached value</v:lpstr></FPVals></property><propertyNameDictionaryElement name="Designer" pid="3"/>"#;
const MEMBER: &str = "docProps/Config-3-Properties.xml";

#[test]
fn member_scoped_literal_fact_does_not_invent_a_configuration_name_or_cached_value() {
    let source = xml(RECORD);
    let (rows, notes, losses) = facts(&[(MEMBER, source.as_bytes())]);
    assert!(losses.is_empty());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target, AttributeTarget::Document);
    assert_eq!(rows[0].name, ATTRIBUTE_NAME);
    assert_eq!(
        rows[0].values,
        vec![
            AttributeValue::String(MEMBER.into()),
            AttributeValue::Integer(3),
            AttributeValue::String(FMTID.into()),
            AttributeValue::Integer(3),
            AttributeValue::String("Designer".into()),
            AttributeValue::String("lpstr".into()),
            AttributeValue::String("Merlin".into()),
        ]
    );
    assert_eq!(notes.provenance[rows[0].id.as_str()].stream(), MEMBER);
}

#[test]
fn identical_pids_in_distinct_namespaces_are_separate_carrier_facts() {
    let source = xml(RECORD);
    let (rows, _, losses) = facts(&[
        (MEMBER, source.as_bytes()),
        ("docProps/Config-7-Properties.xml", source.as_bytes()),
    ]);
    assert!(losses.is_empty());
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].id, rows[1].id);
    assert_eq!(rows[0].values[1], AttributeValue::Integer(3));
    assert_eq!(rows[1].values[1], AttributeValue::Integer(7));
}

#[test]
fn blank_empty_null_and_missing_remain_distinct() {
    let source = xml(
        r#"<property name="Blank" pid="2" IsEquation="False"><v:lpstr/></property><property name="Empty" pid="3" IsEquation="False"><v:empty/></property><property name="Null" pid="4" IsEquation="False"><v:null/></property><property name="Missing" pid="5" IsEquation="False"/>"#,
    );
    let (rows, _, losses) = facts(&[(MEMBER, source.as_bytes())]);
    assert_eq!(rows.len(), 3);
    let find = |name: &str| {
        rows.iter()
            .find(|row| row.values[4] == AttributeValue::String(name.into()))
            .unwrap()
    };
    assert_eq!(find("Blank").values.len(), 7);
    assert_eq!(
        find("Blank").values[6],
        AttributeValue::String(String::new())
    );
    assert_eq!(find("Empty").values.len(), 6);
    assert_eq!(find("Null").values.len(), 6);
    assert_ne!(find("Empty").values[5], find("Null").values[5]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "metadata.configuration-property-value-unsupported"
    );
}

#[test]
fn physical_member_and_root_property_set_must_match() {
    let source = xml(RECORD);
    for name in [
        "Config-3-Properties.xml",
        "nested/docProps/Config-3-Properties.xml",
        "docProps/Config-03-Properties.xml",
        "docProps/Config-+3-Properties.xml",
        "docProps/Config-3-Properties.xml.extra",
    ] {
        let (rows, _, losses) = facts(&[(name, source.as_bytes())]);
        assert!(rows.is_empty());
        assert!(losses.is_empty());
    }
    for wrong in [
        source.replace("config-properties", "other-properties"),
        source.replace(FMTID, "00000000-0000-0000-0000-000000000000"),
        source.replace("ConfigProperties", "Properties"),
    ] {
        let (rows, _, losses) = facts(&[(MEMBER, wrong.as_bytes())]);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}

#[test]
fn duplicate_owners_and_conflicting_record_identities_do_not_choose_a_value() {
    let source = xml(RECORD);
    let (rows, _, losses) = facts(&[(MEMBER, source.as_bytes()), (MEMBER, source.as_bytes())]);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
    for records in [
        format!("{RECORD}{RECORD}"),
        format!("{RECORD}{}", RECORD.replace("Designer", "Other")),
        format!("{RECORD}{}", RECORD.replace("pid=\"3\"", "pid=\"4\"")),
        format!("{RECORD}<property name=\"\" pid=\"3\"><v:lpstr>shadow</v:lpstr></property>"),
        RECORD.replace(
            "propertyNameDictionaryElement name=\"Designer\" pid=\"3\"",
            "propertyNameDictionaryElement name=\"Designer\" pid=\"4\"",
        ),
    ] {
        let source = xml(&records);
        let (rows, _, losses) = facts(&[(MEMBER, source.as_bytes())]);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}

#[test]
fn equations_nested_values_and_unsupported_scalars_stay_opaque() {
    for record in [
        RECORD.replace("IsEquation=\"False\"", "IsEquation=\"True\""),
        RECORD.replace(" IsEquation=\"False\"", ""),
        RECORD.replace("<v:lpstr>Merlin</v:lpstr>", "<v:i4>37</v:i4>"),
        RECORD.replace(
            "<v:lpstr>Merlin</v:lpstr>",
            "<v:lpstr><v:lpstr>Merlin</v:lpstr></v:lpstr>",
        ),
    ] {
        let source = xml(&record);
        let (rows, _, losses) = facts(&[(MEMBER, source.as_bytes())]);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}

#[test]
fn empty_codepage_only_namespace_is_not_a_missing_property() {
    let source = xml(
        r#"<property name="" pid="1"><v:i2>65001</v:i2></property><propertyNameDictionaryElement name="" pid="0"/>"#,
    );
    let (rows, _, losses) = facts(&[(MEMBER, source.as_bytes())]);
    assert!(rows.is_empty());
    assert!(losses.is_empty());
}
