// SPDX-License-Identifier: Apache-2.0
//! The parameter, expression, program, dimension, tolerance, and cache rules
//! of part 10 on variations of the fixture document.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use crate::design::configuration::{DefinitionOverride, PartNumber};
use crate::design::construction::Construction;
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{
    parse, Expression, ExpressionDialect, ExpressionToken, Measure, MeasureKind, MeasureTarget,
    Neutral,
};
use crate::design::feature::{FeatureOperation, FuzzyTolerance};
use crate::design::ops::shared::{Law, Limit, Span};
use crate::design::ops::sketch::{SketchConstraint, SketchRelation};
use crate::design::ops::surface::{CurveDefinition, CurveFeature, EquationCoordinates};
use crate::design::ops::swept::Extrude;
use crate::design::options::SolidOutput;
use crate::design::parameter::{
    Deviations, ParameterDefinition, ParameterOwner, ParameterValue, Tolerance,
};
use crate::design::program::{ParameterProgram, ProgramBody, ProgramStage, Statement};
use crate::design::selection::{Anchor, OccurrencePath, Picks, Selection};
use crate::design::spreadsheet::Spreadsheet;
use crate::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::pmi::{FitZone, LimitsAndFits};
use crate::process::groups::{ProcessDefinition, ProcessOwner, ProcessParameter};
use crate::products::{
    AngularGeneral, ExternalDocument, GeneralTolerance, LinearGeneral, LinearGeneralClass,
    ProductDefinition, Storage,
};
use crate::report::Severity;
use crate::scalar::{FiniteReal, Length};
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
fn a_parameter_a_sketch_dimension_binds_needs_no_name() {
    let mut ir = with_width();
    ir.design.parameters[0].name = None;
    assert_finding(&ir, "states no name but is no spreadsheet cell's parameter");
    ir.design.sketch_constraints.push(SketchConstraint {
        id: id("sketch-constraint", "width"),
        sketch: id("sketch", "profile"),
        relation: SketchRelation::Weight {
            pole: id("sketch-entity", "pole"),
            value: Dimension::Parameter(id("parameter", "width")),
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
            .any(|finding| finding.message.contains("states no name")),
        "{findings:#?}"
    );
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
    ir.design.parameters[0].definition = ParameterDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::Fusion,
        tokens: Vec::new(),
    });
    assert_no_finding_containing(&ir, "source expression");
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

/// `with_width`, a Boolean parameter `flag`, and the fixture's extrude made
/// an extrude whose depth is `depth`.
fn extruding(depth: Dimension<Length>) -> CadIr {
    let mut ir = with_width();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.definition = expression("false", &[]);
    flag.value = Some(ParameterValue::Boolean(false));
    ir.design.parameters.push(flag);
    ir.design.features[0].definition.operation = FeatureOperation::Extrude(Box::new(Extrude {
        profile: Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        },
        loops: None,
        direction: None,
        origin: None,
        span: Some(Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::Amount(depth)),
        }),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: None,
            boolean: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

#[test]
fn a_feature_dimension_binds_a_parameter_of_its_value_type() {
    let ir = extruding(Dimension::Parameter(id("parameter", "flag")));
    assert_finding(
        &ir,
        "a length dimension binds test:design:parameter#flag, whose kind is Boolean",
    );
    let mut ir = with_width();
    ir.design.features[0].definition.fuzzy_tolerance = Some(FuzzyTolerance::Explicit(
        Dimension::Negated(id("parameter", "width")),
    ));
    assert_finding(&ir, "a positive length dimension is `negated`");
    let mut ir = with_width();
    ir.design.parameters[0].definition = expression("-2[mm]", &[]);
    ir.design.parameters[0].value = Some(number(-2.0));
    ir.design.features[0].definition.fuzzy_tolerance = Some(FuzzyTolerance::Explicit(
        Dimension::Parameter(id("parameter", "width")),
    ));
    assert_warning(
        &ir,
        "a positive length dimension binds test:design:parameter#width, whose cached value",
    );
}

#[test]
fn feature_dimensions_of_their_parameters_value_type_break_no_rule() {
    let mut ir = with_width();
    ir.design.features[0].definition.fuzzy_tolerance = Some(FuzzyTolerance::Explicit(
        Dimension::Parameter(id("parameter", "width")),
    ));
    assert_no_design_finding(&ir);
    let ir = extruding(Dimension::Negated(id("parameter", "width")));
    let findings = design_findings(&ir);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains("dimension")),
        "{findings:#?}"
    );
}

#[test]
fn an_unset_parameter_has_no_value() {
    let mut ir = with_width();
    ir.design.parameters[0].definition = ParameterDefinition::Unset;
    assert_finding(&ir, "an `Unset` parameter states a `value`");
    ir.design.parameters[0].value = None;
    assert_no_design_finding(&ir);
}

#[test]
fn fit_designations_are_capital_letters_and_a_grade() {
    let mut ir = with_width();
    ir.design.parameters[0].tolerance = Some(Tolerance::Fit {
        fit: LimitsAndFits {
            form_variance: "h".into(),
            zone: FitZone::Shaft,
            grade: String::new(),
            source: None,
        },
        mate: None,
        deviations: None,
    });
    assert_finding(&ir, "is not capital letters only");
    assert_finding(&ir, "states an empty grade");
}

#[test]
fn a_general_tolerance_needs_a_rule_and_an_entry() {
    let mut ir = with_width();
    ir.design.parameters[0].tolerance = Some(Tolerance::General);
    assert_finding(&ir, "states no general tolerance rule for its quantity");
    let part = id::<ProductDefinitionId>("definition", "part");
    let definition = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == part)
        .unwrap();
    definition.general_tolerance = Some(GeneralTolerance {
        linear: Some(LinearGeneral::Iso2768(LinearGeneralClass::Medium)),
        angular: None,
    });
    assert_no_design_finding(&ir);
    ir.design.parameters[0].definition = expression("0.25[mm]", &[]);
    ir.design.parameters[0].value = Some(number(0.25));
    assert_finding(&ir, "states no entry for it");
}

#[test]
fn names_of_documents_and_external_parameters_need_storage() {
    let mut ir = with_width();
    let external = ParameterRef {
        target: RefTarget::External {
            definition: id("definition", "part"),
            name: NonBlankString::new("d0").unwrap(),
        },
        through: None,
    };
    ir.design.parameters[0].definition = expression("$0", &[external]);
    assert_finding(&ir, "is no external definition");
    let mut ir = with_width();
    ir.design.configurations[1].overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            source_name: Some("part".to_owned()),
            part_number: Some(PartNumber::DocumentName),
            ..DefinitionOverride::default()
        },
    );
    assert_finding(&ir, "but no document of its own stores it");
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

#[track_caller]
fn assert_no_finding_containing(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains(fragment)),
        "a design finding contains {fragment:?}: {findings:#?}"
    );
}

/// `with_width`, where `width` is assigned by the program `rules`, which
/// assigns it `2[mm]`.
fn with_assigned_width() -> CadIr {
    let mut ir = with_width();
    ir.design.parameters[0].definition = ParameterDefinition::Assigned(id("program", "rules"));
    ir.design.programs.push(program(
        "rules",
        vec![Statement::Assign {
            target: reference("width"),
            value: neutral("2[mm]", &[]),
        }],
    ));
    ir
}

#[test]
fn programs_assign_through_paths_and_external_names() {
    let mut ir = with_assigned_width();
    let ProgramBody::Neutral(statements) = &mut ir.design.programs[0].body else {
        unreachable!()
    };
    statements[0] = Statement::Assign {
        target: ParameterRef {
            target: RefTarget::Parameter(id("parameter", "width")),
            through: Some(OccurrencePath(Vec::new())),
        },
        value: neutral("2[mm]", &[]),
    };
    assert_no_finding_containing(&ir, "program assigns");
    ir.design.parameters[0].definition = expression("2[mm]", &[]);
    assert_finding(
        &ir,
        "program assigns test:design:parameter#width through an occurrence path, whose definition is not `assigned`",
    );

    let mut ir = with_width();
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "unread"),
        Storage::External(ExternalDocument::path("unread.prt")),
    ));
    ir.design.programs.push(program(
        "rules",
        vec![Statement::Assign {
            target: ParameterRef {
                target: RefTarget::External {
                    definition: id("definition", "unread"),
                    name: NonBlankString::new("d0").unwrap(),
                },
                through: None,
            },
            value: neutral("4[mm]", &[]),
        }],
    ));
    assert_finding(
        &ir,
        "program assigns d0 of the external definition test:design:definition#unread, which the document does not hold; its body is `source`",
    );

    let unread = ParameterRef {
        target: RefTarget::External {
            definition: id("definition", "unread"),
            name: NonBlankString::new("d1").unwrap(),
        },
        through: None,
    };
    ir.design.programs[0].body = ProgramBody::Neutral(vec![Statement::If {
        condition: neutral("$0 > 1[mm]", std::slice::from_ref(&unread)),
        then: vec![Statement::Assign {
            target: reference("width"),
            value: neutral("$0 * 2", &[unread]),
        }],
        otherwise: Vec::new(),
    }]);
    assert_finding(
        &ir,
        "program reads d1 of the external definition test:design:definition#unread in a condition",
    );
    assert_finding(&ir, "program reads d1 of the external definition test:design:definition#unread in an assigned value");
    ir.design.programs[0].body = ProgramBody::Source {
        dialect: ExpressionDialect::Creo,
        tokens: vec![
            ExpressionToken::Reference(ParameterRef {
                target: RefTarget::External {
                    definition: id("definition", "unread"),
                    name: NonBlankString::new("d0").unwrap(),
                },
                through: None,
            }),
            ExpressionToken::Text(" = 4".to_owned()),
        ],
    };
    assert_no_finding_containing(&ir, "program");
}

#[test]
fn assigned_caches_are_compared() {
    let mut ir = with_assigned_width();
    let mut double = parameter("double", LENGTH);
    double.definition = expression("$0 * 2", &[reference("width")]);
    double.value = Some(number(5.0));
    ir.design.parameters.push(double);
    assert_warning(&ir, "disagrees with its expression's value");
    ir.design.parameters[1].value = Some(number(4.0));
    assert_no_design_finding(&ir);

    let ProgramBody::Neutral(statements) = &mut ir.design.programs[0].body else {
        unreachable!()
    };
    let measure = Measure {
        kind: MeasureKind::Length,
        of: MeasureTarget::Model,
        cached: None,
    };
    statements[0] = Statement::Assign {
        target: reference("width"),
        value: Neutral(parse("@0", &[], &[measure]).unwrap()),
    };
    ir.design.parameters[1].value = Some(number(5.0));
    assert_no_finding_containing(&ir, "disagrees");
}

fn process_parameter(
    key: &str,
    kind: Option<ValueKind>,
    text: &str,
    refs: &[ParamRef],
) -> ProcessParameter {
    ProcessParameter {
        id: id("process-parameter", key),
        owner: ProcessOwner::Setup(id("setup", "job")),
        name: NonBlankString::new(key).unwrap(),
        kind,
        definition: ProcessDefinition::Expression(Expression::Neutral(Neutral(
            parse(text, refs, &[]).unwrap(),
        ))),
        value: None,
        native_ref: None,
    }
}

#[test]
fn process_expressions_type_check() {
    let mut ir = with_width();
    ir.process.parameters.push(process_parameter(
        "depth",
        Some(LENGTH),
        "1[mm] + $0",
        &[ParamRef::Design(reference("width"))],
    ));
    assert_no_finding_containing(&ir, "process-parameter");
    ir.process.parameters.push(process_parameter(
        "bad",
        Some(LENGTH),
        "1[mm] + 1[deg]",
        &[],
    ));
    assert_finding(&ir, "expression does not type-check");
    ir.process.parameters[1] = process_parameter("bad", Some(LENGTH), "true", &[]);
    assert_finding(&ir, "expression's root does not have the parameter's kind");
    ir.process.parameters[1] = process_parameter(
        "bad",
        None,
        "$0 + 1[deg]",
        &[ParamRef::Process(id("process-parameter", "depth"))],
    );
    assert_finding(&ir, "expression does not type-check");
    ir.process.parameters[1] = process_parameter("bad", None, "3 * 2", &[]);
    assert_finding(
        &ir,
        "process parameter has no kind, so its expression is `source`",
    );
    ir.process.parameters[1].definition = ProcessDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::FreeCad,
        tokens: vec![ExpressionToken::Text("3 * 2".to_owned())],
    });
    assert_no_finding_containing(&ir, "process-parameter");
    ir.process.parameters.push(process_parameter(
        "reads",
        Some(LENGTH),
        "$0 * 1[mm]",
        &[ParamRef::Process(id("process-parameter", "bad"))],
    ));
    assert_finding(
        &ir,
        "neutral expression references test:design:process-parameter#bad, a process parameter with no kind",
    );
}

fn equation_curve(x: &str) -> FeatureOperation {
    let law = |text: &str| {
        Law::Expression(Expression::Neutral(Neutral(
            parse::<ParameterRef>(text, &[], &[]).unwrap(),
        )))
    };
    FeatureOperation::Curve(Box::new(CurveFeature {
        definition: CurveDefinition::Equation {
            frame: Construction {
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: None,
                resolved_in: BTreeMap::new(),
            },
            coordinates: EquationCoordinates::Cartesian {
                x: law(x),
                y: law("0[mm]"),
                z: law("0[mm]"),
            },
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn law_expressions_type_check() {
    let mut ir = with_width();
    ir.design.features[0].definition.operation = equation_curve("t * 10[mm]");
    assert_no_finding_containing(&ir, "expression");
    ir.design.features[0].definition.operation = equation_curve("t + 1[mm]");
    assert_finding(&ir, "expression does not type-check");
    ir.design.features[0].definition.operation = equation_curve("t > 0.5");
    assert_finding(&ir, "a law's value is a number");
    ir.design.features[0].definition.operation = equation_curve("bendangle * 1[mm]");
    assert_finding(&ir, "a bend variable occurs only in a bend allowance");
    ir.design.parameters[0].definition = expression("t * 1[mm]", &[]);
    ir.design.features[0].definition.operation = equation_curve("t * 10[mm]");
    assert_finding(&ir, "the law parameter occurs only in a law expression");
}

#[test]
fn a_definitions_general_tolerance_is_checked() {
    let mut ir = with_width();
    let part = id::<ProductDefinitionId>("definition", "part");
    let index = ir
        .model
        .product_definitions
        .iter()
        .position(|definition| definition.id == part)
        .unwrap();
    let deviations = |lower: f64, upper: f64| Deviations {
        lower: FiniteReal::new(lower).unwrap(),
        upper: FiniteReal::new(upper).unwrap(),
    };
    ir.model.product_definitions[index].general_tolerance = Some(GeneralTolerance {
        linear: Some(LinearGeneral::Uniform(deviations(-0.1, 0.2))),
        angular: Some(AngularGeneral::ByDecimals(BTreeMap::from([(
            1,
            deviations(-0.01, 0.01),
        )]))),
    });
    assert_no_design_finding(&ir);
    ir.model.product_definitions[index].general_tolerance = Some(GeneralTolerance {
        linear: Some(LinearGeneral::Uniform(deviations(0.2, -0.1))),
        angular: Some(AngularGeneral::ByDecimals(BTreeMap::new())),
    });
    assert_finding(
        &ir,
        "general tolerance's linear lower deviation 0.2 is not below its upper deviation -0.1",
    );
    assert_finding(
        &ir,
        "general tolerance's angular `by_decimals` rule holds no entry",
    );
    ir.model.product_definitions[index].general_tolerance = Some(GeneralTolerance {
        linear: None,
        angular: None,
    });
    assert_finding(
        &ir,
        "general tolerance states neither a linear nor an angular rule",
    );
}

/// Part 10: a `through` path resolves from the referencing scope's
/// definition. The part places `pin` by occurrence `a`; `pin` places itself
/// again by occurrence `nested`, which the part does not own.
#[test]
fn a_through_path_resolves_from_the_referencing_scope() {
    use crate::design::selection::{PathStep, StepRef};
    use crate::products::Occurrence;
    const FRAGMENT: &str = "is not an occurrence of the previous step's prototype";
    let through = |first: &str| {
        let mut ir = with_width();
        let pin = id::<ProductDefinitionId>("definition", "pin");
        let mut definition = ProductDefinition::new(pin.clone(), Storage::Embedded);
        definition.occurrences.push(id("occurrence", "nested"));
        ir.model.product_definitions.push(definition);
        ir.model
            .product_definitions
            .sort_by(|left, right| left.id.cmp(&right.id));
        let part = id::<ProductDefinitionId>("definition", "part");
        ir.model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id == part)
            .unwrap()
            .occurrences
            .push(id("occurrence", "a"));
        ir.model.occurrences = vec![
            Occurrence::new(id("occurrence", "a"), pin.clone(), None),
            Occurrence::new(id("occurrence", "nested"), pin.clone(), None),
        ];
        let mut bolt = parameter("bolt", LENGTH);
        bolt.scope = pin;
        ir.design.parameters.push(bolt);
        let reference = ParameterRef {
            target: RefTarget::Parameter(id("parameter", "bolt")),
            through: Some(OccurrencePath(vec![PathStep {
                occurrence: StepRef::Occurrence(id("occurrence", first)),
                configuration: None,
            }])),
        };
        ir.design.parameters[0].definition = expression("$0", &[reference]);
        ir.design
            .parameters
            .sort_by(|left, right| left.id.cmp(&right.id));
        ir
    };
    let found = |ir: &CadIr| {
        design_findings(ir)
            .into_iter()
            .any(|finding| finding.message.contains(FRAGMENT))
    };
    assert!(!found(&through("a")));
    assert_finding(&through("nested"), FRAGMENT);
}

/// The fixture with an `External` definition `lib`, and a reference to its
/// parameter `size`.
fn with_library() -> (CadIr, ParameterRef) {
    let mut ir = with_width();
    let library = id::<ProductDefinitionId>("definition", "lib");
    ir.model.product_definitions.push(ProductDefinition::new(
        library.clone(),
        Storage::External(ExternalDocument::default()),
    ));
    ir.model
        .product_definitions
        .sort_by(|left, right| left.id.cmp(&right.id));
    let reference = ParameterRef {
        target: RefTarget::External {
            definition: library,
            name: NonBlankString::new("size").unwrap(),
        },
        through: None,
    };
    (ir, reference)
}

const EXTERNAL_PLACE: &str =
    "an External reference is not the whole tree of an expression that states a design parameter's value";

#[test]
fn an_external_reference_is_the_whole_tree_of_a_parameter_value() {
    let (mut ir, reference) = with_library();
    ir.design.parameters[0].definition = expression("$0", std::slice::from_ref(&reference));
    assert!(design_findings(&ir)
        .iter()
        .all(|finding| !finding.message.contains(EXTERNAL_PLACE)));
    ir.design.parameters[0].definition = expression("$0 + 1[mm]", &[reference]);
    assert_finding(&ir, EXTERNAL_PLACE);
}

#[test]
fn parameters_naming_one_external_parameter_have_one_kind() {
    const FRAGMENT: &str = "with another kind";
    let (mut ir, reference) = with_library();
    ir.design.parameters[0].definition = expression("$0", std::slice::from_ref(&reference));
    let mut depth = parameter("depth", LENGTH);
    depth.definition = expression("$0", std::slice::from_ref(&reference));
    ir.design.parameters.insert(0, depth);
    assert!(design_findings(&ir)
        .iter()
        .all(|finding| !finding.message.contains(FRAGMENT)));
    ir.design.parameters[0].kind = ValueKind::Quantity(Quantity::ANGLE);
    assert_finding(&ir, FRAGMENT);
}
