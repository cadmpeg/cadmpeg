// SPDX-License-Identifier: Apache-2.0
//! Source-less writes of the `DesignConfigurationRoot` record and of the
//! configuration tables beside it.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use super::generated_configuration_document;
use crate::test_support::configuration_table::set_active_row;
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

/// The report of `ir` written without a source, and the written file
/// decoded again.
pub(super) fn write_and_decode(
    ir: &cadmpeg_ir::CadIr,
) -> (
    cadmpeg_ir::report::export::ExportReport,
    cadmpeg_ir::codec::DecodeResult,
) {
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("source-less write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source-less round trip");
    (report, decoded)
}

/// No `export:native.records-omitted` loss of `report` names a
/// configuration document of `written`.
fn assert_configuration_tables_not_omitted(
    report: &cadmpeg_ir::report::export::ExportReport,
    written: &cadmpeg_ir::CadIr,
) {
    let documents = f3d_native(written).design_configurations;
    assert!(!documents.is_empty());
    for loss in &report.losses {
        if loss.code.to_string() != "export/native.records-omitted" {
            continue;
        }
        for document in &documents {
            assert!(
                !loss.message.contains(document.id().as_str()),
                "{}: {loss:#?}",
                document.id()
            );
        }
    }
}

/// `round_trip` holds the configuration documents and configurations of
/// `written`, and its root record names row `row` of table `1`.
pub(super) fn assert_active_row_written(
    round_trip: &cadmpeg_ir::codec::DecodeResult,
    written: &cadmpeg_ir::CadIr,
    row: &str,
) {
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let active = documents[0]
        .active_row()
        .expect("the written root record names a row")
        .clone();
    assert_eq!(
        (active.table_id.as_str(), active.row_id.as_str()),
        ("1", row)
    );
    let mut expected = f3d_native(written).design_configurations;
    expected[0].set_active_row(active).unwrap();
    assert_eq!(documents, expected);
    assert_eq!(
        round_trip.ir().model.configurations,
        written.model.configurations
    );
}

#[test]
fn a_written_configuration_table_is_not_charged_as_omitted() {
    let name = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
    let payload = serde_json::to_vec(&generated_configuration_document()).unwrap();
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_configuration(
                    &synthetic_geometry_smbh(),
                    name,
                    &payload,
                )),
                &DecodeOptions::default(),
            )
            .expect("configuration decode"),
    );
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let mut rooted = source_less.clone();
    set_active_row(&mut rooted, "large");
    for ir in [&source_less, &rooted] {
        let (report, round_trip) = write_and_decode(ir);
        assert_configuration_tables_not_omitted(&report, ir);
        assert_eq!(
            f3d_native(round_trip.ir()).design_configurations.len(),
            f3d_native(ir).design_configurations.len()
        );
    }
}

#[test]
fn a_source_root_record_is_written_under_its_stated_type() {
    let name = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
    let payload = serde_json::to_vec(&generated_configuration_document()).unwrap();
    let archive = crate::test_support::configuration_table::with_root_segment(
        f3d_with_configuration(&synthetic_geometry_smbh(), name, &payload),
        crate::test_support::configuration_table::root_record(
            *b"256",
            crate::test_support::configuration_table::ROOT_RECORD,
            "1",
            "large",
        ),
    );
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .expect("configuration root decode"),
    );
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let (report, round_trip) = write_and_decode(&source_less);
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.message.contains("DR-1063")),
        "{:#?}",
        report.losses
    );
    assert_configuration_tables_not_omitted(&report, &source_less);
    assert_active_row_written(&round_trip, &source_less, "large");
}
