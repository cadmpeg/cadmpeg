// SPDX-License-Identifier: Apache-2.0
//! Source-layout admission and capture handoff controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use serde_json::{json, Value};

use super::{id, record};
use crate::native::bytes::NativeBytes;
use crate::source_fidelity::{RecordLayout, SourceFidelity};

fn wire() -> Value {
    json!({
        "stream": "file", "offset": 4, "byte_len": 12,
        "reads": {"file": [{"start":4,"end":6},{"start":12,"end":13}]},
        "pointers": {"next": 9}, "payloads": {"opaque": "aabb00ff"}
    })
}

fn layout() -> RecordLayout {
    serde_json::from_value(wire()).expect("independent valid source layout")
}

#[test]
fn captures_keep_literal_record_extents_and_innermost_read_intervals() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let values = ctx
        .record("outer", root, || -> Result<_, CodecError> {
            let mut cursor = root;
            let first = cursor.u16_le().unwrap();
            let inner = root.child(4, 12).unwrap();
            let second = ctx.record("inner", inner, || -> Result<_, CodecError> {
                let mut cursor = inner;
                Ok(cursor.u16_le().unwrap())
            })?;
            cursor.skip(10).unwrap();
            Ok((first, second, cursor.u8().unwrap()))
        })
        .unwrap();
    assert_eq!(values, (513, 1541, 13));
    let mut fidelity = SourceFidelity::default();
    for capture in ctx.take_record_captures().unwrap() {
        let (id, layout) = RecordLayout::from_capture(capture);
        fidelity.insert_record_layout(id, layout).unwrap();
    }
    let outer = &fidelity.record_layout()["outer"];
    let inner = &fidelity.record_layout()["inner"];
    assert_eq!(
        (outer.stream(), outer.offset(), outer.byte_len()),
        ("file", 0, 16)
    );
    assert_eq!(
        (inner.stream(), inner.offset(), inner.byte_len()),
        ("file", 4, 8)
    );
    assert_eq!(outer.reads()["file"], [0..2, 12..13]);
    assert_eq!(inner.reads()["file"], [4..6]);
    assert_eq!(outer.reads().len(), 1);
    assert_eq!(inner.reads().len(), 1);
}

#[test]
fn payload_copy_retains_exact_bytes_without_claiming_semantic_reads() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 0xaa, 0xbb, 0, 0xff, 13, 14, 15, 16];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let payload = ctx
        .record("record", root, || -> Result<_, CodecError> {
            let mut cursor = root;
            assert_eq!(cursor.u16_le(), Some(513));
            Ok(NativeBytes::new(
                root.child(8, 12).unwrap().window().to_vec(),
            ))
        })
        .unwrap();
    let capture = ctx.take_record_captures().unwrap().pop().unwrap();
    let (id, mut layout) = RecordLayout::from_capture(capture);
    layout.insert_pointer("next".into(), 9).unwrap();
    layout.insert_payload("opaque".into(), payload).unwrap();
    assert_eq!(layout.reads()["file"], [0..2]);
    assert_eq!(layout.payloads()["opaque"].as_ref(), &[0xaa, 0xbb, 0, 0xff]);
    let mut fidelity = SourceFidelity::default();
    fidelity.insert_record_layout(id, layout).unwrap();
    let wire = serde_json::to_value(&fidelity).unwrap();
    assert_eq!(
        wire["record_layout"]["record"],
        json!({
            "stream":"file", "offset":0, "byte_len":16,
            "reads":{"file":[{"start":0,"end":2}]},
            "pointers":{"next":9}, "payloads":{"opaque":"aabb00ff"}
        })
    );
    let restored: SourceFidelity = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, fidelity);
    let (annotations, records, metadata, layouts) = restored.into_parts();
    assert_eq!(annotations, Default::default());
    assert!(records.is_empty());
    assert!(metadata.is_empty());
    assert_eq!(layouts["record"].reads()["file"], [0..2]);
    assert_eq!(
        layouts["record"].payloads()["opaque"].as_ref(),
        &[0xaa, 0xbb, 0, 0xff]
    );
}

#[test]
fn source_layout_refuses_invalid_ranges_extents_and_coordinate_types() {
    for ranges in [
        json!([]),
        json!([{"start":4,"end":4}]),
        json!([{"start":6,"end":4}]),
        json!([{"start":4,"end":8},{"start":7,"end":9}]),
        json!([{"start":4,"end":8},{"start":8,"end":9}]),
        json!([{"start":12,"end":13},{"start":4,"end":6}]),
    ] {
        let mut value = wire();
        value["reads"]["file"] = ranges;
        assert!(serde_json::from_value::<RecordLayout>(value).is_err());
    }
    for path in [
        "/offset",
        "/byte_len",
        "/reads/file/0/start",
        "/reads/file/0/end",
        "/pointers/next",
    ] {
        for invalid in [json!(-1), json!(0.5), json!("4"), Value::Null, json!(true)] {
            let mut value = wire();
            *value.pointer_mut(path).unwrap() = invalid;
            assert!(
                serde_json::from_value::<RecordLayout>(value).is_err(),
                "{path}"
            );
        }
    }
    let mut overflow = wire();
    overflow["offset"] = json!(u64::MAX - 1);
    overflow["byte_len"] = json!(3);
    assert!(serde_json::from_value::<RecordLayout>(overflow).is_err());
    for stream in ["", "other"] {
        let mut value = wire();
        value["stream"] = json!(stream);
        if stream.is_empty() {
            assert!(serde_json::from_value::<RecordLayout>(value).is_err());
        } else {
            // No stream-length registry is present on this wire. A different
            // stream's range cannot be checked against this record's extent.
            value["reads"]["file"] = json!([{"start":90,"end":100}]);
            assert!(serde_json::from_value::<RecordLayout>(value).is_ok());
        }
    }
    let mut unnamed = wire();
    unnamed["reads"] = json!({"": [{"start":4,"end":6}]});
    assert!(serde_json::from_value::<RecordLayout>(unnamed).is_err());
}

#[test]
fn empty_records_and_u64_end_boundaries_do_not_invent_read_coverage() {
    let empty: RecordLayout = serde_json::from_value(json!({
        "stream":"file", "offset":17, "byte_len":0,
        "reads":{}, "pointers":{}, "payloads":{}
    }))
    .unwrap();
    assert_eq!((empty.offset(), empty.byte_len()), (17, 0));
    assert!(empty.reads().is_empty());
    let boundary: RecordLayout = serde_json::from_value(json!({
        "stream":"file", "offset":u64::MAX - 1, "byte_len":1,
        "reads":{"file":[{"start":u64::MAX - 1,"end":u64::MAX}]},
        "pointers":{}, "payloads":{}
    }))
    .unwrap();
    assert_eq!(boundary.offset(), u64::MAX - 1);
    assert_eq!(boundary.reads()["file"], [u64::MAX - 1..u64::MAX]);
}

#[test]
fn record_layout_maps_refuse_duplicate_wire_keys_and_unknown_fields() {
    for text in [
        r#"{"stream":"file","offset":0,"byte_len":4,"reads":{"file":[{"start":0,"end":1}],"file":[{"start":2,"end":3}]},"pointers":{},"payloads":{}}"#,
        r#"{"stream":"file","offset":0,"byte_len":4,"reads":{},"pointers":{"p":1,"p":2},"payloads":{}}"#,
        r#"{"stream":"file","offset":0,"byte_len":4,"reads":{},"pointers":{},"payloads":{"p":"00","p":"ff"}}"#,
    ] {
        let error = serde_json::from_str::<RecordLayout>(text).unwrap_err();
        assert!(error.to_string().contains("duplicate key"), "{error}");
    }
    let value = wire();
    let member = format!("\"record\":{value}");
    let text = format!("{{\"record_layout\":{{{member},{member}}}}}");
    let error = serde_json::from_str::<SourceFidelity>(&text).unwrap_err();
    assert!(error.to_string().contains("duplicate key record"));
    for path in ["", "/reads/file/0"] {
        let mut value = wire();
        value.pointer_mut(path).unwrap()["zz_bogus"] = json!(true);
        let error = serde_json::from_value::<RecordLayout>(value).unwrap_err();
        assert!(error.to_string().contains("zz_bogus"));
    }
    for field in [
        "stream", "offset", "byte_len", "reads", "pointers", "payloads",
    ] {
        let mut value = wire();
        value.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<RecordLayout>(value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn duplicate_insertion_and_append_preserve_every_existing_fidelity_table() {
    let mut left = SourceFidelity::default();
    left.insert_record_layout("same".into(), layout()).unwrap();
    left.metadata.insert("left".into(), "left".into());
    left.insert_retained_record(id("left"), record(b"left"))
        .unwrap();
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("file"));
    let mut annotations = crate::AnnotationBuilder::new();
    annotations.note("left", &stream, 4, Some("record"));
    left.annotations = annotations.build();
    let before = left.clone();
    assert!(left.insert_record_layout("same".into(), layout()).is_err());
    assert_eq!(left, before);
    let mut right = SourceFidelity::default();
    right.insert_record_layout("same".into(), layout()).unwrap();
    right.insert_record_layout("new".into(), layout()).unwrap();
    right.metadata.insert("right".into(), "right".into());
    right
        .insert_retained_record(id("right"), record(b"right"))
        .unwrap();
    let mut annotations = crate::AnnotationBuilder::new();
    annotations.note("right", &stream, 8, Some("record"));
    right.annotations = annotations.build();
    assert!(left.append(right).is_err());
    assert_eq!(left, before);
    let mut disjoint = SourceFidelity::default();
    disjoint
        .insert_record_layout("new".into(), layout())
        .unwrap();
    left.append(disjoint).unwrap();
    assert_eq!(left.record_layout().len(), 2);
    let original = layout();
    let mut fields = original.clone();
    assert!(fields.insert_pointer("next".into(), 99).is_err());
    assert!(fields
        .insert_payload("opaque".into(), NativeBytes::new(vec![0]))
        .is_err());
    assert_eq!(fields, original);
}

#[test]
fn composition_renames_streams_without_changing_offsets_or_payloads() {
    let original = layout();
    let mapped = original
        .clone()
        .with_stream_prefix("f3d:xref/child/")
        .unwrap();
    assert_eq!(mapped.stream(), "f3d:xref/child/file");
    assert_eq!(mapped.reads()["f3d:xref/child/file"], [4..6, 12..13]);
    assert_eq!(mapped.offset(), 4);
    assert_eq!(mapped.byte_len(), 12);
    assert_eq!(mapped.pointers(), original.pointers());
    assert_eq!(mapped.payloads(), original.payloads());
    let mut value = wire();
    value["reads"]["second"] = json!([{"start":1,"end":3}]);
    let two_streams: RecordLayout = serde_json::from_value(value).unwrap();
    let mapped = two_streams.with_stream_prefix("f3d:xref/child/").unwrap();
    assert_eq!(mapped.reads()["f3d:xref/child/second"], [1..3]);
    assert_eq!(mapped.reads().len(), 2);
    assert_eq!(layout().with_stream_prefix("").unwrap(), original);
}

#[cfg(feature = "schema")]
#[test]
fn source_layout_schema_has_the_exact_closed_wire_fields() {
    let schema = serde_json::to_value(schemars::schema_for!(RecordLayout)).unwrap();
    let fields = schema["properties"].as_object().unwrap();
    assert_eq!(
        fields.keys().map(String::as_str).collect::<Vec<_>>(),
        ["byte_len", "offset", "payloads", "pointers", "reads", "stream"]
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"].as_array().unwrap().len(), 6);
    assert_eq!(
        schema["$defs"]["ReadRangeWire"]["additionalProperties"],
        false
    );
}
