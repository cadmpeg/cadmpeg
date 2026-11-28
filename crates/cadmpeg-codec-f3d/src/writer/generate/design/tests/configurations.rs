// SPDX-License-Identifier: Apache-2.0
//! Configuration cells and their actual timeline record bindings.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::configuration::{
    DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride,
};
use cadmpeg_ir::design::expr::{Expression, Neutral};
use cadmpeg_ir::design::parameter::ParameterDefinition;
use cadmpeg_ir::features::ConfigurationId;
use cadmpeg_ir::CadIr;

use super::{
    assert_length, loss, neutral, number, parameter, parameter_id, reference, root, scoped_cube,
    write, write_bytes,
};
use crate::integration_tests::configuration_wire::{length_override, test_row};
use crate::loss::F3dLossCode;
use crate::records::configuration::table::{ColumnTarget, ConfiguredValue};
use crate::records::configuration::{
    DesignConfiguration as NativeConfiguration, DesignConfigurationKind,
};
use crate::test_support::native_test::{f3d_native, update_f3d_native};

const TOLERANCE: f64 = 1e-9;

fn native_parameters() -> (
    CadIr,
    Vec<cadmpeg_ir::features::parameters::DesignParameter>,
) {
    let mut legacy = Vec::new();
    for (stream, name) in [
        ("f3d:Design1/BulkStream.dat", "RootLength"),
        ("f3d:part@Design1/BulkStream.dat", "MemberLength"),
    ] {
        let native = crate::test_support::configuration_table::parameter(9, name);
        let mut parameter = crate::test_support::configuration_table::neutral_parameter(&native);
        parameter.id = crate::ids::neutral_parameter_id_parts(stream, 9);
        parameter.native_ref = Some(format!("{stream}:design-parameter#9"));
        parameter.value = Some(cadmpeg_ir::features::parameters::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(10.0).unwrap(),
        ));
        legacy.push(parameter);
    }
    let scopes = BTreeMap::from([
        (String::new(), root()),
        (
            "part@".to_owned(),
            cadmpeg_ir::ids::ProductDefinitionId::mint("test:design:product#part").unwrap(),
        ),
    ]);
    let mut ir = CadIr::empty();
    ir.model.parameters = legacy.clone();
    ir.design.parameters = crate::design_v11::parameters::project(
        &legacy,
        &HashMap::default(),
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &mut Vec::new(),
    );
    (ir, legacy)
}

#[test]
fn unchanged_native_parameters_keep_their_document_scopes() {
    let (ir, projected) = native_parameters();
    assert!(matches!(
        super::super::parameters::native_or_neutral(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            &crate::native::F3dNative::default(),
            &ir.model.parameters,
            &projected
        )
        .unwrap(),
        super::super::parameters::ParameterSource::Native(_)
    ));
}

#[test]
fn a_v11_only_parameter_value_edit_selects_the_neutral_writer() {
    let (mut ir, projected) = native_parameters();
    ir.design.parameters[0].value = Some(cadmpeg_ir::design::parameter::ParameterValue::Number(
        crate::test_support::real(9.0),
    ));
    assert!(matches!(
        super::super::parameters::native_or_neutral(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            &crate::native::F3dNative::default(),
            &ir.model.parameters,
            &projected
        )
        .unwrap(),
        super::super::parameters::ParameterSource::Neutral(_)
    ));
}

fn configuration(name: &str) -> DesignConfiguration {
    DesignConfiguration {
        id: ConfigurationId::mint(format!("test:design:configuration#{name}")).unwrap(),
        scope: root(),
        position: None,
        name: name.to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    }
}

fn configured(mut ir: CadIr, key: &str, active: f64, other: f64) -> CadIr {
    let mut a = configuration("A");
    a.overrides
        .parameters
        .insert(parameter_id(key), length_override(active));
    let mut b = configuration("B");
    b.overrides
        .parameters
        .insert(parameter_id(key), length_override(other));
    ir.design.scopes[0].active_configuration = Some(a.id.clone());
    ir.design.configurations = vec![a, b];
    ir
}

fn configured_width() -> CadIr {
    let mut ir = scoped_cube();
    ir.design.parameters = vec![parameter(
        "width",
        "Width",
        neutral(number(2.0, Some(cadmpeg_ir::design::units::UnitSymbol::Mm))),
        Some(2.0),
    )];
    let mut ir = configured(ir, "width", 9.0, 7.0);
    ir.design.configurations.push(configuration("C"));
    ir
}

fn parameter_column(ir: &CadIr, name: &str) -> (String, String, u32) {
    let native = f3d_native(ir);
    let parameter = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.name() == name)
        .unwrap();
    let (document, column) = native
        .design_configurations
        .iter()
        .find_map(|document| {
            document.tables().iter().find_map(|table| {
                table
                    .columns
                    .iter()
                    .find(|column| {
                        column.target == ColumnTarget::Parameter(parameter.record_index)
                            || parameter.owner_record_index().is_some_and(|owner| {
                                column.target == ColumnTarget::OwnedParameter(owner)
                            })
                    })
                    .map(|column| (document, column))
            })
        })
        .expect("the column binds the written parameter record");
    (
        document.entry_name().clone(),
        column.id.clone(),
        parameter.record_index,
    )
}

fn assert_cell(ir: &CadIr, name: &str, row_name: &str, expression: &str, database_value: f64) {
    let (entry, column, _) = parameter_column(ir, name);
    let native = f3d_native(ir);
    let row = native
        .design_configurations
        .iter()
        .find(|document| document.entry_name() == &entry)
        .unwrap()
        .tables()
        .iter()
        .flat_map(|table| &table.rows)
        .find(|row| row.name == row_name)
        .unwrap();
    let cell = row
        .cells
        .iter()
        .find(|cell| cell.column_id == column)
        .unwrap();
    let (ConfiguredValue::Parameter {
        expression: text,
        value,
        ..
    }
    | ConfiguredValue::OwnedParameter {
        expression: text,
        value,
        ..
    }) = &cell.value
    else {
        panic!("a parameter cell")
    };
    assert_eq!(text, expression);
    assert!((value.get() - database_value).abs() < TOLERANCE);
}

fn assert_no_parameter_value_loss(
    report: &cadmpeg_ir::report::export::ExportReport,
    decoded: &CadIr,
    key: &str,
    name: &str,
) {
    let source = parameter_id(key);
    let read = &decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .expect("the written parameter")
        .id;
    assert!(
        loss(report, "export/value.unwritable")
            .iter()
            .all(|message| !message.contains(source.as_str()) && !message.contains(read.as_str())),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_source_less_timeline_writes_active_parameter_cells_and_test_rows() {
    let (report, decoded, _) = write_bytes(&configured_width());
    assert_length(&decoded, "Width", "9 mm", 9.0);
    assert_cell(&decoded, "Width", "A", "9 mm", 0.9);
    assert_cell(&decoded, "Width", "B", "7 mm", 0.7);
    assert_cell(&decoded, "Width", "C", "2 mm", 0.2);
    let (entry, column, _) = parameter_column(&decoded, "Width");
    let test = test_row(&decoded, &entry, &column);
    assert_eq!(test["Expression"], "9 mm");
    assert!((test["Value"].as_f64().unwrap() - 0.9).abs() < TOLERANCE);
    assert_eq!(
        decoded
            .model
            .configurations
            .iter()
            .filter(|row| row.active)
            .map(|row| row.name.as_deref())
            .collect::<Vec<_>>(),
        [Some("A")]
    );
    assert!(
        loss(
            &report,
            &F3dLossCode::WriterConfigurationOmitted.kind().to_string()
        )
        .is_empty(),
        "{:#?}",
        report.losses
    );
    assert_no_parameter_value_loss(&report, &decoded, "width", "Width");
}

#[test]
fn an_active_override_keeps_the_true_loss_of_an_uncarried_previous_base() {
    let mut ir = configured_width();
    ir.design.configurations.pop();
    let (report, decoded) = write(&ir);
    assert_length(&decoded, "Width", "9 mm", 9.0);
    assert!(
        loss(&report, "export/value.unwritable")
            .iter()
            .any(|message| message.contains(parameter_id("width").as_str())),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_timeline_parameter_reference_remains_an_expression_in_design_and_cells() {
    let mut ir = configured_width();
    ir.design.parameters.push(parameter(
        "copy",
        "Copy",
        neutral(reference("width")),
        Some(2.0),
    ));
    ir.design.configurations[0].overrides.parameters.insert(
        parameter_id("copy"),
        ParameterOverride {
            expression: Some(Expression::Neutral(Neutral(reference("width")))),
            value: Some(cadmpeg_ir::design::parameter::ParameterValue::Number(
                crate::test_support::real(9.0),
            )),
        },
    );
    let (report, decoded) = write(&ir);
    assert_length(&decoded, "Copy", "Width", 9.0);
    assert_cell(&decoded, "Copy", "A", "Width", 0.9);
    let width = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .unwrap();
    let copy = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Copy")
        .unwrap();
    assert!(copy.dependencies.contains(&width.id), "{copy:#?}");
    assert_no_parameter_value_loss(&report, &decoded, "width", "Width");
    assert_no_parameter_value_loss(&report, &decoded, "copy", "Copy");
}

#[test]
fn configuration_columns_bind_claimed_dimension_parameters_to_their_roster_records() {
    let ir = configured(super::dimensions::on_xy(), "gap", 20.0, 30.0);
    let (report, decoded) = write(&ir);
    let (_, _, record) = parameter_column(&decoded, "Gap");
    let native = f3d_native(&decoded);
    let parameter = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.record_index == record)
        .unwrap();
    assert!(parameter.owner_record_index().is_some());
    assert_cell(&decoded, "Gap", "A", "20 mm", 2.0);
    assert_cell(&decoded, "Gap", "B", "30 mm", 3.0);
    assert_no_parameter_value_loss(&report, &decoded, "gap", "Gap");
}

#[test]
fn a_dimension_proxy_keeps_configuration_columns_on_the_original_user_parameter() {
    let mut ir = super::dimensions::on_xy();
    ir.design.parameters.push(parameter(
        "gap-copy",
        "GapCopy",
        neutral(reference("gap")),
        Some(20.0),
    ));
    let (report, decoded) = write(&configured(ir, "gap", 20.0, 30.0));
    let (_, _, record) = parameter_column(&decoded, "Gap");
    let native = f3d_native(&decoded);
    assert!(native
        .design_parameters
        .iter()
        .find(|parameter| parameter.record_index == record)
        .unwrap()
        .owner_record_index()
        .is_none());
    assert!(native.design_parameters.iter().any(
        |parameter| parameter.expression() == "Gap" && parameter.owner_record_index().is_some()
    ));
    assert_no_parameter_value_loss(&report, &decoded, "gap", "Gap");
}

#[test]
fn a_driven_dimension_keeps_its_configuration_binding_when_the_timeline_is_generated() {
    let mut ir = super::dimensions::on_xy();
    ir.design
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id("gap"))
        .unwrap()
        .definition = ParameterDefinition::Driven;
    let (report, decoded) = write(&configured(ir, "width", 60.0, 70.0));
    let native = f3d_native(&decoded);
    let gap = native
        .design_parameters
        .iter()
        .find(|parameter| parameter.name() == "Gap")
        .unwrap();
    assert!(gap.owner_record_index().is_some());
    assert!(decoded.design.parameters.iter().any(|parameter| parameter
        .name
        .as_ref()
        .is_some_and(|name| name.as_str() == "Gap")
        && matches!(parameter.definition, ParameterDefinition::Driven)));
    assert_no_parameter_value_loss(&report, &decoded, "width", "Width");
    assert_no_parameter_value_loss(&report, &decoded, "gap", "Gap");
}

#[test]
fn configuration_feature_columns_bind_the_emitted_sketch_scope() {
    let mut ir = configured(super::dimensions::on_xy(), "gap", 20.0, 30.0);
    let sketch = ir
        .design
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.definition.operation,
                cadmpeg_ir::design::feature::FeatureOperation::Sketch(_)
            )
        })
        .unwrap()
        .id
        .clone();
    ir.design.configurations[1].overrides.features.insert(
        sketch,
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
    );
    let (_, decoded) = write(&ir);
    let native = f3d_native(&decoded);
    let sketch = native
        .design_parameter_scopes
        .iter()
        .find(|scope| scope.source_kind_name() == "Sketch")
        .unwrap();
    assert!(native
        .design_configurations
        .iter()
        .flat_map(NativeConfiguration::tables)
        .flat_map(|table| &table.columns)
        .any(|column| column.target == ColumnTarget::Feature(sketch.record_index)));
    let b = decoded
        .design
        .configurations
        .iter()
        .find(|row| row.name == "B")
        .unwrap();
    assert_eq!(
        b.overrides
            .features
            .get(&crate::ids::neutral_feature_id(sketch))
            .and_then(|state| state.suppressed),
        Some(true)
    );
}

#[test]
fn timeline_reduction_retains_configuration_payload_members() {
    let (_, mut ir) = write(&configured_width());
    ir.source = None;
    update_f3d_native(&mut ir, |native| {
        for document in &mut native.design_configurations {
            if document.kind() == DesignConfigurationKind::Table {
                let mut payload = document.payload().clone();
                payload.insert(
                    "retainedMember".to_owned(),
                    cadmpeg_core::json!({"value": "kept"}),
                );
                *document = document.with_payload(payload).unwrap();
            }
        }
    });
    let (_, decoded) = write(&ir);
    assert_length(&decoded, "Width", "9 mm", 9.0);
    assert!(f3d_native(&decoded)
        .design_configurations
        .iter()
        .any(|document| document.payload().get("retainedMember")
            == Some(&cadmpeg_core::json!({"value": "kept"}))));
}

#[test]
fn a_generated_timeline_keeps_an_unresolved_column_without_a_value_charge() {
    let (_, mut ir) = write(&configured_width());
    ir.source = None;
    let target = ColumnTarget::Feature(u32::MAX - 8);
    update_f3d_native(&mut ir, |native| {
        let document = native
            .design_configurations
            .iter_mut()
            .find(|document| !document.tables().is_empty())
            .unwrap();
        let mut payload = document.payload().clone();
        assert!(crate::records::configuration::table::push_column(
            &mut payload,
            document.tables()[0].position,
            "unresolved-scope",
            "Unresolved scope",
            &target,
        ));
        *document = document.with_payload(payload).unwrap();
    });
    let (report, decoded) = write(&ir);
    assert!(f3d_native(&decoded)
        .design_configurations
        .iter()
        .flat_map(NativeConfiguration::tables)
        .flat_map(|table| &table.columns)
        .any(|column| column.id == "unresolved-scope" && column.target == target));
    assert!(
        loss(&report, "export/value.unwritable")
            .iter()
            .all(|message| !message.contains("unresolved-scope")),
        "{:#?}",
        report.losses
    );
}
