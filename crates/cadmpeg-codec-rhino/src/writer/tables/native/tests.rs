// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::chunks::ArchiveVersion;

#[test]
fn typed_group_name_and_uuid_are_authored_without_source_bytes() {
    let mut ir = CadIr::empty();
    let record = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new("rhino:presentation:group#sample").expect("identity"),
        serde_json::from_value(serde_json::json!({"archive_index": 8, "source_uuid": "721d9f97-3645-44c4-8be6-b2cf697d25ce", "name": "Edited group", "links": []})).expect("fields"),
    ).expect("typed record");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("groups".to_owned(), vec![record]);
    let records = groups(&ir).expect("source-free group table");
    assert_eq!(records.len(), 1);
    // Read the authored class payload through the real table parser.
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[super::super::super::table(TCODE_GROUP_TABLE, &records).expect("table")],
    );
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default())
        .expect("context");
    let scan = crate::container::scan(&ctx, &archive).expect("scan");
    let mut decoded = CadIr::empty();
    crate::presentation::install(&ctx, view, &scan, &mut decoded).expect("typed inverse");
    let record = &decoded
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["groups"][0];
    assert_eq!(
        record.field("name"),
        Some(Value::String("Edited group".to_owned()))
    );
    assert_eq!(
        record.field("source_uuid"),
        Some(Value::String(
            "721d9f97-3645-44c4-8be6-b2cf697d25ce".to_owned()
        ))
    );
    assert_eq!(scan.archive, ArchiveVersion::V8);
}
