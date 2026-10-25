// SPDX-License-Identifier: Apache-2.0
//! Pending typed interpretations preserve their actual header and source authority.

use crate::native::entry_contents::{EntryContentUpdate, EntryContents};
use crate::native::{native_id, EntryRecord};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

fn entry(name: &str) -> EntryRecord {
    EntryRecord::new(
        native_id("entry", name),
        name.into(),
        ContainerRole::Auxiliary,
        Vec::new(),
    )
    .unwrap()
}

fn empty_update(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    entry: &EntryRecord,
    float: bool,
) -> EntryContentUpdate {
    let mut cursor = source;
    assert_eq!(cursor.req_u32_le().unwrap(), 0);
    assert!(cursor.is_empty());
    let contents = if float {
        EntryContents::Float { values: Vec::new() }
    } else {
        EntryContents::Color { values: Vec::new() }
    };
    EntryContentUpdate::new(ctx, entry.id(), source, contents).unwrap()
}

#[test]
fn equal_entry_interpretations_merge_once_and_distinct_grammars_keep_their_empty_arity() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&[0; 4], &arena, &DecodePolicy::service()).unwrap();
    let mut records = vec![entry("shared.bin")];
    let updates = [
        empty_update(&ctx, source, &records[0], false),
        empty_update(&ctx, source, &records[0], false),
        empty_update(&ctx, source, &records[0], true),
    ];
    super::super::merge_entry_contents(&ctx, &mut records, updates).unwrap();
    assert_eq!(records[0].name(), "shared.bin");
    assert_eq!(
        records[0].contents(),
        &[
            EntryContents::Color { values: Vec::new() },
            EntryContents::Float { values: Vec::new() },
        ]
    );
    let wire = serde_json::to_value(&records[0]).unwrap();
    let decoded: EntryRecord = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    assert_eq!(wire["contents"][0]["values"], serde_json::json!([]));
    assert_eq!(wire["contents"][1]["values"], serde_json::json!([]));
}

#[test]
fn foreign_or_wrong_header_updates_cannot_change_the_pending_entry() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(&[0; 4], &arena, &DecodePolicy::service()).unwrap();
    let mut actual = entry("actual.bin");
    let before = serde_json::to_value(&actual).unwrap();
    let update = empty_update(&ctx, source, &entry("other.bin"), false);
    assert!(update.apply(&ctx, &mut actual).is_err());
    assert_eq!(serde_json::to_value(&actual).unwrap(), before);
    let foreign_arena = DecodeArena::new();
    let (foreign, foreign_source) =
        DecodeContext::from_root_bytes(&[0; 4], &foreign_arena, &DecodePolicy::service()).unwrap();
    let update = empty_update(&foreign, foreign_source, &actual, false);
    assert!(update.apply(&ctx, &mut actual).is_err());
    assert_eq!(serde_json::to_value(&actual).unwrap(), before);
    let update = empty_update(&ctx, source, &entry("absent.bin"), false);
    assert!(
        super::super::merge_entry_contents(&ctx, std::slice::from_mut(&mut actual), [update],)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&actual).unwrap(), before);
}
