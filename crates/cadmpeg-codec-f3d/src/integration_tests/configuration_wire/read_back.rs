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
    mutation: Mutation,
}

enum Mutation {
    Cell {
        row: &'static str,
        column: &'static str,
        value: CellData,
    },
    ExtraTable,
    OpaqueMember,
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
        let mut payload: serde_json::Map<String, Value> =
            cadmpeg_core::serde_erase::json::from_slice(&bytes).unwrap();
        match fault.mutation {
            Mutation::Cell { row, column, value } => {
                set_cell(&mut payload, 0, row, column, Some(&value));
            }
            Mutation::ExtraTable => {
                let table = table_document("unexpected", &[], &[], &[]);
                payload
                    .get_mut("tables")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(table["tables"][0].clone());
            }
            Mutation::OpaqueMember => {
                payload.insert("opaqueMember".to_owned(), Value::from("changed"));
            }
        }
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
            mutation: Mutation::Cell {
                row: "b",
                column: "P",
                value: CellData::Text("Wrong part number".into()),
            },
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
    native_length(name, 700, 0.2, "2 mm")
}

fn native_length(
    name: &str,
    index: u32,
    cache: f64,
    expression: &str,
) -> crate::records::parameters::DesignParameter {
    use crate::records::identity::{Located, RecordedValue};
    use crate::records::parameters::{
        DesignParameterDiscriminator, DesignParameterDraft, DesignParameterSource,
    };
    use crate::records::references::DesignClassTag;

    DesignParameterDraft {
        id: format!(
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#{}",
            index - 700
        ),
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: index,
        source_ordinal: index - 700,
        source: DesignParameterSource::User {
            source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
            family_discriminator: Some(Located {
                value: DesignParameterDiscriminator::Code0,
                offset: 22,
            }),
        },
        expression: expression.into(),
        expression_offset: 36,
        source_kind_offset: 70,
        unit: Some(RecordedValue {
            value: "mm".into(),
            offset: 110,
        }),
        name: name.into(),
        name_offset: 120,
        evaluated_value: cache,
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
        &std::collections::HashMap::new(),
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
            mutation: Mutation::Cell {
                row: "b",
                column: "P",
                value: CellData::Text("Wrong part number".into()),
            },
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
fn generator_keeps_the_archive_when_readback_exhausts_its_budget() {
    use cadmpeg_core::decode::ResourceLimits;
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};

    let ir = seed(false);
    let document = f3d_native(&ir).design_configurations[0].id();
    let (_, expected) =
        crate::test_support::configuration_table::written_on(&ir, None, &["synthesized"]);
    let input = EncodeInput::new(&ir, None).with_limits(ResourceLimits {
        max_entities: 0,
        ..ResourceLimits::desktop()
    });
    let mut archive = Vec::new();
    let report = F3dCodec
        .plan(input)
        .and_then(|plan| plan.write_to(&mut archive))
        .unwrap();
    assert_eq!(report.write_path().to_string(), "synthesized");
    assert_eq!(archive, expected);
    assert!(
        report.losses.iter().any(|loss| {
            loss.code == crate::loss::F3dLossCode::WriterConfigurationReadBackUnread.kind()
                && loss.message.contains("resource limit on Entities")
                && loss.message.contains(&document)
        }),
        "{:#?}",
        report.losses
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

#[test]
fn patch_charges_a_blank_literal_part_number_that_reads_as_the_row_name() {
    use cadmpeg_ir::design::configuration::PartNumber;

    let (source, _) = wire::fixture(true);
    let mut ir = source.ir().clone();
    let b = wire::named_mut(&mut ir, "B");
    b.name = "Blue".into();
    let id = b.id.clone();
    b.overrides
        .definitions
        .entry(b.scope.clone())
        .or_default()
        .part_number = Some(PartNumber::Text(String::new()));
    let (back, losses) = write_and_decode(&ir, Some(source.source_fidelity()));
    let blue = wire::named(&back, "Blue");
    assert_eq!(
        blue.overrides.definitions[&blue.scope].part_number,
        Some(PartNumber::ConfigurationName)
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.message.contains(id.as_str())
                && loss.message.contains(".part_number")
                && loss.message.contains("Text(\"\")"))
            .count(),
        1,
        "{losses:#?}"
    );
}

#[test]
fn generator_updates_the_existing_test_entry_from_an_unbound_parameter_cell() {
    for (expression, value) in [("7 mm", 0.7), ("2 mm", 0.2)] {
        let mut ir = seed(false);
        ir.model.parameters.clear();
        update_f3d_native(&mut ir, |native| {
            native.design_parameters.clear();
            let table = &mut native.design_configurations[0];
            let mut payload = table.payload().clone();
            set_cell(
                &mut payload,
                0,
                "b",
                "W",
                Some(&CellData::Expression {
                    expression: expression.into(),
                    value: cadmpeg_ir::scalar::FiniteReal::new(value).unwrap(),
                }),
            );
            *table = table.with_payload(payload).unwrap();
        });
        ir.model.configurations = crate::test_support::configuration_table::project(&ir);
        for row in &mut ir.model.configurations {
            row.active = row.name.as_deref() == Some("B");
        }
        let (back, losses) = write_and_decode(&ir, None);
        let entry = wire::test_row(&back, ENTRY, "W");
        assert_eq!(entry["UserName"], "Width");
        assert_eq!(entry["Expression"], expression);
        assert_eq!(entry["Value"], value);
        assert!(
            losses
                .iter()
                .all(|loss| !loss.message.contains("testRowData column W")),
            "{losses:#?}"
        );
    }
}

fn add_height(ir: &mut CadIr) -> cadmpeg_ir::features::ParameterId {
    use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterValue};
    use cadmpeg_ir::scalar::FiniteReal;

    let mut parameter = ir.design.parameters[0].clone();
    parameter.id = cadmpeg_ir::features::ParameterId::mint("test:model:parameter#height").unwrap();
    parameter.name = Some(cadmpeg_core::nonblank_literal!("Height"));
    parameter.value = FiniteReal::new(3.0).map(ParameterValue::Number);
    parameter.definition =
        ParameterDefinition::Expression(wire::length_override(3.0).expression.unwrap());
    parameter.native_ref = None;
    let id = parameter.id.clone();
    ir.design.parameters.push(parameter);
    id
}

#[test]
fn generator_grows_a_column_for_a_parameter_that_exists_only_in_the_destination() {
    let mut ir = seed(false);
    let width = authored_width(&mut ir, false);
    wire::named_mut(&mut ir, "A")
        .overrides
        .parameters
        .insert(width, wire::length_override(9.0));
    let height = add_height(&mut ir);
    wire::named_mut(&mut ir, "B")
        .overrides
        .parameters
        .insert(height, wire::length_override(12.0));

    let (back, losses) = write_and_decode(&ir, None);
    let height = back
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Height")
        .unwrap();
    assert_eq!(height.expression, "3 mm");
    let (expression, cache) = wire::parameter_cell(&back, ENTRY, "b", &height.id);
    assert_eq!(expression, "12 mm", "{losses:#?}");
    assert!((cache - 1.2).abs() < 1e-9, "{cache}");
}

#[test]
fn generator_spells_references_to_renamed_parameters_in_configuration_cells() {
    use cadmpeg_ir::design::configuration::ParameterOverride;
    use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
    use cadmpeg_ir::design::expr::{Expression, Neutral, Node};

    let mut ir = seed(false);
    let record = native_length("Height", 701, 0.3, "3 mm");
    let height_source = crate::ids::neutral_parameter_id(&record);
    update_f3d_native(&mut ir, |native| {
        native.design_parameters.push(record);
        let table = &mut native.design_configurations[0];
        let mut payload = table.payload().clone();
        assert!(crate::records::configuration::table::push_column(
            &mut payload,
            0,
            "H",
            "Height",
            &crate::records::configuration::table::ColumnTarget::Parameter(701),
        ));
        *table = table.with_payload(payload).unwrap();
    });
    let mut height = ir.model.parameters[0].clone();
    height.id = height_source.clone();
    height.name = "Height".into();
    height.expression = "3 mm".into();
    height.value = cadmpeg_ir::scalar::Length::new(3.0)
        .map(cadmpeg_ir::features::parameters::ParameterValue::Length);
    ir.model.parameters.push(height);
    let width = authored_width(&mut ir, false);
    ir.design.parameters[0].name = cadmpeg_core::text::NonBlankString::new("ΔWidth".to_owned());
    wire::named_mut(&mut ir, "A")
        .overrides
        .parameters
        .insert(width.clone(), wire::length_override(9.0));
    wire::named_mut(&mut ir, "B").overrides.parameters.insert(
        height_source,
        ParameterOverride {
            expression: Some(Expression::Neutral(Neutral(Node::Reference(
                ParameterRef {
                    target: RefTarget::Parameter(width),
                    through: None,
                },
            )))),
            value: None,
        },
    );

    let (back, losses) = write_and_decode(&ir, None);
    assert!(back
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "_Width"));
    let height = back
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Height")
        .unwrap();
    let (expression, cache) = wire::parameter_cell(&back, ENTRY, "b", &height.id);
    assert_eq!(expression, "_Width", "{losses:#?}");
    assert!((cache - 0.7).abs() < 1e-9, "{cache}");
}

#[test]
fn patch_names_each_distinct_discarded_active_request() {
    let (source, _) = wire::fixture(true);
    for legacy in ["B", "C"] {
        let mut ir = source.ir().clone();
        let a = wire::named(&ir, "A").id.clone();
        let b = wire::named(&ir, "B").id.clone();
        let c = wire::named(&ir, legacy).id.clone();
        let scope = wire::named(&ir, "B").scope.clone();
        ir.design
            .scopes
            .iter_mut()
            .find(|row| row.definition == scope)
            .unwrap()
            .active_configuration = Some(b.clone());
        for row in &mut ir.model.configurations {
            row.active = row.id == c;
        }
        let (back, losses) = write_and_decode(&ir, Some(source.source_fidelity()));
        assert_eq!(wire::active(&back), Some(&a));
        let named = losses
            .iter()
            .filter(|loss| {
                loss.message
                    .contains("keeps the archive's active configuration state")
            })
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            named.matches(&format!("in place of {b}")).count(),
            1,
            "{losses:#?}"
        );
        assert_eq!(
            named.matches(" active in place of ").count(),
            if b == c { 1 } else { 2 },
            "{losses:#?}"
        );
        assert!(named.contains(&format!("in place of {c}")), "{losses:#?}");
    }
}

#[test]
fn generator_preserves_a_rule_association_when_its_entry_moves() {
    const RULE: &str = "Elsewhere/DesignConfigurationRule.a.dsgcfgrule";

    let mut ir = seed(false);
    update_f3d_native(&mut ir, |native| {
        native.design_configurations.push(
            DesignConfiguration::try_new(
                RULE.into(),
                DesignConfigurationKind::Rule,
                cadmpeg_core::json!({"when": "Width > 1", "activate": "B"})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .unwrap(),
        );
    });
    ir.model.configurations = crate::test_support::configuration_table::project(&ir);

    let (back, losses) = write_and_decode(&ir, None);
    let native = f3d_native(&back);
    let rule = native
        .design_configurations
        .iter()
        .find(|document| document.rule().is_some())
        .unwrap();
    assert_ne!(rule.entry_name(), RULE);
    assert_eq!(
        back.model
            .configurations
            .iter()
            .find(|row| row.name.as_deref() == Some("B"))
            .unwrap()
            .properties[format!("activation_rule:{}", rule.entry_name()).as_str()],
        "Width > 1"
    );
    assert!(
        losses
            .iter()
            .all(|loss| !loss.message.contains("properties.activation_rule:")),
        "{losses:#?}"
    );
}

#[test]
fn patch_keeps_the_production_suppression_of_an_unbound_base_feature() {
    use std::io::Write;

    use crate::layout::base_feature_wide_zero_body as layout;
    use crate::test_support::configuration_table::{root_record, with_root_segment_in};
    use crate::test_support::streams_test::{
        generated_design_base_feature_bulkstream, generated_design_base_feature_metastream,
    };

    let (mut bulk, records) = generated_design_base_feature_bulkstream();
    let (index, start) = *records.last().unwrap();
    let start = usize::try_from(start).unwrap();
    bulk[start + layout::ZERO_BODY_MARKER] = 1;
    bulk[start + 29] = 1;
    bulk[start + layout::SHARED_METADATA_MARKER] = 1;
    bulk[start + layout::SHARED_METADATA_RECORD..start + layout::SHARED_METADATA_RECORD + 8]
        .copy_from_slice(&985u64.to_le_bytes());
    let mut metadata = generated_design_base_feature_metastream(&records);
    let prior = b"00000000-0000-0000-0000-000000001400";
    let at = metadata
        .windows(prior.len())
        .position(|bytes| bytes == prior)
        .unwrap();
    metadata[at..at + prior.len()].copy_from_slice(b"D7015B81-AB4D-4C89-BED7-AA4411E1964A");
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(), &[], &bulk, &metadata
    );
    let payload = table_document(
        "1",
        &[
            ("S", object_target(&index.to_string(), "DcFeature")),
            ("D", property_target("Description")),
        ],
        &[
            ("a", "A", vec![("S", bool_cell(false))]),
            ("b", "B", vec![("S", bool_cell(true))]),
        ],
        &["a", "b"],
    );
    let mut zip = zip::ZipWriter::new_append(Cursor::new(archive)).unwrap();
    zip.start_file(
        ENTRY,
        crate::zip_write::file_options(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&serde_json::to_vec(&payload).unwrap())
        .unwrap();
    let archive = with_root_segment_in(
        zip.finish().unwrap().into_inner(),
        "FusionDesignSegmentType2",
        root_record(*b"256", ROOT_RECORD, "1", "a"),
    );
    let source = wire::decode(&archive);
    let mut ir = source.ir().clone();
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Base Feature"))
        .unwrap();
    let id = feature.id.clone();
    assert!(feature.evaluation.outputs().is_empty());
    assert_eq!(feature.suppressed, Some(true));
    let b = ir
        .model
        .configurations
        .iter_mut()
        .find(|row| row.name.as_deref() == Some("B"))
        .unwrap();
    b.properties.insert(
        cadmpeg_core::nonblank_literal!("property:Description"),
        "Blue".into(),
    );
    let (back, losses) = write_and_decode(&ir, Some(source.source_fidelity()));
    assert_eq!(
        back.model
            .features
            .iter()
            .find(|feature| feature.id == id)
            .unwrap()
            .suppressed,
        Some(true)
    );
    assert!(
        losses
            .iter()
            .all(|loss| !(loss.message.contains(id.as_str())
                && loss.message.contains("current state that the active row"))),
        "{losses:#?}"
    );
}

#[test]
fn readback_detects_an_unplanned_empty_table_and_changed_opaque_members() {
    for (mutation, diagnostic) in [
        (Mutation::ExtraTable, "unplanned table unexpected"),
        (Mutation::OpaqueMember, "configuration payload differs"),
    ] {
        let ir = seed(false);
        CORRUPTION.with_borrow_mut(|fault| {
            *fault = Some(Corruption {
                entry: ENTRY.into(),
                mutation,
            });
        });
        let (_, losses) = write_and_decode(&ir, None);
        assert!(
            losses.iter().any(|loss| loss.message.contains(diagnostic)),
            "{losses:#?}"
        );
    }
}

#[test]
fn generator_places_configuration_rows_in_an_empty_document() {
    let mut ir = seed(false);
    update_f3d_native(&mut ir, |native| {
        native.design_configurations[0] = DesignConfiguration::try_new(
            ENTRY.into(),
            DesignConfigurationKind::Table,
            serde_json::Map::new(),
        )
        .unwrap();
    });
    let (back, _) = write_and_decode(&ir, None);
    let native = f3d_native(&back);
    let [table] = native.design_configurations[0].tables() else {
        panic!("one table")
    };
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(table.columns.len(), 2);
    assert!(back
        .model
        .configurations
        .iter()
        .any(|row| row.active && row.name.as_deref() == Some("A")));
}

#[test]
fn patch_grows_a_feature_column_for_an_inactive_override() {
    use crate::test_support::configuration_table::{root_record, with_root_segment_in};
    use cadmpeg_ir::design::configuration::FeatureStateOverride;

    let plain = wire::decode(wire::SOURCE);
    let entry = wire::empty_table(plain.ir());
    let (feature, _) = wire::feature(plain.ir());
    let table = table_document(
        "1",
        &[("P", property_target("Part Number"))],
        &[("a", "A", vec![]), ("b", "B", vec![])],
        &["a", "b"],
    );
    let source = wire::decode(&with_root_segment_in(
        wire::replaced(wire::SOURCE, &entry, &serde_json::to_vec(&table).unwrap()),
        "FusionDesignSegmentType2",
        root_record(*b"256", ROOT_RECORD, "1", "a"),
    ));
    let mut ir = source.ir().clone();
    let own = ir
        .model
        .features
        .iter()
        .find(|record| record.id == feature)
        .unwrap();
    let suppressed = !own.configuration_state().evaluation.is_suppressed();
    wire::named_mut(&mut ir, "B").overrides.features.insert(
        feature.clone(),
        FeatureStateOverride {
            suppressed: Some(suppressed),
            definition: None,
        },
    );
    let (back, losses) = write_and_decode(&ir, Some(source.source_fidelity()));
    assert_eq!(
        wire::named(&back, "B").overrides.features[&feature].suppressed,
        Some(suppressed),
        "{losses:#?}"
    );
    let native = f3d_native(&back);
    let table = native
        .design_configurations
        .iter()
        .find(|document| document.entry_name() == &entry)
        .unwrap();
    assert!(table.tables()[0].columns.iter().any(|column| matches!(
        column.target,
        crate::records::configuration::table::ColumnTarget::Feature(_)
    )));
}
