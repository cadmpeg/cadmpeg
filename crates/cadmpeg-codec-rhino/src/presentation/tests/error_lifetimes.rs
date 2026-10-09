// SPDX-License-Identifier: Apache-2.0
//! Captured parse errors keep their backing through recovery.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::chunks::ArchiveVersion;
use crate::container::{scan_owned, Scan};
use crate::loss::RhinoLossCode;
use crate::presentation::{install, DIMSTYLE_EXTRA, MATERIAL, TEXT_STYLE, V5_DIMSTYLE};
use crate::test_support::test_dump::{
    class_userdata_v1_with_direct_payload, class_wrapper, class_wrapper_with_userdata,
    crc_chunk, minimal_document, set_test_units, table, utf16_bytes,
};

fn scan_record(table_type: u32, record_type: u32, wrapper: &[u8]) -> Scan<'static> {
    let archive = ArchiveVersion::V5;
    let record = crc_chunk(archive, record_type, wrapper);
    let bytes = minimal_document("50", &[
        table(archive, 0x1000_0014, &[]),
        table(archive, 0x1000_0015, &[]),
        table(archive, table_type, &[record]),
        table(archive, 0x1000_0013, &[]),
    ]);
    let mut scan = scan_owned(bytes).unwrap();
    set_test_units(&mut scan, 1.0);
    scan
}

fn assert_opaque_recovery(scan: Scan<'_>, arena_name: &str, typed_count: usize) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let installed = install(&ctx, &scan, &mut ir).unwrap();
    assert_eq!(installed.opaque_records.len(), 1);
    assert_eq!(installed.losses.len(), 1);
    assert_eq!(installed.losses[0].code, RhinoLossCode::PresentationRecordDropped.kind());
    assert_eq!(ir.native.namespace("rhino").unwrap().arenas()[arena_name].len(), typed_count);
    assert_eq!(installed.opaque_records[0].record.range, scan.tables[2].records[0].range);
    drop(installed);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn material_error_storage_release_preserves_complete_source_recovery() {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend([0; 16]);
    body.extend(7_i32.to_le_bytes());
    body.extend(utf16_bytes(""));
    body.extend([0; 16]);
    body.extend([0; 24]);
    body.extend(f64::NAN.to_le_bytes());
    let mut payload = vec![0x20];
    payload.extend(crc_chunk(ArchiveVersion::V5, 0x4000_8000, &body));
    let wrapper = class_wrapper(ArchiveVersion::V5, MATERIAL.to_wire(), &payload);
    assert_opaque_recovery(scan_record(0x1000_0010, 0x2000_8040, &wrapper), "materials", 0);
}

#[test]
fn text_style_error_storage_release_preserves_complete_source_recovery() {
    let mut payload = vec![0x11];
    payload.extend(7_i32.to_le_bytes());
    payload.extend(utf16_bytes("Default"));
    payload.extend([0; 128]);
    payload.extend(400_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend(f64::NAN.to_le_bytes());
    let wrapper = class_wrapper(ArchiveVersion::V5, TEXT_STYLE.to_wire(), &payload);
    assert_opaque_recovery(scan_record(0x1000_0019, 0x2000_8074, &wrapper), "text_styles", 0);
}

fn invalid_dimension_extra() -> Scan<'static> {
    let mut body = [0; 16].to_vec();
    body.extend(1_i32.to_le_bytes());
    body.push(1);
    body.extend(0_i32.to_le_bytes());
    body.extend(0_i32.to_le_bytes());
    body.extend(f64::NAN.to_le_bytes());
    let extra = super::anonymous(0, &body);
    let userdata = class_userdata_v1_with_direct_payload(
        ArchiveVersion::V5, DIMSTYLE_EXTRA.to_wire(), &extra,
    );
    let wrapper = class_wrapper_with_userdata(
        ArchiveVersion::V5, V5_DIMSTYLE.to_wire(), &super::v5_dimension_style_chunk(), &userdata,
    );
    scan_record(0x1000_0020, 0x2000_8075, &wrapper)
}

#[test]
fn dimension_extra_error_storage_preserves_base_style_and_complete_source() {
    assert_opaque_recovery(invalid_dimension_extra(), "dimension_styles", 1);
}

#[test]
fn dimension_extra_error_backing_stays_admitted_through_recovery_loss() {
    let scan = invalid_dimension_extra();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::MaterializedBytes, "Rhino presentation losses", None);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let CodecError::ResourceLimit(original) = install(&ctx, &scan, &mut ir).unwrap_err()
        else { panic!("recovery loss backing refusal"); };
    drop(probe);
    // The extra scope holds its one bool slot and the finite-value error
    // string while the loss Vec requests the core four-slot growth bound.
    let captured_bytes = 1 + "tolerance upper value is not finite".len();
    assert_eq!(original.used, u64::try_from(captured_bytes).unwrap());
    assert_eq!(original.additional, u64::try_from(4 * std::mem::size_of::<LossNote>()).unwrap());
    assert_eq!(original.operation, "Rhino presentation losses");
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
