// SPDX-License-Identifier: Apache-2.0
//! Source-less writes of the `DesignConfigurationRoot` record and of the
//! configuration tables beside it.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use super::generated_configuration_document;
use crate::test_support::configuration_table::{
    root_record, set_active_row, table_document, with_root_segment, ROOT_RECORD,
};
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;
use crate::F3dCodec;

const ENTRY: &str = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";

/// `archive` decoded, with no source and no unknown native records.
fn source_less(archive: Vec<u8>) -> cadmpeg_ir::CadIr {
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .expect("configuration decode"),
    );
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "f3d",
        &[],
    )
    .unwrap();
    ir
}

/// An archive that holds the generated configuration document.
fn configured() -> Vec<u8> {
    let payload = serde_json::to_vec(&generated_configuration_document()).unwrap();
    f3d_with_configuration(&synthetic_geometry_smbh(), ENTRY, &payload)
}

/// [`configured`] with a root record of index [`ROOT_RECORD`] that names row
/// `large`.
fn rooted() -> Vec<u8> {
    with_root_segment(
        configured(),
        root_record(*b"256", ROOT_RECORD, "1", "large"),
    )
}

/// Whether a type row of `ir`'s Design `MetaStream` lists record `index`.
fn listed(ir: &cadmpeg_ir::CadIr, index: u32) -> bool {
    f3d_native(ir).design_types.iter().any(|design_type| {
        design_type
            .entities
            .values()
            .any(|entity| *entity == u64::from(index))
    })
}

/// The losses of `report` that charge the generated root type row.
fn generated_type_losses(report: &cadmpeg_ir::report::export::ExportReport) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| {
            loss.code.to_string() == "export/record.residual-unavailable"
                && loss.message.contains("DR-1063")
        })
        .count()
}

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
        .plan(EncodeInput::new(ir, None))
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
    let source_less = source_less(configured());
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
    let source_less = source_less(rooted());
    let (report, round_trip) = write_and_decode(&source_less);
    assert_eq!(generated_type_losses(&report), 0, "{:#?}", report.losses);
    assert_configuration_tables_not_omitted(&report, &source_less);
    assert_active_row_written(&round_trip, &source_less, "large");
    assert!(!listed(round_trip.ir(), ROOT_RECORD));
}

#[test]
fn the_generated_type_is_charged_again_on_a_rewrite() {
    let mut ir = source_less(configured());
    set_active_row(&mut ir, "large");
    let mut first_bytes = Vec::new();
    let first = F3dCodec
        .plan(EncodeInput::new(&ir, None))
        .and_then(|plan| plan.write_to(&mut first_bytes))
        .unwrap();
    assert_eq!(generated_type_losses(&first), 1, "{:#?}", first.losses);
    let again = source_less(first_bytes);
    let (second, round_trip) = write_and_decode(&again);
    assert_active_row_written(&round_trip, &again, "large");
    assert_eq!(generated_type_losses(&second), 1, "{:#?}", second.losses);
}

#[test]
fn a_source_row_no_table_holds_is_charged_and_leaves_the_type_table() {
    let payload = serde_json::to_vec(&table_document("1", &[], &[], &[])).unwrap();
    for table_id in ["1", "9"] {
        let ir = source_less(with_root_segment(
            f3d_with_configuration(&synthetic_geometry_smbh(), ENTRY, &payload),
            root_record(*b"256", ROOT_RECORD, table_id, "2"),
        ));
        let document = &f3d_native(&ir).design_configurations[0];
        assert_eq!(
            document.active_row().map(|row| row.table_id.as_str()),
            Some(table_id)
        );
        assert!(ir.model.configurations.is_empty());
        assert!(listed(&ir, ROOT_RECORD));
        let (report, round_trip) = write_and_decode(&ir);
        let charged = report
            .losses
            .iter()
            .filter(|loss| {
                loss.code.to_string() == "export/value.unwritable"
                    && loss.message.contains(document.id().as_str())
                    && loss
                        .message
                        .contains(&format!("table id {table_id} row id 2"))
            })
            .count();
        assert_eq!(charged, 1, "{table_id}: {:#?}", report.losses);
        assert!(f3d_native(round_trip.ir()).design_configurations[0]
            .active_row()
            .is_none());
        assert!(!listed(round_trip.ir(), ROOT_RECORD));
    }
}

#[test]
fn a_table_id_another_document_holds_decodes_renamed() {
    const OTHER: &str = "FusionAssetName[Active]/DesignConfigurationTable.456.dsgcfg";
    let mut ir = source_less(rooted());
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        let other = crate::records::configuration::DesignConfiguration::try_new(
            OTHER.into(),
            native.design_configurations[0].kind(),
            table_document("1", &[], &[], &[])
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap();
        native.design_configurations.push(other);
    });
    ir.model.configurations = crate::test_support::configuration_table::project(&ir);
    let (report, round_trip) = write_and_decode(&ir);
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains("1 written as 1-1")
        }),
        "{:#?}",
        report.losses
    );
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let document = |entry: &str| {
        documents
            .iter()
            .find(|document| document.entry_name().as_str() == entry)
            .expect("a written configuration table document")
    };
    let active = document(ENTRY)
        .active_row()
        .expect("the renamed root record binds to its document");
    assert_eq!(
        (active.table_id.as_str(), active.row_id.as_str()),
        ("1-1", "large")
    );
    assert_eq!(document(ENTRY).tables()[0].id, "1-1");
    assert_eq!(document(OTHER).tables()[0].id, "1");
    assert!(document(OTHER).active_row().is_none());
    assert!(round_trip
        .ir()
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.active));
}

#[test]
fn a_cleared_active_row_leaves_only_written_records_in_the_type_table() {
    let mut ir = source_less(rooted());
    assert!(listed(&ir, ROOT_RECORD));
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        let document = &native.design_configurations[0];
        native.design_configurations[0] =
            crate::records::configuration::DesignConfiguration::try_new(
                document.entry_name().clone(),
                document.kind(),
                document.payload().clone(),
            )
            .unwrap();
    });
    ir.model.configurations = crate::test_support::configuration_table::project(&ir);
    let (_, decoded) = write_and_decode(&ir);
    let round_trip = f3d_native(decoded.ir());
    assert!(round_trip.design_configurations[0].active_row().is_none());
    assert!(!listed(decoded.ir(), ROOT_RECORD));
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("primary record")),
        "{:#?}",
        decoded.report().losses
    );
}

/// A source-less design whose one table holds row `row`, active.
fn active_row_document(row: &str) -> cadmpeg_ir::CadIr {
    let payload = serde_json::to_vec(&table_document(
        "1",
        &[],
        &[(row, "Long", Vec::new())],
        &[row],
    ))
    .unwrap();
    let mut ir = source_less(f3d_with_configuration(
        &synthetic_geometry_smbh(),
        ENTRY,
        &payload,
    ));
    set_active_row(&mut ir, row);
    ir
}

#[test]
fn an_unreadable_row_id_is_charged() {
    let long = "r".repeat(300);
    let (report, round_trip) = write_and_decode(&active_row_document(&long));
    let decoded_active = f3d_native(round_trip.ir()).design_configurations[0]
        .active_row()
        .cloned();
    let charged = report.losses.iter().any(|loss| {
        loss.code.to_string() == "export/value.unwritable" && !loss.message.contains("DR-1063")
    });
    assert!(
        decoded_active.is_some() || charged,
        "the written root record does not decode back and no loss names it: {:#?}",
        report.losses
    );
}

#[test]
fn a_wide_row_id_is_cut_in_the_root_record_and_its_table() {
    let long = format!("{}\u{1F600}{}", "r".repeat(255), "s".repeat(44));
    let cut = "r".repeat(255);
    let ir = active_row_document(&long);
    let (report, round_trip) = write_and_decode(&ir);
    let configuration = crate::ids::neutral_configuration_id(ENTRY, &long);
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains("256 UTF-16 units")
                && loss.message.contains(configuration.as_str())
        }),
        "{:#?}",
        report.losses
    );
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let active = documents[0]
        .active_row()
        .expect("the cut root record decodes");
    assert_eq!(
        (active.table_id.as_str(), active.row_id.as_str()),
        ("1", cut.as_str())
    );
    let rows = documents[0].tables()[0]
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(rows, [cut.as_str()]);
}
