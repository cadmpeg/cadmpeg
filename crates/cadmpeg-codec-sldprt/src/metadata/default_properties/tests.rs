// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use crate::test_support::container::{make_block, outer_header};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

fn xml(records: &str) -> String {
    format!(
        r#"<p:Properties xmlns:p="http://schemas.openxmlformats.org/officeDocument/2006/SolidworksInformation-properties" xmlns:v="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><propertySection fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}">{records}</propertySection></p:Properties>"#
    )
}
fn properties(members: &[(&str, &[u8])]) -> (Vec<SourceAttribute>, Annotations, Vec<LossNote>) {
    let mut source = outer_header();
    for (name, payload) in members {
        source.extend(make_block(0x43, name, payload));
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&source, &arena, &DecodePolicy::service()).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let (mut out, mut notes, mut losses) = (Vec::new(), Annotations::default(), Vec::new());
    append(&ctx, &scan, &mut out, &mut notes, &mut losses).unwrap();
    (out, notes, losses)
}
const MEMBER: &str = "docProps/ISolidWorksInformation.xml";
#[test]
fn saved_aliases_preserve_localized_names_blank_strings_and_scalar_types() {
    let source = xml(
        r#"<property pid="5" name="日期"><v:lpwstr>  2024/1/2  </v:lpwstr></property><propertyNameDictionaryElement pid="5" name="SW-Date"/><property pid="6" name="Blank"><v:lpstr/></property><property pid="7" name="Absent"><v:empty/></property><property pid="8" name="Integer"><v:i4>-37</v:i4></property><property pid="9" name="Flag"><v:bool>true</v:bool></property>"#,
    );
    let (rows, notes, losses) = properties(&[(MEMBER, source.as_bytes())]);
    assert_eq!(rows.len(), 6);
    let find = |name: &str| rows.iter().find(|row| row.name == name).unwrap();
    assert_eq!(
        find("日期").values,
        vec![AttributeValue::String("  2024/1/2  ".into())]
    );
    assert_eq!(
        find("Blank").values,
        vec![AttributeValue::String(String::new())]
    );
    assert_eq!(find("Integer").values, vec![AttributeValue::Integer(-37)]);
    assert_eq!(find("Flag").values, vec![AttributeValue::Boolean(true)]);
    assert!(find("日期").id.as_str().ends_with(":5:%E6%97%A5%E6%9C%9F"));
    assert!(rows
        .iter()
        .all(|row| row.target == AttributeTarget::Document));
    assert_eq!(notes.provenance.len(), 6);
    assert!(losses.is_empty());
    assert!(find("Absent").values.is_empty());
}

#[test]
fn explicit_nil_properties_preserve_identity_without_conflating_missing_values() {
    let source = xml(
        r#"<property pid="2" name="Empty"><v:empty/></property><property pid="3" name="Null"><v:null/></property><property pid="4" name="Blank"><v:lpwstr/></property><property pid="5" name="Missing"/><property pid="6" name="Unknown"><v:unassigned/></property><property pid="7" name="Payload"><v:empty>19</v:empty></property><property pid="8" name="Nested"><v:empty><v:i4>2</v:i4></v:empty></property><property pid="9" name="Attribute"><v:null other="x"/></property>"#,
    );
    let (rows, notes, losses) = properties(&[(MEMBER, source.as_bytes())]);
    assert_eq!(rows.len(), 3);
    for name in ["Empty", "Null"] {
        let row = rows.iter().find(|row| row.name == name).unwrap();
        assert!(row.values.is_empty());
        assert_eq!(
            notes.provenance[row.id.as_str()].tag.as_deref(),
            Some(if name == "Empty" {
                "saved_default_property:empty"
            } else {
                "saved_default_property:null"
            })
        );
    }
    assert_eq!(
        rows.iter().find(|row| row.name == "Blank").unwrap().values,
        vec![AttributeValue::String(String::new())]
    );
    assert_eq!(losses.len(), 5);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "metadata.default-value-unsupported"));
}
#[test]
fn default_namespace_requires_exact_physical_owner_and_xml_namespace() {
    let source = xml(r#"<property pid="2" name="Value"><v:i4>7</v:i4></property>"#);
    for member in [
        "ISolidWorksInformation.xml",
        "nested/docProps/ISolidWorksInformation.xml",
        "docProps/ISolidWorksCustomInformation.xml",
        "docProps/ISolidWorksConfigurationSpecificInformation.xml",
    ] {
        let (rows, _, losses) = properties(&[(member, source.as_bytes())]);
        assert!(rows.is_empty());
        assert!(losses.is_empty());
    }
    let wrong = source.replace("2006/SolidworksInformation-properties", "2006/other");
    let (rows, _, losses) = properties(&[(MEMBER, wrong.as_bytes())]);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
    let (rows, _, losses) = properties(&[(MEMBER, source.as_bytes()), (MEMBER, source.as_bytes())]);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
}
#[test]
fn alias_pid_and_value_pid_collisions_remain_ambiguous_even_for_equal_values() {
    for records in [
        r#"<property pid="2" name="Alias"><v:i4>7</v:i4></property><property pid="3" name="Alias"><v:i4>7</v:i4></property>"#,
        r#"<property pid="2" name="Alias"><v:i4>7</v:i4></property><property pid="2"><v:i4>7</v:i4></property>"#,
    ] {
        let source = xml(records);
        let (rows, _, losses) = properties(&[(MEMBER, source.as_bytes())]);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}
#[test]
fn repeated_same_alias_and_pid_coalesce_without_inventing_a_value() {
    let source = xml(
        r#"<property pid="2" name="Alias"><v:lpstr/></property><propertyNameDictionaryElement pid="2" name="Alias"/><propertyNameDictionaryElement pid="0" name=""/>"#,
    );
    let (rows, _, losses) = properties(&[(MEMBER, source.as_bytes())]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].values, vec![AttributeValue::String(String::new())]);
    assert!(losses.is_empty());
}
#[test]
fn xml_typed_children_must_be_single_and_in_the_value_namespace() {
    for value in [
        "<v:i4>7</v:i4><foreign/>",
        "<i4>7</i4>",
        "<v:i4><v:i4>7</v:i4></v:i4>",
        "<v:i4>2147483648</v:i4>",
        "<v:r8>NaN</v:r8>",
        "<v:bool>-1</v:bool>",
    ] {
        let source = xml(&format!(
            r#"<property pid="2" name="Alias">{value}</property>"#
        ));
        let (rows, _, losses) = properties(&[(MEMBER, source.as_bytes())]);
        assert!(rows.is_empty(), "{value}");
        assert_eq!(losses.len(), 1);
    }
}
#[test]
fn property_xml_respects_retained_byte_limits() {
    let source = xml(r#"<property pid="2" name="Alias"><v:lpstr>value</v:lpstr></property>"#);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    assert!(matches!(
        xml::table(&ctx, source.as_bytes()),
        Err(CodecError::ResourceLimit(_))
    ));
}
fn padded(mut bytes: Vec<u8>) -> Vec<u8> {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}
fn typed_i4(value: i32) -> Vec<u8> {
    let mut out = vec![3, 0, 0, 0];
    out.extend(value.to_le_bytes());
    out
}
fn typed_string(text: &str, codepage: u16) -> Vec<u8> {
    let mut raw = if codepage == 1200 {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        let mut b = text.as_bytes().to_vec();
        b.push(0);
        b
    };
    let count = raw.len() / if codepage == 1200 { 2 } else { 1 };
    let mut out = vec![30, 0, 0, 0];
    out.extend(u32::try_from(count).unwrap().to_le_bytes());
    out.append(&mut raw);
    padded(out)
}
fn legacy(codepage: u16, values: &[(u32, Vec<u8>)], aliases: &[(u32, &str)]) -> Vec<u8> {
    let mut dictionary = Vec::new();
    dictionary.extend(u32::try_from(aliases.len()).unwrap().to_le_bytes());
    for (pid, name) in aliases {
        let raw = if codepage == 1200 {
            name.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            let mut b = name.as_bytes().to_vec();
            b.push(0);
            b
        };
        dictionary.extend(pid.to_le_bytes());
        dictionary.extend(
            u32::try_from(raw.len() / if codepage == 1200 { 2 } else { 1 })
                .unwrap()
                .to_le_bytes(),
        );
        dictionary.extend(raw);
        if codepage == 1200 {
            dictionary = padded(dictionary);
        }
    }
    dictionary = padded(dictionary);
    let mut cp = vec![2, 0, 0, 0];
    cp.extend(codepage.to_le_bytes());
    cp.extend([0, 0]);
    let mut entries = vec![(1, cp)];
    entries.extend_from_slice(values);
    entries.push((0, dictionary));
    let mut section = vec![0; 8 + entries.len() * 8];
    let count = u32::try_from(entries.len()).unwrap();
    section[4..8].copy_from_slice(&count.to_le_bytes());
    for (i, (pid, body)) in entries.iter().enumerate() {
        let at = u32::try_from(section.len()).unwrap();
        let start = 8 + i * 8;
        section[start..start + 4].copy_from_slice(&pid.to_le_bytes());
        section[start + 4..start + 8].copy_from_slice(&at.to_le_bytes());
        section.extend(body);
    }
    let size = u32::try_from(section.len()).unwrap();
    section[..4].copy_from_slice(&size.to_le_bytes());
    let mut out = vec![0; 48];
    out[..2].copy_from_slice(&0xfffeu16.to_le_bytes());
    out[2..4].copy_from_slice(&1u16.to_le_bytes());
    out[24..28].copy_from_slice(&1u32.to_le_bytes());
    out[28..44].copy_from_slice(&[
        0x05, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9,
        0xae,
    ]);
    out[44..48].copy_from_slice(&48u32.to_le_bytes());
    out.extend(section);
    out
}
fn read_legacy(bytes: &[u8]) -> Result<Table, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
    oleps::table(&ctx, bytes)
}
#[test]
fn legacy_utf8_and_utf16_preserve_saved_aliases_and_blank_values() {
    for cp in [65001, 1200] {
        let source = legacy(
            cp,
            &[(2, typed_string("", cp)), (3, typed_i4(-37))],
            &[(0, ""), (2, "日期"), (2, "Date"), (3, "Count")],
        );
        let table = read_legacy(&source).unwrap();
        assert_eq!(table.aliases.len(), 3);
        assert_eq!(
            table.values.iter().find(|v| v.pid == 2).unwrap().value,
            Some(AttributeValue::String(String::new()))
        );
        assert_eq!(
            table.values.iter().find(|v| v.pid == 3).unwrap().value,
            Some(AttributeValue::Integer(-37))
        );
    }
}
#[test]
fn legacy_rejects_bad_directory_bounds_alignment_order_and_overlap() {
    let source = legacy(65001, &[(2, typed_i4(7))], &[(2, "Alias")]);
    for (at, value) in [(52, u32::MAX), (60, 1), (60, 49), (68, 32)] {
        let mut bad = source.clone();
        bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(read_legacy(&bad).is_err(), "{at}:{value}");
    }
}
#[test]
fn legacy_rejects_scalar_tail_bytes_and_missing_string_terminators() {
    let mut scalar = typed_i4(7);
    scalar.extend([1, 0, 0, 0]);
    assert!(read_legacy(&legacy(65001, &[(2, scalar)], &[(2, "Alias")])).is_err());
    let mut value = typed_string("abc", 65001);
    *value.last_mut().unwrap() = 1;
    assert!(read_legacy(&legacy(65001, &[(2, value)], &[(2, "Alias")])).is_err());
}
#[test]
fn legacy_preserves_repeated_value_pids_for_sticky_projection_refusal() {
    let table = read_legacy(&legacy(
        65001,
        &[(2, typed_i4(7)), (2, typed_i4(7))],
        &[(2, "Alias")],
    ))
    .unwrap();
    assert_eq!(table.values.iter().filter(|v| v.pid == 2).count(), 2);
}
#[test]
fn legacy_does_not_guess_an_unsupported_codepage() {
    assert!(matches!(
        read_legacy(&legacy(1252, &[(2, typed_i4(7))], &[(2, "Alias")])),
        Err(CodecError::NotImplemented(_))
    ));
}

#[test]
fn compound_default_owner_requires_the_complete_root_stream_path() {
    let stream = crate::container::CompoundStream {
        path: cadmpeg_ir::StreamName::try_from("ISolidWorksInformation".to_owned()).unwrap(),
        directory_id: 13,
        start_sector: 0,
        payload: legacy(65001, &[(2, typed_i4(7))], &[(2, "Alias")]),
        decoded_payload: None,
        ps_streams: Vec::new(),
        ps_probe_capped: false,
        ps_auxiliary_refusal: None,
    };
    assert!(is_owner(Section::Compound(&stream)));
    for path in [
        "Nested/ISolidWorksInformation",
        "Config-0-Properties",
        "\u{5}DocumentSummaryInformation",
    ] {
        let mut other = stream.clone();
        other.path = cadmpeg_ir::StreamName::try_from(path.to_owned()).unwrap();
        assert!(!is_owner(Section::Compound(&other)));
    }
    let bytes = outer_header();
    let mut scan = crate::test_support::container::scan(&bytes);
    scan.compound_streams.push(stream);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (mut rows, mut notes, mut losses) = (Vec::new(), Annotations::default(), Vec::new());
    append(&ctx, &scan, &mut rows, &mut notes, &mut losses).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].values, vec![AttributeValue::Integer(7)]);
    assert!(losses.is_empty());
}
#[test]
fn repeated_empty_administrative_dictionaries_are_not_one_owner() {
    assert!(read_legacy(&legacy(65001, &[(0, vec![0; 4])], &[])).is_err());
}
#[test]
fn xml_keeps_fmtid_namespaces_separate_and_rejects_repeated_default_sets() {
    let source = xml(r#"<property pid="2" name="Alias"><v:i4>7</v:i4></property>"#);
    let foreign = r#"<propertySection fmtid="{D5CDD502-2E9C-101B-9397-08002B2CF9AE}"><property pid="2" name="Alias"><v:i4>99</v:i4></property></propertySection>"#;
    let mixed = source.replace("</p:Properties>", &format!("{foreign}</p:Properties>"));
    let (rows, _, losses) = properties(&[(MEMBER, mixed.as_bytes())]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].values, vec![AttributeValue::Integer(7)]);
    assert!(losses.is_empty());
    let repeated = mixed.replace("D5CDD502", "D5CDD505");
    let (rows, _, losses) = properties(&[(MEMBER, repeated.as_bytes())]);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
}
#[test]
fn saved_negative_zero_preserves_its_exact_float_bits() {
    let source = xml(r#"<property pid="2" name="Zero"><v:r8>-0.0</v:r8></property>"#);
    let (rows, _, losses) = properties(&[(MEMBER, source.as_bytes())]);
    let AttributeValue::Float(value) = rows[0].values[0] else {
        panic!("float")
    };
    assert_eq!(value.get().to_bits(), (-0.0f64).to_bits());
    assert!(losses.is_empty());
}

#[test]
fn saved_xml_utf16_is_strict_and_does_not_discard_unowned_bytes() {
    let source = xml(r#"<property pid="2" name="Alias"><v:lpstr>scalar</v:lpstr></property>"#);
    let mut encoded = vec![255, 254];
    encoded.extend(source.encode_utf16().flat_map(u16::to_le_bytes));
    let (rows, _, losses) = properties(&[(MEMBER, &encoded)]);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].values,
        vec![AttributeValue::String("scalar".into())]
    );
    assert!(losses.is_empty());
    let mut odd = encoded.clone();
    odd.push(1);
    let mut surrogate = encoded;
    let marker = b"s\0c\0a\0l\0a\0r\0";
    let index = surrogate
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap();
    surrogate[index..index + 2].copy_from_slice(&0xd800u16.to_le_bytes());
    let mut prefixed = vec![0x86];
    prefixed.extend(source.as_bytes());
    for bytes in [odd, surrogate, prefixed] {
        let (rows, _, losses) = properties(&[(MEMBER, &bytes)]);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}
#[test]
fn saved_utf16_text_is_admitted_before_allocating_the_decoded_string() {
    let source = xml(r#"<property pid="2" name="Alias"><v:lpstr>scalar</v:lpstr></property>"#);
    let mut encoded = vec![255, 254];
    encoded.extend(source.encode_utf16().flat_map(u16::to_le_bytes));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    assert!(matches!(
        xml::table(&ctx, &encoded),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(matches!(
        oleps::table(
            &ctx,
            &legacy(1200, &[(2, typed_string("scalar", 1200))], &[(2, "Alias")])
        ),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn explicit_empty_default_namespace_has_the_same_property_semantics_as_no_binding() {
    let prefixed = xml(r#"<property pid="2" name="Alias"><v:i4>7</v:i4></property>"#);
    let empty = prefixed
        .replace("<p:Properties", "<Properties")
        .replace("xmlns:p=", "xmlns=")
        .replace("</p:Properties>", "</Properties>")
        .replace("<propertySection ", "<propertySection xmlns=\"\" ");
    let (expected, _, losses) = properties(&[(MEMBER, prefixed.as_bytes())]);
    assert_eq!(expected.len(), 1);
    assert!(losses.is_empty());
    let (actual, _, losses) = properties(&[(MEMBER, empty.as_bytes())]);
    assert_eq!(actual, expected);
    assert!(losses.is_empty());
    let foreign = empty.replace(
        "<propertySection xmlns=\"\" ",
        "<propertySection xmlns=\"urn:other\" ",
    );
    let (rows, _, losses) = properties(&[(MEMBER, foreign.as_bytes())]);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
}
