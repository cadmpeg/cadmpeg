// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ContentScope, ResourceLimits};
use serde_json::json;

use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use crate::attributes::{AttributeExpression, AttributeTarget, AttributeValue, SourceAttribute};
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::expr::{
    parse, BinaryOp, Constant, Expression, ExpressionDialect, ExpressionToken, Measure,
    MeasureKind, MeasureTarget, Neutral, Node,
};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::selection::OrderedSelection;
use crate::design::units::{Quantity, ValueKind};
use crate::examples::unit_cube;
use crate::features::ParameterId;
use crate::ids::{AppearanceId, AttributeId};
use crate::report::check::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

const ATTRIBUTE: &str = "test:model:attribute#1";
const BINDING: &str = "test:model:appearance-binding#1";

fn attribute(target: AttributeTarget, values: Vec<Option<AttributeValue>>) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(ATTRIBUTE).unwrap(),
        target: target.into(),
        name: "Property".into(),
        values,
        native_ref: None,
    }
}

fn appearance() -> Appearance {
    Appearance {
        id: AppearanceId::mint("test:model:appearance#1").unwrap(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: BTreeMap::new(),
        textures: Vec::new(),
        native_ref: None,
    }
}

fn binding(target: AppearanceTarget) -> AppearanceBinding {
    AppearanceBinding {
        id: BINDING.try_into().unwrap(),
        target: target.into(),
        appearance: appearance().id,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: BTreeMap::new(),
        native_ref: None,
    }
}

/// The messages of the design findings on `entity`.
fn design_findings(ir: &CadIr, entity: &str) -> Vec<String> {
    validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| {
            finding.check == Check::Design && finding.entity.as_deref() == Some(entity)
        })
        .map(|finding| finding.message)
        .collect()
}

fn with_attribute(target: AttributeTarget, values: Vec<Option<AttributeValue>>) -> Vec<String> {
    let mut ir = unit_cube().unwrap();
    ir.model.attributes.push(attribute(target, values));
    design_findings(&ir, ATTRIBUTE)
}

#[test]
fn each_target_arm_names_a_record_of_its_kind() {
    let id = |kind: &str| format!("test:model:{kind}#missing");
    let cases: Vec<(AttributeTarget, &str, String)> = vec![
        (
            AttributeTarget::Shell(id("shell").as_str().try_into().unwrap()),
            "shell",
            id("shell"),
        ),
        (
            AttributeTarget::Loop(id("loop").as_str().try_into().unwrap()),
            "loop",
            id("loop"),
        ),
        (
            AttributeTarget::Region(id("region").as_str().try_into().unwrap()),
            "region",
            id("region"),
        ),
        (
            AttributeTarget::Surface(id("surface").as_str().try_into().unwrap()),
            "surface",
            id("surface"),
        ),
        (
            AttributeTarget::Curve(id("curve").as_str().try_into().unwrap()),
            "curve",
            id("curve"),
        ),
        (
            AttributeTarget::Point(id("point").as_str().try_into().unwrap()),
            "point",
            id("point"),
        ),
        (
            AttributeTarget::Feature(crate::features::FeatureId::mint(id("feature")).unwrap()),
            "feature",
            id("feature"),
        ),
        (
            AttributeTarget::Sketch(crate::sketches::SketchId::mint(id("sketch")).unwrap()),
            "sketch",
            id("sketch"),
        ),
        (
            AttributeTarget::Parameter(ParameterId::mint(id("parameter")).unwrap()),
            "parameter",
            id("parameter"),
        ),
        (
            AttributeTarget::ProductDefinition(id("definition").as_str().try_into().unwrap()),
            "product definition",
            id("definition"),
        ),
        (
            AttributeTarget::Occurrence(id("occurrence").as_str().try_into().unwrap()),
            "occurrence",
            id("occurrence"),
        ),
        (
            AttributeTarget::Tessellation(id("tessellation")),
            "tessellation",
            id("tessellation"),
        ),
        (
            AttributeTarget::Pmi(id("pmi").as_str().try_into().unwrap()),
            "PMI annotation",
            id("pmi"),
        ),
        (
            AttributeTarget::SemanticAnnotation(id("annotation").as_str().try_into().unwrap()),
            "semantic annotation",
            id("annotation"),
        ),
        (
            AttributeTarget::Drawing(id("drawing").as_str().try_into().unwrap()),
            "drawing",
            id("drawing"),
        ),
        (
            AttributeTarget::ViewPresentation(id("view").as_str().try_into().unwrap()),
            "view presentation",
            id("view"),
        ),
    ];
    for (target, kind, id) in cases {
        assert_eq!(
            with_attribute(target, Vec::new()),
            [format!("references missing {kind} `{id}`")]
        );
    }
    let ir = unit_cube().unwrap();
    let geometry = &ir.geometry;
    for target in [
        AttributeTarget::Shell(geometry.shells[0].id.clone()),
        AttributeTarget::Loop(geometry.loops[0].id.clone()),
        AttributeTarget::Region(geometry.regions[0].id.clone()),
        AttributeTarget::Surface(geometry.surfaces[0].id.clone()),
        AttributeTarget::Curve(geometry.curves[0].id.clone()),
        AttributeTarget::Point(geometry.points[0].id.clone()),
    ] {
        assert_eq!(with_attribute(target, Vec::new()), Vec::<String>::new());
    }
}

#[test]
fn a_record_target_names_an_arena_and_an_id_of_it() {
    let ir = unit_cube().unwrap();
    let body = ir.geometry.bodies[0].id.to_string();
    let record = |arena: &str, id: &str| AttributeTarget::Record {
        arena: cadmpeg_core::text::NonBlankString::new(arena).unwrap(),
        id: id.into(),
    };
    assert_eq!(
        with_attribute(record("bodies", &body), Vec::new()),
        Vec::<String>::new()
    );
    assert_eq!(
        with_attribute(record("attributes", ATTRIBUTE), Vec::new()),
        Vec::<String>::new()
    );
    assert_eq!(
        with_attribute(record("faces", &body), Vec::new()),
        [format!(
            "record target `{body}` is no record of arena `faces`"
        )]
    );
    for arena in ["setups", "geometry", "Bodies"] {
        assert_eq!(
            with_attribute(record(arena, &body), Vec::new()),
            [format!("record target names `{arena}`, which is no arena of the model, the design, or the topology")]
        );
    }

    let mut ir = unit_cube().unwrap();
    ir.model.appearances.push(appearance());
    ir.model
        .appearance_bindings
        .push(binding(AppearanceTarget::Record {
            arena: cadmpeg_core::text::NonBlankString::new("point_clouds").unwrap(),
            id: "test:model:cloud#1".into(),
        }));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["record target `test:model:cloud#1` is no record of arena `point_clouds`"]
    );
    ir.model.appearance_bindings[0] = binding(AppearanceTarget::Occurrence(
        "test:model:occurrence#1".try_into().unwrap(),
    ));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["references missing occurrence `test:model:occurrence#1`"]
    );
}

#[test]
fn native_refs_of_appearances_bindings_and_attributes_resolve() {
    let mut ir = unit_cube().unwrap();
    ir.native.namespace_mut("native");
    let mut asset = appearance();
    asset.native_ref = Some("native:missing#1".into());
    let mut assigned = binding(AppearanceTarget::Body(ir.geometry.bodies[0].id.clone()));
    assigned.native_ref = Some("native:missing#2".into());
    let mut property = attribute(AttributeTarget::Document, Vec::new());
    property.native_ref = Some("native:missing#3".into());
    ir.model.appearances.push(asset);
    ir.model.appearance_bindings.push(assigned);
    ir.model.attributes.push(property);
    for (entity, target) in [
        ("test:model:appearance#1", "native:missing#1"),
        (BINDING, "native:missing#2"),
        (ATTRIBUTE, "native:missing#3"),
    ] {
        assert_eq!(
            design_findings(&ir, entity),
            [format!("native_ref `{target}` does not resolve")]
        );
    }
}

fn parameter(id: &ParameterId) -> crate::features::parameters::DesignParameter {
    crate::features::parameters::DesignParameter {
        product_definition: None,
        id: id.clone(),
        owner: None,
        ordinal: 0,
        name: "Mass".into(),
        expression: "1".into(),
        display: None,
        value: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn reference(id: &ParameterId) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id.clone()),
        through: None,
    }
}

fn expression_value(expression: Expression) -> Vec<Option<AttributeValue>> {
    vec![Some(AttributeValue::Expression(AttributeExpression {
        path: "Length".into(),
        expression,
        comment: None,
    }))]
}

fn binary(op: BinaryOp, left: Node<ParameterRef>, right: Node<ParameterRef>) -> Node<ParameterRef> {
    Node::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn measure(anchor: &serde_json::Value) -> Node<ParameterRef> {
    let selection = json!({
        "at": anchor,
        "picks": {"kind": "named", "value": [{"target": {"kind": "entity", "value": {
            "kind": "generated", "value": {"kind": "face", "feature": "test:design:feature#1"}
        }}}]}
    });
    let selection: OrderedSelection =
        crate::design::read::from_str(&selection.to_string(), &ResourceLimits::desktop()).unwrap();
    Node::Measure(Box::new(Measure {
        kind: MeasureKind::Area,
        of: MeasureTarget::Selection(selection),
        cached: None,
    }))
}

#[test]
fn an_attribute_expression_meets_the_expression_rules() {
    let mass = ParameterId::mint("test:design:parameter#mass").unwrap();
    let neutral = |node| expression_value(Expression::Neutral(Neutral(node)));
    let findings = |values| {
        let mut ir = unit_cube().unwrap();
        ir.model.parameters.push(parameter(&mass));
        ir.model
            .attributes
            .push(attribute(AttributeTarget::Document, values));
        design_findings(&ir, ATTRIBUTE)
    };
    let text = || Node::Text("a".into());
    let none = Vec::<String>::new();

    assert_eq!(findings(neutral(Node::Reference(reference(&mass)))), none);
    let absent = ParameterId::mint("test:design:parameter#absent").unwrap();
    assert_eq!(
        findings(neutral(Node::Reference(reference(&absent)))),
        ["references missing parameter `test:design:parameter#absent`"]
    );
    assert_eq!(
        findings(neutral(Node::Reference(ParameterRef {
            target: RefTarget::External {
                definition: "test:model:definition#external".try_into().unwrap(),
                name: cadmpeg_core::text::NonBlankString::new("D1").unwrap(),
            },
            through: None,
        }))),
        ["references missing product definition `test:model:definition#external`"]
    );
    assert_eq!(findings(neutral(text())), none);
    assert_eq!(
        findings(neutral(binary(BinaryOp::Equal, text(), text()))),
        none
    );
    assert_eq!(
        findings(neutral(Node::If(Box::new([
            Node::Boolean(true),
            text(),
            text()
        ])))),
        none
    );
    assert_eq!(
        findings(neutral(binary(
            BinaryOp::Add,
            text(),
            Node::Constant(Constant::Pi)
        ))),
        ["expression text is not a comparison's operand, a branch, or the root"]
    );
    assert_eq!(
        findings(neutral(Node::LawParameter)),
        ["expression holds the law parameter outside a law"]
    );
    assert_eq!(
        findings(neutral(measure(&json!({"kind": "current"})))),
        none
    );
    assert_eq!(
        findings(neutral(measure(&json!({"kind": "input"})))),
        ["expression measure is not anchored `pinned` or `current`"]
    );
    assert_eq!(
        findings(expression_value(Expression::Source {
            dialect: ExpressionDialect::FreeCad,
            tokens: vec![ExpressionToken::Reference(reference(&mass))],
        })),
        ["expression source text holds no text token"]
    );
    assert_eq!(
        findings(expression_value(Expression::Source {
            dialect: ExpressionDialect::FreeCad,
            tokens: vec![
                ExpressionToken::Text("2 * ".into()),
                ExpressionToken::Reference(reference(&absent)),
            ],
        })),
        ["references missing parameter `test:design:parameter#absent`"]
    );
}

fn design_parameter(key: &str, kind: ValueKind) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint(format!("test:design:parameter#{key}")).unwrap(),
        scope: "test:model:product-definition#part".try_into().unwrap(),
        owner: None,
        name: Some(cadmpeg_core::text::NonBlankString::new(key).unwrap()),
        kind,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

#[test]
fn an_attribute_expression_type_checks() {
    let mass = ParameterId::mint("test:design:parameter#mass").unwrap();
    let length = design_parameter("length", ValueKind::Quantity(Quantity::LENGTH));
    let flag = design_parameter("flag", ValueKind::Boolean);
    let refs = [reference(&mass), reference(&length.id), reference(&flag.id)];
    let findings = |text: &str| {
        let mut ir = unit_cube().unwrap();
        ir.model.parameters.push(parameter(&mass));
        ir.design.parameters.extend([length.clone(), flag.clone()]);
        let node = parse(text, &refs, &[]).unwrap();
        ir.model.attributes.push(attribute(
            AttributeTarget::Document,
            expression_value(Expression::Neutral(Neutral(node))),
        ));
        design_findings(&ir, ATTRIBUTE)
    };
    let refused = |reason: &str| [format!("expression does not type-check: {reason}")];

    for admitted in [
        "atan2(1[mm], 2[mm])",
        "min(1[mm], $1, 3[mm])",
        "if($2, 1[mm], $1)",
        "if(1[mm] < 2[mm], \"a\", \"b\")",
        "$0 + 1[deg]",
        "$1 * $1 + 1[mm]^2",
        "sin(30[deg]) == 0.5",
        "not $2 and true",
    ] {
        assert_eq!(findings(admitted), Vec::<String>::new(), "{admitted}");
    }
    assert_eq!(findings("atan2(pi)"), refused("atan2 takes two arguments"));
    assert_eq!(
        findings("sin(1[deg], 2[deg])"),
        refused("sin takes one argument")
    );
    assert_eq!(
        findings("min(1[mm])"),
        refused("min takes at least two arguments")
    );
    assert_eq!(
        findings("if(true, 1[mm], 1[deg])"),
        refused("if takes operands of one quantity")
    );
    assert_eq!(
        findings("if(false, 1[mm], $1 * $1)"),
        refused("if takes operands of one quantity")
    );
    assert_eq!(findings("true + 1"), refused("+ and - takes a number"));
    assert_eq!(findings("$2 + 1"), refused("+ and - takes a number"));
    assert_eq!(
        findings("$1 + 1[deg]"),
        refused("+ and - takes operands of one quantity")
    );
    assert_eq!(
        findings("1[mm]^(1 + 1)"),
        refused("a dimensioned base takes a literal exponent")
    );
    assert_eq!(findings("if(1, 2, 3)"), refused("if takes a Boolean"));
}

#[test]
fn an_attribute_on_a_design_record_or_reading_a_parameter_needs_the_design_class() {
    let mass = ParameterId::mint("test:design:parameter#mass").unwrap();
    let scoped = |target, values| {
        let mut ir = CadIr::empty();
        ir.set_scope(ContentScope::geometry()).unwrap();
        ir.model.attributes.push(attribute(target, values));
        ir.check_scope().map_err(|violation| violation.to_string())
    };
    let violation = Err(
        "the document's scope excludes design but model arena `attributes` holds a reference into it"
            .to_owned(),
    );
    for target in [
        AttributeTarget::Feature(
            crate::features::FeatureId::mint("test:design:feature#1").unwrap(),
        ),
        AttributeTarget::Sketch(crate::sketches::SketchId::mint("test:design:sketch#1").unwrap()),
        AttributeTarget::Parameter(mass.clone()),
    ] {
        assert_eq!(scoped(target, Vec::new()), violation);
    }
    assert_eq!(
        scoped(
            AttributeTarget::Document,
            expression_value(Expression::Neutral(Neutral(Node::Reference(reference(
                &mass
            )))))
        ),
        violation
    );
    assert_eq!(
        scoped(
            AttributeTarget::Document,
            expression_value(Expression::Neutral(Neutral(Node::Constant(Constant::Pi))))
        ),
        Ok(())
    );
    assert_eq!(scoped(AttributeTarget::Document, vec![None]), Ok(()));
}
