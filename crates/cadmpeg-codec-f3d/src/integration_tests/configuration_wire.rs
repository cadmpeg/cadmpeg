// SPDX-License-Identifier: Apache-2.0
//! A configuration table saved as CADIR JSON, read back and written as F3D
//! with its decode sidecar: the written table states the configurations of
//! the `design` records (cadir-v11.md §5.11).

use std::io::{Cursor, Write};
use std::num::NonZeroI8;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::configuration::{
    DefinitionOverride, DesignConfiguration, ParameterOverride, PartNumber,
};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::{CadIr, SourceFidelity};
use serde_json::Value;

use crate::design::configurations::ConfigurationTargets;
use crate::records::configuration::table::{AdmittedTables, ColumnTarget, ConfiguredValue};
use crate::records::configuration::{
    DesignConfiguration as TableDocument, DesignConfigurationKind,
};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, object_target, property_target, root_record, string_cell,
    table_document, with_root_segment_in, written_on, ROOT_RECORD,
};
use crate::test_support::native_test::f3d_native;
use crate::F3dCodec;

const SOURCE: &[u8] = include_bytes!("../../../../corpus/f3d/region_A.f3d");

/// What [`fixture`] configures.
struct Fixture {
    /// The entry of the table document.
    entry: String,
    /// The length parameter of column `W`.
    width: ParameterId,
    /// The feature of column `S`.
    feature: FeatureId,
}

fn decode(bytes: &[u8]) -> DecodeResult {
    F3dCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("decode")
}

/// `archive` with the member `entry` holding `payload`.
fn replaced(archive: &[u8], entry: &str, payload: &[u8]) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for index in 0..source.len() {
        let member = source.by_index_raw(index).unwrap();
        if member.name().unwrap() == entry {
            drop(member);
            zip.start_file(entry, stored).unwrap();
            zip.write_all(payload).unwrap();
        } else {
            zip.raw_copy_file(member).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}

/// The entry of the one empty table document of `ir`.
fn empty_table(ir: &CadIr) -> String {
    let documents = f3d_native(ir).design_configurations;
    let mut empty = documents.iter().filter(|document| {
        document.kind() == DesignConfigurationKind::Table && document.payload().is_empty()
    });
    let entry = empty
        .next()
        .expect("an empty table document")
        .entry_name()
        .clone();
    assert!(empty.next().is_none());
    entry
}

/// The targets of `ir`'s configuration cells.
fn targets<'a>(ir: &'a CadIr, native: &'a crate::native::F3dNative) -> ConfigurationTargets<'a> {
    ConfigurationTargets {
        parameters: &native.design_parameters,
        owners: &native.design_parameter_owners,
        scopes: &native.design_parameter_scopes,
        neutral_parameters: &ir.model.parameters,
        neutral_features: &ir.model.features,
    }
}

/// The `object` value of a column of `target`.
fn object(target: &ColumnTarget) -> Value {
    match target {
        ColumnTarget::Parameter(record) => object_target(&record.to_string(), "UserParameterValue"),
        ColumnTarget::OwnedParameter(record) => {
            object_target(&record.to_string(), "ParameterDoubleValue")
        }
        ColumnTarget::Feature(record) => object_target(&record.to_string(), "DcFeature"),
        ColumnTarget::Property(_) => unreachable!("a record column"),
    }
}

/// The source with its empty table document holding a table `1` whose
/// columns are a part number `P`, a feature `S` and, where `width` holds, a
/// length parameter `W`, and whose rows are `a` (`20 mm`, computed), `b`
/// (`7 mm`, suppressed) and `c` (`12 mm`, computed), with a root record
/// that names row `a` active and a `testRowData` entry of each column with
/// the value of row `a`, decoded.
fn fixture(width: bool) -> (DecodeResult, Fixture) {
    let plain = decode(SOURCE);
    let ir = plain.ir();
    let entry = empty_table(ir);
    let (length, length_target) = length(ir);
    let (feature, feature_target) = feature(ir);
    let mut columns = vec![
        ("P", property_target("Part Number")),
        ("S", object(&feature_target)),
    ];
    let mut a = vec![("P", string_cell("A-1")), ("S", bool_cell(true))];
    let mut b = vec![("P", string_cell("B-1")), ("S", bool_cell(false))];
    let mut c = vec![("P", string_cell("C-1")), ("S", bool_cell(true))];
    if width {
        columns.push(("W", object(&length_target)));
        a.push(("W", expression_cell("20 mm", 2.0)));
        b.push(("W", expression_cell("7 mm", 0.7)));
        c.push(("W", expression_cell("12 mm", 1.2)));
    }
    let mut document = table_document(
        "1",
        &columns,
        &[("a", "A", a), ("b", "B", b), ("c", "C", c)],
        &["a", "b", "c"],
    );
    let mut entries = vec![
        cadmpeg_core::json!({"DataType": "StringOptionValue", "ColumnUniqueID": "P", "Value": "A-1", "Option": 1}),
        cadmpeg_core::json!({"DataType": "BoolValue", "ColumnUniqueID": "S", "Value": true}),
    ];
    if width {
        let name = &ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id == length)
            .expect("the length parameter")
            .name;
        entries.push(cadmpeg_core::json!({
            "DataType": "ParameterDoubleValue",
            "ColumnUniqueID": "W",
            "UserName": name,
            "Expression": "20 mm",
            "Value": 2.0
        }));
    }
    document["testRowData"] = entries
        .iter()
        .map(|entry| Value::from(entry.to_string()))
        .collect();
    let bytes = with_root_segment_in(
        replaced(SOURCE, &entry, &json::to_vec(&document).unwrap()),
        "FusionDesignSegmentType2",
        root_record(*b"256", ROOT_RECORD, "1", "a"),
    );
    let decoded = decode(&bytes);
    let stated = &decoded.ir().design;
    assert_eq!(
        stated.configurations.len(),
        3,
        "the fixture states its rows as design configurations: rows {:?}, scopes {:?}",
        decoded
            .ir()
            .model
            .configurations
            .iter()
            .map(|row| (row.name.clone(), row.active))
            .collect::<Vec<_>>(),
        stated
            .scopes
            .iter()
            .map(|scope| (
                scope.definition.to_string(),
                scope.active_configuration.clone()
            ))
            .collect::<Vec<_>>(),
    );
    assert!(stated
        .scopes
        .iter()
        .any(|scope| scope.active_configuration.is_some()));
    let fixture = Fixture {
        entry,
        width: length,
        feature,
    };
    (decoded, fixture)
}

/// The first length parameter of `ir`'s design that a column can name, and
/// the column's target.
fn length(ir: &CadIr) -> (ParameterId, ColumnTarget) {
    let native = f3d_native(ir);
    let targets = targets(ir, &native);
    ir.model
        .parameters
        .iter()
        .filter(|parameter| matches!(parameter.value, Some(LegacyValue::Length(_))))
        .filter(|parameter| {
            ir.design
                .parameters
                .iter()
                .any(|design| design.id == parameter.id)
        })
        .find_map(|parameter| {
            Some((
                parameter.id.clone(),
                targets.parameter_column(&parameter.id)?,
            ))
        })
        .unwrap_or_else(|| {
            panic!(
            "a design length parameter with a column target: {} parameters, {} design parameters",
            ir.model.parameters.len(),
            ir.design.parameters.len()
        )
        })
}

/// The first computed feature of `ir`'s design that a column can name, and
/// the column's target.
fn feature(ir: &CadIr) -> (FeatureId, ColumnTarget) {
    let native = f3d_native(ir);
    let targets = targets(ir, &native);
    ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter(|feature| {
            ir.design
                .features
                .iter()
                .any(|design| design.id.as_str() == feature.id.as_str())
        })
        .find_map(|feature| Some((feature.id.clone(), targets.feature_column(&feature.id)?)))
        .unwrap_or_else(|| {
        panic!(
            "a computed design feature with a column target: {} features, {} design features, {} scopes",
            ir.model.features.len(),
            ir.design.features.len(),
            native.design_parameter_scopes.len()
        )
    })
}

/// The position of the table document at `entry`.
fn table(documents: &[TableDocument], entry: &str) -> usize {
    documents
        .iter()
        .position(|document| document.entry_name() == entry)
        .expect("the table document")
}

/// The one table of the table document at `entry` of `ir`.
fn stored_table(
    ir: &CadIr,
    entry: &str,
) -> crate::records::configuration::table::ConfigurationTable {
    let documents = f3d_native(ir).design_configurations;
    let mut tables = AdmittedTables::admit(documents[table(&documents, entry)].payload()).tables;
    assert_eq!(tables.len(), 1);
    tables.remove(0)
}

/// `ir` saved as CADIR JSON and read back.
fn saved(ir: &CadIr) -> CadIr {
    let read = CadIr::from_json(&ir.to_canonical_json().expect("CADIR JSON")).expect("read back");
    assert!(!read.model.omitted_arenas_held());
    read
}

/// `ir` written with `sidecar` from the retained archive and decoded again,
/// with the losses of the write.
fn written_with_losses(ir: &CadIr, sidecar: &SourceFidelity) -> (DecodeResult, Vec<LossNote>) {
    let (report, bytes) = written_on(ir, Some(sidecar), &["verbatim_replay", "patched"]);
    (decode(&bytes), report.losses)
}

/// The `testRowData` entry of the column `column` of the table document at
/// `entry` of `ir`.
fn test_row(ir: &CadIr, entry: &str, column: &str) -> Value {
    let documents = f3d_native(ir).design_configurations;
    let mut entries = documents[table(&documents, entry)].payload()["testRowData"]
        .as_array()
        .expect("testRowData")
        .iter()
        .map(|entry| serde_json::from_str::<Value>(entry.as_str().unwrap()).unwrap())
        .filter(|entry| entry["ColumnUniqueID"] == column)
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "{entries:?}");
    entries.remove(0)
}

/// The losses of `losses` that name `path`.
fn naming<'a>(losses: &'a [LossNote], path: &str) -> Vec<&'a LossNote> {
    losses
        .iter()
        .filter(|loss| loss.message.contains(path))
        .collect()
}

/// The id of the active configuration of the scope of `ir` that has one.
fn active(ir: &CadIr) -> Option<&cadmpeg_ir::features::ConfigurationId> {
    ir.design
        .scopes
        .iter()
        .find_map(|scope| scope.active_configuration.as_ref())
}

/// `ir` written with `sidecar` from the retained archive and decoded again.
fn written(ir: &CadIr, sidecar: &SourceFidelity) -> DecodeResult {
    written_with_losses(ir, sidecar).0
}

/// The id of the column of `parameter` in the table document at `entry` of
/// `ir`.
fn parameter_column(ir: &CadIr, entry: &str, parameter: &ParameterId) -> String {
    let native = f3d_native(ir);
    let target = targets(ir, &native)
        .parameter_column(parameter)
        .expect("a column target");
    stored_table(ir, entry)
        .columns
        .into_iter()
        .find(|stored| stored.target == target)
        .expect("the column of the parameter")
        .id
}

/// The expression and the database-unit value of the cell of row `row` in
/// the column of `parameter` of the table document at `entry` of `ir`.
fn parameter_cell(ir: &CadIr, entry: &str, row: &str, parameter: &ParameterId) -> (String, f64) {
    let column = parameter_column(ir, entry, parameter);
    let table = stored_table(ir, entry);
    let row = table
        .rows
        .iter()
        .find(|stored| stored.id == row)
        .unwrap_or_else(|| panic!("row {row}"));
    let cell = row
        .cells
        .iter()
        .find(|cell| cell.column_id == column)
        .unwrap_or_else(|| panic!("row {} has no cell of column {column}", row.id));
    match &cell.value {
        ConfiguredValue::Parameter {
            expression, value, ..
        }
        | ConfiguredValue::OwnedParameter {
            expression, value, ..
        } => (expression.clone(), value.get()),
        other => panic!("a parameter cell: {other:?}"),
    }
}

/// The expression and the value in millimetres of `parameter` of `ir`.
fn own(ir: &CadIr, parameter: &ParameterId) -> (String, f64) {
    let own = ir
        .model
        .parameters
        .iter()
        .find(|own| own.id == *parameter)
        .expect("the parameter");
    let Some(LegacyValue::Length(length)) = &own.value else {
        panic!("a length: {:?}", own.value);
    };
    (own.expression.clone(), length.get())
}

/// The part number that row `name` of `ir` states.
fn part_number<'a>(ir: &'a CadIr, name: &str) -> Option<&'a str> {
    row(ir, name)
        .properties
        .get("property:Part Number")
        .map(String::as_str)
}

/// The design configuration of `ir` named `name`.
fn named<'a>(ir: &'a CadIr, name: &str) -> &'a DesignConfiguration {
    ir.design
        .configurations
        .iter()
        .find(|configuration| configuration.name == name)
        .unwrap_or_else(|| panic!("configuration {name}"))
}

/// The design configuration of `ir` named `name`, to edit.
fn named_mut<'a>(ir: &'a mut CadIr, name: &str) -> &'a mut DesignConfiguration {
    ir.design
        .configurations
        .iter_mut()
        .find(|configuration| configuration.name == name)
        .unwrap_or_else(|| panic!("configuration {name}"))
}

/// The v10 row of `ir` named `name`.
fn row<'a>(ir: &'a CadIr, name: &str) -> &'a cadmpeg_ir::features::parameters::DesignConfiguration {
    ir.model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("row {name}"))
}

/// A parameter override of `millimetres`.
fn length_override(millimetres: f64) -> ParameterOverride {
    let unit = Unit(vec![UnitFactor {
        symbol: UnitSymbol::Mm,
        power: NonZeroI8::new(1).unwrap(),
    }]);
    ParameterOverride {
        expression: Some(Expression::Neutral(Neutral(Node::Number {
            value: NonNegativeReal::new(millimetres).unwrap(),
            unit,
        }))),
        value: Some(ParameterValue::Number(
            FiniteReal::new(millimetres).unwrap(),
        )),
    }
}

#[test]
fn a_configuration_table_survives_a_cadir_write_and_read_whole() {
    let (decoded, fixture) = fixture(true);
    let read = saved(decoded.ir());
    assert_eq!(
        read.design.configurations,
        decoded.ir().design.configurations
    );
    assert_eq!(read.design.scopes, decoded.ir().design.scopes);
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(
        round_trip.ir().design.configurations,
        decoded.ir().design.configurations
    );
    assert_eq!(round_trip.ir().design.scopes, decoded.ir().design.scopes);
    assert_eq!(
        stored_table(round_trip.ir(), &fixture.entry),
        stored_table(decoded.ir(), &fixture.entry)
    );
}

#[test]
fn a_cell_edited_through_cadir_reaches_the_written_table() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let b = named_mut(&mut read, "B");
    let edited = length_override(9.0);
    b.overrides
        .parameters
        .insert(fixture.width.clone(), edited.clone());
    b.overrides
        .features
        .get_mut(&fixture.feature)
        .expect("row b suppresses the feature")
        .suppressed = Some(false);
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(
        row(round_trip.ir(), "B")
            .parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("9 mm")
    );
    let b = named(round_trip.ir(), "B");
    assert_eq!(
        b.overrides.parameters[&fixture.width].expression,
        edited.expression
    );
    assert!(
        b.overrides
            .features
            .get(&fixture.feature)
            .is_none_or(|state| state.suppressed == Some(false)),
        "{:?}",
        b.overrides.features
    );
}

#[test]
fn a_parameter_no_column_names_gets_a_column() {
    let (decoded, fixture) = fixture(false);
    let before = stored_table(decoded.ir(), &fixture.entry).columns.len();
    let mut read = saved(decoded.ir());
    let added = length_override(3.0);
    named_mut(&mut read, "B")
        .overrides
        .parameters
        .insert(fixture.width.clone(), added.clone());
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(
        stored_table(round_trip.ir(), &fixture.entry).columns.len(),
        before + 1
    );
    assert_eq!(
        row(round_trip.ir(), "B")
            .parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("3 mm")
    );
    assert_eq!(
        named(round_trip.ir(), "B").overrides.parameters[&fixture.width].expression,
        added.expression
    );
    let (expression, millimetres) = own(round_trip.ir(), &fixture.width);
    for unchanged in ["a", "c"] {
        let (cell, value) =
            parameter_cell(round_trip.ir(), &fixture.entry, unchanged, &fixture.width);
        assert_eq!(cell, expression, "row {unchanged}");
        assert!(
            (value - millimetres / 10.0).abs() < 1e-9,
            "row {unchanged}: {value}"
        );
    }
    for name in ["A", "C"] {
        let stated = named(round_trip.ir(), name)
            .overrides
            .parameters
            .get(&fixture.width);
        assert!(
            stated.is_none_or(|stated| stated.value
                == Some(ParameterValue::Number(
                    FiniteReal::new(millimetres).unwrap()
                ))),
            "{name}: {stated:?}"
        );
    }
    let documents = f3d_native(round_trip.ir()).design_configurations;
    let column = parameter_column(round_trip.ir(), &fixture.entry, &fixture.width);
    let entries = documents[table(&documents, &fixture.entry)].payload()["testRowData"]
        .as_array()
        .expect("testRowData")
        .iter()
        .map(|entry| serde_json::from_str::<Value>(entry.as_str().unwrap()).unwrap())
        .filter(|entry| entry["ColumnUniqueID"] == column.as_str())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["DataType"], "ParameterDoubleValue");
    assert_eq!(entries[0]["Expression"], expression.as_str());
}

#[test]
fn an_active_row_moved_through_cadir_is_named_by_the_root_record() {
    let (decoded, _) = fixture(true);
    let mut read = saved(decoded.ir());
    let b = named(&read, "B").id.clone();
    let scope = read
        .design
        .scopes
        .iter_mut()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap();
    scope.active_configuration = Some(b.clone());
    let (round_trip, losses) = written_with_losses(&read, decoded.source_fidelity());
    assert!(round_trip
        .ir()
        .design
        .scopes
        .iter()
        .any(|scope| scope.active_configuration.as_ref() == Some(&b)));
    assert!(
        losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains("DesignConfigurationRoot")),
        "{losses:#?}"
    );
}

#[test]
fn a_configuration_added_through_cadir_is_a_row_of_the_written_table() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let mut added = named(&read, "B").clone();
    added.id = cadmpeg_ir::features::ConfigurationId::mint("test:configuration:variant#d").unwrap();
    added.name = "D".into();
    added.native_ref = None;
    added
        .overrides
        .parameters
        .insert(fixture.width.clone(), length_override(4.0));
    read.design.configurations.push(added);
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(
        row(round_trip.ir(), "D")
            .parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("4 mm")
    );
    assert_eq!(stored_table(round_trip.ir(), &fixture.entry).rows.len(), 4);
}

#[test]
fn an_override_of_one_member_writes_the_cell_it_resolves_to() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let mut expression = length_override(9.0);
    expression.value = None;
    named_mut(&mut read, "B")
        .overrides
        .parameters
        .insert(fixture.width.clone(), expression);
    let mut value = length_override(5.0);
    value.expression = None;
    named_mut(&mut read, "C")
        .overrides
        .parameters
        .insert(fixture.width.clone(), value);
    let c = named(&read, "C").id.clone();
    let (round_trip, losses) = written_with_losses(&read, decoded.source_fidelity());
    let (text, evaluated) = parameter_cell(round_trip.ir(), &fixture.entry, "b", &fixture.width);
    assert_eq!(text, "9 mm");
    assert!((evaluated - 0.9).abs() < 1e-9, "{evaluated}");
    let (expression, millimetres) = own(round_trip.ir(), &fixture.width);
    let (text, cached) = parameter_cell(round_trip.ir(), &fixture.entry, "c", &fixture.width);
    assert_eq!(text, expression);
    assert!((cached - millimetres / 10.0).abs() < 1e-9, "{cached}");
    let stale = naming(
        &losses,
        &format!(
            "design.configurations[{c}].overrides.parameters[{}]",
            fixture.width
        ),
    );
    assert_eq!(stale.len(), 1, "{losses:#?}");
    assert_eq!(stale[0].code.to_string(), "export/value.unwritable");
    assert!(
        stale[0].message.contains("value of their expression"),
        "{stale:#?}"
    );
}

#[test]
fn a_value_of_another_kind_is_charged_once() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let c = named_mut(&mut read, "C");
    c.overrides.parameters.insert(
        fixture.width.clone(),
        ParameterOverride {
            expression: None,
            value: Some(ParameterValue::Boolean(true)),
        },
    );
    let c = c.id.clone();
    let (_, losses) = written_with_losses(&read, decoded.source_fidelity());
    let charged = naming(
        &losses,
        &format!(
            "design.configurations[{c}].overrides.parameters[{}]",
            fixture.width
        ),
    );
    assert_eq!(charged.len(), 1, "{losses:#?}");
}

/// An expression-only override that names another parameter caches the
/// value that parameter takes in the same configuration, its override there
/// among them.
#[test]
fn an_expression_that_names_a_parameter_caches_its_override_in_the_same_row() {
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    let mut depth = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == fixture.width)
        .expect("the design parameter")
        .clone();
    depth.id = ParameterId::mint("synthetic:test:parameter#depth").unwrap();
    depth.name = Some(cadmpeg_core::text::NonBlankString::new("depth").unwrap());
    let depth_id = depth.id.clone();
    ir.design.parameters.push(depth);
    let mut referenced = length_override(4.5);
    referenced.value = None;
    let b = named_mut(&mut ir, "B");
    b.overrides.parameters.insert(depth_id.clone(), referenced);
    b.overrides.parameters.insert(
        fixture.width.clone(),
        ParameterOverride {
            expression: Some(Expression::Neutral(Neutral(Node::Reference(
                ParameterRef {
                    target: RefTarget::Parameter(depth_id),
                    through: None,
                },
            )))),
            value: None,
        },
    );
    let mut losses = Vec::new();
    let rows = crate::design_v11::configurations::restate(
        &ir.model.configurations,
        &ir.model.configurations,
        &ir.design,
        &ir.model.product_definitions,
        &ir.model.features,
        &ir.model.parameters,
        &ir.tolerances,
    )
    .into_rows(&mut losses);
    let b = rows
        .iter()
        .find(|row| row.name.as_deref() == Some("B"))
        .expect("row B");
    assert_eq!(
        b.parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("depth"),
        "{losses:#?}"
    );
    let Some(LegacyValue::Length(cached)) = b.parameter_values.get(&fixture.width) else {
        panic!("a length: {:?}", b.parameter_values.get(&fixture.width));
    };
    assert!((cached.get() - 4.5).abs() < 1e-9, "{cached:?}: {losses:#?}");
}

/// A held decode whose design scope names row `b` active, with no other
/// edit: the root record and the `testRowData` entries name row `b`. The
/// patch writes no Design stream parameter, so the parameter cells of row
/// `b` hold the Design stream's expression and value (f3d.md §1.2), and the
/// override of `B` is charged. Row `a`, no longer active, states what `A`
/// resolves: the base.
#[test]
fn an_active_row_moved_in_the_design_of_a_held_decode_writes_its_test_row() {
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    assert!(ir.model.omitted_arenas_held());
    let b = named(&ir, "B").id.clone();
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap()
        .active_configuration = Some(b.clone());
    let (expression, millimetres) = own(&ir, &fixture.width);
    let (round_trip, losses) = written_with_losses(&ir, decoded.source_fidelity());
    assert_eq!(active(round_trip.ir()), Some(&b), "{losses:#?}");
    let width = test_row(round_trip.ir(), &fixture.entry, "W");
    assert_eq!(width["Expression"], expression.as_str(), "{width}");
    assert!(
        (width["Value"].as_f64().unwrap() - millimetres / 10.0).abs() < 1e-9,
        "{width}"
    );
    assert_eq!(pinned(&losses, &b, &fixture.width).len(), 1, "{losses:#?}");
    let (text, value) = parameter_cell(round_trip.ir(), &fixture.entry, "a", &fixture.width);
    assert_eq!(text, expression, "row a states what A resolves, the base");
    assert!((value - millimetres / 10.0).abs() < 1e-9, "{value}");
    assert_eq!(
        test_row(round_trip.ir(), &fixture.entry, "S")["Value"],
        false
    );
    assert_eq!(
        test_row(round_trip.ir(), &fixture.entry, "P")["Value"],
        "B-1"
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("testRowData")),
        "{losses:#?}"
    );
}

/// The editor's switch to row `b` marks the current state stale, so the
/// write regenerates the design, and the generator charges the
/// configuration it does not write.
#[test]
fn an_active_row_moved_through_the_editor_is_charged_where_it_is_not_written() {
    let (decoded, _) = fixture(true);
    let mut ir = decoded.ir().clone();
    let b = named(&ir, "B").id.clone();
    let scope = ir
        .design
        .scopes
        .iter()
        .find(|scope| scope.active_configuration.is_some())
        .unwrap()
        .definition
        .clone();
    ir.edit()
        .unwrap()
        .set_active_configuration(&scope, &b)
        .unwrap();
    let (report, _) = written_on(&ir, Some(decoded.source_fidelity()), &["synthesized"]);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains("active configuration")
                && loss.message.contains(b.as_str())),
        "{:#?}",
        report.losses
    );
}

/// A cell edited in the native table alone, of a row a design configuration
/// states: the design wins (cadir-v11.md §3.4, W2), in the write of the
/// decode and of the CADIR read with the sidecar, and in the rows the
/// generator restates without it.
#[test]
fn a_native_cell_edit_loses_to_the_design_configuration() {
    use crate::records::configuration::table::{set_cell, CellData};
    let (decoded, fixture) = fixture(true);
    let edit = |ir: &mut CadIr| {
        crate::test_support::native_test::update_f3d_native(ir, |native| {
            let document = native
                .design_configurations
                .iter_mut()
                .find(|document| *document.entry_name() == fixture.entry)
                .unwrap();
            let mut payload = document.payload().clone();
            set_cell(
                &mut payload,
                0,
                "b",
                "W",
                Some(&CellData::Expression {
                    expression: "3 mm".into(),
                    value: FiniteReal::new(0.3).unwrap(),
                }),
            );
            *document = document.with_payload(payload).unwrap();
        });
    };
    let mut held = decoded.ir().clone();
    edit(&mut held);
    let mut read = saved(decoded.ir());
    edit(&mut read);
    for (path, target) in [("decode", &held), ("CADIR read", &read)] {
        let (round_trip, losses) = written_with_losses(target, decoded.source_fidelity());
        assert_eq!(
            parameter_cell(round_trip.ir(), &fixture.entry, "b", &fixture.width).0,
            "7 mm",
            "{path}: {losses:#?}"
        );
    }
    let rows = crate::test_support::configuration_table::project(&read);
    let mut losses = Vec::new();
    let restated = crate::design_v11::configurations::restate(
        &rows,
        &rows,
        &read.design,
        &read.model.product_definitions,
        &read.model.features,
        &read.model.parameters,
        &read.tolerances,
    )
    .into_rows(&mut losses);
    let b = restated
        .iter()
        .find(|row| row.name.as_deref() == Some("B"))
        .expect("row B");
    assert_eq!(
        b.parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("7 mm"),
        "{losses:#?}"
    );
}

#[test]
fn a_cell_edited_in_the_design_of_a_held_decode_reaches_the_written_table() {
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    named_mut(&mut ir, "B")
        .overrides
        .parameters
        .insert(fixture.width.clone(), length_override(9.0));
    let round_trip = written(&ir, decoded.source_fidelity());
    let (text, cached) = parameter_cell(round_trip.ir(), &fixture.entry, "b", &fixture.width);
    assert_eq!(text, "9 mm");
    assert!((cached - 0.9).abs() < 1e-9, "{cached}");
}

#[test]
fn an_edit_the_table_has_no_cell_for_is_charged_and_the_rest_is_written() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let b = named_mut(&mut read, "B");
    b.comment = Some("hand-made".into());
    b.overrides
        .parameters
        .insert(fixture.width.clone(), length_override(9.0));
    let id = b.id.to_string();
    let (round_trip, losses) = written_with_losses(&read, decoded.source_fidelity());
    assert!(
        losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/edit.dropped"
                && loss.message.contains(&format!("{id}].comment"))),
        "{losses:#?}"
    );
    assert_eq!(
        row(round_trip.ir(), "B")
            .parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("9 mm")
    );
}

#[test]
fn a_configuration_s_row_holds_what_it_inherits_and_its_parent_s_name() {
    let (decoded, fixture) = fixture(true);
    let mut read = saved(decoded.ir());
    let b = named(&read, "B").id.clone();
    let c = named_mut(&mut read, "C");
    c.parent = Some(b);
    c.overrides.parameters.remove(&fixture.width);
    let scope = c.scope.clone();
    c.overrides.definitions.insert(
        scope,
        DefinitionOverride {
            part_number: Some(PartNumber::ParentConfigurationName),
            ..DefinitionOverride::default()
        },
    );
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(
        row(round_trip.ir(), "C")
            .parameter_overrides
            .get(&fixture.width)
            .map(String::as_str),
        Some("7 mm")
    );
    assert_eq!(part_number(round_trip.ir(), "C"), Some("B"));
}

#[test]
fn a_document_name_part_number_is_written_as_its_text() {
    let (decoded, _) = fixture(false);
    let mut read = saved(decoded.ir());
    let b = named_mut(&mut read, "B");
    let scope = b.scope.clone();
    let mut definition = DefinitionOverride {
        part_number: Some(PartNumber::DocumentName),
        ..DefinitionOverride::default()
    };
    definition.bom_properties.insert(
        cadmpeg_core::text::NonBlankString::new("Title").unwrap(),
        "Bracket".to_owned(),
    );
    b.overrides.definitions.insert(scope, definition);
    let round_trip = written(&read, decoded.source_fidelity());
    assert_eq!(part_number(round_trip.ir(), "B"), Some("Bracket"));
}

/// The resolved value of `parameter` in the configuration `name` of `ir`
/// (cadir-v11.md §5.11), in millimetres: its override's value, else the
/// parameter's own.
fn resolved_millimetres(ir: &CadIr, name: &str, parameter: &ParameterId) -> f64 {
    match named(ir, name)
        .overrides
        .parameters
        .get(parameter)
        .and_then(|stated| stated.value.as_ref())
    {
        Some(ParameterValue::Number(value)) => value.get(),
        Some(other) => panic!("a length: {other:?}"),
        None => own(ir, parameter).1,
    }
}

/// The losses of `losses` that charge the parameter `parameter` of the
/// active configuration `configuration`, written at the Design stream's
/// expression and value.
fn pinned<'a>(
    losses: &'a [LossNote],
    configuration: &cadmpeg_ir::features::ConfigurationId,
    parameter: &ParameterId,
) -> Vec<&'a LossNote> {
    losses
        .iter()
        .filter(|loss| {
            loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains("active configuration")
                && loss.message.contains(configuration.as_str())
                && loss.message.contains(parameter.as_str())
        })
        .collect()
}

/// An override that the active configuration states, written by the patch,
/// which writes no Design stream parameter: the Design stream stores the
/// state of the active row (f3d.md §1.2), so the active row's cell holds the
/// Design stream's expression and value, and the override is charged once.
/// Each configuration reads back at the value the written archive resolves.
#[test]
fn an_override_of_the_active_configuration_the_patch_cannot_write_is_charged() {
    let (decoded, fixture) = fixture(true);
    let (expression, millimetres) = own(decoded.ir(), &fixture.width);
    let a = named(decoded.ir(), "A").id.clone();
    for (path, mut ir) in [
        ("decode", decoded.ir().clone()),
        ("CADIR read", saved(decoded.ir())),
    ] {
        named_mut(&mut ir, "A")
            .overrides
            .parameters
            .insert(fixture.width.clone(), length_override(9.0));
        let (round_trip, losses) = written_with_losses(&ir, decoded.source_fidelity());
        let back = round_trip.ir();
        assert_eq!(own(back, &fixture.width), (expression.clone(), millimetres));
        let (text, value) = parameter_cell(back, &fixture.entry, "a", &fixture.width);
        assert_eq!(text, expression, "{path}");
        assert!((value - millimetres / 10.0).abs() < 1e-9, "{path}: {value}");
        for (name, stated) in [("A", millimetres), ("B", 7.0), ("C", 12.0)] {
            let read = resolved_millimetres(back, name, &fixture.width);
            assert!((read - stated).abs() < 1e-9, "{path}: {name} reads {read}");
        }
        assert_eq!(
            pinned(&losses, &a, &fixture.width).len(),
            1,
            "{path}: {losses:#?}"
        );
    }
}

/// The roles of the other configurations when the Design stream takes the
/// active configuration's value: one that states nothing states the
/// previous value; one that inherits from the active configuration, or from
/// a configuration that states an expression, states nothing; one whose
/// override states only a value keeps the previous expression.
#[test]
fn every_other_configuration_keeps_its_value_when_the_base_takes_the_active_one() {
    use cadmpeg_ir::design::parameter::ParameterDefinition;
    use cadmpeg_ir::features::ConfigurationId;
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    let (_, millimetres) = own(&ir, &fixture.width);
    let previous = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == fixture.width)
        .map(|parameter| parameter.definition.clone())
        .expect("the parameter");
    let ParameterDefinition::Expression(previous) = previous else {
        panic!("an expression: {previous:?}");
    };
    let a = named(&ir, "A").id.clone();
    let b = named(&ir, "B").id.clone();
    named_mut(&mut ir, "A")
        .overrides
        .parameters
        .insert(fixture.width.clone(), length_override(9.0));
    named_mut(&mut ir, "C")
        .overrides
        .parameters
        .remove(&fixture.width);
    let mut cached = length_override(3.0);
    cached.expression = None;
    for (name, parent, stated) in [
        ("D", Some(&a), None),
        ("E", Some(&b), None),
        ("F", None, Some(cached)),
    ] {
        let mut added = named(&ir, "C").clone();
        added.id = ConfigurationId::mint(format!("test:configuration:role#{name}")).unwrap();
        added.name = name.into();
        added.native_ref = None;
        added.parent = parent.cloned();
        added
            .overrides
            .parameters
            .extend(stated.map(|stated| (fixture.width.clone(), stated)));
        ir.design.configurations.push(added);
    }
    let rebased = crate::design_v11::configurations::rebase::rebased(&ir).expect("rebased");
    let out = &rebased.ir;
    assert!(rebased.losses.is_empty(), "{:#?}", rebased.losses);
    let (expression, own_millimetres) = own(out, &fixture.width);
    assert_eq!(expression, "9 mm");
    assert!((own_millimetres - 9.0).abs() < 1e-9, "{own_millimetres}");
    let stated = |name: &str| named(out, name).overrides.parameters.get(&fixture.width);
    assert_eq!(stated("A"), None);
    assert_eq!(
        stated("B"),
        named(&ir, "B").overrides.parameters.get(&fixture.width)
    );
    let c = stated("C").expect("C states the previous value");
    assert_eq!(c.expression.as_ref(), Some(&previous));
    assert_eq!(
        c.value,
        Some(ParameterValue::Number(
            FiniteReal::new(millimetres).unwrap()
        ))
    );
    assert_eq!(stated("D"), None);
    assert_eq!(stated("E"), None);
    let f = stated("F").expect("F keeps its override");
    assert_eq!(f.expression.as_ref(), Some(&previous));
    assert_eq!(
        f.value,
        Some(ParameterValue::Number(FiniteReal::new(3.0).unwrap()))
    );
}

/// A cell that the v10 rows and the design configurations of a held decode
/// both edit: the design configuration wins (cadir-v11.md §3.4, W2), and the
/// v10 edit is charged once.
#[test]
fn a_v10_cell_edit_the_design_configuration_states_otherwise_is_charged() {
    use cadmpeg_ir::scalar::Length;
    let (decoded, fixture) = fixture(true);
    let mut ir = decoded.ir().clone();
    let b = named(&ir, "B").id.clone();
    let row = ir
        .model
        .configurations
        .iter_mut()
        .find(|row| row.id == b)
        .expect("row b");
    row.parameter_overrides
        .insert(fixture.width.clone(), "3 mm".into());
    row.parameter_values.insert(
        fixture.width.clone(),
        LegacyValue::Length(Length::new(3.0).unwrap()),
    );
    named_mut(&mut ir, "B")
        .overrides
        .parameters
        .insert(fixture.width.clone(), length_override(9.0));
    let (round_trip, losses) = written_with_losses(&ir, decoded.source_fidelity());
    assert_eq!(
        parameter_cell(round_trip.ir(), &fixture.entry, "b", &fixture.width).0,
        "9 mm"
    );
    let charged = losses
        .iter()
        .filter(|loss| {
            loss.code.to_string() == "export/edit.dropped"
                && loss.message.contains("v10 configuration rows")
                && loss.message.contains(b.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(charged.len(), 1, "{losses:#?}");
}
