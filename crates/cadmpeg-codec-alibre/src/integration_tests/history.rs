// SPDX-License-Identifier: Apache-2.0
//! The feature history of each specimen: its state order and the key numbers
//! of its features, in millimetres and radians.

use std::f64::consts::PI;

use cadmpeg_ir::design::construction::{InputReference, Relation};
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::ops::dressup::{FilletSet, FilletSize, HoleShape};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{PlanarCurve, SketchPlane};
use cadmpeg_ir::design::ops::swept::SectionForm;
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::Severity;

use super::{decode, BOX, BOX_HOLE, CONE, CYLINDER, CYLINDER_FILLET, LOFT, SMP_BOX};
use crate::loss::AlibreLossCode;

const TOLERANCE_MM: f64 = 1.0e-9;
const BASE_STATES: [&str; 7] = [
    "Origin", "X-Axis", "Y-Axis", "Z-Axis", "XY-Plane", "YZ-Plane", "ZX-Plane",
];

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= TOLERANCE_MM,
        "{actual} differs from {expected}"
    );
}

fn timeline(ir: &CadIr) -> Vec<&Feature> {
    let [Container::Ordered { members, .. }] = ir.design.containers.as_slice() else {
        panic!("one ordered timeline");
    };
    members
        .iter()
        .map(|member| {
            let Member::Feature(id) = member else {
                panic!("feature members");
            };
            ir.design.features.iter().find(|f| &f.id == id).unwrap()
        })
        .collect()
}

fn names(ir: &CadIr) -> Vec<&str> {
    timeline(ir)
        .iter()
        .map(|feature| feature.name.as_deref().unwrap())
        .collect()
}

fn expected_names(extra: &[&str]) -> Vec<String> {
    BASE_STATES
        .iter()
        .chain(extra)
        .map(|name| (*name).to_owned())
        .collect()
}

fn parameter(ir: &CadIr, id: &ParameterId) -> f64 {
    let parameter = ir.design.parameters.iter().find(|p| &p.id == id).unwrap();
    match parameter.value {
        Some(ParameterValue::Number(value)) => value.get(),
        ref other => panic!("{other:?}"),
    }
}

fn amount(ir: &CadIr, dimension: &Dimension<impl Copy + Into<f64>>) -> f64 {
    match dimension {
        Dimension::Parameter(id) => parameter(ir, id),
        other => panic!("{:?}", std::mem::discriminant(other)),
    }
}

fn blind_end(ir: &CadIr, span: Option<&Span<cadmpeg_ir::scalar::Length>>) -> f64 {
    let Some(Span::Between { start, end }) = span else {
        panic!("between span");
    };
    assert!(matches!(**start, Limit::AtOrigin));
    let Limit::Amount(Dimension::Parameter(id)) = &**end else {
        panic!("blind end");
    };
    parameter(ir, id)
}

fn operation<'a>(ir: &'a CadIr, name: &str) -> &'a FeatureOperation {
    &timeline(ir)
        .into_iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap()
        .definition
        .operation
}

fn sketch_curves(ir: &CadIr, name: &str) -> Vec<PlanarCurve> {
    let FeatureOperation::Sketch(id) = operation(ir, name) else {
        panic!("sketch feature");
    };
    ir.design
        .sketch_entities
        .iter()
        .filter(|entity| &entity.sketch == id)
        .filter_map(|entity| match &entity.definition {
            cadmpeg_ir::design::ops::sketch::EntityDefinition::Free(curve) => Some(curve.clone()),
            cadmpeg_ir::design::ops::sketch::EntityDefinition::Defined { .. } => None,
        })
        .collect()
}

fn sketch_origin(ir: &CadIr, name: &str) -> [f64; 3] {
    let FeatureOperation::Sketch(id) = operation(ir, name) else {
        panic!("sketch feature");
    };
    let sketch = ir.design.sketches.iter().find(|s| &s.id == id).unwrap();
    let SketchPlane::Placed(plane) = &sketch.plane else {
        panic!("placed sketch");
    };
    let origin = plane.resolved.as_ref().unwrap().origin.get();
    [origin.x, origin.y, origin.z]
}

fn lines(curves: &[PlanarCurve]) -> Vec<([f64; 2], [f64; 2])> {
    curves
        .iter()
        .filter_map(|curve| match curve {
            PlanarCurve::Line { start, end } => {
                Some(([start.get().u, start.get().v], [end.get().u, end.get().v]))
            }
            _ => None,
        })
        .collect()
}

fn extent(lines: &[([f64; 2], [f64; 2])], axis: usize) -> (f64, f64) {
    let values = lines.iter().flat_map(|(a, b)| [a[axis], b[axis]]);
    values.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

fn assert_rectangle(ir: &CadIr, name: &str, width: f64, height: f64) {
    let curves = sketch_curves(ir, name);
    let lines = lines(&curves);
    assert_eq!(lines.len(), 4);
    let (x0, x1) = extent(&lines, 0);
    let (y0, y1) = extent(&lines, 1);
    close(x1 - x0, width);
    close(y1 - y0, height);
    close(x0 + x1, 0.0);
    close(y0 + y1, 0.0);
    for (start, end) in &lines {
        assert!(
            (start[0] - end[0]).abs() <= TOLERANCE_MM || (start[1] - end[1]).abs() <= TOLERANCE_MM
        );
    }
}

fn circle(ir: &CadIr, name: &str) -> ([f64; 2], f64) {
    let curves = sketch_curves(ir, name);
    let circles: Vec<_> = curves
        .iter()
        .filter_map(|curve| match curve {
            PlanarCurve::Circle { center, radius } => {
                Some(([center.get().u, center.get().v], radius.get()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(circles.len(), 1);
    circles[0]
}

fn extrude_depth(ir: &CadIr, name: &str) -> f64 {
    let FeatureOperation::Extrude(extrude) = operation(ir, name) else {
        panic!("extrude");
    };
    blind_end(ir, extrude.span.as_ref())
}

fn profile_sketch(selection: &cadmpeg_ir::design::selection::OrderedSelection) -> String {
    let Picks::Named(members) = &selection.picks else {
        panic!("named profile");
    };
    match &members.as_slice()[0].target {
        PickTarget::Sketch(id) => id.as_str().to_owned(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_specimen_checks_with_a_design() {
    for (name, bytes) in super::specimens() {
        let result = decode(bytes);
        let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        assert_eq!(result.ir().design.scopes.len(), 1, "{name}");
        let kinds: Vec<_> = result
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect();
        assert!(
            !kinds.contains(&AlibreLossCode::HistoryStreamUnread.kind()),
            "{name}"
        );
        assert!(
            !kinds.contains(&AlibreLossCode::HistoryOperationUnmapped.kind()),
            "{name}"
        );
        assert!(
            !kinds.contains(&AlibreLossCode::HistoryConstraintUnmapped.kind()),
            "{name}"
        );
    }
}

#[test]
fn box_history_is_a_rectangle_extruded() {
    let result = decode(BOX);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "E1"]));
    assert_rectangle(ir, "S1", 20.0, 30.0);
    close(extrude_depth(ir, "E1"), 10.0);
    let FeatureOperation::Extrude(extrude) = operation(ir, "E1") else {
        panic!("extrude");
    };
    assert!(profile_sketch(&extrude.profile).ends_with("sketch#165"));
    close(sketch_origin(ir, "S1")[2], 0.0);
}

#[test]
fn cylinder_history_is_a_circle_extruded() {
    let result = decode(CYLINDER);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "E1"]));
    let (center, radius) = circle(ir, "S1");
    close(center[0], 0.0);
    close(center[1], 0.0);
    close(radius, 10.0);
    close(extrude_depth(ir, "E1"), 15.0);
}

#[test]
fn fillet_history_adds_a_constant_radius() {
    let result = decode(CYLINDER_FILLET);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "E1", "F1"]));
    close(extrude_depth(ir, "E1"), 15.0);
    let FeatureOperation::Fillet(fillet) = operation(ir, "F1") else {
        panic!("fillet");
    };
    let [FilletSet::Edges { edges, section, .. }] = fillet.sets.as_slice() else {
        panic!("one edge set");
    };
    let Some(FilletSize::Radius(radius)) = &section.size else {
        panic!("radius");
    };
    close(amount(ir, &dimension_length(radius)), 2.0);
    assert!(matches!(edges.picks, Picks::Unread));
    assert!(edges.native.is_some());
    let kinds: Vec<_> = result
        .report()
        .losses
        .iter()
        .map(|l| l.code.clone())
        .collect();
    assert!(kinds.contains(&AlibreLossCode::HistoryEdgeSelectionUnresolved.kind()));
}

fn dimension_length(dimension: &Dimension<cadmpeg_ir::scalar::PositiveLength>) -> Dimension<f64> {
    match dimension {
        Dimension::Parameter(id) => Dimension::Parameter(id.clone()),
        other => panic!("{:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn cone_history_revolves_a_triangle_a_full_turn() {
    let result = decode(CONE);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "R1"]));
    let FeatureOperation::Revolve(revolve) = operation(ir, "R1") else {
        panic!("revolve");
    };
    let Some(Span::Between { end, .. }) = &revolve.span else {
        panic!("between span");
    };
    let Limit::Amount(Dimension::Parameter(angle)) = &**end else {
        panic!("blind angle");
    };
    close(parameter(ir, angle), 2.0 * PI);
    let axis = revolve.axis.as_ref().unwrap().resolved.as_ref().unwrap();
    let direction = cadmpeg_ir::math::Vector3::from(axis.direction);
    let origin = axis.origin.get();
    close(direction.z.abs(), 1.0);
    close(origin.x.abs() + origin.y.abs() + origin.z.abs(), 0.0);
    let curves = sketch_curves(ir, "S1");
    let lines = lines(&curves);
    assert_eq!(lines.len(), 3);
    let (r0, r1) = extent(&lines, 0);
    close(r0, 5.0);
    close(r1, 15.0);
    let (h0, h1) = extent(&lines, 1);
    close(h1 - h0, 10.0);
}

#[test]
fn box_hole_history_adds_a_through_hole_sketch_on_the_top_face() {
    let result = decode(BOX_HOLE);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "E1", "HS", "H1"]));
    assert_rectangle(ir, "S1", 20.0, 30.0);
    close(extrude_depth(ir, "E1"), 10.0);
    close(sketch_origin(ir, "HS")[2], 10.0);
    let FeatureOperation::Hole(hole) = operation(ir, "H1") else {
        panic!("hole");
    };
    let HoleShape::Bore(bore) = &hole.shape else {
        panic!("bore");
    };
    let Some(Dimension::Parameter(diameter)) = &bore.diameter else {
        panic!("diameter parameter");
    };
    close(parameter(ir, diameter), 6.0);
    assert!(matches!(hole.extent, Some(Limit::ThroughAll)));
    let Picks::Named(points) = (match &hole.positions {
        cadmpeg_ir::design::ops::dressup::HolePositions::Points { points, .. } => &points.picks,
        other @ cadmpeg_ir::design::ops::dressup::HolePositions::Placed(_) => panic!("{other:?}"),
    }) else {
        panic!("named position");
    };
    assert!(matches!(
        points.as_slice()[0].target,
        PickTarget::SketchEntity(_)
    ));
}

#[test]
fn loft_history_joins_a_square_and_a_circle_on_an_offset_plane() {
    let result = decode(LOFT);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "P2", "S2", "L1"]));
    assert_rectangle(ir, "S1", 20.0, 20.0);
    let (center, radius) = circle(ir, "S2");
    close(center[0], 0.0);
    close(center[1], 0.0);
    close(radius, 8.0);
    close(sketch_origin(ir, "S2")[2], 20.0);
    let FeatureOperation::Datum(datum) = operation(ir, "P2") else {
        panic!("datum");
    };
    let DatumElement::Plane(plane) = &datum.element else {
        panic!("plane datum");
    };
    close(plane.resolved.as_ref().unwrap().origin.get().z, 20.0);
    let [input] = plane.inputs.as_slice() else {
        panic!("one offset input");
    };
    assert_eq!(input.role.as_ref().unwrap().relation, Relation::Offset);
    assert!(matches!(input.reference, Some(InputReference::Picks(_))));
    let FeatureOperation::Loft(loft) = operation(ir, "L1") else {
        panic!("loft");
    };
    assert_eq!(loft.sections.len(), 2);
    let sections: Vec<_> = loft
        .sections
        .iter()
        .map(|section| {
            let SectionForm::Profile(profile) = &section.form else {
                panic!("profile section");
            };
            profile_sketch(profile)
        })
        .collect();
    let [FeatureOperation::Sketch(first), FeatureOperation::Sketch(second)] =
        [operation(ir, "S1"), operation(ir, "S2")]
    else {
        panic!("sketch features");
    };
    assert_eq!(sections, [first.as_str(), second.as_str()]);
}

fn has_loss(result: &cadmpeg_ir::codec::DecodeResult, code: AlibreLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind() && loss.severity == Severity::Warning)
}

#[test]
fn a_sketch_on_a_face_charges_the_unresolved_face_selection() {
    let result = decode(BOX_HOLE);
    assert!(has_loss(
        &result,
        AlibreLossCode::HistoryFaceSelectionUnresolved
    ));
    // The box has no sketch on a face.
    assert!(!has_loss(
        &decode(BOX),
        AlibreLossCode::HistoryFaceSelectionUnresolved
    ));
}

/// Overwrites the `DATA;` section keyword of the `STEP_DATA` stream, so the
/// stream no longer reads as Part 21.
fn without_step_data_section(bytes: &[u8]) -> Vec<u8> {
    let keyword = b"\nDATA;";
    let at = bytes
        .windows(keyword.len())
        .position(|window| window == keyword)
        .unwrap();
    let mut patched = bytes.to_vec();
    patched[at + 4] = b'X';
    patched
}

#[test]
fn a_step_data_stream_that_does_not_read_keeps_the_body_and_charges_the_history() {
    let result = decode(&without_step_data_section(BOX_HOLE));
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.ir().design.features.is_empty());
    assert!(result.ir().design.scopes.is_empty());
    assert!(has_loss(&result, AlibreLossCode::HistoryStreamUnread));
}

#[test]
fn a_sheet_metal_part_states_its_rule() {
    use cadmpeg_ir::design::ops::sheet_metal::BendAllowance;
    let result = decode(SMP_BOX);
    let ir = result.ir();
    let [rule] = ir.design.sheet_metal_rules.as_slice() else {
        panic!("one rule");
    };
    assert_eq!(rule.scope, ir.model.product_definitions[0].id);
    assert_eq!(rule.id.as_str(), "alibre:step:sheet-metal-rule#201");
    assert_eq!(
        rule.native_ref.as_deref(),
        Some("alibre:step:parameter#201")
    );
    let Some(Dimension::Parameter(thickness)) = &rule.thickness else {
        panic!("a thickness bound to its parameter");
    };
    close(parameter(ir, thickness), 1.0);
    let Some(Dimension::Parameter(radius)) = &rule.bend.radius else {
        panic!("a radius bound to its parameter");
    };
    close(parameter(ir, radius), 0.5);
    let Some(BendAllowance::KFactor(Dimension::Parameter(factor))) = &rule.allowance else {
        panic!("a K-factor bound to its parameter");
    };
    assert_eq!(parameter(ir, factor), 0.33);
    let relief = rule.bend.relief.as_ref().unwrap();
    let Some(Dimension::Parameter(depth)) = &relief.depth else {
        panic!("a relief depth");
    };
    let Some(Dimension::Parameter(width)) = &relief.width else {
        panic!("a relief width");
    };
    close(parameter(ir, depth), 1.0);
    close(parameter(ir, width), 0.5);
    let options: Vec<_> = rule.native_options.keys().map(|key| key.as_str()).collect();
    assert_eq!(
        options,
        [
            "alibre:bend-relief-type",
            "alibre:minimum-bend-radius",
            "alibre:minimum-rip-width"
        ]
    );
}

#[test]
fn a_sheet_metal_radius_is_an_equation_of_the_thickness() {
    use cadmpeg_ir::design::dimension::{ParameterRef, RefTarget};
    use cadmpeg_ir::design::expr::{Expression, MulOp, Node};
    use cadmpeg_ir::design::parameter::ParameterDefinition;
    let result = decode(SMP_BOX);
    let ir = result.ir();
    let named = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|p| p.name.as_ref().map(|n| n.as_str()) == Some(name))
            .unwrap()
    };
    let thickness = named("AD_Thickness");
    let radius = named("AD_GlobalBendRadius");
    let ParameterDefinition::Expression(Expression::Neutral(tree)) = &radius.definition else {
        panic!("a neutral expression");
    };
    let Node::Product { first, rest } = &tree.0 else {
        panic!("a product");
    };
    assert_eq!(
        **first,
        Node::Reference(ParameterRef {
            target: RefTarget::Parameter(thickness.id.clone()),
            through: None
        })
    );
    assert!(matches!(
        rest.as_slice(),
        [(MulOp::Divide, Node::Number { .. })]
    ));
    let ParameterDefinition::Expression(Expression::Neutral(depth)) =
        &named("AD_BendReliefDepth").definition
    else {
        panic!("a neutral expression");
    };
    assert!(matches!(depth.0, Node::Reference(_)));
}

#[test]
fn a_sheet_metal_part_keeps_its_integer_and_unitless_parameters() {
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    let result = decode(SMP_BOX);
    let ir = result.ir();
    let named = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|p| p.name.as_ref().map(|n| n.as_str()) == Some(name))
            .unwrap()
    };
    let kind = named("AD_BendReliefType");
    assert_eq!(kind.kind, ValueKind::Integer);
    assert_eq!(kind.value, Some(ParameterValue::Integer(1)));
    let factor = named("AD_KFactor");
    assert_eq!(factor.kind, ValueKind::Quantity(Quantity::REAL));
    assert!(factor.unit.is_none());
    let kinds: Vec<_> = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect();
    assert!(kinds.contains(&AlibreLossCode::HistorySheetMetalSettingsUnmapped.kind()));
    assert!(!kinds.contains(&AlibreLossCode::HistoryParameterEquationUnmapped.kind()));
}

#[test]
fn a_plain_part_states_no_sheet_metal_rule() {
    for (name, bytes) in super::specimens() {
        if name != "smp_box" {
            assert!(
                decode(bytes).ir().design.sheet_metal_rules.is_empty(),
                "{name}"
            );
        }
    }
}

#[test]
fn a_sheet_metal_part_has_its_base_feature_history() {
    let result = decode(SMP_BOX);
    let ir = result.ir();
    assert_eq!(names(ir), expected_names(&["S1", "E1"]));
    assert_rectangle(ir, "S1", 20.0, 30.0);
    close(extrude_depth(ir, "E1"), 1.5);
}

mod synthetic {
    use std::collections::BTreeMap;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::design::ops::dressup::HolePositions;
    use cadmpeg_ir::design::selection::Picks;
    use cadmpeg_ir::ids::ProductDefinitionId;

    use super::*;
    use crate::history::{
        Curve, End, Feature, Frame, History, Operation, Parameter, ParameterKind, Primitive,
        Sketch, Support,
    };

    fn frame() -> Frame {
        Frame {
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        }
    }

    fn parameter(record: u32, name: &str, guid: Option<&str>, value: f64) -> Parameter {
        Parameter {
            record,
            name: name.to_owned(),
            guid: guid.map(str::to_owned),
            kind: ParameterKind::Length,
            equation: None,
            value,
        }
    }

    fn feature(record: u32, name: &str, operation: Operation) -> Feature {
        Feature {
            record,
            name: name.to_owned(),
            operation,
        }
    }

    fn history() -> History {
        let points = [[1.0, 2.0], [-3.0, 4.0]]
            .into_iter()
            .zip([21, 22])
            .map(|(at, record)| Primitive {
                record,
                curve: Curve::Point(at),
            })
            .collect();
        History {
            parameters: vec![
                parameter(1, "D1", Some("{0F0E-GUID}"), 6.0),
                parameter(2, "R1", None, 2.0),
            ],
            features: vec![
                feature(10, "XY-Plane", Operation::Plane { frame: frame() }),
                feature(
                    11,
                    "HS",
                    Operation::Sketch(Box::new(Sketch {
                        record: 20,
                        name: "HS".to_owned(),
                        frame: frame(),
                        support: Support::Face("#0".to_owned()),
                        primitives: points,
                        points: Vec::new(),
                        constraints: Vec::new(),
                    })),
                ),
                feature(
                    12,
                    "H1",
                    Operation::Hole {
                        sketch: 1,
                        diameter: 0,
                        end: End::ThroughAll,
                    },
                ),
                feature(
                    13,
                    "F1",
                    Operation::Fillet {
                        radius: 1,
                        edge_signatures: vec!["#1".to_owned(), "#2".to_owned()],
                    },
                ),
            ],
            losses: Vec::new(),
        }
    }

    fn sheet_metal_history(thickness: f64) -> History {
        let mut history = history();
        let real = |record, name: &str, value| Parameter {
            kind: ParameterKind::Real,
            ..parameter(record, name, None, value)
        };
        history.parameters.extend([
            parameter(30, "AD_Thickness", None, thickness),
            Parameter {
                equation: Some("AD_Thickness/2".to_owned()),
                ..parameter(31, "AD_GlobalBendRadius", None, thickness / 2.0)
            },
            real(32, "AD_KFactor", 0.4),
        ]);
        history
    }

    #[test]
    fn a_part_without_the_thickness_parameter_states_no_rule() {
        let (ir, _) = project();
        assert!(ir.design.sheet_metal_rules.is_empty());
    }

    #[test]
    fn the_declared_parameters_make_one_rule() {
        let (ir, losses) = project_of(&sheet_metal_history(1.0));
        let [rule] = ir.design.sheet_metal_rules.as_slice() else {
            panic!("one rule");
        };
        assert_eq!(rule.id.as_str(), "alibre:step:sheet-metal-rule#30");
        assert!(rule.bend.relief.is_none());
        assert!(rule.native_options.is_empty());
        assert!(rule.allowance.is_some());
        assert!(!losses
            .iter()
            .any(|loss| loss.code == AlibreLossCode::HistorySheetMetalSettingsUnmapped.kind()));
    }

    #[test]
    fn a_thickness_that_is_no_positive_length_states_no_rule_and_is_charged() {
        let (ir, losses) = project_of(&sheet_metal_history(0.0));
        assert!(ir.design.sheet_metal_rules.is_empty());
        assert!(losses
            .iter()
            .any(|loss| loss.code == AlibreLossCode::HistorySheetMetalSettingsUnmapped.kind()));
    }

    #[test]
    fn an_equation_that_is_not_translated_is_charged_and_keeps_its_value() {
        let mut history = sheet_metal_history(1.0);
        history.parameters[3].equation = Some("AD_Thickness+1".to_owned());
        let (ir, losses) = project_of(&history);
        assert!(losses
            .iter()
            .any(|loss| loss.code == AlibreLossCode::HistoryParameterEquationUnmapped.kind()));
        let radius = ir
            .design
            .parameters
            .iter()
            .find(|p| p.name.as_ref().map(|n| n.as_str()) == Some("AD_GlobalBendRadius"))
            .unwrap();
        assert_eq!(
            radius.value,
            Some(ParameterValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap()
            ))
        );
    }

    fn project() -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
        project_of(&history())
    }

    fn project_of(history: &History) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
        let mut ir = CadIr::empty();
        let mut losses = Vec::new();
        let definition = ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("alibre", "model", "product-definition"),
            cadmpeg_ir::identity_key!("root"),
        );
        crate::design::project(&ctx, &mut ir, history, &definition, &mut losses).unwrap();
        (ir, losses)
    }

    #[test]
    fn a_hole_picks_every_point_of_its_sketch() {
        let (ir, _) = project();
        let FeatureOperation::Hole(hole) = operation(&ir, "H1") else {
            panic!("hole");
        };
        let HolePositions::Points { points, centers } = &hole.positions else {
            panic!("point positions");
        };
        assert!(centers.is_none());
        let Picks::Named(members) = &points.picks else {
            panic!("named points");
        };
        let ids: Vec<_> = members
            .as_slice()
            .iter()
            .map(|pick| match &pick.target {
                PickTarget::SketchEntity(id) => id.as_str().to_owned(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            ids,
            [
                "alibre:step:sketch-entity#21",
                "alibre:step:sketch-entity#22"
            ]
        );
    }

    #[test]
    fn a_fillet_keeps_every_signature() {
        let (ir, _) = project();
        let FeatureOperation::Fillet(fillet) = operation(&ir, "F1") else {
            panic!("fillet");
        };
        let [FilletSet::Edges { edges, .. }] = fillet.sets.as_slice() else {
            panic!("one edge set");
        };
        let native = edges.native.as_ref().unwrap();
        assert_eq!(
            native.as_slice(),
            [
                "alibre:topology-signature:#1".to_owned(),
                "alibre:topology-signature:#2".to_owned()
            ]
        );
    }

    #[test]
    fn a_parameter_guid_is_a_source_setting() {
        let (ir, _) = project();
        let key = cadmpeg_ir::codec_name!("alibre:guid");
        let settings: BTreeMap<_, _> = ir
            .design
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.as_ref().unwrap().as_str().to_owned(),
                    parameter.source_settings.contains_key(&key),
                )
            })
            .collect();
        assert!(settings["D1"]);
        assert!(!settings["R1"]);
    }

    #[test]
    fn the_projection_charges_the_face_the_sketch_lies_on() {
        let (_, losses) = project();
        let faces: Vec<_> = losses
            .iter()
            .filter(|loss| loss.code == AlibreLossCode::HistoryFaceSelectionUnresolved.kind())
            .collect();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].severity, Severity::Warning);
    }

    #[test]
    fn a_hole_positioned_by_a_sketch_with_points_charges_no_position_loss() {
        let (_, losses) = project();
        assert!(!losses
            .iter()
            .any(|loss| loss.code == AlibreLossCode::HistoryHolePositionUnresolved.kind()));
    }

    #[test]
    fn a_hole_whose_sketch_has_no_point_charges_its_name() {
        let mut history = history();
        let Operation::Sketch(sketch) = &mut history.features[1].operation else {
            panic!("sketch");
        };
        sketch.primitives.clear();
        let (_, losses) = project_of(&history);
        let notes: Vec<_> = losses
            .iter()
            .filter(|loss| loss.code == AlibreLossCode::HistoryHolePositionUnresolved.kind())
            .collect();
        let [note] = notes.as_slice() else {
            panic!("one loss");
        };
        assert_eq!(note.severity, Severity::Warning);
        assert!(note.message.contains("H1"), "{}", note.message);
    }
}
