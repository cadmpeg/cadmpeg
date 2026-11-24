// SPDX-License-Identifier: Apache-2.0
//! A construction's `resolved`, and each `resolved_in` value, against each
//! input that arithmetic on its reference's stored values decides (part 10,
//! Caches): a point, line, plane, or frame reference under `Coincident`,
//! `Parallel`, `Perpendicular`, `Offset`, `Angle`, `Midway` between two
//! points, or `Coordinates`, and `Center` or `AxisOf` of a circle or arc.
//! Lengths agree within the linear tolerance and directions within the
//! angular tolerance; each disagreement is a Warning.
//!
//! A reference is read from a nested construction's value in the same
//! configuration, from a principal entity of the definition, and, for
//! `resolved` alone, from
//! a pick anchored in the current state that resolves to a vertex, a
//! straight or circular edge, or a planar or cylindrical face of the current
//! geometry. A construction whose `after` holds a step other than `Flip`
//! states a value its inputs do not fix, and is not checked.

use std::f64::consts::PI;

use super::super::dimensions::{from_cache, Bound};
use super::super::parameters::cache_in;
use super::super::records::DesignRecords;
use super::super::walk::ConstructionView;
use super::super::warning;
use crate::design::configuration::DesignConfiguration;
use crate::design::construction::{
    AnyConstruction, AxisValue, ConstructionInput, CurveValue, FrameValue, InputReference,
    PlaneValue, PostStep, Relation, ScalarDimension, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::refs::EntityRef;
use crate::design::selection::{Anchor, PickTarget, Picks, PrincipalEntity};
use crate::features::geometry::FinitePoint3;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::portable::Portable;
use crate::math::Vector3;
use crate::report::check::Finding;
use crate::units::UnitVector3;

/// A value a subject or a reference supplies.
#[derive(Clone, Copy)]
enum Locus {
    Point(Vector3),
    Direction(Vector3),
    /// A point on the line and its unit direction.
    Line(Vector3, Vector3),
    /// A point on the plane and its unit normal.
    Plane(Vector3, Vector3),
    /// The center and the unit axis of a circle or of a surface of revolution.
    Round(Vector3, Vector3),
    /// The origin and the unit x, y, and z axes.
    Frame(Vector3, [Vector3; 3]),
}

/// What a residual measures.
#[derive(Clone, Copy)]
enum Measure {
    Length,
    Angle,
}

/// The residuals of one input, each with what it measures.
type Residuals = Vec<(f64, Measure)>;

struct Reader<'a, 'v> {
    index: &'v DesignRecords<'a>,
    chain: Vec<&'a DesignConfiguration>,
    /// The configuration whose value is read; `None` is `resolved`.
    configuration: Option<&'a str>,
}

/// Checks `construction`'s `resolved` in the scope's active configuration and
/// each `resolved_in` value in its configuration.
pub(in crate::validate::design) fn check_construction<'a>(
    index: &DesignRecords<'a>,
    construction: ConstructionView<'a>,
    scope: Option<&str>,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let after = match construction {
        ConstructionView::Point(c) => &c.after,
        ConstructionView::Direction(c) => &c.after,
        ConstructionView::Axis(c) => &c.after,
        ConstructionView::Plane(c) => &c.after,
        ConstructionView::Frame(c) => &c.after,
        ConstructionView::Curve(_) => return,
    };
    if after.iter().any(|step| !matches!(step, PostStep::Flip(_))) {
        return;
    }
    let tolerances = index.ir.tolerances;
    let (linear, angular) = (tolerances.linear.get(), tolerances.angular.get());
    let active = scope
        .and_then(|scope| index.scopes.get(scope))
        .and_then(|scope| scope.active_configuration.as_ref())
        .map(crate::features::ConfigurationId::as_str);
    let mut values: Vec<(Option<&'a str>, Value)> = Vec::new();
    if let Some(value) = value_of(construction, None) {
        values.push((None, value));
    }
    for configuration in configurations(construction) {
        if let Some(value) = value_of(construction, Some(configuration)) {
            values.push((Some(configuration), value));
        }
    }
    let inputs = construction.inputs();
    for (configuration, value) in values {
        let reader = Reader {
            index,
            chain: index.configuration_chain(configuration.or(active)),
            configuration,
        };
        for input in inputs {
            let residuals = reader.residual(value, input).unwrap_or_default();
            let Some(residual) = residuals
                .into_iter()
                .filter(|(residual, measure)| {
                    *residual
                        > match measure {
                            Measure::Length => linear,
                            Measure::Angle => angular,
                        }
                })
                .map(|(residual, _)| residual)
                .reduce(f64::max)
            else {
                continue;
            };
            {
                let which = configuration.map_or_else(
                    || "resolved".to_owned(),
                    |configuration| format!("resolved_in {configuration}"),
                );
                warning(
                    findings,
                    at,
                    format_args!(
                        "a construction's {which} misses its {:?} input by {residual}",
                        input.role.as_ref().map(|role| &role.relation)
                    ),
                );
            }
        }
    }
}

/// A construction's value, by kind.
#[derive(Clone, Copy)]
enum Value {
    Point(Vector3),
    Direction(Vector3),
    Axis(Vector3, Vector3),
    Plane(Vector3, Vector3, Vector3),
    Frame(Vector3, [Vector3; 3]),
}

fn point(value: FinitePoint3) -> Vector3 {
    let point = value.get();
    Vector3::new(point.x, point.y, point.z)
}

fn unit(value: UnitVector3) -> Vector3 {
    Vector3::from(value)
}

fn axis_value(value: &AxisValue) -> Value {
    Value::Axis(point(value.origin), unit(value.direction))
}

fn plane_value(value: &PlaneValue) -> Value {
    Value::Plane(point(value.origin), unit(value.normal), unit(value.x_axis))
}

fn frame_value(value: &FrameValue) -> Value {
    Value::Frame(
        point(value.origin),
        [unit(value.x_axis), unit(value.y_axis), unit(value.z_axis)],
    )
}

fn value_of(construction: ConstructionView<'_>, configuration: Option<&str>) -> Option<Value> {
    macro_rules! read {
        ($c:expr, $map:expr) => {
            match configuration {
                None => $c.resolved.as_ref().map($map),
                Some(configuration) => $c
                    .resolved_in
                    .iter()
                    .find(|(id, _)| id.as_str() == configuration)
                    .map(|(_, value)| $map(value)),
            }
        };
    }
    match construction {
        ConstructionView::Point(c) => read!(c, |value: &FinitePoint3| Value::Point(point(*value))),
        ConstructionView::Direction(c) => {
            read!(c, |value: &UnitVector3| Value::Direction(unit(*value)))
        }
        ConstructionView::Axis(c) => read!(c, axis_value),
        ConstructionView::Plane(c) => read!(c, plane_value),
        ConstructionView::Frame(c) => read!(c, frame_value),
        ConstructionView::Curve(_) => None,
    }
}

fn configurations(construction: ConstructionView<'_>) -> Vec<&str> {
    construction
        .resolved_in()
        .into_iter()
        .map(crate::features::ConfigurationId::as_str)
        .collect()
}

/// The part of `value` that `subject` names.
fn subject(value: Value, subject: Subject) -> Option<Locus> {
    match (value, subject) {
        (Value::Point(point), Subject::Whole) => Some(Locus::Point(point)),
        (Value::Direction(direction), Subject::Whole) => Some(Locus::Direction(direction)),
        (Value::Axis(origin, direction), Subject::Whole) => Some(Locus::Line(origin, direction)),
        (Value::Plane(origin, normal, _), Subject::Whole) => Some(Locus::Plane(origin, normal)),
        (Value::Plane(origin, ..) | Value::Frame(origin, _), Subject::Origin) => {
            Some(Locus::Point(origin))
        }
        (Value::Plane(origin, _, x), Subject::XAxis) => Some(Locus::Line(origin, x)),
        (Value::Plane(origin, normal, x), Subject::YAxis) => {
            Some(Locus::Line(origin, normal.cross(x)))
        }
        (Value::Frame(origin, axes), Subject::Whole) => Some(Locus::Plane(origin, axes[2])),
        (Value::Frame(origin, axes), Subject::XAxis) => Some(Locus::Line(origin, axes[0])),
        (Value::Frame(origin, axes), Subject::YAxis) => Some(Locus::Line(origin, axes[1])),
        (Value::Frame(origin, axes), Subject::ZAxis) => Some(Locus::Line(origin, axes[2])),
        _ => None,
    }
}

fn direction(locus: Locus) -> Option<Vector3> {
    match locus {
        Locus::Direction(direction)
        | Locus::Line(_, direction)
        | Locus::Plane(_, direction)
        | Locus::Round(_, direction) => Some(direction),
        Locus::Frame(_, axes) => Some(axes[2]),
        Locus::Point(_) => None,
    }
}

/// The angle between two undirected directions, in [0, π/2].
fn skew(first: Vector3, second: Vector3) -> f64 {
    first.cross(second).norm().min(1.0).portable_asin()
}

fn to_line(point: Vector3, origin: Vector3, direction: Vector3) -> f64 {
    (point - origin).cross(direction).norm()
}

fn to_plane(point: Vector3, origin: Vector3, normal: Vector3) -> f64 {
    (point - origin).dot(normal)
}

/// How far `subject` is from lying in `reference`, or `reference` in it, or
/// from coinciding with it where both have one dimension.
fn incidence(subject: Locus, reference: Locus) -> Option<Residuals> {
    let length = |residual: f64| Some(vec![(residual, Measure::Length)]);
    match (subject, reference) {
        (Locus::Point(first), Locus::Point(second)) => length((first - second).norm()),
        (Locus::Point(point), Locus::Line(origin, direction))
        | (Locus::Line(origin, direction), Locus::Point(point)) => {
            length(to_line(point, origin, direction))
        }
        (
            Locus::Point(point),
            Locus::Plane(origin, normal) | Locus::Frame(origin, [_, _, normal]),
        )
        | (Locus::Plane(origin, normal), Locus::Point(point)) => {
            length(to_plane(point, origin, normal).abs())
        }
        (Locus::Line(origin, direction), Locus::Plane(at, normal))
        | (Locus::Plane(at, normal), Locus::Line(origin, direction)) => Some(vec![
            (
                direction.dot(normal).abs().min(1.0).portable_asin(),
                Measure::Angle,
            ),
            (to_plane(origin, at, normal).abs(), Measure::Length),
        ]),
        (Locus::Line(origin, direction), Locus::Line(other, other_direction)) => Some(vec![
            (skew(direction, other_direction), Measure::Angle),
            (to_line(origin, other, other_direction), Measure::Length),
        ]),
        (Locus::Plane(origin, normal), Locus::Plane(other, other_normal)) => Some(vec![
            (skew(normal, other_normal), Measure::Angle),
            (to_plane(origin, other, other_normal).abs(), Measure::Length),
        ]),
        _ => None,
    }
}

impl Reader<'_, '_> {
    fn value<T: Bound + Clone>(&self, dimension: &Dimension<T>) -> Option<T> {
        let (parameter, negated) = match dimension {
            Dimension::Literal(value) => return Some(value.clone()),
            Dimension::Parameter(parameter) => (parameter, false),
            Dimension::Negated(parameter) => (parameter, true),
        };
        from_cache(cache_in(self.index, &self.chain, parameter)?, negated)
    }

    /// The references of an input, each read as a locus.
    fn references(&self, input: &ConstructionInput) -> Option<Vec<Locus>> {
        match input.reference.as_ref()? {
            InputReference::Built(built) => Some(vec![self.built(built)?]),
            InputReference::Picks(selection) => {
                let Picks::Named(picks) = &selection.picks else {
                    return None;
                };
                picks
                    .as_slice()
                    .iter()
                    .map(|pick| match &pick.target {
                        PickTarget::Principal {
                            entity,
                            container: None,
                        } => Some(principal_locus(*entity)),
                        _ if self.configuration.is_none()
                            && matches!(selection.at, Anchor::Current) =>
                        {
                            match pick.resolved.as_deref()? {
                                [entity] => self.entity(entity),
                                _ => None,
                            }
                        }
                        _ => None,
                    })
                    .collect()
            }
        }
    }

    fn built(&self, built: &AnyConstruction) -> Option<Locus> {
        let view = match built {
            AnyConstruction::Point(c) => ConstructionView::Point(c),
            AnyConstruction::Direction(c) => ConstructionView::Direction(c),
            AnyConstruction::Axis(c) => ConstructionView::Axis(c),
            AnyConstruction::Plane(c) => ConstructionView::Plane(c),
            AnyConstruction::Frame(c) => ConstructionView::Frame(c),
            AnyConstruction::Curve(c) => {
                let value = match self.configuration {
                    None => c.resolved.as_ref(),
                    Some(configuration) => c
                        .resolved_in
                        .iter()
                        .find(|(id, _)| id.as_str() == configuration)
                        .map(|(_, value)| value),
                }?;
                return match value {
                    CurveValue::Segment { start, end } => {
                        let (start, end) = (point(*start), point(*end));
                        Some(Locus::Line(start, (end - start).unit()?))
                    }
                    CurveValue::Arc { center, normal, .. }
                    | CurveValue::Circle { center, normal, .. } => {
                        Some(Locus::Round(point(*center), unit(*normal)))
                    }
                };
            }
        };
        Some(match value_of(view, self.configuration)? {
            Value::Point(point) => Locus::Point(point),
            Value::Direction(direction) => Locus::Direction(direction),
            Value::Axis(origin, direction) => Locus::Line(origin, direction),
            Value::Plane(origin, normal, _) => Locus::Plane(origin, normal),
            Value::Frame(origin, axes) => Locus::Frame(origin, axes),
        })
    }

    /// A vertex, a straight or circular edge, or a planar or cylindrical
    /// face of the current geometry.
    fn entity(&self, entity: &EntityRef) -> Option<Locus> {
        let geometry = &self.index.ir.geometry;
        match entity {
            EntityRef::Vertex(id) => {
                let vertex = geometry.vertices.iter().find(|vertex| &vertex.id == id)?;
                let at = geometry.points.iter().find(|at| at.id == vertex.point)?;
                Some(Locus::Point(point(at.position())))
            }
            EntityRef::Edge(id) => {
                let edge = geometry.edges.iter().find(|edge| &edge.id == id)?;
                let curve = edge.curve()?;
                let curve = geometry
                    .curves
                    .iter()
                    .find(|candidate| &candidate.id == curve)?;
                match &curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
                        Some(Locus::Line(point(line.origin()), unit(line.direction())))
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => Some(
                        Locus::Round(point(circle.center()), unit(*circle.frame().axis())),
                    ),
                    _ => None,
                }
            }
            EntityRef::Face(id) => {
                let face = geometry.faces.iter().find(|face| &face.id == id)?;
                let surface = geometry
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)?;
                match &surface.geometry {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => Some(
                        Locus::Plane(point(plane.origin()), unit(*plane.frame().axis())),
                    ),
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => Some(
                        Locus::Round(point(cylinder.origin()), unit(*cylinder.frame().axis())),
                    ),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The residual of one input on the construction's `value`, where
    /// arithmetic decides it.
    fn residual(&self, value: Value, input: &ConstructionInput) -> Option<Residuals> {
        let role = input.role.as_ref()?;
        let subject = subject(value, role.subject)?;
        let angle = |residual: f64| Some(vec![(residual, Measure::Angle)]);
        let length = |residual: f64| Some(vec![(residual, Measure::Length)]);
        match &role.relation {
            Relation::Coincident => {
                let [reference] = self.references(input)?[..] else {
                    return None;
                };
                incidence(subject, reference)
            }
            Relation::Parallel => {
                let [reference] = self.references(input)?[..] else {
                    return None;
                };
                angle(skew(direction(subject)?, direction(reference)?))
            }
            Relation::Perpendicular => {
                let [reference] = self.references(input)?[..] else {
                    return None;
                };
                angle(
                    direction(subject)?
                        .dot(direction(reference)?)
                        .abs()
                        .min(1.0)
                        .portable_asin(),
                )
            }
            Relation::Offset => {
                let [reference] = self.references(input)?[..] else {
                    return None;
                };
                let Some(ScalarDimension::Length(distance)) = &input.value else {
                    return None;
                };
                let distance = self.value(distance)?.get().abs();
                match (subject, reference) {
                    (Locus::Point(first), Locus::Point(second)) => {
                        length(((first - second).norm() - distance).abs())
                    }
                    (Locus::Point(point), Locus::Plane(origin, normal))
                    | (Locus::Plane(origin, normal), Locus::Point(point)) => {
                        length((to_plane(point, origin, normal).abs() - distance).abs())
                    }
                    (Locus::Plane(origin, normal), Locus::Plane(other, other_normal)) => {
                        Some(vec![
                            (skew(normal, other_normal), Measure::Angle),
                            (
                                (to_plane(origin, other, other_normal).abs() - distance).abs(),
                                Measure::Length,
                            ),
                        ])
                    }
                    _ => None,
                }
            }
            Relation::Angle => {
                let [reference] = self.references(input)?[..] else {
                    return None;
                };
                let Some(ScalarDimension::Angle(expected)) = &input.value else {
                    return None;
                };
                let expected = self.value(expected)?.get().abs().rem_euclid(PI);
                let (first, second) = (direction(subject)?, direction(reference)?);
                let between = first.dot(second).clamp(-1.0, 1.0).portable_acos();
                let difference = (expected - between).abs();
                angle(difference.min((PI - expected - between).abs()))
            }
            Relation::Midway => {
                if input.value.is_some() {
                    return None;
                }
                let [Locus::Point(first), Locus::Point(second)] = self.references(input)?[..]
                else {
                    return None;
                };
                let Locus::Point(at) = subject else {
                    return None;
                };
                length((at - (first + second).scale(0.5)).norm())
            }
            Relation::Coordinates => self.coordinates(subject, input),
            Relation::Center => {
                let [Locus::Round(center, _)] = self.references(input)?[..] else {
                    return None;
                };
                let Locus::Point(at) = subject else {
                    return None;
                };
                length((at - center).norm())
            }
            Relation::AxisOf => {
                let [Locus::Round(center, axis)] = self.references(input)?[..] else {
                    return None;
                };
                let Locus::Line(origin, direction) = subject else {
                    return None;
                };
                Some(vec![
                    (skew(direction, axis), Measure::Angle),
                    (to_line(origin, center, axis), Measure::Length),
                ])
            }
            _ => None,
        }
    }

    /// A point's position, a direction's components, or a plane's equation
    /// in the reference frame, the definition frame where the input has no
    /// reference.
    fn coordinates(&self, subject: Locus, input: &ConstructionInput) -> Option<Residuals> {
        let (origin, axes) = match &input.reference {
            None => (
                Vector3::new(0.0, 0.0, 0.0),
                [
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                ],
            ),
            Some(_) => match self.references(input)?[..] {
                [Locus::Frame(origin, axes)] => (origin, axes),
                _ => return None,
            },
        };
        let in_frame = |[x, y, z]: [f64; 3]| axes[0].scale(x) + axes[1].scale(y) + axes[2].scale(z);
        match (subject, input.value.as_ref()?) {
            (Locus::Point(at), ScalarDimension::Position { x, y, z }) => {
                let expected = origin
                    + in_frame([
                        self.value(x)?.get(),
                        self.value(y)?.get(),
                        self.value(z)?.get(),
                    ]);
                Some(vec![((at - expected).norm(), Measure::Length)])
            }
            (
                Locus::Direction(direction) | Locus::Line(_, direction),
                ScalarDimension::Components { x, y, z },
            ) => {
                let expected = in_frame([
                    self.value(x)?.get(),
                    self.value(y)?.get(),
                    self.value(z)?.get(),
                ])
                .unit()?;
                Some(vec![(skew(direction, expected), Measure::Angle)])
            }
            (Locus::Plane(at, normal), ScalarDimension::PlaneEquation { a, b, c, d }) => {
                if input.reference.is_some() {
                    return None;
                }
                let coefficients = Vector3::new(
                    self.value(a)?.get(),
                    self.value(b)?.get(),
                    self.value(c)?.get(),
                );
                let magnitude = coefficients.norm();
                let unit_normal = coefficients.unit()?;
                Some(vec![
                    (skew(normal, unit_normal), Measure::Angle),
                    (
                        (coefficients.dot(at) - self.value(d)?.get()).abs() / magnitude,
                        Measure::Length,
                    ),
                ])
            }
            _ => None,
        }
    }
}

/// A principal entity in the definition frame.
fn principal_locus(principal: PrincipalEntity) -> Locus {
    let origin = Vector3::new(0.0, 0.0, 0.0);
    let [x, y, z] = [
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ];
    match principal {
        PrincipalEntity::Origin => Locus::Point(origin),
        PrincipalEntity::XAxis => Locus::Line(origin, x),
        PrincipalEntity::YAxis => Locus::Line(origin, y),
        PrincipalEntity::ZAxis => Locus::Line(origin, z),
        PrincipalEntity::XyPlane => Locus::Plane(origin, z),
        PrincipalEntity::XzPlane => Locus::Plane(origin, y),
        PrincipalEntity::YzPlane => Locus::Plane(origin, x),
        PrincipalEntity::Frame => Locus::Frame(origin, [x, y, z]),
    }
}

#[cfg(test)]
mod tests;
