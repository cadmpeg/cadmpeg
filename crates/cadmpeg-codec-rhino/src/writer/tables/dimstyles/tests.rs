// SPDX-License-Identifier: Apache-2.0
//! Dimension-style records re-emitted from held typed fields round-trip
//! through the decoder; records with unadmitted children stay omitted.

use super::styles;
use crate::container;
use crate::test_support::test_dump;
use crate::writer::tests::table;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::CadIr;

fn decode_dimstyles(records: &[Vec<u8>], version: &str) -> CadIr {
    use crate::chunks::ArchiveVersion;
    let archive = test_dump::minimal_document(
        version,
        &[
            table(
                crate::writer::TCODE_SETTINGS_TABLE,
                &[test_dump::units_record(
                    if version == "50" {
                        ArchiveVersion::V5
                    } else {
                        ArchiveVersion::V6
                    },
                    2,
                )],
            )
            .expect("settings"),
            table(crate::writer::TCODE_DIMSTYLE_TABLE, records).expect("dimstyles"),
            table(crate::writer::TCODE_OBJECT_TABLE, &[]).expect("objects"),
        ],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, view) = DecodeContext::from_root_bytes(&archive, &arena, &policy).expect("context");
    let scan = container::scan(&ctx, &archive).expect("scan dimstyle archive");
    let mut ir = CadIr::empty();
    let (installed, _) =
        crate::presentation::install(&ctx, view, &scan, &mut ir).expect("read dimstyle grammar");
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(
        installed.opaque_records.is_empty(),
        "{:?}",
        installed.opaque_records
    );
    ir
}

fn utf16(value: &str) -> Vec<u8> {
    test_dump::utf16_bytes(value)
}

fn v5_payload() -> Vec<u8> {
    let mut payload = vec![0x15];
    payload.extend(7_i32.to_le_bytes());
    payload.extend(utf16("legacy dimension style"));
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(6_u32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend(8_i32.to_le_bytes());
    payload.extend(9_u32.to_le_bytes());
    payload.extend(10_u32.to_le_bytes());
    payload.extend(11_i32.to_le_bytes());
    payload.extend(12_i32.to_le_bytes());
    payload.extend(13_i32.to_le_bytes());
    payload.extend(14.0_f64.to_le_bytes());
    payload.extend(15.0_f64.to_le_bytes());
    payload.extend(utf16("<"));
    payload.extend(utf16(">"));
    payload.push(1);
    payload.extend(16.0_f64.to_le_bytes());
    payload.extend(17_u32.to_le_bytes());
    payload.extend(18_i32.to_le_bytes());
    payload.extend(19_u32.to_le_bytes());
    payload.extend(20_i32.to_le_bytes());
    payload.extend(utf16("["));
    payload.extend(utf16("]"));
    payload.extend(21_u32.to_le_bytes());
    payload.extend([0x33; 16]);
    payload.extend(22.0_f64.to_le_bytes());
    payload.extend(23.0_f64.to_le_bytes());
    payload.extend(24_i32.to_le_bytes());
    payload.extend([1, 0]);
    payload
}

fn class_wrapped(
    payload: &[u8],
    class: [u8; 16],
    archive: crate::chunks::ArchiveVersion,
) -> Vec<u8> {
    let wrapper = test_dump::class_wrapper(archive, class, payload);
    test_dump::nested_crc_chunk(archive, 0x2000_8075, &wrapper)
}

#[test]
fn v5_style_without_extra_round_trips() {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V5;
    let source = decode_dimstyles(
        &[class_wrapped(
            &v5_payload(),
            crate::wire::Uuid::to_wire(crate::presentation::V5_DIMSTYLE),
            archive,
        )],
        "50",
    );
    let records = &source
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"];
    assert_eq!(records.len(), 1);
    let exported = styles(&source, crate::RhinoArchiveVersion::V5).expect("typed dimstyles");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.records.len(), 1);
    assert!(exported.written.contains(records[0].id()));
    let output = decode_dimstyles(&exported.records, "50");
    let written = &output
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"];
    assert_eq!(written.len(), 1);
    // The writer re-indexes tables by emission order, as groups and linetypes
    // do; every other held field round-trips.
    let mut expected = records[0].fields().expect("source fields");
    expected.insert("archive_index".to_owned(), serde_json::json!(0));
    assert_eq!(written[0].fields().expect("written fields"), expected);
}

#[test]
fn v5_style_with_extra_stays_omitted() {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V5;
    let source = decode_dimstyles(
        &[class_wrapped(
            &v5_payload(),
            crate::wire::Uuid::to_wire(crate::presentation::V5_DIMSTYLE),
            archive,
        )],
        "50",
    );
    let mut fields = source
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"][0]
        .fields()
        .expect("source fields");
    fields.insert(
        "v5_extra".to_owned(),
        serde_json::json!({"parent_style_uuid": null}),
    );
    let mut held = CadIr::empty();
    let record = cadmpeg_ir::NativeRecord::new(
        source
            .native
            .namespace("rhino")
            .expect("namespace")
            .arenas()["dimension_styles"][0]
            .id()
            .try_into()
            .expect("identity"),
        fields,
    )
    .expect("style with extra");
    held.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("dimension_styles".to_owned(), vec![record]);
    let exported = styles(&held, crate::RhinoArchiveVersion::V5).expect("typed dimstyles");
    assert!(exported.records.is_empty());
    assert_eq!(exported.losses.len(), 1);
    assert!(exported.losses[0]
        .message
        .contains("style-extra is class userdata"));
}

#[test]
fn modern_style_with_named_child_stays_omitted() {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V5;
    let source = decode_dimstyles(
        &[class_wrapped(
            &v5_payload(),
            crate::wire::Uuid::to_wire(crate::presentation::V5_DIMSTYLE),
            archive,
        )],
        "50",
    );
    let mut fields = source
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"][0]
        .fields()
        .expect("source fields");
    fields.remove("v5_extra");
    fields.insert(
        "controls".to_owned(),
        serde_json::json!({
            "legacy_override_parent_count": 0,
            "scale_value": {"byte_len": 12, "sha256": "00"},
        }),
    );
    fields.remove("v5_version");
    let mut held = CadIr::empty();
    let record = cadmpeg_ir::NativeRecord::new(
        source
            .native
            .namespace("rhino")
            .expect("namespace")
            .arenas()["dimension_styles"][0]
            .id()
            .try_into()
            .expect("identity"),
        fields,
    )
    .expect("style with child");
    held.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("dimension_styles".to_owned(), vec![record]);
    let exported = styles(&held, crate::RhinoArchiveVersion::V6).expect("typed dimstyles");
    assert!(exported.records.is_empty());
    assert_eq!(exported.losses.len(), 1);
    assert!(exported.losses[0]
        .message
        .contains("named children the decoder retains as digests only"));
}

fn modern_payload() -> Vec<u8> {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V6;
    let mut component = 1_i32.to_le_bytes().to_vec();
    component.extend(0_i32.to_le_bytes());
    component.extend(13_u32.to_le_bytes());
    component.extend([0x44; 16]);
    component.extend(3_i32.to_le_bytes());
    component.extend(utf16("modern style"));
    let mut payload = test_dump::anonymous_chunk(archive, 0, &component);
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(8_u32.to_le_bytes());
    payload.extend(9_u32.to_le_bytes());
    payload.extend(10_u32.to_le_bytes());
    payload.extend(11_i32.to_le_bytes());
    payload.extend(12_i32.to_le_bytes());
    payload.extend(13_i32.to_le_bytes());
    payload.extend(14.0_f64.to_le_bytes());
    payload.push(1);
    payload.extend(15.0_f64.to_le_bytes());
    payload.extend(16_u32.to_le_bytes());
    payload.extend(17_i32.to_le_bytes());
    payload.extend(utf16("<"));
    payload.extend(utf16(">"));
    payload.extend(utf16("["));
    payload.extend(utf16("]"));
    payload.extend(18.0_f64.to_le_bytes());
    payload.extend([0, 1]);
    payload.extend([0x55; 16]);
    let mut inner = payload;
    let mut controls = 19_u32.to_le_bytes().to_vec();
    controls.push(0);
    controls.extend(20_u32.to_le_bytes());
    controls.extend(21_i32.to_le_bytes());
    for value in [22.0_f64, 23.0, 24.0, 25.0] {
        controls.extend(value.to_le_bytes());
    }
    controls.push(1);
    controls.extend(26_u32.to_le_bytes());
    controls.extend([0x66, 0x66, 0x66, 0x66]);
    controls.extend(27.0_f64.to_le_bytes());
    controls.extend(28_i32.to_le_bytes());
    controls.extend([0x66; 16]);
    controls.extend([29_u8, 30, 31, 32]);
    controls.extend([1_u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    controls.extend([33_u8, 34, 35, 36]);
    controls.extend([
        17_u8, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
    ]);
    controls.extend([37_u8, 38]);
    for value in [39.0_f64, 40.0, 41.0] {
        controls.extend(value.to_le_bytes());
    }
    controls.push(1);
    controls.extend(42.0_f64.to_le_bytes());
    controls.extend(43_i32.to_le_bytes());
    controls.extend(44.0_f64.to_le_bytes());
    controls.extend([1, 0]);
    controls.extend(45_i32.to_le_bytes());
    controls.extend(46_i32.to_le_bytes());
    controls.extend(47.0_f64.to_le_bytes());
    controls.extend(48_u32.to_le_bytes());
    for value in [49.0_f64, 50.0, 51.0] {
        controls.extend(value.to_le_bytes());
    }
    for value in [52_u32, 53, 54, 55] {
        controls.extend(value.to_le_bytes());
    }
    controls.push(0);
    for value in [56_u32, 57, 58] {
        controls.extend(value.to_le_bytes());
    }
    for _ in 0..3 {
        controls.extend([0x77; 16]);
    }
    inner.extend(controls);
    test_dump::anonymous_chunk(archive, 0, &inner)
}

#[test]
fn modern_style_without_children_round_trips() {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V6;
    let source = decode_dimstyles(
        &[class_wrapped(
            &modern_payload(),
            crate::wire::Uuid::to_wire(crate::presentation::DIMSTYLE),
            archive,
        )],
        "60",
    );
    let records = &source
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"];
    assert_eq!(records.len(), 1);
    let exported = styles(&source, crate::RhinoArchiveVersion::V6).expect("typed dimstyles");
    assert!(exported.losses.is_empty(), "{:?}", exported.losses);
    assert_eq!(exported.records.len(), 1);
    let output = decode_dimstyles(&exported.records, "60");
    let written = &output
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["dimension_styles"];
    assert_eq!(written.len(), 1);
    let mut expected = records[0].fields().expect("source fields");
    expected.insert("archive_index".to_owned(), serde_json::json!(0));
    assert_eq!(written[0].fields().expect("written fields"), expected);
}

#[test]
fn v5_style_on_modern_target_stays_omitted() {
    use crate::chunks::ArchiveVersion;
    let archive = ArchiveVersion::V5;
    let source = decode_dimstyles(
        &[class_wrapped(
            &v5_payload(),
            crate::wire::Uuid::to_wire(crate::presentation::V5_DIMSTYLE),
            archive,
        )],
        "50",
    );
    let exported = styles(&source, crate::RhinoArchiveVersion::V8).expect("typed dimstyles");
    assert!(exported.records.is_empty());
    assert_eq!(exported.losses.len(), 1);
    assert!(exported.losses[0]
        .message
        .contains("has no encoding on an archive-60-or-later target"));
}
