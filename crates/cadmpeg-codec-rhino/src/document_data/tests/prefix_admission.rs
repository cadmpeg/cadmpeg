use super::{install, metadata_scan, SETTINGS_TABLE};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

fn first_visit_refusal(scan: &crate::container::Scan<'_>, prior_visits: u64) {
    // With no optional metadata, only the fixed settings ID precedes traversal.
    // Fresh vector backing copies no existing items and executes no traversal.
    let initial_work = u64::try_from("rhino:document:settings#current".len()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = initial_work + prior_visits;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let error = install(&ctx, scan, &mut ir).expect_err("next visit must be admitted first");
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("expected original resource refusal");
    };
    assert_eq!(refusal.operation, "Rhino install traversal");
    assert_eq!(refusal.used, initial_work + prior_visits);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(ir.native.namespace("rhino").is_none());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == refusal));
}

#[test]
fn preview_install_admits_only_the_first_visit() {
    let mut scan = metadata_scan();
    scan.metadata.properties.previews = (0..8193)
        .map(|_| crate::settings::PreviewDescriptor {
            source: crate::settings::SourceRange { range: 0..0 },
            compressed: false,
        })
        .collect();
    first_visit_refusal(&scan, 0);
}

#[test]
fn unsupported_setting_install_admits_only_the_first_visit() {
    let mut scan = metadata_scan();
    scan.metadata.settings.unsupported = (0..8193)
        .map(|_| crate::settings::SettingDescriptor {
            typecode: 0x2000_803f,
            source: crate::settings::SourceRange { range: 0..0 },
        })
        .collect();
    first_visit_refusal(&scan, 0);
}

#[test]
fn setting_table_install_admits_only_the_first_visit() {
    let mut scan = metadata_scan();
    let table = scan.tables.remove(0);
    scan.tables = (0..8193).map(|_| table.clone()).collect();
    first_visit_refusal(&scan, 0);
}

#[test]
fn setting_record_install_admits_only_the_first_visit() {
    let mut scan = metadata_scan();
    let table = crate::container::Table::new(
        SETTINGS_TABLE,
        0..1,
        0..0,
        (0..8193)
            .map(|_| crate::container::Record::long(0x2000_803f, 0..0, 0..0))
            .collect(),
        1,
        std::collections::BTreeMap::new(),
    )
    .unwrap();
    scan.tables = vec![table];
    first_visit_refusal(&scan, 1);
}

#[test]
fn document_install_preserves_the_original_fuse() {
    let scan = metadata_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original document fuse").unwrap_err() else {
        panic!("expected original resource refusal");
    };
    let mut ir = CadIr::empty();
    assert!(matches!(install(&ctx, &scan, &mut ir), Err(CodecError::ResourceLimit(limit)) if limit == original));
    assert!(ir.native.namespace("rhino").is_none());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
