use super::{install, metadata_scan};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

#[test]
fn document_install_preserves_the_original_fuse() {
    let scan = metadata_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) =
        ctx.charge_work(1, "original document fuse").unwrap_err()
    else {
        panic!("expected original resource refusal");
    };
    let mut ir = CadIr::empty();
    assert!(
        matches!(install(&ctx, &scan, &mut ir), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(ir.native.namespace("rhino").is_none());
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn rejected_settings_release_partial_font_text_before_the_next_record() {
    const RECORDS: usize = 16;
    const SCRATCH_BYTES: u64 = 128 * 1024;
    let archive = crate::chunks::ArchiveVersion::V5;
    let mut body = super::annotation_body(0);
    // The minor-one field is absent after the full font face.
    body.truncate(1 + 7 * 8 + 7 * 4);
    body[0] = 0x11;
    body.extend(crate::test_support::test_dump::utf16_bytes(
        &"a".repeat(8192),
    ));
    let record =
        crate::test_support::test_dump::crc_chunk(archive, super::ANNOTATION_SETTINGS, &body);
    let records = vec![record; RECORDS];
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, super::SETTINGS_TABLE, &records),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let mut scan = crate::container::scan_owned(bytes).unwrap();
    crate::test_support::test_dump::set_test_units(&mut scan, 1.0);
    scan.metadata.settings.unsupported.clear();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = CadIr::empty();
    let result = install(&ctx, &scan, &mut ir)
        .expect("rejected font faces do not accumulate in the native workspace");
    assert_eq!(result.opaque_records.len(), RECORDS);
    assert_eq!(
        ir.native
            .namespace("rhino")
            .unwrap()
            .arenas()
            .get("setting_records")
            .unwrap()
            .len(),
        RECORDS
    );
    assert!(ir
        .native
        .namespace("rhino")
        .unwrap()
        .arenas()
        .get("annotation_settings")
        .unwrap()
        .is_empty());
    drop(result);
    let reclaimed = ctx
        .reserve_scoped(SCRATCH_BYTES, "reclaimed setting scratch")
        .expect("candidate and serialization staging storage is released");
    drop(reclaimed);
    ctx.finish_session().unwrap();
}
