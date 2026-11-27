// SPDX-License-Identifier: Apache-2.0
//! Source-less writes of the `DesignConfigurationRoot` record and of the
//! configuration tables beside it.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use super::generated_configuration_document;
use crate::test_support::configuration_table::{
    root_record, set_active_row, table_document, with_root_segment, ROOT_RECORD,
};
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::{f3d_with_configuration, f3d_with_configurations};
use crate::F3dCodec;

const ENTRY: &str = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";

/// The reason the patch path gives for a document whose configurations
/// differ from the projection of its configuration tables.
const DECLINED: &str = "the patch carries the configuration tables as stored";

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
        .plan(EncodeInput::new(ir, None), TargetRequest::Unnamed)
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
        .plan(EncodeInput::new(&ir, None), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut first_bytes))
        .unwrap();
    assert_eq!(generated_type_losses(&first), 1, "{:#?}", first.losses);
    let again = source_less(first_bytes);
    let (second, round_trip) = write_and_decode(&again);
    assert_active_row_written(&round_trip, &again, "large");
    assert_eq!(generated_type_losses(&second), 1, "{:#?}", second.losses);
}

#[test]
fn a_source_row_of_a_table_document_with_no_rows_is_written_as_stated() {
    let payload = serde_json::to_vec(&table_document("1", &[], &[], &[])).unwrap();
    for table_id in ["1", "9"] {
        let ir = source_less(with_root_segment(
            f3d_with_configuration(&synthetic_geometry_smbh(), ENTRY, &payload),
            root_record(*b"256", ROOT_RECORD, table_id, "2"),
        ));
        let document = &f3d_native(&ir).design_configurations[0];
        assert_eq!(
            document
                .active_row()
                .map(|row| (row.table_id.as_str(), row.row_id.as_str())),
            Some((table_id, "2"))
        );
        assert!(ir.model.configurations.is_empty());
        assert!(listed(&ir, ROOT_RECORD));
        let (report, round_trip) = write_and_decode(&ir);
        assert!(
            report
                .losses
                .iter()
                .all(|loss| !loss.message.contains(document.id().as_str())),
            "{table_id}: {:#?}",
            report.losses
        );
        let written = &f3d_native(round_trip.ir()).design_configurations[0];
        let active = written
            .active_row()
            .expect("the written root record names the stated row");
        assert_eq!(
            (active.table_id.as_str(), active.row_id.as_str()),
            (table_id, "2")
        );
        assert_eq!(written.payload(), document.payload());
        assert!(listed(round_trip.ir(), active.record_index));
        assert!(round_trip.ir().model.configurations.is_empty());
    }
}

#[test]
fn a_table_id_another_document_holds_decodes_renamed() {
    const OTHER: &str = "FusionAssetName[Active]/DesignConfigurationTable.456.dsgcfg";
    let mut ir = source_less(rooted());
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        let empty = || {
            table_document("1", &[], &[], &[])
                .as_object()
                .unwrap()
                .clone()
        };
        let rooted = &native.design_configurations[0];
        let mut payload = rooted.payload().clone();
        payload["tables"]
            .as_array_mut()
            .unwrap()
            .push(empty()["tables"][0].clone());
        let mut twinned = crate::records::configuration::DesignConfiguration::try_new(
            rooted.entry_name().clone(),
            rooted.kind(),
            payload,
        )
        .unwrap();
        twinned
            .set_active_row(rooted.active_row().unwrap().clone())
            .unwrap();
        native.design_configurations[0] = twinned;
        let other = crate::records::configuration::DesignConfiguration::try_new(
            OTHER.into(),
            native.design_configurations[0].kind(),
            empty(),
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
    let table_ids = |entry: &str| {
        document(entry)
            .tables()
            .iter()
            .map(|table| table.id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(table_ids(ENTRY), ["1-1", "1"]);
    assert_eq!(table_ids(OTHER), ["1"]);
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

/// No `export:native.records-omitted` loss of `report` drops the Design
/// stream.
fn assert_design_stream_written(report: &cadmpeg_ir::report::export::ExportReport) {
    assert!(
        report.losses.iter().all(|loss| {
            loss.code.to_string() != "export/native.records-omitted"
                || !loss.message.contains("of the Design stream")
        }),
        "{:#?}",
        report.losses
    );
}

#[test]
fn two_populated_tables_and_a_root_record_are_written_as_the_decoder_states_them() {
    const OTHER: &str = "FusionAssetName[Active]/DesignConfigurationTable.456.dsgcfg";
    let first = serde_json::to_vec(&generated_configuration_document()).unwrap();
    let second = serde_json::to_vec(&table_document(
        "2",
        &[],
        &[("x", "Row x", Vec::new())],
        &["x"],
    ))
    .unwrap();
    let ir = source_less(with_root_segment(
        f3d_with_configurations(
            &synthetic_geometry_smbh(),
            &[(ENTRY, &first), (OTHER, &second)],
        ),
        root_record(*b"256", ROOT_RECORD, "1", "large"),
    ));
    let documents = f3d_native(&ir).design_configurations;
    assert_eq!(documents.len(), 2);
    assert!(documents[0].active_row().is_some());
    assert!(ir.model.configurations.is_empty());
    let mut read = ir.clone();
    read.model.drop_omitted_arenas();
    for ir in [&ir, &read] {
        let (report, round_trip) = write_and_decode(ir);
        assert_design_stream_written(&report);
        assert!(
            report.losses.iter().all(|loss| documents
                .iter()
                .all(|document| !loss.message.contains(document.id().as_str()))),
            "{:#?}",
            report.losses
        );
        let written = f3d_native(round_trip.ir()).design_configurations;
        assert_eq!(written.len(), 2);
        let active = written[0]
            .active_row()
            .expect("the written root record names the stated row")
            .clone();
        assert_eq!(
            (active.table_id.as_str(), active.row_id.as_str()),
            ("1", "large")
        );
        assert!(listed(round_trip.ir(), active.record_index));
        for (written, stated) in written.iter().zip(&documents) {
            assert_eq!(written.payload(), stated.payload());
        }
        assert!(round_trip.ir().model.configurations.is_empty());
    }
}

fn key(text: &str) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new(text.to_owned()).unwrap()
}

#[test]
fn an_edited_configuration_reads_back_as_the_neutral_record() {
    let mut ir = source_less(configured());
    set_active_row(&mut ir, "large");
    let id = |row| crate::ids::neutral_configuration_id(ENTRY, row);
    let configurations = &mut ir.model.configurations;
    configurations.retain(|configuration| configuration.id != id("small"));
    let medium = configurations
        .iter_mut()
        .find(|configuration| configuration.id == id("medium"))
        .unwrap();
    medium.name = Some("Mid".into());
    medium.ordinal = 0;
    medium
        .properties
        .insert(key("property:Part Number"), "M-2".into());
    medium
        .properties
        .insert(key("suppressed:S"), "false".into());
    medium.material = Some("Steel".into());
    let large = configurations
        .iter_mut()
        .find(|configuration| configuration.id == id("large"))
        .unwrap();
    large.ordinal = 1;
    let mut extra = large.clone();
    extra.id = id("extra");
    extra.name = Some("Extra".into());
    extra.ordinal = 2;
    extra.active = false;
    extra.properties.remove("parameter:W");
    extra
        .properties
        .insert(key("property:Part Number"), "X-1".into());
    configurations.push(extra);

    let (report, round_trip) = write_and_decode(&ir);
    assert_design_stream_written(&report);
    let charged = report
        .losses
        .iter()
        .filter(|loss| {
            ["export/edit.dropped", "export/value.unwritable"]
                .contains(&loss.code.to_string().as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(charged.len(), 1, "{:#?}", report.losses);
    assert_eq!(charged[0].code.to_string(), "export/edit.dropped");
    assert!(charged[0]
        .message
        .contains(&format!("{}: material", id("medium"))));
    let mut expected = ir.model.configurations.clone();
    expected.sort_by(|left, right| left.id.cmp(&right.id));
    for configuration in &mut expected {
        configuration.material = None;
    }
    assert_eq!(round_trip.ir().model.configurations, expected);
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let rows = documents[0].tables()[0]
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(rows, ["medium", "large", "extra"]);
}

#[test]
fn a_renamed_configuration_keeps_its_rule() {
    const RULE: &str = "FusionAssetName[Active]/DesignConfigurationRule.123.dsgcfgrule";
    let table = serde_json::to_vec(&generated_configuration_document()).unwrap();
    let rule =
        serde_json::to_vec(&cadmpeg_core::json!({"when": "Width > 1", "activate": "Medium"}))
            .unwrap();
    let mut ir = source_less(f3d_with_configurations(
        &synthetic_geometry_smbh(),
        &[(ENTRY, &table), (RULE, &rule)],
    ));
    let rule_key = format!("activation_rule:{RULE}");
    let medium = crate::ids::neutral_configuration_id(ENTRY, "medium");
    let configuration = ir
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id == medium)
        .unwrap();
    assert_eq!(configuration.properties[rule_key.as_str()], "Width > 1");
    configuration.name = Some("Mid".into());

    let (report, round_trip) = write_and_decode(&ir);
    assert_design_stream_written(&report);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains(medium.as_str())),
        "{:#?}",
        report.losses
    );
    let written = round_trip
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.id == medium)
        .expect("the renamed configuration reads back");
    assert_eq!(written.name.as_deref(), Some("Mid"));
    assert_eq!(written.properties[rule_key.as_str()], "Width > 1");
    let rules = f3d_native(round_trip.ir()).design_configurations;
    let rule = rules
        .iter()
        .find_map(crate::records::configuration::DesignConfiguration::rule)
        .unwrap();
    assert_eq!(rule["activate"], "Mid");
}

#[test]
fn a_patch_of_a_document_read_from_cadir_is_not_declined_over_its_configurations() {
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(configured()), &DecodeOptions::default())
            .expect("configuration decode"),
    );
    let mut read = decoded.ir().clone();
    read.model.drop_omitted_arenas();
    assert!(read.model.configurations.is_empty());
    crate::test_support::native_test::update_f3d_native(&mut read, |native| {
        let configuration = &mut native.design_configurations[0];
        let mut payload = configuration.payload().clone();
        payload["tables"][0]["rows"][2]["asset"]["attributes"]["pim:configuration:rowName"] =
            "Huge".into();
        *configuration = configuration.with_payload(payload).unwrap();
    });
    let expected = f3d_native(&read).design_configurations;
    let mut bytes = Vec::new();
    let path =
        crate::test_support::plan_inherited_write(&read, decoded.source_fidelity(), &mut bytes)
            .expect("patch of a CADIR read");
    assert!(
        matches!(path, cadmpeg_ir::report::export::WritePath::Patched { .. }),
        "{path:?}"
    );
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("patched round trip");
    assert_eq!(f3d_native(round_trip.ir()).design_configurations, expected);
}

#[test]
fn a_document_read_from_cadir_writes_the_root_record_its_tables_state() {
    let mut ir = source_less(rooted());
    ir.model.drop_omitted_arenas();
    let (report, round_trip) = write_and_decode(&ir);
    assert_design_stream_written(&report);
    assert_eq!(generated_type_losses(&report), 0, "{:#?}", report.losses);
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let active = documents[0]
        .active_row()
        .expect("the written root record names the stated row");
    assert_eq!(
        (active.table_id.as_str(), active.row_id.as_str()),
        ("1", "large")
    );
}

/// A design with features and parameter scopes.
const DESIGN: &[u8] =
    include_bytes!("../../../../tests/golden/fixtures/design_sketch_constraints.f3d");

/// A design with features and a table document whose one column is a
/// property, so that no column names a feature.
fn designed() -> Vec<u8> {
    use crate::test_support::configuration_table::{property_target, string_cell};
    designed_with(&table_document(
        "1",
        &[("P", property_target("Part Number"))],
        &[
            ("a", "A", vec![("P", string_cell("A-1"))]),
            ("b", "B", vec![("P", string_cell("B-1"))]),
        ],
        &["a", "b"],
    ))
}

/// [`DESIGN`] with table document `document`.
fn designed_with(document: &impl serde::Serialize) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new_append(Cursor::new(DESIGN.to_vec())).unwrap();
    zip.start_file(
        ENTRY,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&serde_json::to_vec(&document).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

/// Make row `a` of `ir` active and its configuration state a feature that
/// is not suppressed as suppressed. Returns the configuration and the
/// feature.
fn contradicted(ir: &mut cadmpeg_ir::CadIr) -> (String, String) {
    set_active_row(ir, "a");
    let mut feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.suppressed != Some(true))
        .expect("a feature that is not suppressed")
        .clone();
    feature.suppressed = Some(true);
    let id = crate::ids::neutral_configuration_id(ENTRY, "a");
    let configuration = ir
        .model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id == id)
        .unwrap();
    assert!(configuration.active);
    configuration
        .feature_states
        .insert(feature.id.clone(), feature.configuration_state());
    (configuration.id.to_string(), feature.id.to_string())
}

#[test]
fn a_suppression_no_column_holds_declines_the_patch_and_is_charged() {
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(designed()), &DecodeOptions::default())
            .expect("design decode"),
    );
    assert!(!decoded.ir().model.configurations.is_empty());
    let mut ir = decoded.ir().clone();
    let (configuration, _) = contradicted(&mut ir);
    let report = crate::test_support::assert_written_with_loss(
        &ir,
        Some(decoded.source_fidelity()),
        "f3d/writer.patch-declined",
        DECLINED,
    );
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() != "f3d/writer.patch-declined"
                && loss.message.contains(&configuration)
        }),
        "{:#?}",
        report.losses
    );
}

/// A configuration that states a definition other than its feature's: the
/// patch path declines, and the regenerated file charges the configuration.
#[test]
fn a_definition_no_column_holds_writes_and_is_charged() {
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(designed()), &DecodeOptions::default())
            .expect("design decode"),
    );
    let mut ir = decoded.ir().clone();
    let feature = ir.model.features[0].clone();
    let id = crate::ids::neutral_configuration_id(ENTRY, "b");
    let mut state = feature.configuration_state();
    state.definition = cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native {
            kind: "Revolve".into(),
            parameters: std::collections::BTreeMap::new(),
        },
    );
    ir.model
        .configurations
        .iter_mut()
        .find(|configuration| configuration.id == id)
        .unwrap()
        .feature_states
        .insert(feature.id.clone(), state);
    let report = crate::test_support::assert_written_with_loss(
        &ir,
        Some(decoded.source_fidelity()),
        "f3d/writer.configuration-omitted",
        id.as_str(),
    );
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.to_string() == "f3d/writer.patch-declined" && loss.message.contains(DECLINED)
        }),
        "{:#?}",
        report.losses
    );
}

/// A column that names a feature through its parameter scope, in a document
/// whose parameter scopes the source-less Design stream has no form for: the
/// generator writes the design from its neutral records and charges each
/// configuration with the native Design stream (cadir-v11.md §3.4, W2). No
/// table is written, so no feature state is written or read back otherwise.
#[test]
fn a_feature_column_beside_parameter_scopes_is_charged_with_its_configuration() {
    use crate::test_support::configuration_table::{bool_cell, object_target};
    let plain = F3dCodec
        .decode(&mut Cursor::new(DESIGN), &DecodeOptions::default())
        .expect("design decode");
    let (scope, feature) = f3d_native(plain.ir())
        .design_parameter_scopes
        .iter()
        .find_map(|scope| {
            let id = crate::ids::neutral_feature_id(scope);
            plain
                .ir()
                .model
                .features
                .iter()
                .any(|feature| feature.id == id)
                .then_some((scope.record_index, id))
        })
        .expect("a parameter scope of a feature");
    let ir = source_less(designed_with(&table_document(
        "1",
        &[("F", object_target(&scope.to_string(), "DcFeature"))],
        &[
            ("a", "A", vec![("F", bool_cell(false))]),
            ("b", "B", vec![("F", bool_cell(true))]),
        ],
        &["a", "b"],
    )));
    let suppressed = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.id == crate::ids::neutral_configuration_id(ENTRY, "b"))
        .and_then(|configuration| configuration.feature_states.get(&feature))
        .expect("the column states the feature in row b");
    assert_eq!(
        suppressed.evaluation,
        cadmpeg_ir::features::parameters::ConfigurationEvaluation::Suppressed {}
    );
    let (report, round_trip) = write_and_decode(&ir);
    assert_eq!(ir.model.configurations.len(), 2);
    for configuration in &ir.model.configurations {
        let id = configuration.id.as_str();
        assert!(
            report.losses.iter().any(|loss| {
                loss.code.to_string() == "f3d/writer.configuration-omitted"
                    && loss.message.contains(id)
            }),
            "{id}: {:#?}",
            report.losses
        );
        assert!(
            report.losses.iter().all(|loss| {
                loss.code.to_string() != "export/edit.dropped" || !loss.message.contains(id)
            }),
            "{id}: {:#?}",
            report.losses
        );
    }
    assert!(f3d_native(round_trip.ir()).design_configurations.is_empty());
    assert!(round_trip.ir().model.configurations.is_empty());
}
