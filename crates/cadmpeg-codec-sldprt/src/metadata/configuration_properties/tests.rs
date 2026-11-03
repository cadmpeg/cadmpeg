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

fn compound_payload() -> Vec<u8> {
    fn section(records: &[(u32, &str)], aliases: &[(u32, &str)]) -> Vec<u8> {
        let mut values = vec![(1, vec![2, 0, 0, 0, 0xe9, 0xfd, 0, 0])];
        for (pid, text) in records {
            let mut value = vec![30, 0, 0, 0];
            value.extend(u32::try_from(text.len() + 1).unwrap().to_le_bytes());
            value.extend(text.as_bytes());
            value.push(0);
            while !value.len().is_multiple_of(4) {
                value.push(0);
            }
            values.push((*pid, value));
        }
        let mut dictionary = u32::try_from(aliases.len() + 1)
            .unwrap()
            .to_le_bytes()
            .to_vec();
        for (pid, text) in std::iter::once(&(0, "")).chain(aliases) {
            dictionary.extend(pid.to_le_bytes());
            dictionary.extend(u32::try_from(text.len() + 1).unwrap().to_le_bytes());
            dictionary.extend(text.as_bytes());
            dictionary.push(0);
        }
        while !dictionary.len().is_multiple_of(4) {
            dictionary.push(0);
        }
        values.push((0, dictionary));
        let mut out = vec![0; 8 + values.len() * 8];
        out[4..8].copy_from_slice(&u32::try_from(values.len()).unwrap().to_le_bytes());
        for (i, (pid, value)) in values.iter().enumerate() {
            let at = u32::try_from(out.len()).unwrap();
            out[8 + i * 8..12 + i * 8].copy_from_slice(&pid.to_le_bytes());
            out[12 + i * 8..16 + i * 8].copy_from_slice(&at.to_le_bytes());
            out.extend(value);
        }
        let size = u32::try_from(out.len()).unwrap();
        out[..4].copy_from_slice(&size.to_le_bytes());
        out
    }
    let summary = section(&[], &[]);
    let custom = section(
        &[
            (3, "stored"),
            (4, ""),
            (5, "stale"),
            (0x0100_0005, "formula"),
        ],
        &[(3, "Designer"), (4, "Blank"), (5, "Mass")],
    );
    let mut out = vec![0; 68];
    out[..2].copy_from_slice(&0xfffeu16.to_le_bytes());
    out[24..28].copy_from_slice(&2u32.to_le_bytes());
    let fmtid = [
        0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 8, 0, 0x2b, 0x2c, 0xf9, 0xae,
    ];
    out[28..44].copy_from_slice(&fmtid);
    out[44..48].copy_from_slice(&68u32.to_le_bytes());
    out[48..64].copy_from_slice(&fmtid);
    out[48] = 5;
    out[64..68].copy_from_slice(&u32::try_from(68 + summary.len()).unwrap().to_le_bytes());
    out.extend(summary);
    out.extend(custom);
    out
}

fn compound_facts(
    names: &[&str],
    payload: &[u8],
) -> (Vec<SourceAttribute>, Annotations, Vec<LossNote>) {
    let bytes = outer_header();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut scan = crate::container::scan(&ctx, root).unwrap();
    for (i, name) in names.iter().enumerate() {
        scan.compound_streams
            .push(crate::container::CompoundStream {
                path: cadmpeg_ir::StreamName::try_from((*name).to_owned()).unwrap(),
                directory_id: u32::try_from(19 + i).unwrap(),
                start_sector: 0,
                payload: payload.to_vec(),
                decoded_payload: None,
                ps_streams: Vec::new(),
                ps_probe_capped: false,
                ps_auxiliary_refusal: None,
            });
    }
    let (mut rows, mut notes, mut losses) = (Vec::new(), Annotations::default(), Vec::new());
    append(&ctx, &scan, &mut rows, &mut notes, &mut losses).unwrap();
    (rows, notes, losses)
}

#[test]
fn legacy_compound_saved_strings_keep_physical_namespaces_and_withhold_formula_bases() {
    let (rows, notes, losses) = compound_facts(
        &["Config-7-Properties", "Config-9-Properties"],
        &compound_payload(),
    );
    assert_eq!(rows.len(), 4);
    for row in &rows {
        assert_eq!(row.target, AttributeTarget::Document);
        assert_eq!(row.name, ATTRIBUTE_NAME);
        assert_ne!(row.values[4], AttributeValue::String("Mass".into()));
        assert_eq!(row.values[2], AttributeValue::String(FMTID.into()));
        assert_eq!(row.values[5], AttributeValue::String("lpstr".into()));
        let member = if row.values[1] == AttributeValue::Integer(7) {
            "Config-7-Properties"
        } else {
            "Config-9-Properties"
        };
        assert_eq!(row.values[0], AttributeValue::String(member.into()));
        assert_eq!(notes.provenance[row.id.as_str()].stream(), member);
    }
    assert_eq!(
        rows.iter()
            .filter(|row| row.values[6] == AttributeValue::String(String::new()))
            .count(),
        2
    );
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "metadata.configuration-property-value-unsupported"));
}

#[test]
fn legacy_compound_carriers_require_exact_root_names_and_complete_two_set_identity() {
    let payload = compound_payload();
    for name in [
        "nested/Config-7-Properties",
        "Config-07-Properties",
        "docProps/Config-7-Properties.xml",
        "Config-+7-Properties",
        "Config-7-Properties.extra",
    ] {
        let (rows, _, losses) = compound_facts(&[name], &payload);
        assert!(rows.is_empty());
        assert!(losses.is_empty());
    }
    for (at, byte) in [(24, 1), (28, 5), (48, 2), (64, 0)] {
        let mut malformed = payload.clone();
        malformed[at] = byte;
        let (rows, _, losses) = compound_facts(&["Config-7-Properties"], &malformed);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
    let (rows, _, losses) =
        compound_facts(&["Config-7-Properties", "Config-7-Properties"], &payload);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
}

#[test]
fn legacy_compound_alias_collisions_do_not_admit_an_earlier_good_value() {
    let payload = compound_payload();
    let set = usize::try_from(u32::from_le_bytes(payload[64..68].try_into().unwrap())).unwrap();
    let mut duplicate_pid = payload.clone();
    duplicate_pid[set + 24..set + 28].copy_from_slice(&3u32.to_le_bytes());
    let mut duplicate_alias_pid = payload.clone();
    let name = duplicate_alias_pid
        .windows(b"Blank\0".len())
        .position(|window| window == b"Blank\0")
        .unwrap();
    duplicate_alias_pid[name - 8..name - 4].copy_from_slice(&3u32.to_le_bytes());
    for bad in [duplicate_pid, duplicate_alias_pid] {
        let (rows, _, losses) = compound_facts(&["Config-7-Properties"], &bad);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}
