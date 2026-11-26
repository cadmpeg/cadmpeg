// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::serde_erase::json;
use serde_json::{json, Value};

use super::{CadirRead, ReadFailure, ReadOptions};
use crate::examples::unit_cube;
use crate::report::loss::LossNote;
use crate::report::strictness::Strictness;
use crate::report::Severity;
use crate::CadIr;

fn wire() -> (CadIr, Value) {
    let ir = unit_cube().unwrap();
    let value = json::to_value(&ir).unwrap();
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

/// A scope that breaks a rule of §2.1 is read and written as stated; the
/// breach is a part 10 finding (§3.3).
#[test]
fn a_scope_that_breaks_its_rules_is_read_as_written() {
    let (_, mut value) = wire();
    for scope in [
        json!({"model": false, "design": true, "process": false}),
        json!({"model": true, "design": false, "process": true}),
        json!({"model": false, "design": false, "process": false}),
    ] {
        value["scope"] = scope.clone();
        let read = read(value.to_string().as_bytes());
        assert!(read.report.is_empty(), "{:?}", read.report);
        assert_eq!(json::to_value(read.document.scope()).unwrap(), scope);
        assert!(!read.document.scope_violations().is_empty());
        assert_eq!(json::to_value(&read.document).unwrap()["scope"], scope);
    }
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
    let text = json::to_string(&ir).unwrap();
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
    let reason = &read.report[0].message;
    assert!(reason.contains("invalid type"), "{reason}");
    assert!(!reason.contains(" at line "), "{reason}");
    assert_eq!(read.document.native_unknowns("cadir").unwrap().len(), 1);
}

#[test]
fn text_that_stops_being_json_keeps_every_record_read_before_it() {
    let ir = unit_cube().unwrap();
    let text = json::to_string(&ir).unwrap();
    let second = json::to_string(&ir.geometry.points[1]).unwrap();
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

/// A given-up part retains the canonical JSON of its text in the read's
/// fidelity, under the id of its `unknowns` stub, and the stub links to the
/// document (§3.3).
#[test]
fn a_given_up_part_retains_the_canonical_json_of_its_text() {
    let (_, mut value) = wire();
    points(&mut value)[0]["position"] = json!("not a point");
    let record = points(&mut value)[0].clone();
    let read = read(json::to_string_pretty(&value).unwrap().as_bytes());
    let stubs = read.document.native_unknowns("cadir").unwrap();
    let [stub] = stubs.as_slice() else {
        panic!("one stub: {stubs:?}");
    };
    assert!(stub.links.is_empty());
    let retained = read.fidelity.retained_record(stub.id.as_str()).unwrap();
    let text = crate::hash::finite_json::to_canonical_json_string(&record).unwrap();
    assert_eq!(retained.data(), Some(text.as_bytes()));
}

/// The ids a give-up entry lists under `field`.
fn listed<'m>(message: &'m str, field: &str) -> Vec<&'m str> {
    message
        .split(&format!("{field}=["))
        .nth(1)
        .and_then(|tail| tail.split(']').next())
        .map(|list| list.split(", ").filter(|id| !id.is_empty()).collect())
        .unwrap_or_default()
}

/// A sidecar read with the document loses the annotations of each record
/// the read gives up and of each record it drops with one, keeps those of a
/// record the read keeps, and gains the retained text of each part the read
/// gives up (§3.3).
#[test]
fn a_given_up_record_takes_the_annotations_of_its_sidecar() {
    let (ir, mut value) = wire();
    let point = ir.geometry.points[0].id.as_str().to_owned();
    points(&mut value)[0]["position"] = json!("not a point");
    let read = read(value.to_string().as_bytes());
    let [entry] = read.report.as_slice() else {
        panic!("one entry: {:?}", read.report);
    };
    let dropped = listed(&entry.message, "dropped");
    let marked = listed(&entry.message, "marked");
    assert!(dropped.contains(&point.as_str()), "{}", entry.message);
    let [kept, ..] = marked.as_slice() else {
        panic!("a marked record: {}", entry.message);
    };
    let kept = (*kept).to_owned();
    let dropped: Vec<String> = dropped.into_iter().map(str::to_owned).collect();
    let stub = read.document.native_unknowns("cadir").unwrap()[0]
        .id
        .clone();
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    let mut annotations = crate::AnnotationBuilder::new();
    for id in dropped.iter().chain([&kept]) {
        annotations.note(id, &stream, 0, None);
    }
    let mut sidecar =
        crate::SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"test source"));
    sidecar.annotations = annotations.build();

    let (document, report, fidelity) = read.with_sidecar(sidecar).unwrap();

    let keys: Vec<&str> = fidelity
        .annotations
        .provenance
        .keys()
        .map(crate::annotations::AnnotationKey::as_str)
        .collect();
    assert_eq!(keys, [kept.as_str()]);
    assert!(fidelity.retained_record(stub.as_str()).is_some());
    assert_eq!(report.len(), 1);
    assert!(document
        .geometry
        .points
        .iter()
        .all(|kept| kept.id.as_str() != point));
}

/// A sidecar read with the document loses the annotations of each record
/// of a stored state's delta that the read drops with a given-up record,
/// under the record's key in that state, and keeps those of the state
/// (§3.2, §3.3).
#[test]
fn a_given_up_record_takes_the_annotations_of_the_delta_records_it_drops() {
    let mut ir = crate::validate::design::fixture::document();
    let face = ir.geometry.faces[0].clone();
    let shell = face.shell.clone();
    let own = crate::topology::Face {
        id: crate::ids::FaceId::mint("test:design:face#delta").unwrap(),
        ..face.clone()
    };
    let evaluation = ir.design.evaluations[0].id.clone();
    ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta
        .records
        .faces = vec![face.clone(), own.clone()];
    ir.finalize();
    let mut value = json::to_value(&ir).unwrap();
    value["geometry"]["shells"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record["id"] == json!(shell.as_str()))
        .expect("the face's shell")["region"] = json!([]);
    let read = read(value.to_string().as_bytes());
    assert!(
        codes(&read.report).contains(&"decode/record.unreadable".to_owned()),
        "{:?}",
        read.report
    );

    let qualified = [
        crate::annotations::AnnotationKey::in_delta(&evaluation, &face.id),
        crate::annotations::AnnotationKey::in_delta(&evaluation, &own.id),
    ];
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    let mut annotations = crate::AnnotationBuilder::new();
    for key in qualified
        .iter()
        .cloned()
        .chain([crate::annotations::AnnotationKey::from(&evaluation)])
    {
        annotations.note(&key, &stream, 0, None);
    }
    let mut sidecar =
        crate::SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"test source"));
    sidecar.annotations = annotations.build();

    let (_, _, fidelity) = read.with_sidecar(sidecar).unwrap();

    let keys: Vec<&str> = fidelity
        .annotations
        .provenance
        .keys()
        .map(crate::annotations::AnnotationKey::as_str)
        .collect();
    assert_eq!(keys, [evaluation.as_str()]);
}

/// A release that stops on an error after it dropped records keeps the id
/// of each record it dropped, so a sidecar read with the document loses
/// their annotations (§3.3).
#[test]
fn a_release_that_stops_on_an_error_keeps_the_records_it_dropped() {
    use cadmpeg_core::text::NonBlankString;

    use crate::native::{NativeField, NativeRecord};

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut document = unit_cube().unwrap();
    let point = document.geometry.points.remove(0).id;
    let needing: Vec<crate::ids::VertexId> = document
        .geometry
        .vertices
        .iter()
        .filter(|vertex| vertex.point == point)
        .map(|vertex| vertex.id.clone())
        .collect();
    assert!(!needing.is_empty());
    // The rewrite of the stubs after the release refuses an arena that
    // states one stub id twice.
    let stub = NativeRecord::from_identity(
        crate::ids::Identity::new("cadir:record:unknown#0").unwrap(),
        [(
            "links".to_owned(),
            NativeField::TextList(vec![point.as_str().to_owned()]),
        )],
    );
    document
        .native
        .namespace_mut(cadmpeg_core::text::CodecId::new("cadir".to_owned()).unwrap())
        .arenas_mut()
        .insert(
            NonBlankString::new("unknowns").unwrap(),
            vec![stub.clone(), stub],
        );
    let mut reader = super::Reader {
        bytes: b"",
        ctx: &ctx,
        limits: cadmpeg_core::decode::ResourceLimits::default(),
        report: Vec::new(),
        parts: Vec::new(),
        part_entries: std::collections::BTreeMap::new(),
        lost: vec![(point.as_str().to_owned(), 0)],
        removed: crate::annotations::Removed::default(),
        stop: None,
        refusal: None,
    };

    reader.release_lost(&mut document).unwrap();

    let [entry] = reader.report.as_slice() else {
        panic!("one entry: {:?}", reader.report);
    };
    assert!(
        entry.message.contains("are not released"),
        "{}",
        entry.message
    );
    let removed = reader.removed.after(&document);
    for vertex in &needing {
        assert!(document
            .geometry
            .vertices
            .iter()
            .all(|kept| kept.id != *vertex));
        assert!(
            removed.names(&crate::annotations::AnnotationKey::from(vertex)),
            "{vertex}"
        );
    }
}

/// Each record that needs a given-up record is dropped, transitively, until
/// no record of the document names a dropped one; each record that holds
/// one as a member is kept with a mark; and the entry that gives the record
/// up names both (§3.3).
#[test]
fn a_record_that_needs_a_given_up_record_is_dropped_and_named() {
    let (ir, mut value) = wire();
    let point = ir.geometry.points[0].id.as_str().to_owned();
    let original = value.clone();
    points(&mut value)[0]["position"] = json!("not a point");
    let read = read(value.to_string().as_bytes());
    let [entry] = read.report.as_slice() else {
        panic!("one entry: {:?}", read.report);
    };
    let dropped = listed(&entry.message, "dropped");
    let marked = listed(&entry.message, "marked");
    assert!(dropped.contains(&point.as_str()), "{}", entry.message);
    assert!(!marked.is_empty(), "{}", entry.message);

    let names = |text: &str, id: &str| text.contains(&format!("\"{id}\""));
    let records = |document: &Value| -> Vec<Value> {
        let namespaces = document["native"]
            .as_object()
            .into_iter()
            .flat_map(|native| {
                native
                    .values()
                    .filter_map(Value::as_object)
                    .flat_map(|namespace| namespace.values())
            });
        ["geometry", "model", "design", "process"]
            .into_iter()
            .filter_map(|home| document[home].as_object())
            .flat_map(|home| home.values())
            .chain(namespaces)
            .filter_map(Value::as_array)
            .flatten()
            .cloned()
            .collect()
    };
    let before = records(&original);
    let record = |id: &str| {
        before
            .iter()
            .find(|record| record["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} is a record of the document"))
            .to_string()
    };
    let text = json::to_string(&read.document).unwrap();
    for id in &dropped {
        assert!(!names(&text, id), "the document still names {id}");
    }
    assert!(
        dropped
            .iter()
            .any(|id| *id != point && !names(&record(id), &point)),
        "the drop reaches a record that does not name {point}: {}",
        entry.message
    );
    let after = records(&json::to_value(&read.document).unwrap());
    for id in &marked {
        let held = record(id);
        assert!(dropped.iter().any(|gone| names(&held, gone)), "{id}");
        let kept = after
            .iter()
            .find(|record| record["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} is kept"));
        assert_ne!(kept.to_string(), held, "{id} is kept with a mark");
    }
}

/// The entry that gives a record up names each native record that links to
/// it, and the document keeps no link to it (§3.3).
#[test]
fn a_native_record_that_links_to_a_given_up_record_is_named() {
    const ANNOTATION: &str = "test:native:annotation#1";
    let mut ir = unit_cube().unwrap();
    let point = ir.geometry.points[0].id.as_str().to_owned();
    let annotation = crate::native::NativeRecord::new(
        crate::ids::Identity::new(ANNOTATION).unwrap(),
        json!({"links": [point]}).as_object().cloned().unwrap(),
    )
    .unwrap();
    ir.native
        .namespace_mut(crate::codec_id!("test"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("annotations"),
            vec![annotation],
        );
    let mut value = json::to_value(&ir).unwrap();
    points(&mut value)[0]["position"] = json!("not a point");
    let read = read(value.to_string().as_bytes());
    let entry = read
        .report
        .iter()
        .find(|entry| entry.code == crate::unknown::part::record_unreadable_kind())
        .unwrap_or_else(|| panic!("{:?}", read.report));
    assert!(
        listed(&entry.message, "dropped")
            .into_iter()
            .chain(listed(&entry.message, "marked"))
            .any(|id| id == ANNOTATION),
        "{}",
        entry.message
    );
    let native = json::to_string(&read.document.native).unwrap();
    assert!(!native.contains(&format!("\"{point}\"")), "{native}");
}

/// A record nested past the depth bound is given up without a reading of
/// its value, and each record that needs it is dropped (§2.5, §3.3).
#[test]
fn a_record_given_up_for_its_depth_drops_each_record_that_needs_it() {
    let (ir, mut value) = wire();
    let point = ir.geometry.points[0].id.as_str().to_owned();
    let mut position = json!(0);
    for _ in 0..140 {
        position = json!([position]);
    }
    points(&mut value)[0]["position"] = position;
    let read = read(value.to_string().as_bytes());
    let vertex = ir
        .geometry
        .vertices
        .iter()
        .find(|vertex| json::to_string(vertex).unwrap().contains(&point))
        .unwrap()
        .id
        .as_str();
    assert!(read
        .document
        .geometry
        .vertices
        .iter()
        .all(|kept| kept.id.as_str() != vertex));
    let entry = read
        .report
        .iter()
        .find(|entry| entry.message.contains("past the bound"))
        .unwrap_or_else(|| panic!("{:?}", read.report));
    assert!(
        listed(&entry.message, "dropped").contains(&vertex),
        "{}",
        entry.message
    );
}

/// A record that states a member twice with different values is given up,
/// and its part retains its text as stated, both values included (§3.3).
#[test]
fn a_record_that_states_a_member_twice_retains_both_values() {
    let ir = unit_cube().unwrap();
    let text = json::to_string(&ir).unwrap();
    let point = json::to_string(&ir.geometry.points[0]).unwrap();
    assert_eq!(text.matches(&point).count(), 1);
    let restated = format!("{},\"position\":[9.5,8.5,7.5]}}", &point[..point.len() - 1]);
    let read = read(text.replacen(&point, &restated, 1).as_bytes());
    assert!(read
        .document
        .geometry
        .points
        .iter()
        .all(|kept| kept.id != ir.geometry.points[0].id));
    assert!(
        read.fidelity
            .retained_records()
            .values()
            .any(|retained| retained.data() == Some(restated.as_bytes())),
        "{:?}",
        read.fidelity.retained_records()
    );
}

/// A record that holds an expression deeper than the bound of an expression
/// is given up with its text retained, and the rest of the document is read
/// (§2.5, §3.3).
#[test]
fn a_record_past_the_expression_bound_is_given_up() {
    use crate::design::parameter::{DesignParameter, ParameterDefinition};

    let shallow: crate::design::expr::Neutral =
        json::from_value(json!({"text": "1", "refs": [], "measures": []})).unwrap();
    let parameter = |id: &str| DesignParameter {
        id: crate::features::ParameterId::mint(id).unwrap(),
        scope: crate::ids::ProductDefinitionId::mint("synthetic:test:product-definition#root")
            .unwrap(),
        owner: None,
        name: None,
        kind: crate::design::units::ValueKind::Text,
        unit: None,
        definition: ParameterDefinition::Expression(crate::design::expr::Expression::Neutral(
            shallow.clone(),
        )),
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: std::collections::BTreeMap::new(),
        source_settings: std::collections::BTreeMap::new(),
        native_ref: None,
    };
    let mut ir = unit_cube().unwrap();
    ir.design.parameters.extend([
        parameter("synthetic:test:parameter#a-deep"),
        parameter("synthetic:test:parameter#b-shallow"),
    ]);
    let mut value = json::to_value(&ir).unwrap();
    let mut deep = "1".to_owned();
    for _ in 0..=crate::bounds::MAX_EXPRESSION_DEPTH / 6 {
        deep = format!("sin(true or true and true == 1 + 1 * {deep})");
    }
    let text = &mut value["design"]["parameters"][0]["definition"]["value"]["value"]["text"];
    assert_eq!(text, &json!("1"));
    *text = json!(deep);
    let record = value["design"]["parameters"][0].clone();
    let read = read(value.to_string().as_bytes());

    assert_eq!(codes(&read.report), ["decode/record.unreadable"]);
    assert_eq!(
        read.document.design.parameters[..],
        ir.design.parameters[1..]
    );
    assert_eq!(read.document.geometry, ir.geometry);
    let stubs = read.document.native_unknowns("cadir").unwrap();
    let retained = read.fidelity.retained_record(stubs[0].id.as_str()).unwrap();
    let text = crate::hash::finite_json::to_canonical_json_string(&record).unwrap();
    assert_eq!(retained.data(), Some(text.as_bytes()));
}

/// The text of `ir` with the last character of the id of point 0, in that
/// point's record, made an invalid UTF-8 byte.
fn point_id_corrupted(ir: &CadIr) -> Vec<u8> {
    let text = json::to_string(ir).unwrap();
    let point = json::to_string(&ir.geometry.points[0]).unwrap();
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
    let text = json::to_string(&ir).unwrap();
    let point = json::to_string(&ir.geometry.points[0]).unwrap();
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
