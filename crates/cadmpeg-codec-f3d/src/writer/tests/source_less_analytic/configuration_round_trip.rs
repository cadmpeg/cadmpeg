// SPDX-License-Identifier: Apache-2.0
//! Writes of every configuration table shape that the synthetic archive
//! holds, on the patch path and on the generator path. A document the decode
//! returned, also read from CADIR, replays the source (cadir-v11.md §3.1,
//! Rule B). The archive's design states no configuration, so an active row
//! that the native records and the v10 rows move is written: the patch
//! rewrites the root record in place or charges it, and the generator
//! writes it.

use std::io::{Cursor, Read};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::configuration_table::{
    bool_cell, object_target, project, property_target, root_record, string_cell, table_document,
    unsourced, with_root_segment, written_on, ROOT_RECORD,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configurations;
use crate::F3dCodec;

const TABLE: &str = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
const RULE: &str = "FusionAssetName[Active]/DesignConfigurationRule.123.dsgcfgrule";
const EMPTY: &str = "FusionAssetName[Active]/DesignConfigurationTable.456.dsgcfg";

/// One table shape of [`shapes`].
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// The row that the root record names.
    root: Option<&'static str>,
    /// Whether a rule document activates `Tall`.
    rule: bool,
    /// Whether a second table document holds a table with no rows.
    second_table: bool,
    /// Whether the native records and the v10 rows move the active row to
    /// `wide` before the write.
    moved: bool,
}

/// Every combination of a root record naming no row, a row with a cell for
/// each column or a row with one cell; a rule document or none; a second
/// table with no rows or none; and the active row moved after the decode.
/// The rows of the table are `short` (a cell for each column), `tall` (a
/// property cell only) and `wide` (no cell). Column `W` names a parameter
/// record and column `S` a feature record that the archive does not hold.
fn shapes() -> Vec<Shape> {
    let mut shapes = Vec::new();
    for root in [None, Some("short"), Some("tall")] {
        for bits in 0..8_u8 {
            let moved = bits & 4 != 0;
            if moved && root.is_none() {
                continue;
            }
            shapes.push(Shape {
                root,
                rule: bits & 1 != 0,
                second_table: bits & 2 != 0,
                moved,
            });
        }
    }
    shapes
}

/// The archive of `shape`.
fn archive(shape: Shape) -> Vec<u8> {
    let table = serde_json::to_vec(&table_document(
        "1",
        &[
            ("P", property_target("Part Number")),
            ("W", object_target("71", "UserParameterValue")),
            ("S", object_target("40", "DcFeature")),
        ],
        &[
            (
                "short",
                "Short",
                vec![
                    ("P", string_cell("S-1")),
                    (
                        "W",
                        crate::test_support::configuration_table::expression_cell("10 mm", 1.0),
                    ),
                    ("S", bool_cell(false)),
                ],
            ),
            ("tall", "Tall", vec![("P", string_cell("T-1"))]),
            ("wide", "Wide", Vec::new()),
        ],
        &["short", "tall", "wide"],
    ))
    .unwrap();
    let rule = serde_json::to_vec(&cadmpeg_core::json!({"when": "Width > 1", "activate": "Tall"}))
        .unwrap();
    let empty = serde_json::to_vec(&table_document(
        "2",
        &[("P", property_target("Part Number"))],
        &[],
        &[],
    ))
    .unwrap();
    let mut documents = vec![(TABLE, table.as_slice())];
    if shape.rule {
        documents.push((RULE, &rule));
    }
    if shape.second_table {
        documents.push((EMPTY, &empty));
    }
    let archive = f3d_with_configurations(&synthetic_geometry_smbh(), &documents);
    match shape.root {
        Some(row) => with_root_segment(archive, root_record(*b"256", ROOT_RECORD, "1", row)),
        None => archive,
    }
}

/// The bytes of each configuration document entry of `archive`.
fn documents(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut documents = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        if [TABLE, RULE, EMPTY].contains(&name.as_str()) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            documents.push((name, bytes));
        }
    }
    documents.sort();
    documents
}

/// `report` charges no configuration and names no configuration document.
fn assert_uncharged<'a>(
    shape: Shape,
    losses: impl IntoIterator<Item = &'a cadmpeg_ir::report::loss::LossNote>,
) {
    for loss in losses {
        let code = loss.code.to_string();
        assert!(
            ![
                "export/edit.dropped",
                "export/value.unwritable",
                "f3d/writer.configuration-omitted",
            ]
            .contains(&code.as_str())
                && [TABLE, RULE, EMPTY]
                    .iter()
                    .all(|entry| !loss.message.contains(entry)),
            "{shape:?}: {loss:#?}"
        );
    }
}

#[test]
fn every_table_shape_is_written_as_stated_with_only_a_moved_root_charged() {
    for shape in shapes() {
        let archive = archive(shape);
        let decoded = EditableDecodeResult::from(
            F3dCodec
                .decode(
                    &mut Cursor::new(archive.as_slice()),
                    &DecodeOptions::default(),
                )
                .expect("shape decode"),
        );
        let mut ir = decoded.ir().clone();
        if shape.moved {
            crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
                native
                    .design_configurations
                    .iter_mut()
                    .find(|document| document.entry_name() == TABLE)
                    .unwrap()
                    .set_active_row(
                        crate::records::configuration::DesignConfigurationActiveRow {
                            record_index: ROOT_RECORD,
                            table_id: "1".into(),
                            row_id: "wide".into(),
                        },
                    )
                    .unwrap();
            });
            ir.model.configurations = project(&ir);
        }
        let mut read = ir.clone();
        read.model.drop_omitted_arenas();
        let fidelity = Some(decoded.source_fidelity());
        let preserved = if shape.moved {
            "patched"
        } else {
            "verbatim_replay"
        };
        for (path, target, fidelity, expected) in [
            ("decode", &ir, fidelity, preserved),
            ("CADIR read", &read, fidelity, preserved),
            ("source-less decode", &unsourced(&ir), None, "synthesized"),
            (
                "source-less CADIR read",
                &unsourced(&read),
                None,
                "synthesized",
            ),
        ] {
            let (report, bytes) = written_on(target, fidelity, &[expected]);
            let moved_root = |loss: &&cadmpeg_ir::report::loss::LossNote| {
                loss.code.to_string() == "export/value.unwritable"
                    && loss.message.contains("DesignConfigurationRoot")
                    && loss.message.contains("wide")
            };
            assert_eq!(
                report.losses.iter().filter(moved_root).count(),
                usize::from(shape.moved && fidelity.is_some()),
                "{shape:?} {path}: {:#?}",
                report.losses
            );
            assert_uncharged(shape, report.losses.iter().filter(|loss| !moved_root(loss)));
            assert_eq!(documents(&bytes), documents(&archive), "{shape:?} {path}");
        }
    }
}
