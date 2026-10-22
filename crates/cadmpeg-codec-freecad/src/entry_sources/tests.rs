// SPDX-License-Identifier: Apache-2.0
//! Source identity and navigation controls for archive entry interpretation.

use super::EntrySources;
use crate::native::EntryRecord;
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy, View};
use std::collections::BTreeMap;

fn entry(name: &str) -> EntryRecord {
    EntryRecord::new(
        crate::native::native_id("entry", name),
        name.into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn sources_keep_actual_named_member_views_without_copying_or_read_credit() {
    let bytes = b"prefixfirstsecond";
    let arena = DecodeArena::new();
    let (context, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let first = context
        .register_named_slice(root, ByteRange { start: 6, end: 11 }, "first.bin")
        .unwrap();
    let second = context
        .register_named_slice(root, ByteRange { start: 11, end: 17 }, "second.bin")
        .unwrap();
    let views = BTreeMap::from([("first.bin".into(), first), ("second.bin".into(), second)]);
    let sources = EntrySources::new(&context, &views).unwrap();
    for (name, expected, expected_len) in [
        ("first.bin", &bytes[6..11], 5),
        ("second.bin", &bytes[11..17], 6),
    ] {
        let record = entry(name);
        let view = sources.view(&record).unwrap();
        assert_eq!(view.window(), expected);
        assert_eq!(view.window().as_ptr(), expected.as_ptr());
        assert_eq!(sources.byte_len(&record).unwrap(), expected_len);
        let capture = context.capture_part_extent(view).unwrap();
        assert_eq!(capture.stream(), name);
        assert_eq!(capture.offset(), 0);
        let native = serde_json::to_value(record).unwrap();
        for field in ["data", "byte_len", "sha256", "length"] {
            assert!(native.get(field).is_none(), "{field}");
        }
    }
    assert!(context.take_record_captures().unwrap().is_empty());
}

#[test]
fn absent_entry_does_not_resolve_by_a_shared_native_identity_or_drop_a_sibling() {
    let bytes = b"real sibling";
    let arena = DecodeArena::new();
    let (context, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let views = BTreeMap::from([("real.bin".into(), root)]);
    let sources = EntrySources::new(&context, &views).unwrap();
    let actual = entry("real.bin");
    let mut wire = serde_json::to_value(&actual).unwrap();
    wire["name"] = serde_json::json!("missing.bin");
    let missing: EntryRecord = serde_json::from_value(wire).unwrap();
    assert_eq!(actual.id(), missing.id());
    assert!(sources.view(&missing).is_err());
    assert_eq!(sources.view(&actual).unwrap().window(), bytes);
}

#[test]
fn detached_and_foreign_views_cannot_supply_entry_source_provenance() {
    let bytes = b"member";
    let arena = DecodeArena::new();
    let (context, _) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let detached = BTreeMap::from([("member.bin".into(), View::over_retained(bytes))]);
    assert!(EntrySources::new(&context, &detached).is_err());
    let other_arena = DecodeArena::new();
    let (_other_context, foreign) =
        DecodeContext::from_root_bytes(bytes, &other_arena, &DecodePolicy::service()).unwrap();
    let foreign = BTreeMap::from([("member.bin".into(), foreign)]);
    assert!(EntrySources::new(&context, &foreign).is_err());
}
