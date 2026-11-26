// SPDX-License-Identifier: Apache-2.0
//! The forms of decoded sketch records that a planar sketch (§6.1 of the
//! CADIR v11 specification) states as they are: its curves, its frame, and
//! the relations and dimensions of the decoded vocabulary.

use cadmpeg_core::text::NonBlankString;

use crate::design::construction::{FrameValue, ScalarDimension};
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{
    AngleOperands, DistanceAxis, Operand, Planar, PlanarCurve, RadialMeasure, SketchRelation,
};
use crate::design::units::Quantity;
use crate::features::parameters::ParameterValue as StoredValue;
use crate::features::ParameterId;
use crate::math::portable::Portable;
use crate::scalar::{Angle, Length};
use crate::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition as Geometry, SketchLocus, SketchPlacement,
};
use crate::units::{UnitVector2, UnitVector3};

/// The planar curve of a decoded sketch geometry, where the geometry is a
/// point, a line, a circle, an arc, an ellipse, or a NURBS curve whose
/// directions have a length.
#[must_use]
pub fn planar_curve(geometry: &SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        Geometry::Point { position } => PlanarCurve::Point(*position),
        Geometry::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        Geometry::ReferenceLine { origin, direction } => {
            let [u, v] = [direction.get().u, direction.get().v];
            let length = u.portable_hypot(v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([u / length, v / length])?,
            }
        }
        Geometry::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => PlanarCurve::Arc {
            center: *center,
            radius: *radius,
            start: *start_angle,
            end: *end_angle,
        },
        Geometry::Ellipse {
            center,
            major_angle,
            radii,
            bounds,
        } => {
            let (sin, cos) = major_angle.get().portable_sin_cos();
            PlanarCurve::Ellipse {
                center: *center,
                major: UnitVector2::new([cos, sin])?,
                radii: [radii.major(), radii.minor()],
                range: *bounds,
            }
        }
        Geometry::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
        _ => return None,
    })
}

/// The frame `placement` stores, where the decoder resolved one: its `x`
/// axis the stored `u` axis and its `z` axis the stored normal.
#[must_use]
pub fn stored_frame(placement: SketchPlacement) -> Option<FrameValue> {
    let (origin, normal, u_axis) = placement.resolved()?;
    let z_axis = UnitVector3::normalized_nonzero(normal)?;
    let x_axis = UnitVector3::normalized_nonzero(u_axis)?;
    let y_axis = UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))?;
    Some(FrameValue {
        origin,
        x_axis,
        y_axis,
        z_axis,
    })
}

/// The points a decoded sketch names by a locus.
pub trait Loci {
    /// The operand of the point that `locus` names, where the sketch states
    /// that point as an entity.
    fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>>;

    /// The position of the point that `locus` names.
    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]>;
}

/// The value of a dimension: the design parameter it binds, or the value
/// that its decoded parameter record stores.
#[derive(Debug, Clone, PartialEq)]
pub enum DimensionValue {
    /// A design parameter.
    Parameter(ParameterId),
    /// A stored value.
    Stored(StoredValue),
}

impl DimensionValue {
    /// The length the value states, where it is a parameter or a stored
    /// length.
    #[must_use]
    pub fn length(&self) -> Option<Dimension<Length>> {
        match self {
            Self::Parameter(parameter) => Some(Dimension::Parameter(parameter.clone())),
            Self::Stored(StoredValue::Length(length)) => Some(Dimension::Literal(*length)),
            Self::Stored(_) => None,
        }
    }

    /// The angle the value states, where it is a parameter or a stored angle.
    #[must_use]
    pub fn angle(&self) -> Option<Dimension<Angle>> {
        match self {
            Self::Parameter(parameter) => Some(Dimension::Parameter(parameter.clone())),
            Self::Stored(StoredValue::Angle(angle)) => Some(Dimension::Literal(*angle)),
            Self::Stored(_) => None,
        }
    }

    /// The value as a scalar of `quantity`, an angle or a length.
    #[must_use]
    pub fn scalar(&self, quantity: Quantity) -> Option<ScalarDimension> {
        if quantity == Quantity::ANGLE {
            self.angle().map(ScalarDimension::Angle)
        } else {
            self.length().map(ScalarDimension::Length)
        }
    }
}

/// The value that the decoded parameter record `parameter` of `parameters`
/// stores, where it is a value of `quantity`, a length or an angle.
#[must_use]
pub fn stored_value(
    parameters: &[crate::features::parameters::DesignParameter],
    parameter: &ParameterId,
    quantity: Quantity,
) -> Option<DimensionValue> {
    let value = parameters
        .iter()
        .find(|candidate| &candidate.id == parameter)?
        .value
        .as_ref()?;
    match (quantity, value) {
        (Quantity::LENGTH, StoredValue::Length(_)) | (Quantity::ANGLE, StoredValue::Angle(_)) => {
            Some(DimensionValue::Stored(value.clone()))
        }
        _ => None,
    }
}

/// The geometric relation that `kind` states, where its operands are
/// entities or points that `loci` states.
pub fn geometric(kind: &Kind, loci: &impl Loci) -> Option<SketchRelation<Planar>> {
    let entity = |id: &SketchEntityId| Operand::Entity(id.clone());
    Some(match kind {
        Kind::Coincident { entities } if entities.len() == 2 => SketchRelation::Coincident {
            a: entity(&entities[0]),
            b: entity(&entities[1]),
        },
        Kind::CoincidentLoci { loci: pair } if pair.len() == 2 => SketchRelation::Coincident {
            a: loci.operand(&pair[0])?,
            b: loci.operand(&pair[1])?,
        },
        Kind::PointOnObject { point, entity: id } => SketchRelation::Coincident {
            a: loci.operand(point)?,
            b: entity(id),
        },
        Kind::Midpoint { point, entity: id } => match loci.operand(point)? {
            Operand::Entity(point) => SketchRelation::Midpoint {
                point,
                of: entity(id),
            },
            _ => return None,
        },
        Kind::Concentric { first, second } => SketchRelation::Concentric {
            a: entity(first),
            b: entity(second),
        },
        Kind::Collinear { first, second } => SketchRelation::Collinear {
            a: entity(first),
            b: entity(second),
        },
        Kind::Parallel { first, second } => SketchRelation::Parallel {
            a: entity(first),
            b: entity(second),
        },
        Kind::Perpendicular { first, second } => SketchRelation::Perpendicular {
            a: entity(first),
            b: entity(second),
        },
        Kind::Tangent { first, second } => SketchRelation::Tangent {
            a: entity(first),
            b: entity(second),
            contact: None,
        },
        Kind::Horizontal { entity: id } => SketchRelation::Horizontal {
            a: entity(id),
            b: None,
        },
        Kind::Vertical { entity: id } => SketchRelation::Vertical {
            a: entity(id),
            b: None,
        },
        Kind::Equal { first, second } => SketchRelation::Equal {
            members: vec![entity(first), entity(second)],
        },
        Kind::Fixed { entity: id } => SketchRelation::Fixed { of: entity(id) },
        Kind::Symmetric {
            first,
            second,
            axis,
        } => SketchRelation::Symmetric {
            a: loci.operand(first)?,
            b: loci.operand(second)?,
            about: entity(axis),
        },
        _ => return None,
    })
}

/// The parameter that the dimension `kind` binds and the quantity it
/// measures, where `kind` is a distance, a radius, a diameter, or an angle.
#[must_use]
pub fn dimension_parameter(kind: &Kind) -> Option<(&ParameterId, Quantity)> {
    Some(match kind {
        Kind::Distance { parameter, .. }
        | Kind::DistanceLoci { parameter, .. }
        | Kind::HorizontalDistance { parameter, .. }
        | Kind::VerticalDistance { parameter, .. }
        | Kind::Radius { parameter, .. }
        | Kind::Diameter { parameter, .. } => (parameter, Quantity::LENGTH),
        Kind::Angle { parameter, .. } => (parameter, Quantity::ANGLE),
        _ => return None,
    })
}

/// The dimension relation of `kind` that states `value`. A horizontal or
/// vertical distance orders its points so the stated value is the
/// non-negative difference (§6.1).
pub fn dimension(
    kind: &Kind,
    value: &DimensionValue,
    loci: &impl Loci,
) -> Option<SketchRelation<Planar>> {
    let along = |first: &SketchLocus, second: &SketchLocus, axis: usize, along| {
        let (a, b) = (loci.position(first)?, loci.position(second)?);
        let (a_operand, b_operand) = (loci.operand(first)?, loci.operand(second)?);
        let (a_operand, b_operand) = if b[axis] >= a[axis] {
            (a_operand, b_operand)
        } else {
            (b_operand, a_operand)
        };
        Some(SketchRelation::Distance {
            a: a_operand,
            b: b_operand,
            along: Some(along),
            doubled: None,
            value: value.length()?,
        })
    };
    Some(match kind {
        Kind::DistanceLoci { first, second, .. } => SketchRelation::Distance {
            a: loci.operand(first)?,
            b: loci.operand(second)?,
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: value.length()?,
        },
        Kind::Distance { entities, .. } if entities.len() == 2 => SketchRelation::Distance {
            a: Operand::Entity(entities[0].clone()),
            b: Operand::Entity(entities[1].clone()),
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: value.length()?,
        },
        Kind::HorizontalDistance { first, second, .. } => {
            along(first, second, 0, DistanceAxis::Horizontal)?
        }
        Kind::VerticalDistance { first, second, .. } => {
            along(first, second, 1, DistanceAxis::Vertical)?
        }
        Kind::Radius { entity, .. } => SketchRelation::Radial {
            of: entity.clone(),
            measure: RadialMeasure::Radius,
            value: value.length()?,
        },
        Kind::Diameter { entity, .. } => SketchRelation::Radial {
            of: entity.clone(),
            measure: RadialMeasure::Diameter,
            value: value.length()?,
        },
        Kind::Angle { first, second, .. } => SketchRelation::Angle {
            between: AngleOperands::Between([
                Operand::Entity(first.clone()),
                Operand::Entity(second.clone()),
            ]),
            sector: None,
            value: value.angle()?,
        },
        _ => return None,
    })
}

/// Whether `kind` is a relation of the decoded vocabulary that states a
/// value. A `Native` relation states one where its decoder's table says so.
#[must_use]
pub fn is_dimension(kind: &Kind) -> bool {
    match kind {
        Kind::PointCoordinateValues { .. }
        | Kind::MidpointCoordinate { .. }
        | Kind::Offset { .. }
        | Kind::ArcAngle { .. }
        | Kind::EllipseAngle { .. }
        | Kind::Distance { .. }
        | Kind::DistanceLoci { .. }
        | Kind::DistanceLociValue { .. }
        | Kind::PolarDistance { .. }
        | Kind::AngleDifference { .. }
        | Kind::HorizontalDistance { .. }
        | Kind::VerticalDistance { .. }
        | Kind::RepeatedDistance { .. }
        | Kind::RepeatedLength { .. }
        | Kind::ParallelLineSetDistance { .. }
        | Kind::Angle { .. }
        | Kind::AngleToAxis { .. }
        | Kind::Radius { .. }
        | Kind::RepeatedRadius { .. }
        | Kind::Diameter { .. }
        | Kind::RepeatedDiameter { .. }
        | Kind::SnellsLaw { .. }
        | Kind::Weight { .. } => true,
        Kind::Native { .. }
        | Kind::Disabled {}
        | Kind::Coincident { .. }
        | Kind::Polygon { .. }
        | Kind::SplineGroup { .. }
        | Kind::RectangularPattern { .. }
        | Kind::CircularPattern { .. }
        | Kind::TextFrame { .. }
        | Kind::TextPath { .. }
        | Kind::CoincidentLoci { .. }
        | Kind::SameCoordinate { .. }
        | Kind::PointOnObject { .. }
        | Kind::Midpoint { .. }
        | Kind::ProjectedCopy { .. }
        | Kind::AtIntersection { .. }
        | Kind::Concentric { .. }
        | Kind::Coradial { .. }
        | Kind::Collinear { .. }
        | Kind::Symmetric { .. }
        | Kind::PointSymmetric { .. }
        | Kind::Horizontal { .. }
        | Kind::Vertical { .. }
        | Kind::Parallel { .. }
        | Kind::Perpendicular { .. }
        | Kind::Tangent { .. }
        | Kind::TangentLoci { .. }
        | Kind::Curvature { .. }
        | Kind::Equal { .. }
        | Kind::Fixed { .. }
        | Kind::ScalarEquality { .. }
        | Kind::EqualDistance { .. }
        | Kind::InternalAlignment { .. }
        | Kind::Group { .. }
        | Kind::Text { .. } => false,
    }
}

/// The relation that states `kind` in its decoder's vocabulary, named
/// `<codec>:<native class>` for a native relation and `<codec>:<decoded
/// kind>` otherwise, on the entities `kind` names and with `values`.
#[must_use]
pub fn native_relation(
    codec: &str,
    kind: &Kind,
    values: Vec<ScalarDimension>,
) -> SketchRelation<Planar> {
    let name = match kind {
        Kind::Native { native_kind, .. } => qualified(codec, native_kind.as_str()),
        _ => qualified(
            codec,
            &tag(kind).unwrap_or_else(|| "sketch_relation".to_owned()),
        ),
    };
    SketchRelation::Native {
        kind: name,
        operands: operands(kind),
        values,
    }
}

/// The kind of an entity with no planar curve, named `<codec>:<native
/// class>` for a native geometry and `<codec>:<decoded kind>` otherwise.
#[must_use]
pub fn native_entity_kind(codec: &str, geometry: &SketchGeometry) -> NonBlankString {
    match geometry.definition() {
        Geometry::Native { native_kind } => qualified(codec, native_kind.as_str()),
        _ => qualified(
            codec,
            &tag(geometry).unwrap_or_else(|| "sketch_entity".to_owned()),
        ),
    }
}

/// `name` in the namespace of `codec`.
fn qualified(codec: &str, name: &str) -> NonBlankString {
    let prefix = format!("{codec}:");
    let name = if name.starts_with(&prefix) {
        name.to_owned()
    } else {
        format!("{prefix}{name}")
    };
    NonBlankString::new(name).unwrap_or_else(|| cadmpeg_core::nonblank_literal!("sketch"))
}

/// The serialized `kind` tag of `value`.
fn tag(value: &impl serde::Serialize) -> Option<String> {
    cadmpeg_core::serde_erase::to_json_value(value)
        .ok()?
        .get("kind")?
        .as_str()
        .map(str::to_owned)
}

/// The distinct entities that `kind` names, in the order it names them.
fn operands(kind: &Kind) -> Vec<Operand<Planar>> {
    let entities = match kind {
        Kind::Native { entities, .. }
        | Kind::Coincident { entities }
        | Kind::Distance { entities, .. } => entities.clone(),
        Kind::CoincidentLoci { loci } => loci.iter().map(locus_entity).cloned().collect(),
        Kind::PointOnObject { point, entity } | Kind::Midpoint { point, entity } => {
            vec![locus_entity(point).clone(), entity.clone()]
        }
        Kind::DistanceLoci { first, second, .. }
        | Kind::HorizontalDistance { first, second, .. }
        | Kind::VerticalDistance { first, second, .. } => {
            vec![locus_entity(first).clone(), locus_entity(second).clone()]
        }
        Kind::Radius { entity, .. } | Kind::Diameter { entity, .. } => vec![entity.clone()],
        Kind::Angle { first, second, .. } => vec![first.clone(), second.clone()],
        _ => Vec::new(),
    };
    let mut operands = Vec::with_capacity(entities.len());
    for entity in entities {
        if !operands.contains(&entity) {
            operands.push(entity);
        }
    }
    operands.into_iter().map(Operand::Entity).collect()
}

/// The entity a locus lies on.
fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(id)
        | SketchLocus::Start(id)
        | SketchLocus::End(id)
        | SketchLocus::Center(id) => id,
    }
}

#[cfg(test)]
mod tests;
