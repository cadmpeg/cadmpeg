// SPDX-License-Identifier: Apache-2.0
//! The parameter, expression, program, dimension, tolerance, and cache rules
//! of part 10 on variations of the fixture document.

use std::collections::BTreeMap;

use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::expr::{
    parse, Expression, ExpressionDialect, ExpressionToken, Measure, MeasureKind, MeasureTarget,
    Neutral,
};
use crate::design::feature::FeatureOperation;
use crate::design::ops::sketch::{SketchConstraint, SketchRelation};
use crate::design::parameter::{ParameterDefinition, ParameterOwner, ParameterValue, Tolerance};
use crate::design::program::{ParameterProgram, ProgramBody, ProgramStage, Statement};
use crate::design::selection::{Anchor, Picks, Selection};
use crate::design::spreadsheet::Spreadsheet;
use crate::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use crate::document::CadIr;
use crate::pmi::LimitsAndFits;
use crate::report::Severity;
use crate::scalar::FiniteReal;
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, design_findings, document, feature, id, parameter,
    unresolved,
};

const LENGTH: ValueKind = ValueKind::Quantity(Quantity::LENGTH);

fn reference(key: &str) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(id("parameter", key)),
        through: None,
    }
}

fn neutral(text: &str, refs: &[ParameterRef]) -> Neutral {
    Neutral(parse(text, refs, &[]).unwrap())
}

fn expression(text: &str, refs: &[ParameterRef]) -> ParameterDefinition {
    ParameterDefinition::Expression(Expression::Neutral(neutral(text, refs)))
}

fn number(value: f64) -> ParameterValue {
    ParameterValue::Number(FiniteReal::new(value).unwrap())
}

/// The fixture with one literal length parameter, `width`, of 2 mm.
fn with_width() -> CadIr {
    let mut ir = document();
    let mut width = parameter("width", LENGTH);
    width.definition = expression("2[mm]", &[]);
    width.value = Some(number(2.0));
    ir.design.parameters.push(width);
    ir
}

#[track_caller]
fn assert_warning(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings
            .iter()
            .any(|finding| finding.severity == Severity::Warning
                && finding.message.contains(fragment)),
        "no design warning containing {fragment:?} in {findings:#?}"
    );
}

#[test]
fn a_literal_parameter_breaks_no_rule() {
    assert_no_design_finding(&with_width());
}

#[test]
fn names_are_unique_per_scope_and_owner() {
    let mut ir = with_width();
    let mut again = ir.design.parameters[0].clone();
    again.id = id("parameter", "again");
    ir.design.parameters.push(again.clone());
    assert_finding(
        &ir,
        "parameters of the scope with no owner are named \"width\"",
    );
    again.owner = Some(ParameterOwner::Feature(feature("extrude")));
    ir.design.parameters[1] = again;
    assert_no_design_finding(&ir);
    ir.design.parameters[1].name = None;
    assert_finding(&ir, "states no name but is no spreadsheet cell's parameter");
}

#[test]
fn owners_belong_to_the_scope() {
    let mut ir = with_width();
    ir.design.parameters[0].owner = Some(ParameterOwner::Feature(feature("missing")));
    assert_finding(&ir, "is no feature of the design");
    ir.design.parameters[0].owner = Some(ParameterOwner::Container(id("container", "timeline")));
    assert_no_design_finding(&ir);
    ir.design.parameters[0].owner = Some(ParameterOwner::Attribute(id("attribute", "missing")));
    assert_finding(
        &ir,
        "is no model attribute with a value naming the parameter",
    );
}

#[test]
fn values_and_units_follow_the_kind() {
    let mut ir = with_width();
    ir.design.parameters[0].value = Some(ParameterValue::Boolean(true));
    assert_finding(&ir, "does not follow its kind");
    let mut ir = with_width();
    ir.design.parameters[0].unit = Some(Unit(vec![UnitFactor {
        symbol: UnitSymbol::Deg,
        power: 1.try_into().unwrap(),
    }]));
    assert_finding(&ir, "parameter's unit has the quantity");
    ir.design.parameters[0].unit = Some(Unit(vec![UnitFactor {
        symbol: UnitSymbol::In,
        power: 1.try_into().unwrap(),
    }]));
    assert_no_design_finding(&ir);
}

#[test]
fn each_sheet_is_named_by_one_feature_that_owns_its_cells() {
    let mut ir = with_width();
    let sheet = Spreadsheet::new(
        id("spreadsheet", "sheet"),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
    )
    .unwrap();
    ir.design.spreadsheets.push(sheet);
    assert_finding(
        &ir,
        "spreadsheet is named by no feature's spreadsheet operation",
    );
    let mut ir = with_width();
    let mut cells = unresolved(
        "cells",
        crate::design::feature::OperationFamily::Spreadsheet,
    );
    cells.definition.operation = FeatureOperation::Spreadsheet(id("spreadsheet", "missing"));
    ir.design.features.push(cells);
    assert_finding(&ir, "is no spreadsheet of the design");
}

fn program(key: &str, body: Vec<Statement>) -> ParameterProgram {
    ParameterProgram {
        id: id("program", key),
        scope: id("definition", "part"),
        owner: None,
        stage: None,
        name: None,
        body: ProgramBody::Neutral(body),
        native_ref: None,
    }
}

#[test]
fn programs_assign_exactly_their_parameters() {
    let mut ir = with_width();
    ir.design.parameters[0].definition = ParameterDefinition::Assigned(id("program", "missing"));
    assert_finding(&ir, "which is no program of the design");
    let mut ir = with_width();
    ir.design.parameters[0].definition = ParameterDefinition::Assigned(id("program", "rules"));
    ir.design.programs.push(program("rules", Vec::new()));
    assert_finding(
        &ir,
        "program assigns test:design:parameter#width on no path",
    );
    ir.design.programs[0] = program(
        "rules",
        vec![Statement::If {
            condition: neutral("true", &[]),
            then: vec![Statement::Assign {
                target: reference("width"),
                value: neutral("3[mm]", &[]),
            }],
            otherwise: Vec::new(),
        }],
    );
    assert_no_design_finding(&ir);
    ir.design.programs[0].owner = Some(feature("extrude"));
    ir.design.programs[0].stage = Some(ProgramStage::Initial);
    assert_finding(&ir, "program has an owner and states the stage");
    let mut ir = with_width();
    ir.design.programs.push(program(
        "rules",
        vec![Statement::Assign {
            target: reference("width"),
            value: neutral("true", &[]),
        }],
    ));
    assert_finding(&ir, "whose definition is not `assigned` by this program");
    assert_finding(&ir, "expression's root does not have the parameter's kind");
}

#[test]
fn expressions_type_check_and_resolve() {
    let mut ir = with_width();
    ir.design.parameters[0].definition = expression("1[mm] + 1[deg]", &[]);
    assert_finding(&ir, "expression does not type-check");
    ir.design.parameters[0].definition = expression("$0 * 2", &[reference("missing")]);
    assert_finding(&ir, "which is no parameter of the design");
    ir.design.parameters[0].definition = ParameterDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::Fusion,
        tokens: vec![ExpressionToken::Reference(reference("width"))],
    });
    assert_finding(&ir, "source expression holds no text token");
    let mut ir = with_width();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.definition = expression("$0", &[reference("width")]);
    ir.design.parameters.push(flag);
    assert_finding(&ir, "expression's root does not have the parameter's kind");
}

#[test]
fn measures_are_anchored_and_sized() {
    let mut ir = with_width();
    let measure = Measure {
        kind: MeasureKind::Distance,
        of: MeasureTarget::Selection(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        }),
        cached: None,
    };
    ir.design.parameters[0].definition = ParameterDefinition::Expression(Expression::Neutral(
        Neutral(parse("@0", &[], &[measure]).unwrap()),
    ));
    assert_finding(
        &ir,
        "anchored `input` outside a feature-owned parameter's expression",
    );
    ir.design.parameters[0].owner = Some(ParameterOwner::Feature(feature("extrude")));
    assert_no_design_finding(&ir);
}

#[test]
fn reference_cycles_are_warnings() {
    let mut ir = with_width();
    ir.design.parameters[0].definition = expression("$0 + 1[mm]", &[reference("width")]);
    ir.design.parameters[0].value = None;
    assert_warning(&ir, "reference each other in a cycle");
    assert!(design_findings(&ir)
        .iter()
        .all(|finding| finding.severity == Severity::Warning));
}

#[test]
fn dimensions_bind_parameters_of_their_scope() {
    let mut ir = with_width();
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "missing")));
    assert_finding(
        &ir,
        "a dimension binds test:design:parameter#missing, which is no parameter",
    );
    let mut ir = with_width();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.scope = id("definition", "other");
    flag.definition = expression("false", &[]);
    ir.design.parameters.push(flag);
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "flag")));
    assert_finding(&ir, "not of its owner's scope");
}

#[test]
fn driven_parameters_are_bound() {
    let mut ir = with_width();
    ir.design.parameters.push(parameter("measured", LENGTH));
    assert_warning(&ir, "driven parameter is bound by no dimension");
    ir.design.sketch_constraints.push(SketchConstraint {
        id: id("sketch-constraint", "measured"),
        sketch: id("sketch", "profile"),
        relation: SketchRelation::Weight {
            pole: id("sketch-entity", "pole"),
            value: Dimension::Negated(id("parameter", "measured")),
        },
        name: None,
        active: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    });
    let findings = design_findings(&ir);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains("bound by no dimension")),
        "{findings:#?}"
    );
    assert_finding(&ir, "a positive real dimension is `negated`");
    assert_finding(&ir, "binds test:design:parameter#measured, whose kind is");
}

#[test]
fn fit_designations_are_capital_letters_and_a_grade() {
    let mut ir = with_width();
    ir.design.parameters[0].tolerance = Some(Tolerance::Fit {
        fit: LimitsAndFits {
            form_variance: "h".into(),
            zone_variance: String::new(),
            grade: String::new(),
            source: String::new(),
        },
        mate: None,
        deviations: None,
    });
    assert_finding(&ir, "is not capital letters only");
    assert_finding(&ir, "states an empty grade");
}

#[test]
fn cached_values_agree_with_their_expressions() {
    let mut ir = with_width();
    ir.design.parameters[0].value = Some(number(3.0));
    assert_warning(&ir, "disagrees with its expression's value");
    let mut ir = with_width();
    let mut double = parameter("double", LENGTH);
    double.definition = expression("$0 * 2", &[reference("width")]);
    double.value = Some(number(4.0));
    ir.design.parameters.push(double);
    assert_no_design_finding(&ir);
    ir.design.parameters[1].value = Some(number(4.5));
    assert_warning(&ir, "disagrees with its expression's value");
    let mut ir = with_width();
    let mut count = parameter("count", ValueKind::Integer);
    count.definition = expression("7 / 2", &[]);
    ir.design.parameters.push(count);
    assert_warning(&ir, "evaluates to 3.5, not a whole number");
}
