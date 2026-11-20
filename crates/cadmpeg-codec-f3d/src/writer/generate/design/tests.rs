// SPDX-License-Identifier: Apache-2.0
//! The geometry-true form of a design scope: what a written file decodes
//! back to, and the losses the writer charges.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, Folder, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::expr::{
    BinaryOp, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::{ArrangementId, ContainerId, EvaluationId};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
use cadmpeg_ir::design::scope::{DesignScope, OccurrenceDesign};
use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ConfigurationId, FeatureId, ParameterId};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::NonNegativeReal;
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

const SKETCH: &str = "test:design:feature#sketch";
const EXTRUDE: &str = "test:design:feature#extrude";
const HOLE: &str = "test:design:feature#hole";

fn parameter_id(name: &str) -> ParameterId {
    ParameterId::mint(format!("test:design:parameter#{name}")).expect("identity grammar")
}

fn reference(name: &str) -> Node<ParameterRef> {
    Node::Reference(ParameterRef {
        target: RefTarget::Parameter(parameter_id(name)),
        through: None,
    })
}

fn number(value: f64, unit: Option<UnitSymbol>) -> Node<ParameterRef> {
    Node::Number {
        value: NonNegativeReal::new(value).expect("a nonnegative test value"),
        unit: Unit(
            unit.into_iter()
                .map(|symbol| UnitFactor {
                    symbol,
                    power: NonZeroI8::new(1).expect("power 1"),
                })
                .collect(),
        ),
    }
}

fn neutral(node: Node<ParameterRef>) -> ParameterDefinition {
    ParameterDefinition::Expression(Expression::Neutral(Neutral(node)))
}

fn parameter(
    key: &str,
    name: &str,
    definition: ParameterDefinition,
    value: Option<f64>,
) -> DesignParameter {
    DesignParameter {
        id: parameter_id(key),
        scope: root(),
        owner: None,
        name: NonBlankString::new(name.to_owned()),
        kind: ValueKind::Quantity(Quantity::LENGTH),
        unit: None,
        definition,
        value: value.map(|value| ParameterValue::Number(crate::test_support::real(value))),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

fn root() -> ProductDefinitionId {
    ProductDefinitionId::mint("test:design:product#root").expect("identity grammar")
}

fn feature(id: &str, family: OperationFamily) -> Feature {
    Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved { family },
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

/// The unit cube with a timeline of an unresolved sketch and extrude, and
/// four parameters: `Width = 60 mm`, `Half = Width / 2`, `Wall` in another
/// dialect, and the extrude's `D1 = 2 mm`.
fn scoped_cube() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    let sketch = FeatureId::mint(SKETCH).expect("identity grammar");
    let extrude = FeatureId::mint(EXTRUDE).expect("identity grammar");
    ir.design.features.extend([
        feature(SKETCH, OperationFamily::Sketch),
        feature(EXTRUDE, OperationFamily::Extrude),
    ]);
    ir.design.containers.push(Container::Ordered {
        id: timeline.clone(),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(sketch), Member::Feature(extrude.clone())],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: root(),
        roots: vec![timeline],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: Position::After(Member::Feature(extrude.clone())),
            bodies: vec![ir.geometry.bodies[0].id.clone()],
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    let mut owned = parameter(
        "d1",
        "D1",
        neutral(number(2.0, Some(UnitSymbol::Mm))),
        Some(2.0),
    );
    owned.owner = Some(ParameterOwner::Feature(extrude));
    ir.design.parameters.extend([
        parameter(
            "width",
            "Width",
            neutral(number(60.0, Some(UnitSymbol::Mm))),
            Some(60.0),
        ),
        parameter(
            "half",
            "Half",
            neutral(Node::Binary {
                op: BinaryOp::Divide,
                left: Box::new(reference("width")),
                right: Box::new(number(2.0, None)),
            }),
            Some(30.0),
        ),
        parameter(
            "wall",
            "Wall",
            ParameterDefinition::Expression(Expression::Source {
                dialect: ExpressionDialect::SolidWorks,
                tokens: vec![ExpressionToken::Text("5".into())],
            }),
            Some(5.0),
        ),
        owned,
    ]);
    ir
}

fn write(ir: &CadIr) -> (ExportReport, CadIr) {
    let (report, decoded, _) = write_bytes(ir);
    (report, decoded)
}

fn write_bytes(ir: &CadIr) -> (ExportReport, CadIr, Vec<u8>) {
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the writer writes the design scope");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("the written file decodes");
    crate::test_support::assert_decodes_back(&report, decoded.report());
    (report, decoded.into_parts().0, bytes)
}

fn loss<'a>(report: &'a ExportReport, code: &str) -> Vec<&'a str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The decoded parameter named `name`: its expression and its length in
/// millimetres.
fn decoded(ir: &CadIr, name: &str) -> Option<(String, f64)> {
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)?;
    let Some(cadmpeg_ir::features::ParameterValue::Length(length)) = &parameter.value else {
        return None;
    };
    Some((parameter.expression.clone(), length.get()))
}

fn assert_length(ir: &CadIr, name: &str, expression: &str, millimetres: f64) {
    let (text, value) = decoded(ir, name).unwrap_or_else(|| {
        panic!(
            "{name} decodes as a length parameter: {:#?}",
            ir.model.parameters
        )
    });
    assert_eq!(text, expression, "{name}");
    assert!((value - millimetres).abs() < 1e-9, "{name}: {value}");
}

#[test]
fn user_parameters_are_written_and_decode_by_name_and_value() {
    let (_, ir) = write(&scoped_cube());
    assert_length(&ir, "Width", "60 mm", 60.0);
    assert_length(&ir, "Half", "(Width / 2)", 30.0);
    assert_length(&ir, "Wall", "5 mm", 5.0);
    assert!(ir
        .model
        .parameters
        .iter()
        .all(|parameter| parameter.owner.is_none()));
    assert_eq!(ir.geometry.bodies.len(), 1);
}

#[test]
fn a_parameter_of_another_dialect_is_written_as_its_value() {
    let (report, _) = write(&scoped_cube());
    let flattened = loss(&report, "export/parameter.flattened");
    assert_eq!(flattened.len(), 1, "{:#?}", report.losses);
    assert!(flattened[0].contains(parameter_id("wall").as_str()));
    assert!(!flattened[0].contains(parameter_id("width").as_str()));
}

#[test]
fn a_model_parameter_of_an_unwritten_feature_is_written_as_a_user_parameter() {
    let (report, ir) = write(&scoped_cube());
    assert_length(&ir, "D1", "2 mm", 2.0);
    let missing = loss(&report, "export/evaluation.missing");
    assert_eq!(missing.len(), 1, "{:#?}", report.losses);
    assert!(
        missing[0].contains(&format!("{EXTRUDE} (")),
        "{}",
        missing[0]
    );
    assert!(missing[0].contains(parameter_id("d1").as_str()));
}

#[test]
fn every_feature_not_written_is_charged_evaluation_missing() {
    let (report, _) = write(&scoped_cube());
    let missing = loss(&report, "export/evaluation.missing");
    assert_eq!(missing.len(), 1, "{:#?}", report.losses);
    assert!(missing[0].contains(SKETCH) && missing[0].contains(EXTRUDE));
    assert!(loss(
        &report,
        &F3dLossCode::WriterParameterOmitted.kind().to_string()
    )
    .is_empty());
}

#[test]
fn an_illegal_name_is_renamed() {
    let mut ir = scoped_cube();
    ir.design.parameters.push(parameter(
        "thickness",
        "wall thickness",
        neutral(number(3.0, Some(UnitSymbol::Mm))),
        Some(3.0),
    ));
    let (report, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "wall_thickness", "3 mm", 3.0);
    let renamed = report
        .losses
        .iter()
        .find(|loss| loss.code.to_string() == "export/parameter.renamed")
        .expect("a renamed parameter is charged");
    assert_eq!(renamed.severity, Severity::Info);
    assert!(renamed.message.contains(parameter_id("thickness").as_str()));
    assert!(renamed
        .message
        .contains("`wall thickness` -> `wall_thickness`"));
}

#[test]
fn a_parameter_with_no_cache_and_no_expressible_expression_is_not_written() {
    let mut ir = scoped_cube();
    ir.design.parameters.push(parameter(
        "uncached",
        "Uncached",
        ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::SolidWorks,
            tokens: vec![ExpressionToken::Text("\"D1@Sketch1\" * 2".into())],
        }),
        None,
    ));
    let (report, decoded_ir) = write(&ir);
    assert!(decoded(&decoded_ir, "Uncached").is_none());
    assert!(loss(&report, "export/parameter.flattened")
        .iter()
        .any(|message| message.contains("no cached value")
            && message.contains(parameter_id("uncached").as_str())));
}

#[test]
fn a_reference_cycle_is_written_as_a_cached_value_where_it_is_broken() {
    let mut ir = scoped_cube();
    ir.design.parameters.extend([
        parameter("a", "A", neutral(reference("b")), Some(7.0)),
        parameter("b", "B", neutral(reference("a")), Some(7.0)),
    ]);
    let (report, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "A", "7 mm", 7.0);
    assert_length(&decoded_ir, "B", "A", 7.0);
    assert!(loss(&report, "export/parameter.flattened")
        .iter()
        .any(|message| message.contains(parameter_id("a").as_str())
            && !message.contains(parameter_id("b").as_str())));
}

#[test]
fn a_comment_is_written_with_its_parameter() {
    let mut ir = scoped_cube();
    ir.design.parameters[0].comment = Some("overall width".into());
    let (_, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "Width", "60 mm", 60.0);
    assert!(f3d_native(&decoded_ir)
        .design_parameters
        .iter()
        .any(
            |parameter| parameter.name() == "Width" && parameter.comment() == Some("overall width")
        ));
}

#[test]
fn a_failure_of_the_design_module_drops_the_design_not_the_body() {
    super::FAIL_NEXT_PLAN.with(|fail| fail.set(true));
    let (report, ir) = write(&scoped_cube());
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert!(ir.model.parameters.is_empty());
    assert!(loss(&report, "export/evaluation.missing")
        .iter()
        .any(
            |message| message.contains("a test failure of the design module")
                && message.contains(SKETCH)
        ));
    assert!(report.losses.iter().any(|loss| {
        loss.code.to_string() == F3dLossCode::WriterParameterOmitted.kind().to_string()
            && loss.message.contains(parameter_id("width").as_str())
    }));
}

#[test]
fn edited_design_parameters_replace_the_native_records() {
    let mut ir = scoped_cube();
    ir.design.parameters.retain(|parameter| {
        parameter
            .name
            .as_ref()
            .is_some_and(|name| name.as_str() == "Width")
    });
    ir.design.parameters[0].definition = neutral(number(70.0, Some(UnitSymbol::Mm)));
    ir.design.parameters[0].value = Some(ParameterValue::Number(crate::test_support::real(70.0)));
    f3d_native_mut(&mut ir).design_parameters.push(
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#0".into(),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .expect("class tag"),
                record_index: 700,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::User {
                    source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
                    family_discriminator: Some(crate::records::identity::Located {
                        value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                        offset: 22,
                    }),
                },
                expression: "60 mm".into(),
                expression_offset: 36,
                source_kind_offset: 70,
                unit: Some(crate::records::identity::RecordedValue {
                    value: "mm".into(),
                    offset: 110,
                }),
                name: "Width".into(),
                name_offset: 120,
                evaluated_value: 6.0,
                evaluated_value_offset: 150,
            },
        )
        .expect("a native user parameter"),
    );
    let (report, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "Width", "70 mm", 70.0);
    assert_eq!(decoded_ir.model.parameters.len(), 1);
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/native.records-omitted"),
        "{:#?}",
        report.losses
    );
}

/// The Design stream of a written archive, and the frame of the one record
/// of each type as its primary index bounds it.
struct DesignStream {
    bulk: Vec<u8>,
    meta: crate::metastream::MetaStream,
}

impl DesignStream {
    fn read(archive: &[u8]) -> Self {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive)).expect("a ZIP archive");
        let mut entry = |name: &str| {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(
                &mut zip
                    .by_name(&format!(
                        "{}/Design1/{name}",
                        crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
                    ))
                    .expect("the Design stream"),
                &mut bytes,
            )
            .expect("the entry reads");
            bytes
        };
        let bulk = entry("BulkStream.dat");
        let meta = crate::metastream::parse(&entry("MetaStream.dat"), "MetaStream.dat")
            .expect("the MetaStream parses");
        Self { bulk, meta }
    }

    /// The record index and frame of the one record of type `guid`.
    fn frame(&self, guid: &str) -> (u64, &[u8]) {
        let records = self
            .meta
            .types
            .iter()
            .filter(|row| row.type_guid.as_str().eq_ignore_ascii_case(guid))
            .flat_map(|row| row.entities.values().copied())
            .collect::<Vec<_>>();
        let [record] = records.as_slice() else {
            panic!("one record of type {guid}: {records:?}");
        };
        let position = self
            .meta
            .records
            .iter()
            .position(|entry| entry.entity_id == *record)
            .expect("a primary-index entry");
        let start = usize::try_from(self.meta.records[position].bulk_offset).expect("offset");
        let end = self
            .meta
            .records
            .get(position + 1)
            .map_or(self.bulk.len(), |next| {
                usize::try_from(next.bulk_offset).expect("offset")
            });
        (*record, &self.bulk[start..end])
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}

#[test]
fn the_cube_is_one_base_feature_item_of_the_timeline() {
    let (report, ir) = write(&scoped_cube());
    let base_features = ir
        .model
        .features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
            ) => Some(bodies),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [bodies] = base_features.as_slice() else {
        panic!("one Base Feature: {:#?}", ir.model.features);
    };
    let resolved = match bodies {
        cadmpeg_ir::features::BodySelection::Bodies(bodies)
        | cadmpeg_ir::features::BodySelection::Resolved { bodies, .. } => bodies.as_slice(),
        other => panic!("the Base Feature resolves its body: {other:?}"),
    };
    assert_eq!(resolved, [ir.geometry.bodies[0].id.clone()]);
    let native = f3d_native(&ir);
    let [timeline] = native.design_feature_timelines.as_slice() else {
        panic!("one timeline");
    };
    assert_eq!(timeline.frame().items().len(), 1);
    assert!(loss(&report, "export/record.residual-unavailable")
        .iter()
        .any(|message| message.contains("component aggregate")));
}

#[test]
fn the_history_is_one_insertion_state() {
    let (_, ir) = write(&scoped_cube());
    let native = f3d_native(&ir);
    let [history] = native.asm_histories.as_slice() else {
        panic!("one ASM history");
    };
    let preamble = history.preamble.expect("a history preamble");
    assert_eq!((preamble.stream_size, preamble.history_entry_count), (3, 1));
    let [head, root] = history.states() else {
        panic!("two states: {:#?}", history.states());
    };
    assert_eq!(
        (
            head.state_id,
            head.previous_ref,
            head.next_ref,
            head.node_index
        ),
        (3, None, Some(1), 0)
    );
    assert_eq!(
        (
            root.state_id,
            root.previous_ref,
            root.next_ref,
            root.node_index
        ),
        (2, Some(0), None, 1)
    );
    assert!(root.bulletin_boards.is_empty());
    let [board] = head.bulletin_boards.as_slice() else {
        panic!("one bulletin board");
    };
    assert_eq!((board.owner_ref, board.number), (0, 2));
    let inserted = board
        .changes
        .iter()
        .map(|change| (change.old_ref(), change.new_ref()))
        .collect::<Vec<_>>();
    let slots = i64::try_from(inserted.len()).expect("slot count");
    assert!(slots > 0);
    assert_eq!(
        inserted,
        (1..=slots)
            .map(|slot| (None, Some(slot)))
            .collect::<Vec<_>>()
    );
    assert!(crate::history::graph_is_coherent(history));
}

#[test]
fn every_base_feature_face_is_named_by_its_token() {
    let (_, ir) = write(&scoped_cube());
    let native = f3d_native(&ir);
    let mut tokens = native
        .persistent_subentity_tags
        .iter()
        .map(|tag| {
            assert_eq!(tag.design_references, [-300]);
            assert_eq!(tag.selector, 1);
            tag.token.as_str().to_owned()
        })
        .collect::<Vec<_>>();
    tokens.sort();
    assert_eq!(tokens, ["0", "1", "2", "3", "4", "5"]);
    let [link] = native.persistent_design_links.as_slice() else {
        panic!("one body name");
    };
    assert_eq!((link.design_id.as_str(), link.design_reference), ("300", 1));
}

#[test]
fn the_scope_is_the_result_body_form_with_its_item_tail() {
    let (_, _, bytes) = write_bytes(&scoped_cube());
    let stream = DesignStream::read(&bytes);
    let (scope, frame) = stream.frame("D7015B81-AB4D-4C89-BED7-AA4411E1964A");
    // One body: the kind at 169 + 52, the 78-byte tail, the paired header and
    // the 30-byte paired scope record.
    assert_eq!(frame.len(), 271 + 52 + 30);
    assert_eq!(u32_at(frame, 20), 2);
    assert_eq!(u32_at(frame, 161 + 52), 3);
    assert_eq!(u32_at(frame, 165 + 52), 12);
    let tail = &frame[193 + 52..271 + 52];
    assert_eq!(u32_at(tail, 0), 1);
    assert_eq!(&tail[16..19], [0, 0, 0]);
    assert_eq!(tail[30], 0);
    assert_eq!(u32_at(tail, 31), 2);
    assert_eq!((tail[62], tail[63], tail[64]), (1, 0, 0));
    assert_eq!(
        u64_at(frame, 271 + 52 + 7) & 0xffff_ffff,
        scope & 0xffff_ffff
    );
}

#[test]
fn the_persistent_id_table_maps_the_base_feature_identity() {
    let (_, _, bytes) = write_bytes(&scoped_cube());
    let stream = DesignStream::read(&bytes);
    let (scope, _) = stream.frame("D7015B81-AB4D-4C89-BED7-AA4411E1964A");
    let (_, table) = stream.frame("5D89B935-3F04-4CAB-81F5-E7B309E7E13F");
    assert_eq!(u32_at(table, 21), 1);
    assert_eq!(u64_at(table, 25), scope);
    assert_eq!((u32_at(table, 33), u64_at(table, 37)), (1, 300));
    assert_eq!(u32_at(table, 45), 1);
    assert_eq!((u64_at(table, 49), u64_at(table, 57)), (300, scope));
    assert_eq!(u32_at(table, 65), 0);
    let (_, origin) = stream.frame("2972A3B8-3627-49BC-8C3D-18707605EC44");
    assert_eq!(origin.len(), 115);
    assert_eq!(u32_at(origin, 23), 7);
    let planes = [71, 82, 93].map(|at| u64_at(origin, at + 1));
    assert_eq!([planes[1], planes[2]], [planes[0] + 1, planes[0] + 2]);
}

#[test]
fn the_parameter_tails_name_the_parameter_collection() {
    let (_, ir, bytes) = write_bytes(&scoped_cube());
    let stream = DesignStream::read(&bytes);
    let (collection, frame) = stream.frame("B0556F58-EE09-46A7-B18F-E296F48E5B13");
    let written = u32_at(frame, 46);
    assert_eq!(u32_at(frame, 30), written);
    assert_eq!(
        usize::try_from(written).expect("count"),
        ir.model.parameters.len()
    );
    assert_eq!(
        frame.len(),
        61 + 11 * usize::try_from(written).expect("count")
    );
    assert!(collection > 0);
}

#[test]
fn a_document_with_no_body_writes_the_timeline_with_no_item() {
    let mut ir = scoped_cube();
    ir.geometry = cadmpeg_ir::document::Topology::default();
    ir.design.scopes[0].current[0].bodies.clear();
    let (report, decoded_ir) = write(&ir);
    assert!(decoded_ir.geometry.bodies.is_empty());
    assert_length(&decoded_ir, "Width", "60 mm", 60.0);
    let native = f3d_native(&decoded_ir);
    let [timeline] = native.design_feature_timelines.as_slice() else {
        panic!("one timeline");
    };
    assert!(timeline.frame().items().is_empty());
    assert!(native.asm_histories.is_empty());
    assert!(!loss(&report, "export/output.empty").is_empty());
    assert_eq!(loss(&report, "export/evaluation.missing").len(), 1);
}

#[test]
fn a_container_or_attribute_parameter_is_written_without_its_owner() {
    let mut ir = scoped_cube();
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    let attribute =
        cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#mass").expect("identity grammar");
    let mut bound = parameter(
        "bound",
        "Bound",
        neutral(number(4.0, Some(UnitSymbol::Mm))),
        Some(4.0),
    );
    bound.owner = Some(ParameterOwner::Container(timeline.clone()));
    let mut linked = parameter(
        "linked",
        "Linked",
        neutral(number(7.0, Some(UnitSymbol::Mm))),
        Some(7.0),
    );
    linked.owner = Some(ParameterOwner::Attribute(attribute.clone()));
    ir.design.parameters.extend([bound, linked]);
    let (report, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "Bound", "4 mm", 4.0);
    assert_length(&decoded_ir, "Linked", "7 mm", 7.0);
    let unwritable = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code.to_string() == "export/value.unwritable"
                && loss.message.contains("without their owners")
        })
        .collect::<Vec<_>>();
    assert_eq!(unwritable.len(), 1, "{:#?}", report.losses);
    assert_eq!(unwritable[0].severity, Severity::Warning);
    let message = &unwritable[0].message;
    assert!(
        message.contains(&format!(
            "{} (owner container {timeline})",
            parameter_id("bound")
        )),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "{} (owner attribute {attribute})",
            parameter_id("linked")
        )),
        "{message}"
    );
    assert!(!message.contains(parameter_id("d1").as_str()), "{message}");
    assert!(
        !message.contains(parameter_id("width").as_str()),
        "{message}"
    );
}

#[test]
fn the_timeline_is_written_without_its_folders_and_rollback() {
    let mut ir = scoped_cube();
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    let Container::Ordered {
        folders, rollback, ..
    } = &mut ir.design.containers[0]
    else {
        panic!("the fixture's timeline is ordered");
    };
    folders.push(Folder {
        name: Some("Setup".into()),
        first: 0,
        last: 1,
    });
    *rollback = Rollback::Before(Member::Feature(
        FeatureId::mint(EXTRUDE).expect("identity grammar"),
    ));
    let (report, _) = write(&ir);
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    );
    assert_eq!(omitted.len(), 1, "{:#?}", report.losses);
    assert!(
        omitted[0].contains(&format!("{timeline} folder `Setup` (members 0..=1)")),
        "{}",
        omitted[0]
    );
    assert!(
        omitted[0].contains(&format!("{timeline} rollback (before {EXTRUDE})")),
        "{}",
        omitted[0]
    );
}

/// Add a design scope of a placed part to `ir`: its timeline holds a Hole
/// feature, and it owns the parameter `Depth`.
fn place_part_scope(ir: &mut CadIr) -> (ProductDefinitionId, ContainerId) {
    let part = ProductDefinitionId::mint("test:design:product#part").expect("identity grammar");
    let history =
        ContainerId::mint("test:design:container#part-timeline").expect("identity grammar");
    ir.design
        .features
        .push(feature(HOLE, OperationFamily::Hole));
    ir.design.containers.push(Container::Ordered {
        id: history.clone(),
        kind: OrderedKind::Timeline,
        members: vec![Member::Feature(
            FeatureId::mint(HOLE).expect("identity grammar"),
        )],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: part.clone(),
        roots: vec![history.clone()],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
    });
    let mut depth = parameter(
        "depth",
        "Depth",
        neutral(number(8.0, Some(UnitSymbol::Mm))),
        Some(8.0),
    );
    depth.scope = part.clone();
    ir.design.parameters.push(depth);
    ir.model
        .occurrences
        .push(cadmpeg_ir::products::Occurrence::new(
            OccurrenceId::mint("test:model:occurrence#part").expect("identity grammar"),
            part.clone(),
            None,
        ));
    (part, history)
}

#[test]
fn a_placed_design_scope_is_charged_with_its_containers_features_and_parameters() {
    let mut ir = scoped_cube();
    let (part, history) = place_part_scope(&mut ir);
    let (report, decoded_ir) = write(&ir);
    assert_length(&decoded_ir, "Width", "60 mm", 60.0);
    assert!(decoded(&decoded_ir, "Depth").is_none());
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    );
    assert!(
        omitted
            .iter()
            .any(|message| message.contains("design scope") && message.contains(part.as_str())),
        "{:#?}",
        report.losses
    );
    assert!(
        omitted
            .iter()
            .any(|message| message.contains("design container")
                && message.contains(history.as_str())
                && !message.contains("container#timeline")),
        "{:#?}",
        report.losses
    );
    assert!(
        loss(&report, "export/evaluation.missing")
            .iter()
            .any(|message| message.contains(&format!("{HOLE} (design scope of {part})"))),
        "{:#?}",
        report.losses
    );
    let depth = parameter_id("depth");
    let parameters = loss(
        &report,
        &F3dLossCode::WriterParameterOmitted.kind().to_string(),
    );
    assert!(
        parameters.iter().any(
            |message| message.contains(&format!("{depth} (definition {part})"))
                && !message.contains(parameter_id("width").as_str())
        ),
        "{:#?}",
        report.losses
    );
}

#[test]
fn the_root_scopes_configuration_arrangements_and_occurrence_states_are_charged() {
    let mut ir = scoped_cube();
    let configuration =
        ConfigurationId::mint("test:design:configuration#long").expect("identity grammar");
    let arrangement =
        ArrangementId::mint("test:design:arrangement#open").expect("identity grammar");
    let occurrence = OccurrenceId::mint("test:model:occurrence#part").expect("identity grammar");
    let scope = &mut ir.design.scopes[0];
    scope.active_configuration = Some(configuration.clone());
    scope.active_arrangements.insert(arrangement.clone());
    scope.occurrences.insert(
        occurrence.clone(),
        OccurrenceDesign {
            configuration: None,
            arrangements: Vec::new(),
            grounded: Some(true),
            flexible: None,
            parameters: BTreeMap::new(),
        },
    );
    let (report, _) = write(&ir);
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    );
    let root = root();
    for value in [
        format!("{root} active configuration {configuration}"),
        format!("{root} active arrangement {arrangement}"),
        format!("{root} design state of occurrence {occurrence}"),
    ] {
        assert!(
            omitted.iter().any(|message| message.contains(&value)),
            "{value}: {:#?}",
            report.losses
        );
    }
}

#[test]
fn a_dropped_design_charges_every_scope_container_feature_and_parameter() {
    let mut ir = scoped_cube();
    let (part, history) = place_part_scope(&mut ir);
    // A v10 design record: the export wrapper then charges no design arena.
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            product_definition: None,
            id: parameter_id("legacy"),
            owner: None,
            ordinal: 0,
            name: "Legacy".into(),
            expression: "1 mm".into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    super::FAIL_NEXT_PLAN.with(|fail| fail.set(true));
    let (report, _) = write(&ir);
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    );
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    assert!(
        omitted
            .iter()
            .any(|message| message.contains("design scope")
                && message.contains(root().as_str())
                && message.contains(part.as_str())),
        "{:#?}",
        report.losses
    );
    assert!(
        omitted
            .iter()
            .any(|message| message.contains("design container")
                && message.contains(timeline.as_str())
                && message.contains(history.as_str())),
        "{:#?}",
        report.losses
    );
    assert!(
        loss(&report, "export/evaluation.missing")
            .iter()
            .any(
                |message| message.contains("a test failure of the design module")
                    && message.contains(SKETCH)
                    && message.contains(HOLE)
            ),
        "{:#?}",
        report.losses
    );
    assert!(
        loss(
            &report,
            &F3dLossCode::WriterParameterOmitted.kind().to_string()
        )
        .iter()
        .any(|message| message.contains(parameter_id("width").as_str())
            && message.contains(parameter_id("depth").as_str())),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_stale_current_state_and_its_pose_are_charged() {
    let mut ir = scoped_cube();
    let occurrence = OccurrenceId::mint("test:model:occurrence#pin").expect("identity grammar");
    let state = &mut ir.design.scopes[0].current[0];
    state.status = EvaluationStatus::Stale;
    state.poses.insert(
        OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(occurrence.clone()),
            configuration: None,
        }]),
        Transform::identity(),
    );
    let (report, _) = write(&ir);
    let root = root();
    let stale = format!("{root} current state after {EXTRUDE} (stale)");
    assert!(
        loss(&report, "export/evaluation.stale")
            .iter()
            .any(|message| message.contains(&stale)),
        "{:#?}",
        report.losses
    );
    let pose = format!("{root} pose of `{occurrence}` in the current state after {EXTRUDE}");
    assert!(
        loss(
            &report,
            &F3dLossCode::WriterDesignRecordOmitted.kind().to_string()
        )
        .iter()
        .any(|message| message.contains(&pose)),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_stale_and_a_current_evaluation_are_each_charged_once_by_name() {
    let mut ir = scoped_cube();
    let timeline = ContainerId::mint("test:design:container#timeline").expect("identity grammar");
    let stale = EvaluationId::mint("test:design:evaluation#tool").expect("identity grammar");
    let current = EvaluationId::mint("test:design:evaluation#start").expect("identity grammar");
    ir.design.evaluations.extend([
        Evaluation {
            id: stale.clone(),
            scope: root(),
            configuration: None,
            of: Evaluated::Tool(FeatureId::mint(EXTRUDE).expect("identity grammar")),
            status: EvaluationStatus::Stale,
            content: None,
            native_ref: None,
        },
        Evaluation {
            id: current.clone(),
            scope: root(),
            configuration: None,
            of: Evaluated::State(Position::Start(timeline.clone())),
            status: EvaluationStatus::Current,
            content: Some(StateContent {
                base: StateBase::Empty,
                delta: StateDelta::default(),
            }),
            native_ref: None,
        },
    ]);
    let root = root();
    let stale = format!("{stale}: {root} tool body of {EXTRUDE} (stale)");
    let current = format!("{current}: {root} state at the start of {timeline}");
    for dropped in [false, true] {
        super::FAIL_NEXT_PLAN.with(|fail| fail.set(dropped));
        let (report, _) = write(&ir);
        assert!(
            loss(&report, "export/evaluation.stale")
                .iter()
                .any(|message| message.contains(&stale)),
            "dropped {dropped}: {:#?}",
            report.losses
        );
        assert!(
            loss(
                &report,
                &F3dLossCode::WriterDesignRecordOmitted.kind().to_string()
            )
            .iter()
            .any(|message| message.contains("evaluation") && message.contains(&current)),
            "dropped {dropped}: {:#?}",
            report.losses
        );
        let charged_by_name = [
            "design.scopes",
            "design.containers",
            "design.features",
            "design.parameters",
            "design.evaluations",
        ];
        assert!(
            !loss(&report, "export/arena.omitted").iter().any(|message| {
                charged_by_name
                    .iter()
                    .any(|arena| message.contains(&format!("`{arena}`")))
            }),
            "dropped {dropped}: {:#?}",
            report.losses
        );
    }
}

#[test]
fn an_integer_parameter_is_written_as_a_real_with_a_note() {
    let mut ir = scoped_cube();
    let mut count = parameter("count", "Count", neutral(number(4.0, None)), None);
    count.kind = ValueKind::Integer;
    count.value = Some(ParameterValue::Integer(4));
    ir.design.parameters.push(count);
    let (report, decoded_ir) = write(&ir);
    assert!(decoded_ir
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "Count"));
    let notes = report
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::WriterIntegerAsReal.kind())
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1, "{:#?}", report.losses);
    assert_eq!(notes[0].severity, Severity::Info);
    assert!(notes[0].message.contains(parameter_id("count").as_str()));
    assert!(!notes[0].message.contains(parameter_id("width").as_str()));
}

#[test]
fn a_parameter_display_flag_and_unstated_body_visibility_are_charged() {
    let mut ir = scoped_cube();
    ir.design.parameters[0].display =
        Some(cadmpeg_ir::design::parameter::DimensionDisplay::Diameter);
    ir.geometry.bodies[0].visible = None;
    let body = ir.geometry.bodies[0].id.to_string();
    let param = ir.design.parameters[0].id.to_string();
    let (report, _) = write(&ir);
    let unwritable = loss(&report, "export/value.unwritable");
    assert!(
        unwritable
            .iter()
            .any(|message| message.contains("display") && message.contains(&param)),
        "{:#?}",
        report.losses
    );
    assert!(
        unwritable
            .iter()
            .any(|message| message.contains("visible") && message.contains(&body)),
        "{:#?}",
        report.losses
    );
}
