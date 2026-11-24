// SPDX-License-Identifier: Apache-2.0
//! Solved planar sketch geometry against its constraints (part 10, Caches),
//! where the sketch's status is `Current`: the residual of each constraint
//! that incidence and arithmetic on points, lines, circles, and arcs decide,
//! and a point's position against the end or center of each curve it bounds.
//! Lengths agree within the linear tolerance, and directions within the
//! angular tolerance and what the linear tolerance of a segment's ends
//! leaves of its direction; each disagreement is a Warning.
//!
//! An entity is read where it is `Free`, or a `Frame` point: a spline, a
//! conic, and a derived or instanced curve are not checked. A dimension
//! bound to a parameter reads the parameter's cache in the scope's active
//! configuration.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use super::super::dimensions::{from_cache, Bound};
use super::super::parameters::cache_in;
use super::super::records::{DesignRecords, MemberKey};
use super::super::warning;
use crate::design::configuration::DesignConfiguration;
use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::ops::sketch::{
    AngleOperands, DefiningRole, DistanceAxis, EntityDefinition, EntityForm, Operand, Planar,
    PlanarCurve, RadialMeasure, SketchConstraint, SketchEntity, SketchRelation,
};
use crate::design::selection::PrincipalEntity;
use crate::report::check::Finding;

/// A point, a line, or a circle of the sketch plane.
#[derive(Clone, Copy)]
enum Shape {
    Point([f64; 2]),
    /// A point on the line, its unit direction, and its length where it is a
    /// segment.
    Line([f64; 2], [f64; 2], Option<f64>),
    /// The center and the radius; an arc is read on its circle.
    Circle([f64; 2], f64),
}

/// What a residual measures.
#[derive(Clone, Copy)]
enum Measure {
    Length,
    /// An angle between directions that segments give, with the sum over
    /// those segments of 2 / length: each end of a segment is known within
    /// the linear tolerance, so its direction within that many times it.
    Angle(f64),
}

struct Sketch<'a, 'v> {
    index: &'v DesignRecords<'a>,
    chain: Vec<&'a DesignConfiguration>,
    curves: BTreeMap<&'a str, &'a PlanarCurve>,
}

pub(super) fn check_sketches(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    let tolerances = index.ir.tolerances;
    let (linear, angular) = (tolerances.linear.get(), tolerances.angular.get());
    let mut entities: BTreeMap<&str, Vec<&SketchEntity<Planar>>> = BTreeMap::new();
    for entity in &design.sketch_entities {
        entities
            .entry(entity.sketch.as_str())
            .or_default()
            .push(entity);
    }
    let mut constraints: BTreeMap<&str, Vec<&SketchConstraint<Planar>>> = BTreeMap::new();
    for constraint in &design.sketch_constraints {
        constraints
            .entry(constraint.sketch.as_str())
            .or_default()
            .push(constraint);
    }
    for record in &design.sketches {
        if record.solve.status != EvaluationStatus::Current {
            continue;
        }
        let id = record.id.as_str();
        let active = index
            .scope_of(MemberKey::Feature(record.owner.as_str()))
            .and_then(|scope| index.scopes.get(scope))
            .and_then(|scope| scope.active_configuration.as_ref())
            .map(crate::features::ConfigurationId::as_str);
        let held = entities.get(id).map(Vec::as_slice).unwrap_or_default();
        let sketch = Sketch {
            index,
            chain: index.configuration_chain(active),
            curves: held
                .iter()
                .filter_map(|entity| Some((entity.id.as_str(), solved(entity)?)))
                .collect(),
        };
        for constraint in constraints.get(id).map(Vec::as_slice).unwrap_or_default() {
            let Some((residual, measure)) = sketch.residual(&constraint.relation) else {
                continue;
            };
            let within = match measure {
                Measure::Length => linear,
                Measure::Angle(spread) => linear.mul_add(spread, angular),
            };
            if residual > within {
                warning(
                    findings,
                    constraint.id.as_str(),
                    format_args!("the solved sketch geometry misses the constraint by {residual}"),
                );
            }
        }
        for entity in held {
            let Some(PlanarCurve::Point(point)) = sketch.curves.get(entity.id.as_str()) else {
                continue;
            };
            let point = coordinates(*point);
            for role in &entity.roles {
                let Some(curve) = sketch.curves.get(role.of.as_str()) else {
                    continue;
                };
                let Some(expected) = role_position(curve, &role.role) else {
                    continue;
                };
                let distance = (point[0] - expected[0]).hypot(point[1] - expected[1]);
                if distance > linear {
                    warning(
                        findings,
                        entity.id.as_str(),
                        format_args!(
                            "the point is {distance} from the {:?} of entity {}",
                            role.role,
                            role.of.as_str()
                        ),
                    );
                }
            }
        }
    }
}

/// An entity's solved curve, where the entity is `Free` or a `Frame` point.
fn solved(entity: &SketchEntity<Planar>) -> Option<&PlanarCurve> {
    match &entity.definition {
        EntityDefinition::Free(curve) => Some(curve),
        EntityDefinition::Defined { form, solved } => match **form {
            EntityForm::Frame => solved.as_ref(),
            _ => None,
        },
    }
}

fn coordinates(point: crate::units::FinitePoint2) -> [f64; 2] {
    let point = point.get();
    [point.u, point.v]
}

/// The position of `role` on `curve`: a segment's or an arc's ends, and a
/// circle's or an arc's center.
fn role_position(curve: &PlanarCurve, role: &DefiningRole) -> Option<[f64; 2]> {
    match (curve, role) {
        (PlanarCurve::Line { start, .. }, DefiningRole::Start) => Some(coordinates(*start)),
        (PlanarCurve::Line { end, .. }, DefiningRole::End) => Some(coordinates(*end)),
        (
            PlanarCurve::Circle { center, .. } | PlanarCurve::Arc { center, .. },
            DefiningRole::Center,
        ) => Some(coordinates(*center)),
        (
            PlanarCurve::Arc {
                center,
                radius,
                start,
                end,
            },
            DefiningRole::Start | DefiningRole::End,
        ) => {
            let angle = if matches!(role, DefiningRole::Start) {
                start.get()
            } else {
                end.get()
            };
            let [x, y] = coordinates(*center);
            Some([
                radius.get().mul_add(angle.cos(), x),
                radius.get().mul_add(angle.sin(), y),
            ])
        }
        _ => None,
    }
}

impl Sketch<'_, '_> {
    /// The value of a dimension: its literal, or its parameter's cache in the
    /// active configuration.
    fn value<T: Bound + Clone>(&self, dimension: &Dimension<T>) -> Option<T> {
        let (parameter, negated) = match dimension {
            Dimension::Literal(value) => return Some(value.clone()),
            Dimension::Parameter(parameter) => (parameter, false),
            Dimension::Negated(parameter) => (parameter, true),
        };
        from_cache(cache_in(self.index, &self.chain, parameter)?, negated)
    }

    fn shape(&self, operand: &Operand<Planar>) -> Option<Shape> {
        match operand {
            Operand::Entity(entity) => shape(self.curves.get(entity.as_str())?),
            Operand::Frame(PrincipalEntity::Origin) => Some(Shape::Point([0.0, 0.0])),
            Operand::Frame(PrincipalEntity::XAxis) => {
                Some(Shape::Line([0.0, 0.0], [1.0, 0.0], None))
            }
            Operand::Frame(PrincipalEntity::YAxis) => {
                Some(Shape::Line([0.0, 0.0], [0.0, 1.0], None))
            }
            Operand::Frame(_) | Operand::Model(_) => None,
        }
    }

    /// The constraint's residual and what it measures, where incidence and
    /// arithmetic decide it.
    fn residual(&self, relation: &SketchRelation<Planar>) -> Option<(f64, Measure)> {
        let length = |residual: f64| Some((residual, Measure::Length));
        let angle = |residual: f64, spread: f64| Some((residual, Measure::Angle(spread)));
        match relation {
            SketchRelation::Coincident { a, b } => {
                length(incidence(self.shape(a)?, self.shape(b)?)?)
            }
            SketchRelation::Horizontal { a, b } => match (self.shape(a)?, b) {
                (line @ Shape::Line(_, direction, _), None) => {
                    angle(direction[1].abs().asin(), spread(line))
                }
                (Shape::Point(first), Some(b)) => {
                    let Shape::Point(second) = self.shape(b)? else {
                        return None;
                    };
                    length((first[1] - second[1]).abs())
                }
                _ => None,
            },
            SketchRelation::Vertical { a, b } => match (self.shape(a)?, b) {
                (line @ Shape::Line(_, direction, _), None) => {
                    angle(direction[0].abs().asin(), spread(line))
                }
                (Shape::Point(first), Some(b)) => {
                    let Shape::Point(second) = self.shape(b)? else {
                        return None;
                    };
                    length((first[0] - second[0]).abs())
                }
                _ => None,
            },
            SketchRelation::Parallel { a, b } => {
                let (a, b) = (self.shape(a)?, self.shape(b)?);
                let (first, second) = (direction(a)?, direction(b)?);
                angle(
                    cross(first, second).abs().min(1.0).asin(),
                    spread(a) + spread(b),
                )
            }
            SketchRelation::Perpendicular { a, b } => {
                let (a, b) = (self.shape(a)?, self.shape(b)?);
                let (first, second) = (direction(a)?, direction(b)?);
                angle(
                    dot(first, second).abs().min(1.0).asin(),
                    spread(a) + spread(b),
                )
            }
            SketchRelation::Tangent { a, b, .. } => {
                length(tangency(self.shape(a)?, self.shape(b)?)?)
            }
            SketchRelation::Distance {
                a,
                b,
                along,
                doubled,
                value,
            } => {
                let expected = self.value(value)?.get().abs();
                let (first, second) = (self.shape(a)?, self.shape(b)?);
                let measured = match along {
                    None | Some(DistanceAxis::Aligned) => separation(first, second)?,
                    Some(DistanceAxis::Horizontal) => axis_separation(first, second, 0)?,
                    Some(DistanceAxis::Vertical) => axis_separation(first, second, 1)?,
                    Some(DistanceAxis::Along(_)) => return None,
                };
                let factor = if *doubled == Some(true) { 2.0 } else { 1.0 };
                length((measured * factor - expected).abs())
            }
            SketchRelation::Angle {
                between: AngleOperands::Between([a, b]),
                value,
                ..
            } => {
                let expected = self.value(value)?.get().rem_euclid(TAU);
                let (a, b) = (self.shape(a)?, self.shape(b)?);
                let (first, second) = (direction(a)?, direction(b)?);
                let between = dot(first, second).clamp(-1.0, 1.0).acos();
                let candidates = [between, PI - between, PI + between, TAU - between];
                angle(
                    candidates
                        .into_iter()
                        .map(|candidate| {
                            let difference = (expected - candidate).rem_euclid(TAU);
                            difference.min(TAU - difference)
                        })
                        .fold(f64::INFINITY, f64::min),
                    spread(a) + spread(b),
                )
            }
            SketchRelation::Radial {
                of, measure, value, ..
            } => {
                let Shape::Circle(_, radius) = shape(self.curves.get(of.as_str())?)? else {
                    return None;
                };
                let measured = match measure {
                    RadialMeasure::Radius => radius,
                    RadialMeasure::Diameter => 2.0 * radius,
                    _ => return None,
                };
                length((measured - self.value(value)?.get()).abs())
            }
            SketchRelation::Equal { members } => {
                let sizes = members
                    .iter()
                    .map(|member| match self.shape(member)? {
                        Shape::Line(_, _, Some(length)) => Some((false, length)),
                        Shape::Circle(_, radius) => Some((true, radius)),
                        Shape::Line(_, _, None) | Shape::Point(_) => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
                let circles = sizes.first()?.0;
                if sizes.iter().any(|(circle, _)| *circle != circles) {
                    return None;
                }
                let (low, high) = sizes.iter().fold(
                    (f64::INFINITY, f64::NEG_INFINITY),
                    |(low, high), (_, size)| (low.min(*size), high.max(*size)),
                );
                length(high - low)
            }
            _ => None,
        }
    }
}

fn shape(curve: &PlanarCurve) -> Option<Shape> {
    match curve {
        PlanarCurve::Point(point) => Some(Shape::Point(coordinates(*point))),
        PlanarCurve::Line { start, end } => {
            let (start, end) = (coordinates(*start), coordinates(*end));
            let length = (end[0] - start[0]).hypot(end[1] - start[1]);
            (length > 0.0).then(|| {
                Shape::Line(
                    start,
                    [(end[0] - start[0]) / length, (end[1] - start[1]) / length],
                    Some(length),
                )
            })
        }
        PlanarCurve::InfiniteLine { through, direction } => {
            Some(Shape::Line(coordinates(*through), direction.get(), None))
        }
        PlanarCurve::Circle { center, radius } | PlanarCurve::Arc { center, radius, .. } => {
            Some(Shape::Circle(coordinates(*center), radius.get()))
        }
        _ => None,
    }
}

fn dot(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0].mul_add(second[0], first[1] * second[1])
}

fn cross(first: [f64; 2], second: [f64; 2]) -> f64 {
    first[0].mul_add(second[1], -(first[1] * second[0]))
}

/// 2 / length of a segment; an unbounded line gives its direction exactly.
fn spread(shape: Shape) -> f64 {
    match shape {
        Shape::Line(_, _, Some(length)) => 2.0 / length,
        Shape::Line(_, _, None) | Shape::Point(_) | Shape::Circle(..) => 0.0,
    }
}

fn direction(shape: Shape) -> Option<[f64; 2]> {
    match shape {
        Shape::Line(_, direction, _) => Some(direction),
        Shape::Point(_) | Shape::Circle(..) => None,
    }
}

/// The distance from `point` to the line through `through` along `direction`.
fn to_line(point: [f64; 2], through: [f64; 2], direction: [f64; 2]) -> f64 {
    cross(direction, [point[0] - through[0], point[1] - through[1]]).abs()
}

fn between(first: [f64; 2], second: [f64; 2]) -> f64 {
    (first[0] - second[0]).hypot(first[1] - second[1])
}

/// How far a point lies from the other operand, a point, a line, or a circle.
fn incidence(first: Shape, second: Shape) -> Option<f64> {
    match (first, second) {
        (Shape::Point(first), Shape::Point(second)) => Some(between(first, second)),
        (Shape::Point(point), Shape::Line(through, direction, _))
        | (Shape::Line(through, direction, _), Shape::Point(point)) => {
            Some(to_line(point, through, direction))
        }
        (Shape::Point(point), Shape::Circle(center, radius))
        | (Shape::Circle(center, radius), Shape::Point(point)) => {
            Some((between(point, center) - radius).abs())
        }
        _ => None,
    }
}

/// How far a line and a circle, or two circles, are from touching.
fn tangency(first: Shape, second: Shape) -> Option<f64> {
    match (first, second) {
        (Shape::Line(through, direction, _), Shape::Circle(center, radius))
        | (Shape::Circle(center, radius), Shape::Line(through, direction, _)) => {
            Some((to_line(center, through, direction) - radius).abs())
        }
        (Shape::Circle(first, first_radius), Shape::Circle(second, second_radius)) => {
            let distance = between(first, second);
            Some(
                (distance - (first_radius + second_radius))
                    .abs()
                    .min((distance - (first_radius - second_radius).abs()).abs()),
            )
        }
        _ => None,
    }
}

/// The distance between two points, a point and a line, or two parallel
/// lines.
fn separation(first: Shape, second: Shape) -> Option<f64> {
    match (first, second) {
        (Shape::Line(through, direction, _), Shape::Line(other, other_direction, _)) => {
            (cross(direction, other_direction).abs() == 0.0)
                .then(|| to_line(other, through, direction))
        }
        (Shape::Point(_), Shape::Point(_) | Shape::Line(..))
        | (Shape::Line(..), Shape::Point(_)) => incidence(first, second),
        _ => None,
    }
}

/// The distance between two points along the sketch's x or y axis.
fn axis_separation(first: Shape, second: Shape, axis: usize) -> Option<f64> {
    let (Shape::Point(first), Shape::Point(second)) = (first, second) else {
        return None;
    };
    Some((first[axis] - second[axis]).abs())
}

#[cfg(test)]
mod tests;
