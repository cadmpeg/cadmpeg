// SPDX-License-Identifier: Apache-2.0
//! The geometry-true form of a design scope: what a written file decodes
//! back to, and the losses the writer charges.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::expr::{
    BinaryOp, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::NonNegativeReal;

use crate::loss::F3dLossCode;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

const SKETCH: &str = "test:design:feature#sketch";
const EXTRUDE: &str = "test:design:feature#extrude";

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
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the writer writes the design scope");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    crate::test_support::assert_decodes_back(&report, decoded.report());
    (report, decoded.into_parts().0)
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
