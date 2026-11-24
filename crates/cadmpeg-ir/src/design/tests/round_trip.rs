// SPDX-License-Identifier: Apache-2.0
//! Every design type, written, read back strictly, and written again; the two
//! texts are equal.

use std::num::NonZeroI8;

use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fixtures::{
    self, angle, container, feature, id, length, positive_length, real, selections, text,
};
use super::round_trip;
use crate::design::construction::{kinds, FrameAxis, InputRole, Relation, Subject};
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{
    AddOp, BendVariable, BinaryOp, Constant, ExpressionToken, Function, Measure, MeasureKind,
    MeasureTarget, MulOp, Neutral, Node, UnaryOp,
};
use crate::design::names::{InstanceIndex, NativeName, NativeNameValue};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, CornerRound, Hand, LimitOffset, LoopNesting, NativeOption,
    ProjectionDirection, SectionCondition, SheetOutput, SolidOutput, Solidify, Support,
    SymmetricMeasure, Targets, Thickness, ThinEnds, ThinWall, WallSide,
};
use crate::design::selection::{
    DatumPart, Fragment, OccurrencePath, Ordered, PathStep, StepRef, Unordered,
};
use crate::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, UnitSystem, ValueKind};
use crate::scalar::{Angle, FiniteReal, Length};

#[derive(Default)]
struct Report {
    failures: Vec<String>,
}

impl Report {
    fn check<T: Serialize + DeserializeOwned>(&mut self, name: &str, samples: Vec<T>) {
        assert!(!samples.is_empty(), "{name} has no sample");
        for sample in samples {
            if let Err(error) = round_trip(&sample) {
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }
}

fn native_options() -> Vec<NativeOption> {
    vec![
        NativeOption::Bool(Dimension::Literal(true)),
        NativeOption::Integer(Dimension::Literal(-3)),
        NativeOption::Real(Dimension::Literal(real(0.5))),
        NativeOption::Length(Dimension::Parameter(id("parameter", 1))),
        NativeOption::Angle(Dimension::Negated(id("parameter", 2))),
        NativeOption::Text(Dimension::Literal(text("blind"))),
        NativeOption::Selection(selections().remove(0)),
        NativeOption::OrderedSelection(selections().remove(1)),
    ]
}

fn thin_walls() -> Vec<ThinWall> {
    vec![
        ThinWall {
            thickness: Thickness::OneSide {
                value: Dimension::Literal(positive_length(1.0)),
                side: Some(WallSide::Inside),
            },
            ends: Some(ThinEnds::Capped(Dimension::Literal(positive_length(0.5)))),
            corner_round: Some(CornerRound::Round(Dimension::Literal(positive_length(0.2)))),
        },
        ThinWall {
            thickness: Thickness::Symmetric(Dimension::Literal(positive_length(2.0))),
            ends: None,
            corner_round: None,
        },
    ]
}

fn booleans() -> Vec<Boolean> {
    vec![
        Boolean::NewBody,
        Boolean::Apply {
            op: BooleanOp::Cut,
            targets: Some(Targets::Selected(selections().remove(0))),
        },
        Boolean::Apply {
            op: BooleanOp::Join,
            targets: None,
        },
    ]
}

fn section_conditions() -> Vec<SectionCondition> {
    vec![
        SectionCondition::Free {
            support: Some(Support::Computed),
        },
        SectionCondition::Default,
        SectionCondition::Direction {
            direction: fixtures::direction_constructions().remove(0),
            angle: Some(Dimension::Literal(angle(0.1))),
            weight: None,
        },
        SectionCondition::NormalToProfile {
            angle: None,
            weight: None,
        },
        SectionCondition::Tangent {
            support: Some(Support::Faces(selections().remove(0))),
            weight: None,
            flip: Some(Dimension::Literal(false)),
        },
        SectionCondition::Curvature {
            support: None,
            weight: None,
            flip: None,
        },
        SectionCondition::Native(text("catia:mean")),
    ]
}

fn units() -> Vec<Unit> {
    let factor = |symbol, power| UnitFactor {
        symbol,
        power: NonZeroI8::new(power).unwrap(),
    };
    vec![
        Unit::default(),
        fixtures::mm(),
        Unit(vec![factor(UnitSymbol::In, 1), factor(UnitSymbol::Min, -1)]),
    ]
}

fn measures() -> Vec<Measure> {
    let kinds = [
        MeasureKind::Length,
        MeasureKind::Area,
        MeasureKind::Volume,
        MeasureKind::Mass,
        MeasureKind::Distance,
        MeasureKind::Angle,
        MeasureKind::Radius,
        MeasureKind::Coordinate(FrameAxis::Z),
    ];
    let mut out = kinds
        .into_iter()
        .map(|kind| Measure {
            kind,
            of: MeasureTarget::Model,
            cached: None,
        })
        .collect::<Vec<_>>();
    out.push(fixtures::measure());
    out
}

fn process_nodes() -> Vec<Node<ParamRef>> {
    vec![
        Node::Reference(ParamRef::Process(id("process-parameter", 1))),
        Node::Unary {
            op: UnaryOp::Negate,
            operand: Box::new(Node::Reference(ParamRef::Design(fixtures::parameter_ref(
                1,
            )))),
        },
    ]
}

#[test]
fn every_design_type_round_trips() {
    let mut report = Report::default();
    report.check("ConfigurationRef", fixtures::configuration_refs());
    report.check("Member", fixtures::members());
    report.check("Position", fixtures::positions());
    report.check("Evaluated", fixtures::evaluated());
    report.check("EntityRef", fixtures::entity_refs());
    report.check("RecordRef", fixtures::record_refs());
    report.check("NameTable", fixtures::name_tables());
    report.check("NameEntry", fixtures::name_entries());
    report.check("NativeName", vec![fixtures::native_name()]);
    report.check("NativeNameValue", fixtures::native_name_values());
    report.check("TopoName", fixtures::topo_names());
    report.check("NameKind", fixtures::name_kinds());
    report.check("Generator", fixtures::generators());
    report.check("GenerationRole", fixtures::generation_roles());
    report.check("HoleFaceRole", fixtures::hole_face_roles());
    report.check("Selection<Ordered>", fixtures::selections::<Ordered>());
    report.check("Selection<Unordered>", fixtures::selections::<Unordered>());
    report.check("Anchor", fixtures::anchors());
    report.check("Picks<Ordered>", fixtures::picks::<Ordered>());
    report.check("Picks<Unordered>", fixtures::picks::<Unordered>());
    report.check("Picked", fixtures::picks_full());
    report.check("PickTarget", fixtures::pick_targets());
    report.check("NativeOwner", fixtures::native_owners());
    report.check("PickContext", fixtures::pick_contexts());
    report.check("DefinitionRef", vec![fixtures::definition_ref()]);
    report.check("OccurrencePath", vec![fixtures::occurrence_path()]);
    report.check(
        "PathStep",
        vec![PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", 2)),
            configuration: None,
        }],
    );
    report.check(
        "StepRef",
        vec![
            StepRef::Occurrence(id("occurrence", 1)),
            StepRef::Native(fixtures::native_name()),
        ],
    );
    report.check("PrincipalEntity", fixtures::principal_entities());
    report.check(
        "DatumPart",
        vec![
            DatumPart::Principal(fixtures::principal_entities()[0]),
            DatumPart::Point(2),
        ],
    );
    report.check("Witness", fixtures::witnesses());
    report.check("PickDomain", fixtures::pick_domains());
    report.check("RuleTerm<Ordered>", fixtures::rule_terms::<Ordered>());
    report.check("RuleTerm<Unordered>", fixtures::rule_terms::<Unordered>());
    report.check("SelectionRule", fixtures::selection_rules());
    report.check("Propagation", fixtures::propagations());
    report.check("Continuity", fixtures::continuities());
    report.check(
        "Construction<kinds::Point>",
        fixtures::point_constructions(),
    );
    report.check(
        "Construction<kinds::Direction>",
        fixtures::direction_constructions(),
    );
    report.check(
        "Construction<kinds::Axis>",
        fixtures::constructions::<kinds::Axis>(fixtures::axis_value(), fixtures::axis_value()),
    );
    report.check(
        "Construction<kinds::Plane>",
        fixtures::plane_constructions(),
    );
    report.check(
        "Construction<kinds::Frame>",
        fixtures::constructions::<kinds::Frame>(fixtures::frame_value(), fixtures::frame_value()),
    );
    let mut curves = fixtures::curve_values();
    let circle = curves.remove(2);
    let segment = curves.remove(0);
    report.check(
        "Construction<kinds::Curve>",
        fixtures::constructions::<kinds::Curve>(segment, circle),
    );
    report.check("ConstructionInput", fixtures::construction_inputs());
    report.check("InputReference", fixtures::input_references());
    report.check("AnyConstruction", fixtures::any_constructions());
    report.check(
        "InputRole",
        vec![InputRole {
            subject: Subject::ZAxis,
            relation: Relation::Parallel,
        }],
    );
    report.check("Subject", fixtures::subjects());
    report.check("Relation", fixtures::relations());
    report.check("ScalarDimension", fixtures::scalar_dimensions());
    report.check("PostStep", fixtures::post_steps());
    report.check("FrameAxis", vec![FrameAxis::X, FrameAxis::Y, FrameAxis::Z]);
    report.check("SourceMode", vec![fixtures::source_mode()]);
    report.check("AxisValue", vec![fixtures::axis_value()]);
    report.check("PlaneValue", vec![fixtures::plane_value()]);
    report.check("FrameValue", vec![fixtures::frame_value()]);
    report.check("CurveValue", fixtures::curve_values());
    report.check(
        "ValueKind",
        vec![
            ValueKind::Quantity(Quantity::LENGTH),
            ValueKind::Integer,
            ValueKind::Boolean,
            ValueKind::Text,
            ValueKind::Native(text("creo:yes_no")),
        ],
    );
    report.check("Quantity", vec![Quantity::AREA, Quantity::ANGLE]);
    report.check("Expression<ParameterRef>", fixtures::expressions());
    report.check("Expression<ParamRef>", fixtures::process_expressions());
    report.check(
        "Neutral<ParameterRef>",
        fixtures::nodes().into_iter().map(Neutral).collect(),
    );
    report.check(
        "Neutral<ParamRef>",
        process_nodes().into_iter().map(Neutral).collect(),
    );
    report.check(
        "ExpressionToken<ParameterRef>",
        vec![
            ExpressionToken::Text("+ 1".to_owned()),
            ExpressionToken::Reference(fixtures::parameter_ref(1)),
        ],
    );
    report.check(
        "ExpressionToken<ParamRef>",
        vec![ExpressionToken::Reference(ParamRef::Process(id(
            "process-parameter",
            1,
        )))],
    );
    report.check("ExpressionDialect", fixtures::dialects());
    report.check("ParameterRef", fixtures::parameter_refs());
    report.check(
        "RefTarget",
        vec![
            RefTarget::Parameter(id("parameter", 1)),
            RefTarget::External {
                definition: id("product-definition", 1),
                name: text("Length"),
            },
        ],
    );
    report.check(
        "ParamRef",
        vec![
            ParamRef::Process(id("process-parameter", 1)),
            ParamRef::Design(fixtures::parameter_ref(2)),
        ],
    );
    report.check("Node<ParameterRef>", fixtures::nodes());
    report.check("Node<ParamRef>", process_nodes());
    report.check("Constant", vec![Constant::Pi, Constant::E]);
    report.check(
        "BendVariable",
        vec![
            BendVariable::Angle,
            BendVariable::InsideRadius,
            BendVariable::Thickness,
        ],
    );
    report.check("UnaryOp", vec![UnaryOp::Negate, UnaryOp::Not]);
    report.check(
        "BinaryOp",
        vec![
            BinaryOp::Power,
            BinaryOp::Equal,
            BinaryOp::NotEqual,
            BinaryOp::Less,
            BinaryOp::LessEqual,
            BinaryOp::Greater,
            BinaryOp::GreaterEqual,
        ],
    );
    report.check("AddOp", vec![AddOp::Add, AddOp::Subtract]);
    report.check("MulOp", vec![MulOp::Multiply, MulOp::Divide]);
    report.check("Function", Function::ALL.to_vec());
    report.check("Unit", units());
    report.check("UnitFactor", units().remove(2).0);
    report.check("UnitSymbol", UnitSymbol::ALL.to_vec());
    report.check("Measure", measures());
    report.check(
        "MeasureKind",
        measures().into_iter().map(|measure| measure.kind).collect(),
    );
    report.check(
        "MeasureTarget",
        vec![MeasureTarget::Model, fixtures::measure().of],
    );
    report.check(
        "Dimension<Length>",
        vec![
            Dimension::<Length>::Literal(length(2.5)),
            Dimension::Parameter(id("parameter", 1)),
            Dimension::Negated(id("parameter", 1)),
        ],
    );
    report.check("Dimension<bool>", vec![Dimension::<bool>::Literal(false)]);
    report.check(
        "Dimension<Angle, ParamRef>",
        vec![
            Dimension::<Angle, ParamRef>::Literal(angle(0.5)),
            Dimension::Parameter(ParamRef::Process(id("process-parameter", 1))),
        ],
    );
    report.check(
        "Dimension<Hand>",
        vec![Dimension::<Hand>::Literal(Hand::Left)],
    );
    report.check(
        "Dimension<NonBlankString>",
        vec![Dimension::<NonBlankString>::Literal(text("M6"))],
    );
    report.check("Dimension<u32>", vec![Dimension::<u32>::Literal(4)]);
    report.check(
        "Dimension<FiniteReal>",
        vec![Dimension::<FiniteReal>::Literal(real(-0.5))],
    );
    report.check(
        "UnitSystem",
        vec![
            UnitSystem {
                length: UnitSymbol::In,
                angle: UnitSymbol::Deg,
                mass: Some(UnitSymbol::Lb),
                time: Some(UnitSymbol::S),
            },
            UnitSystem {
                length: UnitSymbol::Mm,
                angle: UnitSymbol::Rad,
                mass: None,
                time: None,
            },
        ],
    );
    report.check("NativeOption", native_options());
    report.check("Hand", vec![Hand::Right, Hand::Left]);
    report.check(
        "LoopNesting",
        vec![
            LoopNesting::Alternate,
            LoopNesting::OneLevel,
            LoopNesting::Independent,
            LoopNesting::EveryLevel,
        ],
    );
    report.check(
        "SolidOutput",
        vec![
            SolidOutput {
                body: Some(BodyForm::Thin(thin_walls().remove(0))),
                boolean: Some(Boolean::NewBody),
            },
            SolidOutput {
                body: None,
                boolean: None,
            },
        ],
    );
    let mut forms = vec![BodyForm::Solid, BodyForm::Sheet];
    forms.extend(thin_walls().into_iter().map(BodyForm::Thin));
    report.check("BodyForm", forms);
    report.check("ThinWall", thin_walls());
    report.check(
        "CornerRound",
        vec![
            CornerRound::Sharp,
            CornerRound::Round(Dimension::Literal(positive_length(0.1))),
        ],
    );
    report.check(
        "Thickness",
        vec![
            Thickness::OneSide {
                value: Dimension::Literal(positive_length(1.0)),
                side: None,
            },
            Thickness::Symmetric(Dimension::Literal(positive_length(1.0))),
            Thickness::Offsets {
                outside: Dimension::Literal(length(1.0)),
                inside: Dimension::Literal(length(-0.5)),
            },
            Thickness::Native(text("creo:both")),
        ],
    );
    report.check(
        "WallSide",
        vec![
            WallSide::Inside,
            WallSide::Outside,
            WallSide::Left,
            WallSide::Right,
            WallSide::Native(text("nx:centered")),
        ],
    );
    report.check(
        "ThinEnds",
        vec![
            ThinEnds::Capped(Dimension::Literal(positive_length(1.0))),
            ThinEnds::Merged,
        ],
    );
    report.check("Boolean", booleans());
    report.check(
        "BooleanOp",
        vec![
            BooleanOp::Join,
            BooleanOp::Cut,
            BooleanOp::Intersect,
            BooleanOp::Assemble,
            BooleanOp::Sew,
            BooleanOp::Compound,
            BooleanOp::UnionTrim,
            BooleanOp::Native(text("catia:remove_lump")),
        ],
    );
    report.check(
        "Targets",
        vec![Targets::All, Targets::Selected(selections().remove(0))],
    );
    report.check(
        "SheetOutput",
        vec![
            SheetOutput {
                solid: Some(Solidify::IfClosed),
                boolean: booleans().pop(),
            },
            SheetOutput {
                solid: None,
                boolean: None,
            },
        ],
    );
    report.check(
        "Solidify",
        vec![
            Solidify::Sheet,
            Solidify::IfClosed,
            Solidify::CapPlanarOpenings,
        ],
    );
    report.check(
        "SymmetricMeasure",
        vec![SymmetricMeasure::PerSide, SymmetricMeasure::Total],
    );
    report.check(
        "LimitOffset",
        vec![
            LimitOffset::Distance(Dimension::Literal(length(1.0))),
            LimitOffset::Translated(Dimension::Literal(length(-1.0))),
            LimitOffset::Angle(Dimension::Literal(angle(0.2))),
        ],
    );
    report.check("SectionCondition", section_conditions());
    report.check(
        "Support",
        vec![Support::Faces(selections().remove(0)), Support::Computed],
    );
    report.check(
        "ProjectionDirection",
        vec![
            ProjectionDirection::SurfaceNormal,
            ProjectionDirection::SketchNormal,
            ProjectionDirection::Along(fixtures::direction_constructions().remove(1)),
            ProjectionDirection::Native(text("nx:vector")),
        ],
    );
    report.check("SketchRegion", vec![fixtures::sketch_region()]);
    report.check("BoundaryUse", fixtures::sketch_region().outer);
    report.check("Fragment", vec![Fragment { index: 2, count: 5 }]);
    report.check("InstanceIndex", vec![InstanceIndex(vec![0, 3, -1])]);
    report.check("StateRef", fixtures::state_refs());
    report.check(
        "OccurrencePath through",
        vec![ParameterRef {
            target: RefTarget::Parameter(id("parameter", 1)),
            through: Some(OccurrencePath(Vec::new())),
        }],
    );
    report.check(
        "NativeName integers",
        vec![NativeName {
            codec: crate::codec_id!("creo"),
            value: NativeNameValue::Integers(Vec::new()),
        }],
    );
    report.check("ContainerId", vec![container(1)]);
    report.check("FeatureId", vec![feature(1)]);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}
