// SPDX-License-Identifier: Apache-2.0
//! The forms of decoded sketch records that a planar sketch (§6.1 of the
//! CADIR v11 specification) states as they are: its curves, its frame, and
//! the relations and dimensions of the decoded vocabulary.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_core::text::NonBlankString;

use crate::design::construction::{FrameValue, ScalarDimension};
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{
    AngleOperands, DefiningRole, DistanceAxis, EntityDefinition, EntityRole, Operand, Planar,
    PlanarCurve, RadialMeasure, SketchEntity, SketchRelation,
};
use crate::design::options::NativeOptions;
use crate::design::units::Quantity;
use crate::features::parameters::ParameterValue as StoredValue;
use crate::features::ParameterId;
use crate::math::portable::Portable;
use crate::scalar::{Angle, FiniteReal, Length};
use crate::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchPlacement,
};
use crate::units::{FinitePoint2, UnitVector2, UnitVector3};

/// The planar curve of a decoded sketch geometry, where the geometry is a
/// point, a line, a circle, an arc, an ellipse, or a NURBS curve whose
/// directions have a length.
#[must_use]
pub fn planar_curve(geometry: &SketchGeometry) -> Option<PlanarCurve> {
    Some(match geometry.definition() {
        SketchGeometryDefinition::Point { position } => PlanarCurve::Point(*position),
        SketchGeometryDefinition::Line { start, end } => PlanarCurve::Line {
            start: *start,
            end: *end,
        },
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let [u, v] = [direction.get().u, direction.get().v];
            let length = u.portable_hypot(v);
            PlanarCurve::InfiniteLine {
                through: *origin,
                direction: UnitVector2::new([u / length, v / length])?,
            }
        }
        SketchGeometryDefinition::Circle { center, radius } => PlanarCurve::Circle {
            center: *center,
            radius: *radius,
        },
        SketchGeometryDefinition::Arc {
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
        SketchGeometryDefinition::Ellipse {
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
        SketchGeometryDefinition::Nurbs { curve } => PlanarCurve::Nurbs(curve.clone()),
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

    /// The dimensionless value the value states, where it is a parameter or
    /// a stored real.
    #[must_use]
    pub fn ratio(&self) -> Option<Dimension<FiniteReal>> {
        match self {
            Self::Parameter(parameter) => Some(Dimension::Parameter(parameter.clone())),
            Self::Stored(StoredValue::Real(value)) => Some(Dimension::Literal(*value)),
            Self::Stored(_) => None,
        }
    }

    /// The value as a scalar of `quantity`: an angle, a dimensionless
    /// ratio, or a length.
    #[must_use]
    pub fn scalar(&self, quantity: Quantity) -> Option<ScalarDimension> {
        match quantity {
            Quantity::ANGLE => self.angle().map(ScalarDimension::Angle),
            Quantity::REAL => self.ratio().map(ScalarDimension::Ratio),
            _ => self.length().map(ScalarDimension::Length),
        }
    }
}

/// The quantity of the value that the decoded parameter record `parameter`
/// of `parameters` stores: a length, an angle, or a real.
#[must_use]
pub fn stored_quantity(
    parameters: &[crate::features::parameters::DesignParameter],
    parameter: &ParameterId,
) -> Option<Quantity> {
    match parameters
        .iter()
        .find(|candidate| &candidate.id == parameter)?
        .value
        .as_ref()?
    {
        StoredValue::Length(_) => Some(Quantity::LENGTH),
        StoredValue::Angle(_) => Some(Quantity::ANGLE),
        StoredValue::Real(_) => Some(Quantity::REAL),
        _ => None,
    }
}

/// The value that the decoded parameter record `parameter` of `parameters`
/// stores, where it is a value of `quantity`: a length, an angle, or a real.
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
        (Quantity::LENGTH, StoredValue::Length(_))
        | (Quantity::ANGLE, StoredValue::Angle(_))
        | (Quantity::REAL, StoredValue::Real(_)) => Some(DimensionValue::Stored(value.clone())),
        _ => None,
    }
}

/// The `Driven` parameter `parameter` of `quantity` that a reference
/// dimension of the sketch of `owner` binds where its scope states no
/// parameter of that id (§6.1 Dimensions, D11). Its value is the one its
/// record stores where that is of `quantity`.
pub fn driven_parameter(
    parameters: &[crate::features::parameters::DesignParameter],
    parameter: &ParameterId,
    quantity: Quantity,
    scope: &crate::ids::ProductDefinitionId,
    owner: &crate::features::FeatureId,
    name: Option<NonBlankString>,
) -> crate::design::parameter::DesignParameter {
    use crate::design::parameter::{ParameterDefinition, ParameterOwner, ParameterValue};
    let value = match stored_value(parameters, parameter, quantity) {
        Some(DimensionValue::Stored(StoredValue::Length(length))) => FiniteReal::new(length.get()),
        Some(DimensionValue::Stored(StoredValue::Angle(angle))) => FiniteReal::new(angle.get()),
        Some(DimensionValue::Stored(StoredValue::Real(value))) => Some(value),
        _ => None,
    };
    crate::design::parameter::DesignParameter {
        id: parameter.clone(),
        scope: scope.clone(),
        owner: Some(ParameterOwner::Feature(owner.clone())),
        name,
        kind: crate::design::units::ValueKind::Quantity(quantity),
        unit: None,
        definition: ParameterDefinition::Driven,
        value: value.map(ParameterValue::Number),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: parameters
            .iter()
            .find(|candidate| &candidate.id == parameter)
            .and_then(|source| source.native_ref.clone()),
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
/// measures, where `kind` is a distance, a radius, a diameter, or an angle,
/// or a native relation that names a parameter and whose quantity `native`
/// gives from its native class and its parameter.
pub fn dimension_parameter(
    kind: &Kind,
    native: impl FnOnce(&str, &ParameterId) -> Option<Quantity>,
) -> Option<(&ParameterId, Quantity)> {
    Some(match kind {
        Kind::Distance { parameter, .. }
        | Kind::DistanceLoci { parameter, .. }
        | Kind::HorizontalDistance { parameter, .. }
        | Kind::VerticalDistance { parameter, .. }
        | Kind::Radius { parameter, .. }
        | Kind::Diameter { parameter, .. } => (parameter, Quantity::LENGTH),
        Kind::Angle { parameter, .. } => (parameter, Quantity::ANGLE),
        Kind::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } => (parameter, native(native_kind.as_str(), parameter)?),
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
/// value. A `Native` relation states one where it names a parameter.
#[must_use]
pub fn is_dimension(kind: &Kind) -> bool {
    match kind {
        Kind::Native { parameter, .. } => parameter.is_some(),
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
        Kind::Disabled {}
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
/// kind>` otherwise, on the operands `kind` names and with `values`. A
/// point that `loci` states is its operand, and the curve it lies on
/// otherwise.
#[must_use]
pub fn native_relation(
    codec: &NonBlankString,
    kind: &Kind,
    values: Vec<ScalarDimension>,
    loci: &impl Loci,
) -> SketchRelation<Planar> {
    let name = match kind {
        Kind::Native { native_kind, .. } => qualified(codec, native_kind.as_str()),
        _ => qualified(codec, relation_name(kind)),
    };
    let mut operands = Vec::new();
    for operand in named(kind).into_iter().map(|named| match named {
        Named::Entity(id) => Operand::Entity(id.clone()),
        Named::Locus(locus) => loci
            .operand(locus)
            .unwrap_or_else(|| Operand::Entity(locus_entity(locus).clone())),
    }) {
        if !operands.contains(&operand) {
            operands.push(operand);
        }
    }
    SketchRelation::Native {
        kind: name,
        operands,
        values,
    }
}

/// The kind of an entity with no planar curve, named `<codec>:<native
/// class>` for a native geometry and `<codec>:<decoded kind>` otherwise.
#[must_use]
pub fn native_entity_kind(codec: &NonBlankString, geometry: &SketchGeometry) -> NonBlankString {
    let name = match geometry.definition() {
        SketchGeometryDefinition::Native { native_kind } => native_kind.as_str(),
        SketchGeometryDefinition::Point { .. } => "point",
        SketchGeometryDefinition::Line { .. } => "line",
        SketchGeometryDefinition::ReferenceLine { .. } => "reference_line",
        SketchGeometryDefinition::Circle { .. } => "circle",
        SketchGeometryDefinition::Arc { .. } => "arc",
        SketchGeometryDefinition::Ellipse { .. } => "ellipse",
        SketchGeometryDefinition::Hyperbola { .. } => "hyperbola",
        SketchGeometryDefinition::Parabola { .. } => "parabola",
        SketchGeometryDefinition::Nurbs { .. } => "nurbs",
        SketchGeometryDefinition::Text { .. } => "text",
        SketchGeometryDefinition::ExternalReference { .. } => "external_reference",
    };
    qualified(codec, name)
}

/// `name` in the namespace of `codec`.
pub(crate) fn qualified(codec: &NonBlankString, name: &str) -> NonBlankString {
    match name
        .strip_prefix(codec.as_str())
        .filter(|tail| tail.starts_with(':'))
    {
        Some(tail) => codec.with_suffix(tail),
        None => codec.with_suffix(&format!(":{name}")),
    }
}

/// The decoded kind of a relation.
fn relation_name(kind: &Kind) -> &'static str {
    match kind {
        Kind::Native { .. } => "native",
        Kind::Disabled {} => "disabled",
        Kind::Coincident { .. } => "coincident",
        Kind::Polygon { .. } => "polygon",
        Kind::SplineGroup { .. } => "spline_group",
        Kind::RectangularPattern { .. } => "rectangular_pattern",
        Kind::CircularPattern { .. } => "circular_pattern",
        Kind::TextFrame { .. } => "text_frame",
        Kind::TextPath { .. } => "text_path",
        Kind::CoincidentLoci { .. } => "coincident_loci",
        Kind::SameCoordinate { .. } => "same_coordinate",
        Kind::PointOnObject { .. } => "point_on_object",
        Kind::Midpoint { .. } => "midpoint",
        Kind::PointCoordinateValues { .. } => "point_coordinate_values",
        Kind::MidpointCoordinate { .. } => "midpoint_coordinate",
        Kind::Offset { .. } => "offset",
        Kind::ProjectedCopy { .. } => "projected_copy",
        Kind::AtIntersection { .. } => "at_intersection",
        Kind::Concentric { .. } => "concentric",
        Kind::Coradial { .. } => "coradial",
        Kind::Collinear { .. } => "collinear",
        Kind::Symmetric { .. } => "symmetric",
        Kind::PointSymmetric { .. } => "point_symmetric",
        Kind::Horizontal { .. } => "horizontal",
        Kind::Vertical { .. } => "vertical",
        Kind::Parallel { .. } => "parallel",
        Kind::Perpendicular { .. } => "perpendicular",
        Kind::Tangent { .. } => "tangent",
        Kind::TangentLoci { .. } => "tangent_loci",
        Kind::Curvature { .. } => "curvature",
        Kind::Equal { .. } => "equal",
        Kind::Fixed { .. } => "fixed",
        Kind::ArcAngle { .. } => "arc_angle",
        Kind::EllipseAngle { .. } => "ellipse_angle",
        Kind::Distance { .. } => "distance",
        Kind::DistanceLoci { .. } => "distance_loci",
        Kind::DistanceLociValue { .. } => "distance_loci_value",
        Kind::PolarDistance { .. } => "polar_distance",
        Kind::AngleDifference { .. } => "angle_difference",
        Kind::ScalarEquality { .. } => "scalar_equality",
        Kind::EqualDistance { .. } => "equal_distance",
        Kind::HorizontalDistance { .. } => "horizontal_distance",
        Kind::VerticalDistance { .. } => "vertical_distance",
        Kind::RepeatedDistance { .. } => "repeated_distance",
        Kind::RepeatedLength { .. } => "repeated_length",
        Kind::ParallelLineSetDistance { .. } => "parallel_line_set_distance",
        Kind::Angle { .. } => "angle",
        Kind::AngleToAxis { .. } => "angle_to_axis",
        Kind::Radius { .. } => "radius",
        Kind::RepeatedRadius { .. } => "repeated_radius",
        Kind::Diameter { .. } => "diameter",
        Kind::RepeatedDiameter { .. } => "repeated_diameter",
        Kind::SnellsLaw { .. } => "snells_law",
        Kind::Weight { .. } => "weight",
        Kind::InternalAlignment { .. } => "internal_alignment",
        Kind::Group { .. } => "group",
        Kind::Text { .. } => "text",
    }
}

/// An operand that a decoded relation names: an entity, or a point on an
/// entity.
enum Named<'k> {
    Entity(&'k SketchEntityId),
    Locus(&'k SketchLocus),
}

/// The operands that `kind` names, in the order it names them, for the
/// relations whose operands a native relation states.
fn named(kind: &Kind) -> Vec<Named<'_>> {
    match kind {
        Kind::Native { entities, .. }
        | Kind::Coincident { entities }
        | Kind::Distance { entities, .. } => entities.iter().map(Named::Entity).collect(),
        Kind::CoincidentLoci { loci } => loci.iter().map(Named::Locus).collect(),
        Kind::PointOnObject { point, entity } | Kind::Midpoint { point, entity } => {
            vec![Named::Locus(point), Named::Entity(entity)]
        }
        Kind::DistanceLoci { first, second, .. }
        | Kind::HorizontalDistance { first, second, .. }
        | Kind::VerticalDistance { first, second, .. } => {
            vec![Named::Locus(first), Named::Locus(second)]
        }
        Kind::Symmetric {
            first,
            second,
            axis,
        } => vec![
            Named::Locus(first),
            Named::Locus(second),
            Named::Entity(axis),
        ],
        Kind::Radius { entity, .. } | Kind::Diameter { entity, .. } => {
            vec![Named::Entity(entity)]
        }
        Kind::Angle { first, second, .. } => vec![Named::Entity(first), Named::Entity(second)],
        _ => Vec::new(),
    }
}

/// The loci at a curve's start, end, or center that `kind` names.
fn point_loci(kind: &Kind) -> impl Iterator<Item = &SketchLocus> {
    named(kind).into_iter().filter_map(|named| match named {
        Named::Locus(locus) if !matches!(locus, SketchLocus::Entity(_)) => Some(locus),
        _ => None,
    })
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

/// The point entities of one sketch that stand at the start, end, or center
/// of its curves (§6.1, Points are entities): for each such point of a
/// curve, the one point entity whose record the source names as that point
/// of the curve and whose position is the point's.
#[derive(Debug, Default)]
pub struct StoredPoints {
    at: Vec<(SketchLocus, SketchEntityId)>,
}

impl StoredPoints {
    /// The stored points of the curves of `entities`. `names` says whether
    /// the source names a point entity as a point of a curve, and `same`
    /// whether two positions are one. The decode session's resource refusal
    /// ends the search.
    pub fn new(
        ctx: &DecodeContext<'_>,
        entities: &[&crate::sketches::SketchEntity],
        names: impl Fn(
            &crate::sketches::SketchEntity,
            &SketchLocus,
            &crate::sketches::SketchEntity,
        ) -> bool,
        same: impl Fn([f64; 2], [f64; 2]) -> bool,
    ) -> Result<Self, ResourceLimit> {
        let points = entities
            .iter()
            .filter_map(|entity| match entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => {
                    Some((*entity, [position.u, position.v]))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut at = Vec::new();
        for curve in entities {
            if matches!(
                curve.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            ) {
                continue;
            }
            for locus in [
                SketchLocus::Start(curve.id().clone()),
                SketchLocus::End(curve.id().clone()),
                SketchLocus::Center(curve.id().clone()),
            ] {
                let Some(position) = locus_position(ctx, &curve.geometry, &locus)? else {
                    continue;
                };
                let mut hits = points.iter().filter(|(point, stored)| {
                    same(*stored, [position.u, position.v]) && names(curve, &locus, point)
                });
                if let (Some((point, _)), None) = (hits.next(), hits.next()) {
                    at.push((locus, point.id().clone()));
                }
            }
        }
        Ok(Self { at })
    }

    /// The point entity at the point of a curve that `locus` names.
    #[must_use]
    pub fn at(&self, locus: &SketchLocus) -> Option<&SketchEntityId> {
        self.at
            .iter()
            .find(|(at, _)| at == locus)
            .map(|(_, point)| point)
    }

    /// The roles of the point entity `point` on the curves it stands on.
    #[must_use]
    pub fn roles(&self, point: &SketchEntityId) -> Vec<EntityRole<Planar>> {
        let mut roles = self
            .at
            .iter()
            .filter(|(_, at)| at == point)
            .map(|(locus, _)| {
                let (of, role) = defining_role(locus);
                EntityRole {
                    of: of.clone(),
                    role,
                }
            })
            .collect::<Vec<EntityRole<Planar>>>();
        roles.sort_by(|a, b| (a.of.as_str(), rank(&a.role)).cmp(&(b.of.as_str(), rank(&b.role))));
        roles
    }
}

/// The curve and the role on it of the point `locus` names.
fn defining_role(locus: &SketchLocus) -> (&SketchEntityId, DefiningRole) {
    match locus {
        SketchLocus::Start(of) => (of, DefiningRole::Start),
        SketchLocus::End(of) => (of, DefiningRole::End),
        SketchLocus::Center(of) | SketchLocus::Entity(of) => (of, DefiningRole::Center),
    }
}

/// The order of the roles of one point on one curve.
fn rank(role: &DefiningRole) -> u8 {
    match role {
        DefiningRole::Start => 0,
        DefiningRole::End => 1,
        _ => 2,
    }
}

/// The points at the start, end, or center of a curve that the relations of
/// one sketch name (§6.1, Points are entities): the stored point entity
/// there, else a point entity minted for the locus, with the role it holds
/// on its curve, or the point entity that the locus names on a point.
#[derive(Debug, Default)]
pub struct LocusPoints {
    points: Vec<LocusPoint>,
    unplaced: Vec<SketchLocus>,
}

#[derive(Debug)]
struct LocusPoint {
    locus: SketchLocus,
    entity: SketchEntityId,
    position: FinitePoint2,
    minted: bool,
}

impl LocusPoints {
    /// The points of the loci that `kinds`, the relations of a sketch whose
    /// entities are `entities` and whose stored points are `stored`, name.
    /// The decode session's resource refusal ends the search.
    pub fn new<'k>(
        ctx: &DecodeContext<'_>,
        kinds: impl IntoIterator<Item = &'k Kind>,
        entities: &[&crate::sketches::SketchEntity],
        stored: &StoredPoints,
    ) -> Result<Self, ResourceLimit> {
        let mut out = Self::default();
        let position = |id: &SketchEntityId| {
            entities
                .iter()
                .find(|entity| entity.id() == id)
                .and_then(|entity| match entity.geometry.definition() {
                    SketchGeometryDefinition::Point { position } => Some(*position),
                    _ => None,
                })
        };
        for locus in kinds.into_iter().flat_map(point_loci) {
            if out.points.iter().any(|point| &point.locus == locus) || out.unplaced.contains(locus)
            {
                continue;
            }
            let point = match stored.at(locus) {
                Some(entity) => position(entity).map(|position| LocusPoint {
                    locus: locus.clone(),
                    entity: entity.clone(),
                    position,
                    minted: false,
                }),
                None => Self::unstored(ctx, locus, entities)?,
            };
            match point {
                Some(point) => out.points.push(point),
                None => out.unplaced.push(locus.clone()),
            }
        }
        Ok(out)
    }

    /// The point `locus` names where the sketch stores none there: the
    /// point entity it names on a point, else a point minted at its
    /// position on its curve, where the curve has a point there and no
    /// entity holds the minted id.
    fn unstored(
        ctx: &DecodeContext<'_>,
        locus: &SketchLocus,
        entities: &[&crate::sketches::SketchEntity],
    ) -> Result<Option<LocusPoint>, ResourceLimit> {
        let curve = locus_entity(locus);
        let Some(curve_entity) = entities.iter().find(|entity| entity.id() == curve) else {
            return Ok(None);
        };
        let geometry = &curve_entity.geometry;
        if let SketchGeometryDefinition::Point { position } = geometry.definition() {
            return Ok(Some(LocusPoint {
                locus: locus.clone(),
                entity: curve.clone(),
                position: *position,
                minted: false,
            }));
        }
        let Some(position) = locus_position(ctx, geometry, locus)? else {
            return Ok(None);
        };
        let entity = SketchEntityId::mint(format!(
            "{}:{}",
            curve.as_str(),
            match locus {
                SketchLocus::Start(_) => "start",
                SketchLocus::End(_) => "end",
                _ => "center",
            }
        ))
        .ok()
        .filter(|id| entities.iter().all(|entity| entity.id() != id));
        Ok(entity.map(|entity| LocusPoint {
            locus: locus.clone(),
            entity,
            position,
            minted: true,
        }))
    }

    /// The point entity that `locus` names.
    #[must_use]
    pub fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        self.point(locus)
            .map(|point| Operand::Entity(point.entity.clone()))
    }

    /// The position of the point that `locus` names.
    #[must_use]
    pub fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        self.point(locus)
            .map(|point| [point.position.u, point.position.v])
    }

    /// Whether `kind` names a point that its curve does not have (an end of
    /// a closed curve, or the center of a curve with none), or a point whose
    /// minted id another entity holds; such a point is stated as its curve.
    #[must_use]
    pub fn unplaced(&self, kind: &Kind) -> bool {
        point_loci(kind).any(|locus| self.unplaced.contains(locus))
    }

    /// The minted point entities of the sketch `sketch`.
    #[must_use]
    pub fn entities(&self, sketch: &SketchId) -> Vec<SketchEntity<Planar>> {
        self.points
            .iter()
            .filter(|point| point.minted)
            .map(|point| {
                let (of, role) = defining_role(&point.locus);
                SketchEntity {
                    id: point.entity.clone(),
                    sketch: sketch.clone(),
                    definition: EntityDefinition::Free(PlanarCurve::Point(point.position)),
                    roles: vec![EntityRole {
                        of: of.clone(),
                        role,
                    }],
                    construction: None,
                    centerline: None,
                    native: None,
                    native_options: NativeOptions::new(),
                    source_settings: NativeOptions::new(),
                    native_ref: None,
                }
            })
            .collect()
    }

    fn point(&self, locus: &SketchLocus) -> Option<&LocusPoint> {
        self.points.iter().find(|point| &point.locus == locus)
    }
}

/// The position of the point `locus` names on the curve `geometry`: the
/// ends of a line, an arc, a bounded ellipse, parabola, or hyperbola, and a
/// NURBS curve (its points at the ends of its knot domain), and the center
/// of a circle, an arc, or an ellipse.
fn locus_position(
    ctx: &DecodeContext<'_>,
    geometry: &SketchGeometry,
    locus: &SketchLocus,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        return Ok(closed_form_position(geometry, locus));
    };
    let poles = curve.pole_rows();
    let knots = curve.knots().as_slice();
    let Some(parameter) =
        crate::eval::nurbs_pcurve_parameter_domain(curve.degree(), knots, poles.count())
            .and_then(|domain| locus_end(locus, domain.endpoints()))
    else {
        return Ok(None);
    };
    crate::eval::finite_or_refusal(crate::eval::nurbs_pcurve_uv(
        ctx,
        curve.degree(),
        knots,
        &poles.raw_points(),
        poles.weights().as_deref(),
        parameter,
    ))
}

/// The parameter of the end of a curve that `locus` names, of the curve
/// whose parameter range is `[start, end]`.
fn locus_end(locus: &SketchLocus, [start, end]: [f64; 2]) -> Option<f64> {
    match locus {
        SketchLocus::Start(_) => Some(start),
        SketchLocus::End(_) => Some(end),
        _ => None,
    }
}

/// The position of the point `locus` names on the curve `geometry`, a curve
/// whose points have a closed form.
fn closed_form_position(geometry: &SketchGeometry, locus: &SketchLocus) -> Option<FinitePoint2> {
    let at = |origin: &FinitePoint2, [x, y]: [f64; 2]| {
        FinitePoint2::new(crate::math::Point2 {
            u: origin.u + x,
            v: origin.v + y,
        })
    };
    let rotated = |angle: f64, [x, y]: [f64; 2]| {
        let (sin, cos) = angle.portable_sin_cos();
        [x * cos - y * sin, x * sin + y * cos]
    };
    let end = |range: [f64; 2]| locus_end(locus, range);
    match (geometry.definition(), locus) {
        (SketchGeometryDefinition::Line { start, .. }, SketchLocus::Start(_)) => Some(*start),
        (SketchGeometryDefinition::Line { end, .. }, SketchLocus::End(_)) => Some(*end),
        (
            SketchGeometryDefinition::Circle { center, .. }
            | SketchGeometryDefinition::Arc { center, .. }
            | SketchGeometryDefinition::Ellipse { center, .. },
            SketchLocus::Center(_),
        ) => Some(*center),
        (
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            },
            _,
        ) => {
            let (sin, cos) = end([start_angle.get(), end_angle.get()])?.portable_sin_cos();
            at(center, [radius.get() * cos, radius.get() * sin])
        }
        (
            SketchGeometryDefinition::Ellipse {
                center,
                major_angle,
                radii,
                bounds: Some([start, end_parameter]),
            },
            _,
        ) => {
            let (sin, cos) = end([start.get(), end_parameter.get()])?.portable_sin_cos();
            at(
                center,
                rotated(
                    major_angle.get(),
                    [radii.major().get() * cos, radii.minor().get() * sin],
                ),
            )
        }
        (
            SketchGeometryDefinition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds: Some([start, end_parameter]),
            },
            _,
        ) => {
            let parameter = end([start.get(), end_parameter.get()])?;
            at(
                center,
                rotated(
                    major_angle.get(),
                    [
                        major_radius.get() * parameter.portable_cosh(),
                        minor_radius.get() * parameter.portable_sinh(),
                    ],
                ),
            )
        }
        (
            SketchGeometryDefinition::Parabola {
                vertex,
                axis_angle,
                focal_length,
                bounds: Some([start, end_parameter]),
            },
            _,
        ) => {
            let y = end([start.get(), end_parameter.get()])?;
            at(
                vertex,
                rotated(axis_angle.get(), [y * y / (4.0 * focal_length.get()), y]),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
