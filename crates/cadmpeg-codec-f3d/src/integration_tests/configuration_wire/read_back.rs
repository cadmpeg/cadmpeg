// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceFidelity};
use serde_json::Value;

use crate::integration_tests::configuration_wire as wire;
use crate::records::configuration::table::{set_cell, CellData};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, object_target, property_target, string_cell, table_document,
    ROOT_RECORD,
};
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::F3dCodec;

const ENTRY: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.test.dsgcfg";

struct Corruption {
    entry: String,
    row: &'static str,
    column: &'static str,
    value: CellData,
}

thread_local! {
    static CORRUPTION: RefCell<Option<Corruption>> = const { RefCell::new(None) };
}

pub(crate) fn corrupted(archive: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;

    CORRUPTION.with_borrow_mut(|fault| {
        let fault = fault.take()?;
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
        let mut entry = zip.by_name(&fault.entry).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        let mut payload = cadmpeg_core::serde_erase::json::from_slice(&bytes).unwrap();
        set_cell(&mut payload, 0, fault.row, fault.column, Some(&fault.value));
        Some(wire::replaced(
            archive,
            &fault.entry,
            &serde_json::to_vec(&payload).unwrap(),
        ))
    })
}

fn write_and_decode(ir: &CadIr, sidecar: Option<&SourceFidelity>) -> (CadIr, Vec<LossNote>) {
    let path = if sidecar.is_some() {
        "patched"
    } else {
        "synthesized"
    };
    let (report, archive) =
        crate::test_support::configuration_table::written_on(ir, sidecar, &[path]);
    let back = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    (back.into_parts().0, report.losses)
}

#[test]
fn patch_readback_detects_an_archive_cell_changed_after_planning() {
    let (source, fixture) = wire::fixture(true);
    let mut ir = source.ir().clone();
    wire::named_mut(&mut ir, "B")
        .overrides
        .parameters
        .insert(fixture.width, wire::length_override(9.0));
    CORRUPTION.with_borrow_mut(|fault| {
        *fault = Some(Corruption {
            entry: fixture.entry,
            row: "b",
            column: "P",
            value: CellData::Text("Wrong part number".into()),
        });
    });
    let (_, losses) = write_and_decode(&ir, Some(source.source_fidelity()));
    assert!(
        losses.iter().any(|loss| {
            loss.message.contains("written archive")
                && loss.message.contains("row b, column P")
                && loss.message.contains("Wrong part number")
        }),
        "{losses:#?}"
    );
}

fn native_width(name: &str) -> crate::records::parameters::DesignParameter {
    use crate::records::identity::{Located, RecordedValue};
    use crate::records::parameters::{
        DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
    };
    use crate::records::references::DesignClassTag;

    DesignParameterDraft {
        id: "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#0".into(),
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 700,
        source_ordinal: 0,
        source: DesignParameterSource::User {
            source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 22,
            }),
        },
        expression: "2 mm".into(),
        expression_offset: 36,
        source_kind_offset: 70,
        unit: Some(RecordedValue {
            value: "mm".into(),
            offset: 110,
        }),
        name: name.into(),
        name_offset: 120,
        evaluated_value: 0.2,
        evaluated_value_offset: 150,
    }
    .try_into()
    .unwrap()
}

fn seed(active_cell: bool) -> CadIr {
    use crate::records::configuration::DesignConfigurationActiveRow;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut payload = table_document(
        "1",
        &[
            ("W", object_target("700", "UserParameterValue")),
            ("S", object_target("40", "DcFeature")),
            ("P", property_target("Part Number")),
        ],
        &[
            (
                "a",
                "A",
                vec![
                    ("W", expression_cell("2 mm", 0.2)),
                    ("S", bool_cell(true)),
                    ("P", string_cell("A-1")),
                ],
            ),
            (
                "b",
                "B",
                vec![
                    ("W", expression_cell("7 mm", 0.7)),
                    ("S", bool_cell(active_cell)),
                    ("P", string_cell("B-1")),
                ],
            ),
        ],
        &["a", "b"],
    );
    payload["testRowData"] = Value::Array([
        cadmpeg_core::json!({"DataType": "BoolValue", "ColumnUniqueID": "S", "Value": true}),
        cadmpeg_core::json!({"DataType": "ParameterDoubleValue", "ColumnUniqueID": "W", "UserName": "Width", "Expression": "2 mm", "Value": 0.2}),
    ].iter().map(|entry| Value::from(entry.to_string())).collect());
    let mut table = DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        payload.as_object().unwrap().clone(),
    )
    .unwrap();
    table
        .set_active_row(DesignConfigurationActiveRow {
            record_index: ROOT_RECORD,
            table_id: "1".into(),
            row_id: "a".into(),
        })
        .unwrap();
    update_f3d_native(&mut ir, |native| {
        native.design_parameters.push(native_width("Width"));
        native.design_configurations.push(table);
    });
    let native = f3d_native(&ir);
    ir.model.parameters = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                source_parameters: &native.design_parameters,
                ..Default::default()
            },
        )
        .unwrap()
        .parameters
    });
    let targets = wire::targets(&ir, &native);
    ir.model.configurations =
        crate::design::configurations::project(&native.design_configurations, &targets)
            .unwrap()
            .configurations;
    ir
}

fn authored_width(ir: &mut CadIr, rename: bool) -> cadmpeg_ir::features::ParameterId {
    let definition = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .map_or_else(crate::tree::file_root_id, |definition| {
            definition.id.clone()
        });
    let scopes = BTreeMap::from([(String::new(), definition.clone())]);
    ir.design.parameters = crate::design_v11::parameters::project(
        &ir.model.parameters,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &mut Vec::new(),
    );
    ir.design.configurations = crate::design_v11::configurations::project(
        &ir.model.configurations,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &ir.design.parameters,
        &mut Vec::new(),
    )
    .records;
    let a = wire::named(ir, "A").id.clone();
    ir.design.scopes.push(DesignScope {
        definition,
        roots: Vec::new(),
        active_configuration: Some(a),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    if rename {
        ir.design.parameters[0].name = Some(cadmpeg_core::nonblank_literal!("RenamedWidth"));
    }
    ir.design.parameters[0].id.clone()
}

#[test]
fn generator_rebinds_a_resolved_column_and_uses_the_written_parameter_name() {
    let mut ir = seed(false);
    let width = authored_width(&mut ir, true);
    wire::named_mut(&mut ir, "A")
        .overrides
        .parameters
        .insert(width, wire::length_override(9.0));
    let (back, losses) = write_and_decode(&ir, None);
    let native = f3d_native(&back);
    let width = &native.design_parameters[0];
    assert_ne!(width.record_index, 700);
    assert_eq!(width.name(), "RenamedWidth");
    let width = crate::ids::neutral_parameter_id(width);
    let row = back
        .model
        .configurations
        .iter()
        .find(|row| row.name.as_deref() == Some("B"))
        .unwrap();
    assert_eq!(
        row.parameter_overrides.get(&width).map(String::as_str),
        Some("7 mm"),
        "{losses:#?}"
    );
    let test_width = wire::test_row(&back, ENTRY, "W");
    assert_eq!(test_width["UserName"], "RenamedWidth");
    assert_eq!(test_width["Expression"], "9 mm");
    assert_eq!(wire::parameter_cell(&back, ENTRY, "a", &width).0, "9 mm");
}

#[test]
fn generator_reads_the_moved_rows_unresolved_feature_value() {
    for computed in [false, true] {
        let mut ir = seed(computed);
        for row in &mut ir.model.configurations {
            row.active = row.name.as_deref() == Some("B");
        }
        let (back, losses) = write_and_decode(&ir, None);
        assert_eq!(
            wire::test_row(&back, ENTRY, "S")["Value"],
            computed,
            "{losses:#?}"
        );
        assert!(
            !losses
                .iter()
                .any(|loss| loss.message.contains("testRowData")
                    && loss.message.contains("column S")),
            "{losses:#?}"
        );
    }
}

#[test]
fn generator_readback_detects_an_archive_cell_changed_after_planning() {
    let ir = seed(false);
    CORRUPTION.with_borrow_mut(|fault| {
        *fault = Some(Corruption {
            entry: ENTRY.into(),
            row: "b",
            column: "P",
            value: CellData::Text("Wrong part number".into()),
        });
    });
    let (_, losses) = write_and_decode(&ir, None);
    assert!(
        losses.iter().any(|loss| {
            loss.message.contains("written archive")
                && loss.message.contains("row b, column P")
                && loss.message.contains("Wrong part number")
        }),
        "{losses:#?}"
    );
}

#[test]
fn a_parameter_rename_updates_the_test_row_without_a_value_edit() {
    let mut ir = seed(false);
    authored_width(&mut ir, true);
    ir.model.parameters[0].name = "RenamedWidth".into();
    let (back, _) = write_and_decode(&ir, None);
    let width = wire::test_row(&back, ENTRY, "W");
    assert_eq!(width["UserName"], "RenamedWidth");
    assert_eq!(width["Expression"], "2 mm");
}
