// SPDX-License-Identifier: Apache-2.0
//! The dimensions of a written sketch as Fusion sketch dimensions (f3d.md
//! §3.1, "Sketch dimensions"): which source dimensions the writer writes, the
//! measurement of each on the written geometry, and the parameter each owns.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, AngleSector, DistanceAxis, Operand, Planar, RadialMeasure, SketchConstraint,
    SketchRelation,
};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::SketchConstraintId;

use super::{orientation, CurveGeometry, Entity, Frame, PlannedCurve, RelationKind};
use crate::writer::generate::design::expression;
use crate::writer::generate::design::parameters::{OwnedParameter, ParameterPlan};

/// The smallest distance of a linear dimension line from its anchors, in
/// cm.
const MIN_OFFSET: f64 = 0.1;
/// The distance of a linear dimension line from its anchors, as a share of
/// the measured value.
const OFFSET_SHARE: f64 = 0.2;
/// The distance of a radial text position from the centre, in radii.
const RADIAL_TEXT: f64 = 1.5;
/// The direction of a circle's attach point, from its u axis.
const CIRCLE_ATTACH: f64 = std::f64::consts::FRAC_PI_4;
const HALF_TURN: f64 = std::f64::consts::PI;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

/// The kind of a written dimension: its frame type and kind mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::writer::generate::design) enum DimensionKind {
    Aligned,
    Horizontal,
    Vertical,
    Radius,
    Diameter,
    Angle,
}

impl DimensionKind {
    /// The kind mask (f3d.md §3.1, "Sketch dimensions").
    pub(in crate::writer::generate::design) const fn mask(self) -> u64 {
        match self {
            Self::Aligned => 0x4000,
            Self::Horizontal => 0x40_0000,
            Self::Vertical => 0x80_0000,
            Self::Radius => 0x2_0000,
            Self::Diameter => 0x1_0000,
            Self::Angle => 0x8000,
        }
    }

    /// The parameter's source-kind stem.
    pub(in crate::writer::generate::design) const fn stem(self) -> &'static str {
        match self {
            Self::Aligned | Self::Horizontal | Self::Vertical => "Linear Dimension",
            Self::Radius => "Radial Dimension",
            Self::Diameter => "Diameter Dimension",
            Self::Angle => "Angular Dimension",
        }
    }

    /// The prefix after `fx: `: `Ø` for a diameter, `R` for a radius.
    pub(in crate::writer::generate::design) const fn symbol(self) -> &'static str {
        match self {
            Self::Radius => "R",
            Self::Diameter => "\u{d8}",
            Self::Aligned | Self::Horizontal | Self::Vertical | Self::Angle => "",
        }
    }

    const fn quantity(self) -> Quantity {
        match self {
            Self::Angle => Quantity::ANGLE,
            Self::Aligned | Self::Horizontal | Self::Vertical | Self::Radius | Self::Diameter => {
                Quantity::LENGTH
            }
        }
    }

    const fn unit(self) -> UnitSymbol {
        match self {
            Self::Angle => UnitSymbol::Deg,
            Self::Aligned | Self::Horizontal | Self::Vertical | Self::Radius | Self::Diameter => {
                UnitSymbol::Mm
            }
        }
    }
}

/// The measurement of a written dimension in Fusion sketch coordinates (cm,
/// rad).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::writer::generate::design) enum Measure {
    /// The dimension-line ends `P0` and `P1`, the anchors `A1` and `A2`,
    /// the value `|P0 P1|` and the text position.
    Linear {
        p0: [f64; 2],
        p1: [f64; 2],
        a1: [f64; 2],
        a2: [f64; 2],
        value: f64,
        text: [f64; 2],
    },
    /// The measured curve, its centre, the attach point on it, its radius
    /// and the text position.
    Radial {
        curve: usize,
        centre: [f64; 2],
        attach: [f64; 2],
        radius: f64,
        text: [f64; 2],
    },
    /// Each line's end and start point positions, the intersection of the
    /// two carriers, the arc radius, the angle `θ` between `P0 - P1` and
    /// `P2 - P3`, and the quadrant: `0` measures `θ`, `1` its supplement.
    Angle {
        points: [[f64; 2]; 4],
        vertex: [f64; 2],
        radius: f64,
        theta: f64,
        quadrant: u32,
    },
}

impl Measure {
    /// The measured value: a length, a radius, or the angle of the
    /// quadrant.
    pub(in crate::writer::generate::design) fn value(&self) -> f64 {
        match *self {
            Self::Linear { value, .. } => value,
            Self::Radial { radius, .. } => radius,
            Self::Angle {
                theta, quadrant, ..
            } => {
                if quadrant == 0 {
                    theta
                } else {
                    HALF_TURN - theta
                }
            }
        }
    }

    /// The arc point of an angle: on the circle about the vertex, inside the
    /// sector whose angle is the value.
    pub(in crate::writer::generate::design) fn arc(&self) -> Option<[f64; 2]> {
        let Self::Angle {
            points: [p0, p1, p2, p3],
            vertex,
            radius,
            quadrant,
            ..
        } = *self
        else {
            return None;
        };
        let (u1, u2) = (unit(sub(p0, p1))?, unit(sub(p2, p3))?);
        let bisector = if quadrant == 0 {
            add(u1, u2)
        } else {
            sub(u1, u2)
        };
        Some(add(vertex, scale(unit(bisector)?, radius)))
    }
}

/// A written dimension of a sketch: its kind, its members in semantic order,
/// its measurement and the parameter it owns.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::writer::generate::design) struct PlannedDimension {
    pub(in crate::writer::generate::design) kind: DimensionKind,
    pub(in crate::writer::generate::design) members: Vec<Entity>,
    pub(in crate::writer::generate::design) measure: Measure,
    pub(in crate::writer::generate::design) parameter: OwnedParameter,
}

/// What a dimension's value binds.
#[derive(Clone, Debug, PartialEq)]
enum Binding<'a> {
    Parameter(&'a ParameterId),
    /// A literal in canonical units: mm or rad.
    Literal(f64),
}

/// A dimension measured on the written geometry, before its parameter is
/// planned.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::writer::generate::design) struct Candidate<'a> {
    pub(in crate::writer::generate::design) constraint: &'a SketchConstraintId,
    kind: DimensionKind,
    members: Vec<Entity>,
    measure: Measure,
    binding: Binding<'a>,
}

/// The tolerances a measured value is checked with: cm and rad.
#[derive(Clone, Copy, Debug)]
pub(in crate::writer::generate::design) struct Tolerances {
    pub(in crate::writer::generate::design) linear: f64,
    pub(in crate::writer::generate::design) angular: f64,
}

impl Candidate<'_> {
    /// The written dimension with source ordinal `ordinal`: the parameter it
    /// binds when the dimension can own it, else a new `d<ordinal>` naming
    /// it; or the reason it is not written.
    pub(in crate::writer::generate::design) fn plan(
        self,
        parameters: &mut ParameterPlan,
        ordinal: u32,
        tolerances: Tolerances,
    ) -> Result<PlannedDimension, &'static str> {
        let Self {
            kind,
            members,
            mut measure,
            binding,
            ..
        } = self;
        let quantity = kind.quantity();
        let value = match &binding {
            Binding::Parameter(parameter) => {
                parameters.value(parameter, quantity).ok_or(match kind {
                    DimensionKind::Angle => "its parameter is not written as an angle",
                    _ => "its parameter is not written as a length",
                })?
            }
            Binding::Literal(value) if kind == DimensionKind::Angle => *value,
            Binding::Literal(value) => value / MM_PER_CM,
        };
        if kind != DimensionKind::Angle && value < 0.0 {
            return Err("a Fusion dimension is unsigned");
        }
        if let Measure::Angle {
            theta, quadrant, ..
        } = &mut measure
        {
            *quadrant = u32::from((*theta - value).abs() > (HALF_TURN - *theta - value).abs());
        }
        let (measured, tolerance) = match kind {
            DimensionKind::Angle => (measure.value(), tolerances.angular),
            DimensionKind::Diameter => (2.0 * measure.value(), tolerances.linear),
            _ => (measure.value(), tolerances.linear),
        };
        if value.is_nan() || (measured - value).abs() > tolerance {
            return Err("its value disagrees with its written geometry");
        }
        let parameter = match binding {
            Binding::Parameter(id) => match parameters.claim(id, quantity, ordinal) {
                Some(claimed) => claimed,
                None => {
                    let name = parameters
                        .name(id)
                        .ok_or("its parameter is not written")?
                        .to_owned();
                    parameters.owned(ordinal, name, kind.unit(), value, true)
                }
            },
            Binding::Literal(literal) => {
                let text = expression::quantity_literal(literal, Some(kind.unit()))
                    .ok_or("its value has no Fusion literal")?;
                parameters.owned(ordinal, text, kind.unit(), value, false)
            }
        };
        Ok(PlannedDimension {
            kind,
            members,
            measure,
            parameter,
        })
    }
}

/// The written geometry a dimension is measured on.
pub(super) struct Geometry<'p> {
    pub(super) points: &'p [[f64; 2]],
    pub(super) curves: &'p [PlannedCurve],
    pub(super) frame: Frame,
    pub(super) angular: f64,
}

/// The dimension `constraint` states, measured on `geometry`, with each
/// operand written as `member` gives it; or the reason it is not written.
pub(super) fn candidate<'a>(
    constraint: &'a SketchConstraint<Planar>,
    member: &dyn Fn(&Operand<Planar>) -> Result<Entity, &'static str>,
    geometry: &Geometry<'_>,
) -> Result<Candidate<'a>, &'static str> {
    let (kind, members, measure, binding) = match &constraint.relation {
        SketchRelation::Distance {
            a,
            b,
            along,
            doubled,
            value,
        } => {
            if *doubled == Some(true) {
                return Err(
                    "the geometry of a Fusion linear diameter is not stated (f3d-open-items.md DR-917)",
                );
            }
            let binding = binding(value, Length::get)?;
            let (a, b) = (member(a)?, member(b)?);
            let (kind, members, measure) = match along {
                None => return Err("it states no axis"),
                Some(DistanceAxis::Along(_)) => {
                    return Err("Fusion stores no dimension along a line")
                }
                Some(DistanceAxis::Aligned) => geometry.aligned(a, b)?,
                Some(DistanceAxis::Horizontal) => geometry.axial(a, b, geometry.frame.x)?,
                Some(DistanceAxis::Vertical) => geometry.axial(a, b, geometry.frame.y)?,
            };
            (kind, members, measure, binding)
        }
        SketchRelation::Radial { of, measure, value } => {
            let kind = match measure {
                RadialMeasure::Radius => DimensionKind::Radius,
                RadialMeasure::Diameter => DimensionKind::Diameter,
                RadialMeasure::MajorRadius | RadialMeasure::MinorRadius => {
                    return Err("the writer writes the radius or diameter of an arc or circle")
                }
            };
            let binding = binding(value, Length::get)?;
            let curve = member(&Operand::Entity(of.clone()))?;
            (kind, vec![curve], geometry.radial(curve)?, binding)
        }
        SketchRelation::Angle {
            between,
            sector,
            value,
        } => {
            let AngleOperands::Between([a, b]) = between else {
                return Err("the writer writes the angle between two lines");
            };
            if matches!(
                sector,
                Some(AngleSector::Major | AngleSector::Directed { .. } | AngleSector::Native(_))
            ) {
                return Err(
                    "Fusion's angular dimension holds the angle between two lines of at most a half turn",
                );
            }
            let binding = binding(value, Angle::get)?;
            let (a, b) = (member(a)?, member(b)?);
            (
                DimensionKind::Angle,
                vec![a, b],
                geometry.angle(a, b)?,
                binding,
            )
        }
        SketchRelation::Length { .. } => return Err("Fusion stores no arc-length dimension"),
        _ => return Err("it is no dimension"),
    };
    Ok(Candidate {
        constraint: &constraint.id,
        kind,
        members,
        measure,
        binding,
    })
}

fn binding<T: Copy>(
    value: &Dimension<T>,
    canonical: impl Fn(T) -> f64,
) -> Result<Binding<'_>, &'static str> {
    match value {
        Dimension::Parameter(parameter) => Ok(Binding::Parameter(parameter)),
        Dimension::Literal(value) => Ok(Binding::Literal(canonical(*value))),
        Dimension::Negated(_) => Err("a Fusion dimension cannot negate its parameter"),
    }
}

/// The written members of a linear dimension, in semantic order.
type Linear = (DimensionKind, Vec<Entity>, Measure);

impl Geometry<'_> {
    fn point(&self, entity: Entity) -> Option<[f64; 2]> {
        match entity {
            Entity::Point(point) => self.points.get(point).copied(),
            Entity::Curve(_) => None,
        }
    }

    /// The start and end of a written line.
    fn line(&self, entity: Entity) -> Option<([f64; 2], [f64; 2])> {
        let Entity::Curve(curve) = entity else {
            return None;
        };
        match self.curves.get(curve)?.geometry {
            CurveGeometry::Line { start, end } => {
                Some((*self.points.get(start)?, *self.points.get(end)?))
            }
            CurveGeometry::Arc { .. } | CurveGeometry::Circle { .. } => None,
        }
    }

    fn round(&self, entity: Entity) -> bool {
        matches!(entity, Entity::Curve(curve) if self.curves.get(curve).is_some_and(|curve| !matches!(curve.geometry, CurveGeometry::Line { .. })))
    }

    /// An aligned distance: between two points, a point and a line, or two
    /// parallel lines.
    fn aligned(&self, a: Entity, b: Entity) -> Result<Linear, &'static str> {
        if self.round(a) || self.round(b) {
            return Err(
                "the boundary a Fusion linear dimension to a circular curve measures is not stated (f3d-open-items.md DR-917)",
            );
        }
        let measured = match (self.point(a), self.point(b), self.line(a), self.line(b)) {
            (Some(pa), Some(pb), _, _) => {
                let direction = unit(sub(pa, pb)).ok_or("the points coincide")?;
                (vec![a, b], linear(direction, pa, pb))
            }
            (Some(point), None, None, Some(line)) | (None, Some(point), Some(line), None) => {
                let (line_entity, point_entity) = if self.line(a).is_some() {
                    (a, b)
                } else {
                    (b, a)
                };
                let normal = normal(line).ok_or("its line has no length")?;
                let foot = foot(point, line);
                (vec![line_entity, point_entity], linear(normal, foot, point))
            }
            (None, None, Some(first), Some(second)) => {
                let (n1, n2) = (
                    normal(first).ok_or("its line has no length")?,
                    normal(second).ok_or("its line has no length")?,
                );
                if cross(n1, n2).abs() > self.angular {
                    return Err("the lines are not parallel");
                }
                let a2 = scale(add(second.0, second.1), 0.5);
                (vec![a, b], linear(n1, foot(a2, first), a2))
            }
            _ => return Err("the writer writes a distance between points and lines"),
        };
        Ok((DimensionKind::Aligned, measured.0, measured.1))
    }

    /// A horizontal or vertical distance between two points, along the
    /// source sketch axis `axis` in Fusion's sketch coordinates.
    fn axial(&self, a: Entity, b: Entity, axis: [f64; 2]) -> Result<Linear, &'static str> {
        let (Some(pa), Some(pb)) = (self.point(a), self.point(b)) else {
            return Err("the writer writes a horizontal or vertical dimension between two points");
        };
        let (kind, direction) = match orientation(axis)? {
            RelationKind::Vertical => (DimensionKind::Vertical, [0.0, 1.0]),
            _ => (DimensionKind::Horizontal, [1.0, 0.0]),
        };
        Ok((kind, vec![a, b], linear(direction, pa, pb)))
    }

    /// The radius of a written arc or circle.
    fn radial(&self, entity: Entity) -> Result<Measure, &'static str> {
        let unmeasured = "the writer writes the radius or diameter of an arc or circle";
        let Entity::Curve(curve) = entity else {
            return Err(unmeasured);
        };
        let (center, radius, direction) = match self.curves.get(curve).ok_or(unmeasured)?.geometry {
            CurveGeometry::Arc {
                center,
                radius,
                angles,
                ..
            } => (center, radius, 0.5 * (angles[0] + angles[1])),
            CurveGeometry::Circle { center, radius } => (center, radius, CIRCLE_ATTACH),
            CurveGeometry::Line { .. } => return Err(unmeasured),
        };
        let centre = *self.points.get(center).ok_or(unmeasured)?;
        let (sin, cos) = direction.portable_sin_cos();
        Ok(Measure::Radial {
            curve,
            centre,
            attach: add(centre, [radius * cos, radius * sin]),
            radius,
            text: add(
                centre,
                [RADIAL_TEXT * radius * cos, RADIAL_TEXT * radius * sin],
            ),
        })
    }

    /// The angle between two written lines.
    fn angle(&self, a: Entity, b: Entity) -> Result<Measure, &'static str> {
        let (Some((s1, e1)), Some((s2, e2))) = (self.line(a), self.line(b)) else {
            return Err("the writer writes the angle between two lines");
        };
        let (d1, d2) = (sub(e1, s1), sub(e2, s2));
        let (Some(u1), Some(u2)) = (unit(d1), unit(d2)) else {
            return Err("its line has no length");
        };
        let sine = cross(u1, u2);
        if sine.abs() <= self.angular {
            return Err("the lines are parallel");
        }
        // The carriers meet at `s1 + t d1` with `(s1 + t d1 - s2) × d2 = 0`.
        let t = cross(sub(s2, s1), d2) / cross(d1, d2);
        Ok(Measure::Angle {
            points: [e1, s1, e2, s2],
            vertex: add(s1, scale(d1, t)),
            radius: 0.5 * dot(d1, d1).sqrt().min(dot(d2, d2).sqrt()),
            theta: sine.abs().portable_atan2(dot(u1, u2)),
            quadrant: 0,
        })
    }
}

/// The measurement of a linear dimension along `direction` from anchor `a1`
/// to anchor `a2`: the dimension line lies beside both anchors at a
/// distance in proportion to the value.
fn linear(direction: [f64; 2], a1: [f64; 2], a2: [f64; 2]) -> Measure {
    let side = [-direction[1], direction[0]];
    let value = dot(sub(a1, a2), direction).abs();
    let at = dot(side, a1).max(dot(side, a2)) + (OFFSET_SHARE * value).max(MIN_OFFSET);
    let p1 = add(a1, scale(side, at - dot(side, a1)));
    let p0 = add(a2, scale(side, at - dot(side, a2)));
    let value = dot(sub(p1, p0), sub(p1, p0)).sqrt();
    Measure::Linear {
        p0,
        p1,
        a1,
        a2,
        value,
        text: scale(add(p0, p1), 0.5),
    }
}

/// The unit normal of a line, its direction turned a quarter turn.
fn normal((start, end): ([f64; 2], [f64; 2])) -> Option<[f64; 2]> {
    unit(sub(end, start)).map(|direction| [-direction[1], direction[0]])
}

/// The point of the bounded line nearest the foot of `point` on its carrier.
fn foot(point: [f64; 2], (start, end): ([f64; 2], [f64; 2])) -> [f64; 2] {
    let span = sub(end, start);
    let length = dot(span, span);
    if length == 0.0 {
        return start;
    }
    let t = (dot(sub(point, start), span) / length).clamp(0.0, 1.0);
    add(start, scale(span, t))
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: [f64; 2], factor: f64) -> [f64; 2] {
    [a[0] * factor, a[1] * factor]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let length = dot(a, a).sqrt();
    (length > 0.0 && length.is_finite()).then(|| scale(a, 1.0 / length))
}
