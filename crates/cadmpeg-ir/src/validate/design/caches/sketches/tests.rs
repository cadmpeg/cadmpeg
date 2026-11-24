// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::ops::sketch::{
    AngleOperands, DefiningRole, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve,
    RadialMeasure, Sketch, SketchConstraint, SketchEntity, SketchPlane, SketchRelation,
    SketchSolve,
};
use crate::design::parameter::ParameterValue;
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::math::Point2;
use crate::report::Severity;
use crate::scalar::{Angle, FiniteReal, Length, PositiveAngle, PositiveLength};
use crate::units::FinitePoint2;
use crate::validate::design::fixture::{design_findings, document, id, parameter};

/// The tolerances in force in these tests, exact in binary64.
const LINEAR: f64 = 1.0 / 1024.0;
const ANGULAR: f64 = 1.0 / 1024.0;

const MISSES: &str = "misses the constraint by";

fn point(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2 { u, v }).unwrap()
}

fn line(start: [f64; 2], end: [f64; 2]) -> PlanarCurve {
    PlanarCurve::Line {
        start: point(start[0], start[1]),
        end: point(end[0], end[1]),
    }
}

fn entity(key: &str, curve: PlanarCurve, roles: Vec<EntityRole<Planar>>) -> SketchEntity<Planar> {
    SketchEntity {
        id: id("sketch-entity", key),
        sketch: id("sketch", "profile"),
        definition: EntityDefinition::Free(curve),
        roles,
        construction: None,
        centerline: None,
        native: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

fn on(key: &str) -> Operand<Planar> {
    Operand::Entity(id("sketch-entity", key))
}

/// The fixture with a sketch of the extrude holding `entities` and one
/// constraint `relation`.
fn sketched(
    entities: Vec<SketchEntity<Planar>>,
    relation: Option<SketchRelation<Planar>>,
) -> CadIr {
    let mut ir = document();
    ir.tolerances.linear = PositiveLength::new(LINEAR).unwrap();
    ir.tolerances.angular = PositiveAngle::new(ANGULAR).unwrap();
    ir.design.sketches.push(Sketch {
        id: id("sketch", "profile"),
        owner: id("feature", "extrude"),
        name: None,
        plane: SketchPlane::Block,
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    });
    ir.design.sketch_entities = entities;
    ir.design.sketch_constraints = relation
        .into_iter()
        .map(|relation| SketchConstraint {
            id: id("sketch-constraint", "c"),
            sketch: id("sketch", "profile"),
            relation,
            name: None,
            active: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir
}

fn warnings(ir: &CadIr, fragment: &str) -> Vec<Option<String>> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| {
            finding.severity == Severity::Warning && finding.message.contains(fragment)
        })
        .map(|finding| finding.entity)
        .collect()
}

fn on_line(offset: f64) -> CadIr {
    sketched(
        vec![
            entity("line", line([0.0, 0.0], [10.0, 0.0]), Vec::new()),
            entity("point", PlanarCurve::Point(point(5.0, offset)), Vec::new()),
        ],
        Some(SketchRelation::Coincident {
            a: on("point"),
            b: on("line"),
        }),
    )
}

#[test]
fn a_point_coincident_with_a_line_lies_on_it() {
    assert!(warnings(&on_line(0.0), MISSES).is_empty());
    assert!(warnings(&on_line(LINEAR), MISSES).is_empty());
    assert_eq!(
        warnings(&on_line(2.0 * LINEAR), MISSES),
        vec![Some("test:design:sketch-constraint#c".to_owned())]
    );
    let mut stale = on_line(2.0 * LINEAR);
    stale.design.sketches[0].solve.status = EvaluationStatus::Stale;
    assert!(warnings(&stale, MISSES).is_empty());
}

fn distance(value: Dimension<Length>, cached: Option<f64>) -> CadIr {
    let mut ir = sketched(
        vec![
            entity("a", PlanarCurve::Point(point(0.0, 0.0)), Vec::new()),
            entity("b", PlanarCurve::Point(point(10.0, 0.0)), Vec::new()),
        ],
        Some(SketchRelation::Distance {
            a: on("a"),
            b: on("b"),
            along: None,
            doubled: None,
            value,
        }),
    );
    let mut bound = parameter("d", ValueKind::Quantity(Quantity::LENGTH));
    bound.value = cached.map(|value| ParameterValue::Number(FiniteReal::new(value).unwrap()));
    ir.design.parameters.push(bound);
    ir
}

#[test]
fn a_distance_reads_its_literal_or_its_parameter_cache() {
    let literal = |value: f64| Dimension::Literal(Length::new(value).unwrap());
    assert!(warnings(&distance(literal(10.0), None), MISSES).is_empty());
    assert!(warnings(&distance(literal(10.0 + LINEAR), None), MISSES).is_empty());
    assert_eq!(
        warnings(&distance(literal(10.0 + 2.0 * LINEAR), None), MISSES).len(),
        1
    );
    let bound = || Dimension::Parameter(id("parameter", "d"));
    assert!(warnings(&distance(bound(), Some(10.0)), MISSES).is_empty());
    assert_eq!(warnings(&distance(bound(), Some(11.0)), MISSES).len(), 1);
    assert!(warnings(&distance(bound(), None), MISSES).is_empty());
}

#[test]
fn a_radial_dimension_and_an_angle_read_their_circle_and_lines() {
    let circle = PlanarCurve::Circle {
        center: point(1.0, 1.0),
        radius: PositiveLength::new(5.0).unwrap(),
    };
    let diameter = |value: f64| {
        sketched(
            vec![entity("circle", circle.clone(), Vec::new())],
            Some(SketchRelation::Radial {
                of: id("sketch-entity", "circle"),
                measure: RadialMeasure::Diameter,
                value: Dimension::Literal(Length::new(value).unwrap()),
            }),
        )
    };
    assert!(warnings(&diameter(10.0), MISSES).is_empty());
    assert_eq!(warnings(&diameter(11.0), MISSES).len(), 1);

    let angle = |value: f64| {
        sketched(
            vec![
                entity("x", line([0.0, 0.0], [1.0, 0.0]), Vec::new()),
                entity("y", line([0.0, 0.0], [0.0, 1.0]), Vec::new()),
            ],
            Some(SketchRelation::Angle {
                between: AngleOperands::Between([on("x"), on("y")]),
                sector: None,
                value: Dimension::Literal(Angle::new(value).unwrap()),
            }),
        )
    };
    assert!(warnings(&angle(FRAC_PI_2), MISSES).is_empty());
    assert!(warnings(&angle(3.0 * FRAC_PI_2), MISSES).is_empty());
    assert_eq!(warnings(&angle(1.0), MISSES).len(), 1);
}

#[test]
fn a_point_lies_at_the_end_of_the_curve_it_bounds() {
    let ended = |v: f64| {
        let role = EntityRole {
            of: id("sketch-entity", "line"),
            role: DefiningRole::End,
        };
        sketched(
            vec![
                entity("line", line([0.0, 0.0], [10.0, 0.0]), Vec::new()),
                entity("end", PlanarCurve::Point(point(10.0, v)), vec![role]),
            ],
            None,
        )
    };
    let fragment = "from the End of entity";
    assert!(warnings(&ended(LINEAR), fragment).is_empty());
    assert_eq!(
        warnings(&ended(2.0 * LINEAR), fragment),
        vec![Some("test:design:sketch-entity#end".to_owned())]
    );
}
