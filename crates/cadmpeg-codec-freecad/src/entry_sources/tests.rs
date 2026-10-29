// SPDX-License-Identifier: Apache-2.0
//! Source identity and navigation controls for archive entry interpretation.

use super::EntrySources;
use crate::native::EntryRecord;
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};
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
        .register_slice(root, ByteRange { start: 6, end: 11 })
        .unwrap();
    let second = context
        .register_slice(root, ByteRange { start: 11, end: 17 })
        .unwrap();
    let views = BTreeMap::from([("first.bin".into(), first), ("second.bin".into(), second)]);
    let sources = EntrySources::new(&views);
    for (name, expected) in [("first.bin", &bytes[6..11]), ("second.bin", &bytes[11..17])] {
        let record = entry(name);
        let view = sources.view(&record).unwrap();
        assert_eq!(view.window(), expected);
        assert_eq!(view.window().as_ptr(), expected.as_ptr());
        let native = serde_json::to_value(record).unwrap();
        for field in ["data", "byte_len", "sha256", "length"] {
            assert!(native.get(field).is_none(), "{field}");
        }
    }
}

#[test]
fn absent_entry_does_not_resolve_by_a_shared_native_identity_or_drop_a_sibling() {
    let bytes = b"real sibling";
    let arena = DecodeArena::new();
    let (_context, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let views = BTreeMap::from([("real.bin".into(), root)]);
    let sources = EntrySources::new(&views);
    let actual = entry("real.bin");
    let mut wire = serde_json::to_value(&actual).unwrap();
    wire["name"] = serde_json::json!("missing.bin");
    let missing: EntryRecord = serde_json::from_value(wire).unwrap();
    assert_eq!(actual.id(), missing.id());
    assert!(sources.view(&missing).is_err());
    assert_eq!(sources.view(&actual).unwrap().window(), bytes);
}
