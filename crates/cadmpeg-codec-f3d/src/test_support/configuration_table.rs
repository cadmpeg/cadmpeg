// SPDX-License-Identifier: Apache-2.0
//! Synthetic configuration-table documents and Design configuration root records.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::features::parameters::DesignParameter as NeutralParameter;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::{CadIr, SourceFidelity};
use serde_json::{Map, Value};

use crate::design::configurations::ConfigurationTargets;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::test_support::{indexed_header, lp_ascii, lp_utf16};
use crate::F3dCodec;

/// `ir` with no source and no unknown native records.
pub(crate) fn unsourced(ir: &CadIr) -> CadIr {
    let mut ir = ir.clone();
    ir.source = None;
    ir.set_native_unknowns(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "f3d",
        &[],
    )
    .unwrap();
    ir
}

/// `ir` written with `fidelity`: the report, which names one of the write
/// paths `paths`, and the bytes.
pub(crate) fn written_on(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    paths: &[&str],
) -> (ExportReport, Vec<u8>) {
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, fidelity))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write");
    let path = report.write_path().to_string();
    assert!(
        paths.contains(&path.as_str()),
        "{path}: {:#?}",
        report.losses
    );
    (report, bytes)
}

/// The Design stream of the parameter records of [`Natives`].
pub(crate) const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn typed(kind: &str, value: Value) -> Value {
    json!({"type": kind, "value": value})
}

fn component_value(component: &str, members: Value) -> Value {
    typed(
        &format!("autodesk.configuration:components.{component}"),
        members,
    )
}

fn asset(kind: &str, attributes: Value, components: Value) -> Value {
    json!({
        "id": "", "revisionId": "",
        "type": format!("autodesk.configuration:assets.{kind}"),
        "attributes": attributes, "components": components
    })
}

fn component(name: &str, kind: &str, properties: Value) -> Value {
    json!({
        "name": name,
        "type": format!("autodesk.configuration:components.{kind}"),
        "properties": properties
    })
}

/// The `object` value of a document-property column.
pub(crate) fn property_target(key: &str) -> Value {
    component_value(
        "configuredProperty-1.0.0",
        json!({
            "id": typed("String", json!("00000000-0000-0000-0000-000000000000")),
            "propertyKey": typed("String", json!(key)),
            "propertySet": typed("String", json!("")),
            "type": typed("String", json!("Property"))
        }),
    )
}

/// The `object` value of a column that names Design record `record` of `kind`.
pub(crate) fn object_target(record: &str, kind: &str) -> Value {
    component_value(
        "configuredObject-1.0.0",
        json!({"id": typed("String", json!(record)), "type": typed("String", json!(kind))}),
    )
}

/// The `data` value of a parameter cell.
pub(crate) fn expression_cell(expression: &str, value: f64) -> Value {
    component_value(
        "variant.expression-1.0.0",
        json!({
            "expression": typed("String", json!(expression)),
            "value": component_value("variant.float64-1.0.0", json!({"value": typed("Float64", json!(value))}))
        }),
    )
}

/// The `data` value of a feature cell.
pub(crate) fn bool_cell(value: bool) -> Value {
    component_value(
        "variant.bool-1.0.0",
        json!({"value": typed("Bool", json!(value))}),
    )
}

/// The `data` value of a property cell.
pub(crate) fn string_cell(text: &str) -> Value {
    component_value(
        "variant.string-1.0.0",
        json!({"value": typed("String", json!(text))}),
    )
}

/// One table row: id, name, and `(column id, data)` cells.
pub(crate) type Row<'a> = (&'a str, &'a str, Vec<(&'a str, Value)>);

/// A table document with one table `table_id`, the `(column id, object)`
/// columns, `rows` in array order, and the authored `row_order`.
pub(crate) fn table_document(
    table_id: &str,
    columns: &[(&str, Value)],
    rows: &[Row<'_>],
    row_order: &[&str],
) -> Value {
    let column_data = columns
        .iter()
        .map(|(id, object)| {
            (
                (*id).to_owned(),
                component_value(
                    "columnData-1.1.0",
                    json!({
                        "id": typed("String", json!(id)),
                        "isCustom": typed("Bool", json!(false)),
                        "object": object,
                        "title": typed("String", json!(id))
                    }),
                ),
            )
        })
        .collect::<Map<_, _>>();
    let rows = rows
        .iter()
        .map(|(id, name, cells)| {
            let cells = cells
                .iter()
                .map(|(column, data)| {
                    (
                        (*column).to_owned(),
                        component_value(
                            "rowCell-1.0.0",
                            json!({"columnId": typed("String", json!(column)), "data": data}),
                        ),
                    )
                })
                .collect::<Map<_, _>>();
            json!({"asset": asset(
                "row-1.0.0",
                json!({"pim:configuration:rowId": id, "pim:configuration:rowName": name}),
                json!([
                    component("rowData", "rowData-1.0.0", json!({
                        "configRevisionId": typed("String", json!(format!("revision-{id}"))),
                        "default": typed("Bool", json!(false)),
                        "rowCells": typed("map<autodesk.configuration:components.rowCell-1.0.0>", Value::Object(cells))
                    })),
                    component("rowDefinition", "rowDefinition-1.0.0", json!({"id": typed("String", json!(id))}))
                ]),
            )})
        })
        .collect::<Vec<_>>();
    let column_order = columns.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    json!({
        "asset": asset("configuration-1.0.0", json!({}), json!([
            component("tableVersion", "tableData-1.0.0", json!({"tableVersion": typed("Int32", json!(1))}))
        ])),
        "tables": [{
            "asset": asset(
                "configurationTable-1.1.0",
                json!({"pim:configuration:tableId": table_id}),
                json!([component("tableDefinition", "tableDefinition-1.1.0", json!({
                    "columnData": typed("map<autodesk.configuration:components.columnData-1.1.0>", Value::Object(column_data)),
                    "id": typed("String", json!(table_id)),
                    "tableName": typed("String", json!("Configurations")),
                    "tableType": typed("String", json!("Configurations"))
                }))]),
            ),
            "rows": rows,
            "tableView": {"asset": asset("tableView-1.0.0", json!({}), json!([
                component("viewData", "tableViewData-1.0.0", json!({
                    "columnOrder": typed("String", json!(column_order)),
                    "rowOrder": typed("String", json!(row_order))
                }))
            ]))}
        }],
        "cachedRevIds": {},
        "testRowData": []
    })
}

/// A Design `DesignConfigurationRoot` record that names `row_id` of `table_id`.
pub(crate) fn root_record(
    class_tag: [u8; 3],
    record_index: u32,
    table_id: &str,
    row_id: &str,
) -> Vec<u8> {
    let mut record = Vec::new();
    indexed_header(&mut record, class_tag, record_index);
    record.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut record, "DesignConfigurationRoot");
    record.extend_from_slice(&[0; 11]);
    record.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut record, table_id);
    lp_utf16(&mut record, row_id);
    record.extend_from_slice(&0u32.to_le_bytes());
    record
}

/// Indexed-record index of the root record that [`with_root_segment`] stores.
pub(crate) const ROOT_RECORD: u32 = 349;

/// `archive` with a `FusionDesignSegmentType1` segment whose one primary record
/// is `root`, which must carry class tag `256` and index [`ROOT_RECORD`].
pub(crate) fn with_root_segment(archive: Vec<u8>, root: Vec<u8>) -> Vec<u8> {
    with_root_segment_in(archive, "FusionDesignSegmentType1", root)
}

/// [`with_root_segment`] with the segment folder `segment`.
pub(crate) fn with_root_segment_in(archive: Vec<u8>, segment: &str, root: Vec<u8>) -> Vec<u8> {
    use std::io::Write;

    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "",
            1,
            "Design",
            &[ROOT_RECORD.into()],
        )],
        &[(ROOT_RECORD.into(), 0)],
    );
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for index in 0..source.len() {
        zip.raw_copy_file(source.by_index_raw(index).unwrap())
            .unwrap();
    }
    for (name, payload) in [("BulkStream.dat", root), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/{segment}/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The neutral configurations that the native configuration documents of `ir`
/// project against its native and neutral records.
pub(crate) fn project(
    ir: &cadmpeg_ir::CadIr,
) -> Vec<cadmpeg_ir::features::parameters::DesignConfiguration> {
    let native = crate::test_support::native_test::f3d_native(ir);
    crate::design::configurations::project(
        &native.design_configurations,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &native.design_parameters,
            owners: &native.design_parameter_owners,
            scopes: &native.design_parameter_scopes,
            neutral_parameters: &ir.model.parameters,
            neutral_features: &ir.model.features,
        },
    )
    .expect("configuration projection")
    .configurations
}

/// Attach the active row `row_id` of table `1` to the first native
/// configuration document and project again.
pub(crate) fn set_active_row(ir: &mut cadmpeg_ir::CadIr, row_id: &str) {
    crate::test_support::native_test::update_f3d_native(ir, |native| {
        native.design_configurations[0]
            .set_active_row(
                crate::records::configuration::DesignConfigurationActiveRow {
                    record_index: ROOT_RECORD,
                    table_id: "1".into(),
                    row_id: row_id.into(),
                },
            )
            .unwrap();
    });
    ir.model.configurations = project(ir);
}

/// Document parameter record `index` in the test parameter stream.
pub(crate) fn parameter(index: u32, name: &str) -> DesignParameter {
    let mut frame = crate::design::test_support::parameter_record(
        None,
        "10 mm",
        "User Parameter",
        Some("mm"),
        name,
        1.0,
    );
    frame[7..11].copy_from_slice(&index.to_le_bytes());
    crate::design::decode::parameters::parse_design_parameter_record(&frame).unwrap()
}

pub(crate) fn owner(stream: &str, index: u32, parameter: u32) -> DesignParameterOwner {
    json::from_value(json!({
        "id": format!("{stream}:design-parameter-owner#{index}"),
        "byte_offset": 2000,
        "frame_length": 104,
        "class_tag": "351",
        "record_index": index,
        "scope_record_index": 314,
        "local_ordinal": 0,
        "evaluated_value": 1.0,
        "evaluated_value_offset": 2040,
        "parameter_record_index": parameter,
        "owned_ordinal": 1,
        "variant": 0,
        "companion_record_index": index + 1,
    }))
    .unwrap()
}

pub(crate) fn scope(index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{index}"),
        DesignFeatureKind::Extrude,
        index,
    )
}

pub(crate) fn neutral_parameter(native: &DesignParameter) -> NeutralParameter {
    NeutralParameter {
        product_definition: None,
        id: crate::ids::neutral_parameter_id(native),
        owner: None,
        ordinal: 0,
        name: native.name().to_owned(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

pub(crate) fn neutral_feature(scope: &DesignParameterScope, dependency: &str) -> Feature {
    Feature {
        product_definition: None,
        id: crate::ids::neutral_feature_id(scope),
        ordinal: 1,
        name: Some("Extrusion 2".into()),
        suppressed: Some(false),
        dependencies: std::iter::once(FeatureId::mint(dependency).unwrap()).collect(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Extrude".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

/// Native and neutral records of a configuration table whose columns name
/// parameter records 202 and 329 (through owner 330) and parameter scope 314.
pub(crate) struct Natives {
    pub(crate) parameters: Vec<DesignParameter>,
    pub(crate) owners: Vec<DesignParameterOwner>,
    pub(crate) scopes: Vec<DesignParameterScope>,
    pub(crate) neutral_parameters: Vec<NeutralParameter>,
    pub(crate) neutral_features: Vec<Feature>,
}

impl Natives {
    pub(crate) fn complete() -> Self {
        let parameters = vec![parameter(202, "hh"), parameter(329, "d4")];
        let scopes = vec![scope(314)];
        Self {
            neutral_parameters: parameters.iter().map(neutral_parameter).collect(),
            neutral_features: vec![neutral_feature(&scopes[0], "f3d:model:feature#sketch")],
            owners: vec![owner(STREAM, 330, 329)],
            parameters,
            scopes,
        }
    }

    /// Run the decoder's configuration pass over `documents` and these
    /// records: the configurations it states, with the neutral features in
    /// the state that the pass leaves them.
    pub(crate) fn attach(
        &mut self,
        documents: &[crate::records::configuration::DesignConfiguration],
    ) -> Vec<cadmpeg_ir::features::parameters::DesignConfiguration> {
        let native = crate::native::F3dNative {
            design_configurations: documents.to_vec(),
            design_parameters: self.parameters.clone(),
            design_parameter_owners: self.owners.clone(),
            design_parameter_scopes: self.scopes.clone(),
            ..Default::default()
        };
        let mut model = cadmpeg_ir::document::Model::default();
        model.parameters = self.neutral_parameters.clone();
        model.features = std::mem::take(&mut self.neutral_features);
        let producers = crate::design::configurations::body_producers(&model.features);
        crate::design::configurations::attach(&native, &mut model, &producers, &mut Vec::new())
            .unwrap();
        self.neutral_features = model.features;
        model.configurations
    }

    pub(crate) fn targets(&self) -> ConfigurationTargets<'_> {
        ConfigurationTargets {
            parameters: &self.parameters,
            owners: &self.owners,
            scopes: &self.scopes,
            neutral_parameters: &self.neutral_parameters,
            neutral_features: &self.neutral_features,
        }
    }
}
