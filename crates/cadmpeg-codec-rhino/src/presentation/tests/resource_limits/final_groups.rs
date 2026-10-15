// SPDX-License-Identifier: Apache-2.0
//! Final group passes visit known source entries individually.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::ArchiveVersion;

fn group_scan() -> crate::container::Scan<'static> {
    use crate::test_support::test_dump::{class_wrapper, crc_chunk, minimal_document, table};

    let archive = ArchiveVersion::V5;
    let records: Vec<_> = [7_i32, 8, 7].into_iter().map(|index| {
        let mut payload = vec![0x10];
        payload.extend(index.to_le_bytes());
        payload.extend(0_i32.to_le_bytes());
        crc_chunk(archive, 0x2000_8073,
            &class_wrapper(archive, crate::presentation::GROUP.to_wire(), &payload))
    }).collect();
    let bytes = minimal_document("50", &[
        table(archive, 0x1000_0014, &[]),
        table(archive, 0x1000_0015, &[]),
        table(archive, 0x1000_0018, &records),
        table(archive, 0x1000_0013, &[]),
    ]);
    let scan = crate::container::scan_owned(bytes).unwrap();
    assert_eq!(scan.tables.len(), 4);
    assert_eq!(scan.tables[2].records.len(), 3);
    assert!(scan.objects.is_empty());
    assert!(scan.metadata.layers.is_empty());
    scan
}

#[test]
fn group_index_diagnostics_do_not_prepay_all_index_visits() {
    let scan = group_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    // Two distinct group indexes. Tables and the only nonempty record list
    // contain four and three entries; all other install sources are empty.
    // A two-visit bulk fee can therefore come only from the final index pass.
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino install traversal", Some(2));
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::presentation::install(&ctx, &scan, &mut ir).unwrap();
    drop(probe);
    let groups = &ir.native.namespace("rhino").unwrap().arenas()["groups"];
    assert_eq!(groups.len(), 3);
    assert_eq!(groups.iter().filter(|record| record.field("archive_index") == Some(serde_json::json!(7))).count(), 2);
    assert_eq!(groups.iter().filter(|record| record.field("archive_index") == Some(serde_json::json!(8))).count(), 1);
    for record in groups {
        assert_eq!(record.field("links"), Some(serde_json::json!([])));
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn final_group_link_pass_refuses_only_the_first_source_visit() {
    let scan = group_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "Rhino group link traversal", None);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let CodecError::ResourceLimit(refusal) = crate::presentation::install(&ctx, &scan, &mut ir).unwrap_err()
        else { panic!("group link visit refusal"); };
    drop(probe);
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino group link traversal");
    assert_eq!(refusal.additional, 1);
    assert_eq!(refusal.used, refusal.limit);
    assert!(ir.native.namespace("rhino").is_none());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}
