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
