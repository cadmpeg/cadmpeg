// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use serde_json::{json, Value};

use super::{CadirRead, ReadFailure, ReadOptions};
use crate::examples::unit_cube;
use crate::report::loss::LossNote;
use crate::report::strictness::Strictness;
use crate::report::Severity;
use crate::CadIr;

fn wire() -> (CadIr, Value) {
    let ir = unit_cube().unwrap();
    let value = serde_json::to_value(&ir).unwrap();
    (ir, value)
}

fn read(bytes: &[u8]) -> CadirRead {
    CadIr::read(bytes, &ReadOptions::default()).unwrap()
}

fn codes(report: &[LossNote]) -> Vec<String> {
    report
        .iter()
        .map(|entry| format!("{}/{}", entry.code.namespace(), entry.code.local_code()))
        .collect()
}

fn points(value: &mut Value) -> &mut Vec<Value> {
    value["geometry"]["points"].as_array_mut().unwrap()
}

#[test]
fn canonical_text_reads_with_an_empty_report() {
    let (ir, value) = wire();
    let read = read(value.to_string().as_bytes());
    assert_eq!(read.document, ir);
    assert!(read.report.is_empty(), "{:?}", read.report);
}

#[test]
fn an_arena_out_of_order_is_sorted_and_the_strict_reader_refuses_it() {
    let (ir, mut value) = wire();
    points(&mut value).swap(0, 1);
    let text = value.to_string();
    let read = read(text.as_bytes());
    assert_eq!(read.document, ir);
    assert_eq!(codes(&read.report), ["read/spelling.normalized"]);
    assert!(CadIr::from_json(&text).is_err());

    let strict = ReadOptions {
        strictness: Strictness {
            floor: Some(Severity::Info),
            ..Strictness::default()
        },
        ..ReadOptions::default()
    };
    assert!(matches!(
        CadIr::read(text.as_bytes(), &strict),
        Err(ReadFailure::StrictRejected { entry: 0, .. })
    ));
}

#[test]
fn an_id_restated_with_one_value_is_read_once() {
    let (ir, mut value) = wire();
    let copy = points(&mut value)[0].clone();
    points(&mut value).insert(1, copy);
    let text = value.to_string();
    let read = read(text.as_bytes());
    assert_eq!(read.document, ir);
    assert_eq!(codes(&read.report), ["read/spelling.normalized"]);
    assert!(CadIr::from_json(&text).is_err());
}

#[test]
fn an_id_restated_with_different_values_keeps_no_copy() {
    let (ir, mut value) = wire();
    let mut copy = points(&mut value)[0].clone();
    copy["native_ref"] = json!("another record");
    points(&mut value).insert(1, copy);
    let read = read(value.to_string().as_bytes());
    assert_eq!(
        read.document.geometry.points.len(),
        ir.geometry.points.len() - 1
    );
    assert_eq!(codes(&read.report), ["read/record.repeated"]);
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 2);
}

#[test]
fn a_member_spelled_in_another_case_is_read_and_an_unknown_member_is_left_out() {
    let (ir, mut value) = wire();
    let point = points(&mut value)[0].as_object_mut().unwrap();
    let position = point.remove("position").unwrap();
    point.insert("Position".into(), position);
    point.insert("extra".into(), json!(1));
    let root = value.as_object_mut().unwrap();
    let fragment = root.remove("fragment").unwrap();
    root.insert("Fragment".into(), fragment);
    root.insert("extra".into(), json!(1));
    root.insert("repro".into(), Value::Null);
    let read = read(value.to_string().as_bytes());
    assert_eq!(read.document, ir);
    let mut codes = codes(&read.report);
    codes.sort();
    assert_eq!(
        codes,
        [
            "read/member.ignored",
            "read/member.ignored",
            "read/spelling.normalized",
            "read/spelling.normalized",
            "read/spelling.normalized",
        ]
    );
}

#[test]
fn an_invalid_utf8_sequence_in_a_string_is_replaced() {
    let mut ir = unit_cube().unwrap();
    ir.geometry.points[0].native_ref = Some("ab".into());
    let text = serde_json::to_string(&ir).unwrap();
    let marker = "\"native_ref\":\"ab\"";
    assert_eq!(text.matches(marker).count(), 1);
    let bytes = text
        .replace(marker, "\"native_ref\":\"a\u{1}b\"")
        .into_bytes()
        .into_iter()
        .map(|byte| if byte == 1 { 0xFF } else { byte })
        .collect::<Vec<u8>>();
    let read = read(&bytes);
    assert_eq!(
        read.document.geometry.points[0].native_ref.as_deref(),
        Some("a\u{FFFD}b")
    );
    assert_eq!(codes(&read.report), ["read/value.normalized"]);
}

#[test]
fn a_record_its_type_refuses_is_given_up_and_the_rest_is_read() {
    let (ir, mut value) = wire();
    points(&mut value)[0]["position"] = json!("not a point");
    let read = read(value.to_string().as_bytes());
    assert_eq!(read.document.geometry.points[..], ir.geometry.points[1..]);
    assert_eq!(codes(&read.report), ["decode/record.unreadable"]);
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 1);
}

#[test]
fn text_that_stops_being_json_keeps_every_record_read_before_it() {
    let ir = unit_cube().unwrap();
    let text = serde_json::to_string(&ir).unwrap();
    let second = serde_json::to_string(&ir.geometry.points[1]).unwrap();
    let cut = text.find(&second).unwrap() + second.len() / 2;
    let read = read(&text.as_bytes()[..cut]);
    assert_eq!(read.document.geometry.points[..], ir.geometry.points[..1]);
    assert!(codes(&read.report).contains(&"decode/record.unreadable".to_owned()));
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 1);
}

#[test]
fn another_version_other_units_and_no_object_refuse_the_read() {
    let (_, mut value) = wire();
    value["ir_version"] = json!("10");
    assert!(matches!(
        CadIr::read(value.to_string().as_bytes(), &ReadOptions::default()),
        Err(ReadFailure::Version(_))
    ));
    let (_, mut value) = wire();
    value.as_object_mut().unwrap().remove("units");
    assert!(matches!(
        CadIr::read(value.to_string().as_bytes(), &ReadOptions::default()),
        Err(ReadFailure::Units(_))
    ));
    assert!(matches!(
        CadIr::read(b"[]", &ReadOptions::default()),
        Err(ReadFailure::Malformed(_))
    ));
}

/// The text of `ir` with the last character of the id of point 0, in that
/// point's record, made an invalid UTF-8 byte.
fn point_id_corrupted(ir: &CadIr) -> Vec<u8> {
    let text = serde_json::to_string(ir).unwrap();
    let point = serde_json::to_string(&ir.geometry.points[0]).unwrap();
    assert_eq!(text.matches(&point).count(), 1);
    let id = ir.geometry.points[0].id.as_str();
    let stated = format!("\"id\":\"{id}\"");
    let corrupt = format!("\"id\":\"{}\u{1}\"", &id[..id.len() - 1]);
    let point_corrupt = point.replacen(&stated, &corrupt, 1);
    assert_ne!(point, point_corrupt);
    text.replacen(&point, &point_corrupt, 1)
        .into_bytes()
        .into_iter()
        .map(|byte| if byte == 1 { 0xFF } else { byte })
        .collect()
}

#[test]
fn an_id_with_an_invalid_utf8_sequence_is_given_up_and_the_rest_is_read() {
    let ir = unit_cube().unwrap();
    let read = read(&point_id_corrupted(&ir));
    assert_eq!(read.document.geometry.points[..], ir.geometry.points[1..]);
    assert_eq!(codes(&read.report), ["decode/record.unreadable"]);
    assert!(read.report[0].message.contains("geometry.points[0]"));
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 1);
}

#[test]
fn an_id_whose_format_is_no_codec_id_is_given_up_and_the_rest_is_read() {
    for format in ["Step", "st-ep", "st\u{e9}p"] {
        let (ir, mut value) = wire();
        let id = ir.geometry.points[0].id.as_str();
        let (_, rest) = id.split_once(':').unwrap();
        points(&mut value)[0]["id"] = json!(format!("{format}:{rest}"));
        let read = read(value.to_string().as_bytes());
        assert_eq!(
            read.document.geometry.points[..],
            ir.geometry.points[1..],
            "{format}"
        );
        assert_eq!(
            codes(&read.report),
            ["decode/record.unreadable"],
            "{format}"
        );
        assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 1);
    }
}

#[test]
fn an_omitted_collection_beside_an_unknown_member_is_given_up() {
    let (ir, mut value) = wire();
    let root = value.as_object_mut().unwrap();
    let geometry = root.remove("geometry").unwrap();
    root.insert("geom".into(), geometry);
    let read = read(value.to_string().as_bytes());
    assert!(read.document.geometry.points.is_empty());
    assert_eq!(read.document.model, ir.model);
    let mut codes = codes(&read.report);
    codes.sort();
    assert_eq!(codes, ["decode/record.unreadable", "read/member.ignored"]);
    assert!(read
        .report
        .iter()
        .all(|entry| !entry.message.contains("is read as empty")));
}

#[test]
fn a_member_in_another_case_beside_its_exact_name_is_one_member_stated_twice() {
    let (ir, mut value) = wire();
    let root = value.as_object_mut().unwrap();
    let fragment = root["fragment"].clone();
    root.insert("Fragment".into(), fragment);
    let read_one = read(value.to_string().as_bytes());
    assert_eq!(read_one.document, ir);
    assert_eq!(
        codes(&read_one.report),
        ["read/spelling.normalized", "read/spelling.normalized"]
    );

    let root = value.as_object_mut().unwrap();
    let other = !root["fragment"].as_bool().unwrap();
    root.insert("Fragment".into(), json!(other));
    let read_two = read(value.to_string().as_bytes());
    let mut codes_two = codes(&read_two.report);
    codes_two.sort();
    assert_eq!(
        codes_two,
        ["read/record.repeated", "read/spelling.normalized"]
    );

    let (ir, mut value) = wire();
    let point = points(&mut value)[0].as_object_mut().unwrap();
    point.insert("Position".into(), json!({"x": 9.0, "y": 9.0, "z": 9.0}));
    let read_record = read(value.to_string().as_bytes());
    assert_eq!(
        read_record.document.geometry.points[..],
        ir.geometry.points[1..]
    );
    assert!(codes(&read_record.report)
        .iter()
        .all(|code| code == "read/record.repeated"));
}

#[test]
fn a_key_restated_with_different_values_in_a_kept_record_is_given_up_as_parts() {
    let mut ir = unit_cube().unwrap();
    ir.geometry.points[0].native_ref = None;
    let text = serde_json::to_string(&ir).unwrap();
    let point = serde_json::to_string(&ir.geometry.points[0]).unwrap();
    assert_eq!(text.matches(&point).count(), 1);
    let restated = format!(
        "{},\"native_ref\":\"a\",\"native_ref\":\"b\"}}",
        &point[..point.len() - 1]
    );
    let read = read(text.replacen(&point, &restated, 1).as_bytes());
    assert_eq!(read.document.geometry, ir.geometry);
    assert_eq!(codes(&read.report), ["read/record.repeated"]);
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 2);
}
