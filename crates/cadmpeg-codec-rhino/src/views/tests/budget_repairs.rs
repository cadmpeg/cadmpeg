// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn view_staging_stays_materialized_through_native_install() {
    let archive = ArchiveVersion::V5;
    let mut body = crc_chunk(archive, super::super::VIEW_NAME, &utf16_bytes("saved view"));
    let end_marker = short_chunk(archive, super::super::TCODE_ENDOFTABLE, 0);
    let name_len = body.len();
    body.extend(&end_marker);
    let view = crc_chunk_excluding(archive, super::super::VIEW_RECORD, &body,
        &[0..name_len, name_len..body.len()]);
    let mut list = 1_i32.to_le_bytes().to_vec();
    list.extend(view);
    let bytes = crate::test_support::test_dump::minimal_document("50", &[
        crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
        crate::test_support::test_dump::table(archive, 0x1000_0015, &[
            crc_chunk(archive, super::super::NAMED_VIEWS, &list),
            crc_chunk(archive, super::super::ACTIVE_VIEWS, &list),
        ]),
        crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
    ]);
    let mut scan = crate::container::scan_owned(bytes).unwrap();
    crate::test_support::test_dump::set_test_units(&mut scan, 1.0);
    for (dimension, operation) in [
        (ResourceDimension::MaterializedBytes, "Rhino view name"),
        (ResourceDimension::MaterializedBytes, "Rhino view list records"),
        (ResourceDimension::RetainedBytes, "serialize native record"),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::MaterializedBytes { policy.limits.max_materialized_bytes = cap; }
            else { policy.limits.max_retained_bytes = cap; }
            let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            super::super::install(&ctx, &scan, &mut cadmpeg_ir::document::CadIr::empty())
        });
    }
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let installed = super::super::install(&cadmpeg_test_support::service_decode_context(), &scan, &mut ir).unwrap();
    assert!(installed.losses.is_empty());
    assert!(installed.opaque_records.is_empty());
    let records = &ir.native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.field("name") == Some(serde_json::json!("saved view"))));
}
