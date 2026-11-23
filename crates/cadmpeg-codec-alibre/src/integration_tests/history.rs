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

use super::{decode, BOX, BOX_HOLE, CONE, CYLINDER, CYLINDER_FILLET, LOFT};
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
