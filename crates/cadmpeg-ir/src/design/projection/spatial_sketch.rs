// SPDX-License-Identifier: Apache-2.0
//! The forms of decoded spatial sketch records that a spatial sketch (§6.1
//! of the CADIR v11 specification) states as they are: its curves in the
//! coordinates of its definition, and the relations and dimensions of the
//! decoded vocabulary.

use std::f64::consts::TAU;

use crate::design::construction::ScalarDimension;
use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::ids::SpatialSketchConstraintId;
use crate::design::ops::sketch::{
    BoundedCurve, DistanceAxis, EntityDefinition, EntityForm, Operand, SketchConstraint,
    SketchEntity, SketchRelation, SketchSolve, Spatial, SpatialCurve, SpatialSketch,
};
use crate::design::options::NativeOptions;
use crate::design::selection::Anchor;
use crate::design::units::Quantity;
use crate::document::Model;
use crate::features::geometry::FiniteVector3;
use crate::features::{FeatureId, ParameterId};
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::InlineCurve;
use crate::sketches::{
    SpatialSketchConstraintDefinitionInput as Kind, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition as Geometry,
};
use crate::topology::IncreasingParameterInterval;
use crate::units::{OrthonormalFrame3, UnitVector3};
use cadmpeg_core::text::NonBlankString;

use super::sketch::{
    native_integer, native_operands, operand_options, qualified, stored_quantity, stored_value,
    DimensionValue,
};

/// A spatial sketch restated as the sketch of its owner, with the facts the
/// restatement states only by their native kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Restated {
    /// The sketch.
    pub sketch: SpatialSketch,
    /// Its entities.
    pub entities: Vec<SketchEntity<Spatial>>,
    /// Its relations and dimensions.
    pub constraints: Vec<SketchConstraint<Spatial>>,
    /// The entities with no curve, which state their native kind.
    pub native_entities: Vec<String>,
    /// The relations stated by their native kind, on the entities they
    /// name and with the values they hold.
    pub native_relations: Vec<String>,
    /// The dimensions that bind no design parameter: they state the value
    /// their parameter record stores, or no value.
    pub unbound: Vec<String>,
}

/// Restate `record` of `model`, written by `codec`, as the spatial sketch of
/// the feature `owner`, whose selections anchor at `at`. A dimension binds
/// its parameter where `bound` says the design states it with the
/// dimension's quantity.
pub fn restate(
    codec: &NonBlankString,
    owner: &FeatureId,
    record: &crate::sketches::SpatialSketch,
    model: &Model,
    at: &Anchor,
    bound: impl Fn(&ParameterId, Quantity) -> bool,
) -> Restated {
    let mut out = Restated {
        sketch: SpatialSketch {
            id: record.id.clone(),
            owner: owner.clone(),
            name: record.name.clone(),
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: record.native_ref.clone(),
        },
        entities: Vec::new(),
        constraints: Vec::new(),
        native_entities: Vec::new(),
        native_relations: Vec::new(),
        unbound: Vec::new(),
    };
    let entities = model
        .spatial_sketch_entities
        .iter()
        .filter(|entity| entity.sketch == record.id)
        .collect::<Vec<_>>();
    for entity in &entities {
        let definition = match spatial_curve(&entity.geometry) {
            Some(curve) => EntityDefinition::Free(curve),
            None => {
                out.native_entities.push(entity.id().as_str().to_owned());
                EntityDefinition::Defined {
                    form: Box::new(EntityForm::Native(qualified(
                        codec,
                        native_geometry(&entity.geometry),
                    ))),
                    solved: None,
                }
            }
        };
        out.entities.push(SketchEntity {
            id: entity.id().clone(),
            sketch: record.id.clone(),
            definition,
            roles: Vec::new(),
            construction: entity.construction.then_some(true),
            centerline: None,
            native: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: entity.native_ref.clone(),
        });
    }
    for constraint in model
        .spatial_sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == record.id)
    {
        let name = constraint.id.as_str();
        let Ok(id) = SpatialSketchConstraintId::mint(name) else {
            out.native_relations.push(name.to_owned());
            continue;
        };
        let kind = constraint.definition.kind();
        let dimension =
            dimension_parameter(kind, &model.parameters).map(|(parameter, quantity)| {
                let value = if bound(parameter, quantity) {
                    Some(DimensionValue::Parameter(parameter.clone()))
                } else {
                    out.unbound.push(name.to_owned());
                    stored_value(&model.parameters, parameter, quantity)
                };
                (quantity, value)
            });
        let relation = match relation(kind, dimension.as_ref().map(|(_, value)| value)) {
            Some(relation) => relation,
            None => {
                out.native_relations.push(name.to_owned());
                native(codec, kind, dimension, &entities, at)
            }
        };
        out.constraints.push(SketchConstraint {
            id,
            sketch: record.id.clone(),
            relation,
            name: None,
            active: None,
            native_options: native_options(codec, kind),
            source_settings: NativeOptions::new(),
            native_ref: constraint.native_ref.clone(),
        });
    }
    out
}

/// The spatial curve of a decoded spatial geometry: a point, a line, a
/// circle, an arc, or a NURBS curve.
#[must_use]
pub fn spatial_curve(geometry: &SpatialSketchGeometry) -> Option<SpatialCurve> {
    let bounded = |carrier, range| {
        Some(SpatialCurve::Bounded(BoundedCurve {
            carrier,
            range: match range {
                Some(range) => Some(IncreasingParameterInterval::new(range)?),
                None => None,
            },
        }))
    };
    let circle = |center, normal: &UnitVector3, reference: &UnitVector3, radius| {
        OrthonormalFrame3::from_units(*normal, *reference)
            .map(|frame| InlineCurve::Circle(CircleCurve::new(center, frame, radius)))
    };
    match geometry.definition() {
        Geometry::Point { position } => Some(SpatialCurve::Point(*position)),
        Geometry::Line { start, end } => {
            let along = end.get().vector_from(start.get());
            let direction = UnitVector3::normalized_nonzero(FiniteVector3::new(along)?)?;
            bounded(
                InlineCurve::Line(LineCurve::new(*start, direction)),
                Some([0.0, along.norm()]),
            )
        }
        Geometry::Circle {
            center,
            normal,
            reference_direction,
            radius,
        } => bounded(circle(*center, normal, reference_direction, *radius)?, None),
        Geometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let (start, end) = (start_angle.get(), end_angle.get());
            let end = if end <= start { end + TAU } else { end };
            bounded(
                circle(*center, normal, reference_direction, *radius)?,
                Some([start, end]),
            )
        }
        Geometry::Nurbs { curve } => bounded(InlineCurve::Nurbs((**curve).clone()), None),
        Geometry::NurbsSurface { .. } | Geometry::Native { .. } => None,
    }
}

/// The kind of a geometry with no spatial curve.
fn native_geometry(geometry: &SpatialSketchGeometry) -> &str {
    match geometry.definition() {
        Geometry::Native { native_kind } => native_kind.as_str(),
        Geometry::NurbsSurface { .. } => "nurbs_surface",
        Geometry::Point { .. } => "point",
        Geometry::Line { .. } => "line",
        Geometry::Circle { .. } => "circle",
        Geometry::Arc { .. } => "arc",
        Geometry::Nurbs { .. } => "nurbs",
    }
}

type SpatialKind = Kind<UnitVector3, crate::scalar::PositiveLength>;

/// The parameter that the dimension `kind` binds and the quantity it
/// measures. A native dimension measures the quantity of the value its
/// parameter record stores.
fn dimension_parameter<'k>(
    kind: &'k SpatialKind,
    parameters: &[crate::features::parameters::DesignParameter],
) -> Option<(&'k ParameterId, Quantity)> {
    match kind {
        Kind::PointDistance { parameter, .. }
        | Kind::PointLineDistance { parameter, .. }
        | Kind::LineLength { parameter, .. }
        | Kind::RepeatedLineLength { parameter, .. }
        | Kind::ParallelLineDistance { parameter, .. }
        | Kind::RepeatedParallelLineDistance { parameter, .. }
        | Kind::ParallelLineSetDistance { parameter, .. } => Some((parameter, Quantity::LENGTH)),
        Kind::Offset {
            parameter: Some(parameter),
            ..
        } => Some((&parameter.id, Quantity::LENGTH)),
        Kind::Native {
            parameter: Some(parameter),
            ..
        } => Some((parameter, stored_quantity(parameters, parameter)?)),
        _ => None,
    }
}

/// The typed relation that states `kind` with `value`, where its operands
/// are entities and a dimension has a length.
fn relation(
    kind: &SpatialKind,
    value: Option<&Option<DimensionValue>>,
) -> Option<SketchRelation<Spatial>> {
    let entity = |id: &SpatialSketchEntityId| Operand::Entity(id.clone());
    let length = || value?.as_ref()?.length();
    let distance = |a, b| {
        Some(SketchRelation::Distance {
            a: entity(a),
            b: entity(b),
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: length()?,
        })
    };
    Some(match kind {
        Kind::Coincident { first, second } => SketchRelation::Coincident {
            a: entity(first),
            b: entity(second),
        },
        Kind::Symmetric {
            first,
            second,
            axis,
        } => SketchRelation::Symmetric {
            a: entity(first),
            b: entity(second),
            about: entity(axis),
        },
        Kind::PointOnSurface { point, surface } => SketchRelation::Coincident {
            a: entity(point),
            b: entity(surface),
        },
        Kind::Midpoint { point, entity: of } => SketchRelation::Midpoint {
            point: point.clone(),
            of: entity(of),
        },
        Kind::Tangent { first, second } => SketchRelation::Tangent {
            a: entity(first),
            b: entity(second),
            contact: None,
        },
        Kind::PointDistance { first, second, .. }
        | Kind::ParallelLineDistance { first, second, .. } => distance(first, second)?,
        Kind::PointLineDistance { point, line, .. } => distance(point, line)?,
        Kind::LineLength { entity: of, .. } => SketchRelation::Length {
            of: vec![of.clone()],
            value: length()?,
        },
        _ => return None,
    })
}

/// The state a native relation `kind` stores, as the native option
/// `<codec>:native_state`, and what the decoder read of its operands.
fn native_options(codec: &NonBlankString, kind: &SpatialKind) -> NativeOptions {
    let mut options = NativeOptions::new();
    let Kind::Native {
        native_state,
        operands,
        ..
    } = kind
    else {
        return options;
    };
    if let Some((name, value)) =
        native_state.and_then(|state| native_integer(codec, "native_state", state))
    {
        options.insert(name, value);
    }
    operand_options(codec, operands, &mut options);
    options
}

/// The relation that states `kind` by its native kind, named
/// `<codec>:<native class>` for a native relation and `<codec>:<decoded
/// kind>` otherwise, on the entities it names and with the value of its
/// `dimension` and the values it stores. A native relation's operands are
/// those [`native_operands`] states anchored at `at`: the entity of `entities`
/// whose record an operand names, where one entity has it.
fn native(
    codec: &NonBlankString,
    kind: &SpatialKind,
    dimension: Option<(Quantity, Option<DimensionValue>)>,
    entities: &[&crate::sketches::SpatialSketchEntity],
    at: &Anchor,
) -> SketchRelation<Spatial> {
    let mut values = dimension
        .and_then(|(quantity, value)| value?.scalar(quantity))
        .into_iter()
        .collect::<Vec<_>>();
    let (name, entities): (&str, Vec<&SpatialSketchEntityId>) = match kind {
        Kind::Native {
            native_kind,
            operands,
            ..
        } => {
            return SketchRelation::Native {
                kind: qualified(codec, native_kind.as_str()),
                operands: native_operands(codec, operands, at, |operand| {
                    record(operand.native_ref.as_deref()?, entities)
                }),
                values,
            }
        }
        Kind::Coincident { first, second } => ("coincident", vec![first, second]),
        Kind::Symmetric {
            first,
            second,
            axis,
        } => ("symmetric", vec![first, second, axis]),
        Kind::PointOnSurface { point, surface } => ("point_on_surface", vec![point, surface]),
        Kind::Midpoint { point, entity } => ("midpoint", vec![point, entity]),
        Kind::Tangent { first, second } => ("tangent", vec![first, second]),
        Kind::PointDistance { first, second, .. } => ("point_distance", vec![first, second]),
        Kind::PointLineDistance { point, line, .. } => ("point_line_distance", vec![point, line]),
        Kind::LineLength { entity, .. } => ("line_length", vec![entity]),
        Kind::RepeatedLineLength { entities, .. } => {
            ("repeated_line_length", entities.iter().collect())
        }
        Kind::ParallelLineDistance { first, second, .. } => {
            ("parallel_line_distance", vec![first, second])
        }
        Kind::RepeatedParallelLineDistance { pairs, .. } => (
            "repeated_parallel_line_distance",
            pairs
                .iter()
                .flat_map(|pair| [&pair.first, &pair.second])
                .collect(),
        ),
        Kind::ParallelLineSetDistance { first, second, .. } => (
            "parallel_line_set_distance",
            first.iter().chain(second).collect(),
        ),
        Kind::Offset {
            sources,
            results,
            normal,
            distance,
            parameter,
        } => {
            if parameter.is_none() {
                values.push(ScalarDimension::Length(Dimension::Literal(
                    crate::scalar::Length::from(*distance),
                )));
            }
            values.extend(components(normal));
            ("offset", sources.iter().chain(results).collect())
        }
        Kind::ParallelToDirection { entity, direction } => {
            values.extend(components(direction));
            ("parallel_to_direction", vec![entity])
        }
        Kind::SplineGroup { entities } => ("spline_group", entities.iter().collect()),
    };
    let mut operands = Vec::with_capacity(entities.len());
    for entity in entities {
        let operand = Operand::Entity(entity.clone());
        if !operands.contains(&operand) {
            operands.push(operand);
        }
    }
    SketchRelation::Native {
        kind: qualified(codec, name),
        operands,
        values,
    }
}

/// The one entity of `entities` whose record is `native`.
fn record(
    native: &str,
    entities: &[&crate::sketches::SpatialSketchEntity],
) -> Option<Operand<Spatial>> {
    let mut named = entities
        .iter()
        .filter(|entity| entity.native_ref.as_deref() == Some(native));
    match (named.next(), named.next()) {
        (Some(entity), None) => Some(Operand::Entity(entity.id().clone())),
        _ => None,
    }
}

/// The components of `direction`, as literals.
fn components(direction: &UnitVector3) -> Option<ScalarDimension> {
    let raw = direction.as_raw();
    let literal = |value: f64| crate::scalar::FiniteReal::new(value).map(Dimension::Literal);
    Some(ScalarDimension::Components {
        x: literal(raw.x)?,
        y: literal(raw.y)?,
        z: literal(raw.z)?,
    })
}

#[cfg(test)]
mod tests;
