// SPDX-License-Identifier: Apache-2.0
//! A configuration table saved as CADIR JSON, read back and written as F3D
//! with its decode sidecar: the written table states the configurations of
//! the `design` records (cadir-v11.md §5.11).

use std::io::{Cursor, Write};
use std::num::NonZeroI8;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::configuration::{DesignConfiguration, ParameterOverride};
use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};
use cadmpeg_ir::features::parameters::ParameterValue as LegacyValue;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::{CadIr, SourceFidelity};
use serde_json::Value;

use crate::design::configurations::ConfigurationTargets;
use crate::records::configuration::table::{AdmittedTables, ColumnTarget};
use crate::records::configuration::{
    DesignConfiguration as TableDocument, DesignConfigurationKind,
};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, object_target, property_target, root_record, string_cell,
    table_document, with_root_segment_in, ROOT_RECORD,
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
/// length parameter `W`, and whose rows are `a` (`20 mm`, computed) and `b`
/// (`7 mm`, suppressed), with a root record that names row `a` active,
/// decoded.
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
    if width {
        columns.push(("W", object(&length_target)));
        a.push(("W", expression_cell("20 mm", 2.0)));
        b.push(("W", expression_cell("7 mm", 0.7)));
    }
    let document = table_document("1", &columns, &[("a", "A", a), ("b", "B", b)], &["a", "b"]);
    let bytes = with_root_segment_in(
        replaced(SOURCE, &entry, &json::to_vec(&document).unwrap()),
        "FusionDesignSegmentType2",
        root_record(*b"256", ROOT_RECORD, "1", "a"),
    );
    let decoded = decode(&bytes);
    let stated = &decoded.ir().design;
    assert_eq!(
        stated.configurations.len(),
        2,
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

/// `ir` written with `sidecar` and decoded again.
fn written(ir: &CadIr, sidecar: &SourceFidelity) -> DecodeResult {
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(ir, Some(sidecar)), TargetRequest::Unnamed)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write");
    decode(&bytes)
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
}
