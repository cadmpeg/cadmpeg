// SPDX-License-Identifier: Apache-2.0
//! View installation admits each table and record before its fallible body.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::container::{Record, Table};

fn table(records: Vec<Record>) -> Table {
    let count = records.len();
    Table::new(super::super::SETTINGS, 0..8, 0..0, records, count,
        std::collections::BTreeMap::new()).unwrap()
}

fn assert_first_visit(tables: Vec<Table>, preceding_visits: u64) {
    let archive = crate::chunks::ArchiveVersion::V5;
    let mut scan = crate::container::scan_owned(
        crate::test_support::test_dump::minimal_document("50", &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ]),
    ).unwrap();
    scan.tables = tables;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = preceding_visits;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let error = super::super::install(&ctx, &scan, &mut ir)
        .expect_err("only the next view visit is refused");
    let CodecError::ResourceLimit(refusal) = error else { panic!("view visit refusal"); };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino install traversal");
    assert_eq!((refusal.used, refusal.additional), (preceding_visits, 1));
    assert!(ir.native.namespace("rhino").is_none());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn view_table_traversal_refuses_only_first_visit() {
    assert_first_visit(vec![table(Vec::new()); 1024], 0);
}

#[test]
fn view_record_traversal_refuses_only_first_visit() {
    let records = vec![Record::short(0x1234, 0..0, 0); 1024];
    assert_first_visit(vec![table(records)], 1);
}
