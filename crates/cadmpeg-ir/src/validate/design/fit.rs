// SPDX-License-Identifier: Apache-2.0
//! The kinds a construction input's reference holds (§5.9, What a relation
//! reads; part 10 "Constructions"). §5.9 tabulates what each relation reads
//! and the kinds of reference that cannot supply it, and what each `Built`
//! kind and each pick target supplies. A `Built` construction fits as a pick
//! of its kind would. A pick whose target does not state its kind (a body, a
//! feature result, a sketch) and a `Native` pick fit every relation. The
//! measuring origin, the second pick of an `AtParameter` or `Coordinates`
//! reference, is a point, and a `Fold` input picks four straight edges.
//!
//! This module checks a `Built` reference's kind against the table, that the
//! measuring origin may be a point, and that each pick of a `Fold` reference
//! may be a straight edge.
//!
//! Interim: a `Built` `Axis` fits `PrincipalNormal` and `Binormal`, and a
//! `Built` `Direction` fits `Coincident`, where the table refuses both. Of a
//! reference's picks, the module reads only the measuring origin and the
//! picks of a `Fold` input. Those two checks refuse a name of kind `Body` and
//! a `Sketch` target, which §5.9 lists among the targets that fit every
//! relation.

use crate::design::construction::{AnyConstruction, InputReference, Relation};
use crate::design::names::NameKind;
use crate::design::ops::datum::DatumElement;
use crate::design::ops::sketch::{
    EntityDefinition, EntityForm, PlanarCurve, SketchSpace, SpatialCurve,
};
use crate::design::refs::EntityRef;
use crate::design::selection::{DatumPart, PickTarget, Picked, Picks, PrincipalEntity};
use crate::geometry::SolvedCurveGeometry;

use super::constructions::{datum_element, DatumFeature};
use super::index::DesignIndex;
use super::names::derived_kinds;
use super::walk::Kind;

/// Checks the kinds that an input of `relation` references.
pub(super) fn check_input(
    index: &DesignIndex<'_>,
    relation: &Relation,
    reference: Option<&InputReference>,
    error: &mut dyn FnMut(std::fmt::Arguments<'_>),
) {
    match reference {
        Some(InputReference::Built(built)) => {
            let kind = built_kind(built);
            if !built_fits(relation, kind) {
                error(format_args!(
                    "a nested {} construction is the reference of a {relation:?} input, which does not take one",
                    kind.name()
                ));
            }
        }
        Some(InputReference::Picks(selection)) => {
            let Picks::Named(picks) = &selection.picks else {
                return;
            };
            let picks = picks.as_slice();
            if let (Relation::AtParameter | Relation::Coordinates, [_, origin]) = (relation, picks)
            {
                if !may_be_point(index, origin) {
                    error(format_args!(
                        "the measuring origin, the second pick of a {relation:?} reference, is not a point"
                    ));
                }
            }
            if matches!(relation, Relation::Fold)
                && picks.iter().any(|pick| !may_be_straight_edge(index, pick))
            {
                error(format_args!(
                    "a Fold reference picks something other than a straight edge"
                ));
            }
        }
        None => {}
    }
}

const fn built_kind(built: &AnyConstruction) -> Kind {
    match built {
        AnyConstruction::Point(_) => Kind::Point,
        AnyConstruction::Direction(_) => Kind::Direction,
        AnyConstruction::Axis(_) => Kind::Axis,
        AnyConstruction::Plane(_) => Kind::Plane,
        AnyConstruction::Frame(_) => Kind::Frame,
        AnyConstruction::Curve(_) => Kind::Curve,
    }
}

/// Whether a nested construction of `kind` may be the one pick of an input
/// of `relation`.
const fn built_fits(relation: &Relation, kind: Kind) -> bool {
    match relation {
        Relation::Coordinates => matches!(kind, Kind::Frame),
        Relation::Center | Relation::AxisOf => matches!(kind, Kind::Curve),
        Relation::PrincipalNormal | Relation::Binormal => {
            matches!(kind, Kind::Curve | Kind::Axis)
        }
        Relation::Parallel | Relation::Perpendicular | Relation::Angle => {
            !matches!(kind, Kind::Point)
        }
        Relation::Tangent | Relation::AtParameter => !matches!(kind, Kind::Point | Kind::Direction),
        Relation::Offset | Relation::Radius | Relation::Limit => !matches!(kind, Kind::Direction),
        Relation::Coincident
        | Relation::Midway
        | Relation::Centroid
        | Relation::Projection
        | Relation::Fold
        | Relation::Native(_) => true,
    }
}

/// Whether `resolved` holds only entities `admits` takes, where it is stated.
fn resolved_admits(pick: &Picked, admits: fn(&EntityRef) -> bool) -> bool {
    pick.resolved
        .as_ref()
        .is_none_or(|resolved| resolved.iter().all(admits))
}

/// Whether a pick may pick a point: a vertex, a sketch point, a points datum
/// or its point, an origin, or a target whose kind is not stated.
fn may_be_point(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    let target = match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.is_empty() || kinds.contains(&NameKind::Vertex)
        }
        PickTarget::SketchEntity(entity) => index
            .sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_may_be(&entity.definition, planar_point)),
        PickTarget::SpatialSketchEntity(entity) => index
            .spatial_sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_may_be(&entity.definition, spatial_point)),
        PickTarget::Datum { feature, part } => match part {
            Some(DatumPart::Point(_)) => true,
            Some(DatumPart::Principal(entity)) => *entity == PrincipalEntity::Origin,
            None => matches!(
                datum_element(index, feature.as_str()),
                DatumFeature::Unstated | DatumFeature::Datum(DatumElement::Points(_))
            ),
        },
        PickTarget::Principal { entity, .. } | PickTarget::SketchFrame { entity, .. } => {
            *entity == PrincipalEntity::Origin
        }
        PickTarget::Native { .. }
        | PickTarget::Publication(_)
        | PickTarget::FeatureResult(_)
        | PickTarget::TemplateInput { .. } => true,
        PickTarget::Sketch(_)
        | PickTarget::SketchRegion { .. }
        | PickTarget::Feature(_)
        | PickTarget::Component => false,
    };
    target && resolved_admits(pick, |entity| matches!(entity, EntityRef::Vertex(_)))
}

/// Whether a pick may pick a straight edge: an edge or curve, a straight
/// sketch entity, an axis, or a target whose kind is not stated.
fn may_be_straight_edge(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    let axis = |entity: &PrincipalEntity| {
        matches!(
            entity,
            PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::ZAxis
        )
    };
    let target = match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.is_empty() || kinds.contains(&NameKind::Edge) || kinds.contains(&NameKind::Curve)
        }
        PickTarget::SketchEntity(entity) => index
            .sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_may_be(&entity.definition, planar_straight)),
        PickTarget::SpatialSketchEntity(entity) => index
            .spatial_sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| entity_may_be(&entity.definition, spatial_straight)),
        PickTarget::Datum { feature, part } => match part {
            Some(DatumPart::Principal(entity)) => axis(entity),
            Some(DatumPart::Point(_)) => false,
            None => matches!(
                datum_element(index, feature.as_str()),
                DatumFeature::Unstated | DatumFeature::Datum(DatumElement::Axis(_))
            ),
        },
        PickTarget::Principal { entity, .. } | PickTarget::SketchFrame { entity, .. } => {
            axis(entity)
        }
        PickTarget::Native { .. }
        | PickTarget::Publication(_)
        | PickTarget::FeatureResult(_)
        | PickTarget::TemplateInput { .. } => true,
        PickTarget::Sketch(_)
        | PickTarget::SketchRegion { .. }
        | PickTarget::Feature(_)
        | PickTarget::Component => false,
    };
    target
        && resolved_admits(pick, |entity| {
            matches!(entity, EntityRef::Edge(_) | EntityRef::Curve(_))
        })
}

/// Whether a sketch entity's curve may be one `admits` takes. A `Frame` is a
/// point entity; a `Text` or `BlockInstance` is no curve and no point; a
/// conic is curved; a `Native` form, or another form with no solved curve,
/// states no kind.
fn entity_may_be<S: SketchSpace>(
    definition: &EntityDefinition<S>,
    admits: fn(Option<&S::Curve>) -> bool,
) -> bool {
    match definition {
        EntityDefinition::Free(curve) => admits(Some(curve)),
        EntityDefinition::Defined { form, solved } => match form.as_ref() {
            EntityForm::Frame => admits(None),
            EntityForm::Text(_) | EntityForm::BlockInstance(_) | EntityForm::Conic { .. } => false,
            EntityForm::Native(_) => true,
            EntityForm::Interpolated(_) | EntityForm::Derived(_) | EntityForm::Instanced { .. } => {
                solved.as_ref().is_none_or(|curve| admits(Some(curve)))
            }
        },
    }
}

/// A point, or no stated curve (a frame entity, which is a point).
const fn planar_point(curve: Option<&PlanarCurve>) -> bool {
    match curve {
        None => true,
        Some(PlanarCurve::Spatial(curve)) => spatial_point(Some(curve)),
        Some(curve) => matches!(curve, PlanarCurve::Point(_)),
    }
}

const fn spatial_point(curve: Option<&SpatialCurve>) -> bool {
    matches!(curve, None | Some(SpatialCurve::Point(_)))
}

/// A curve that may be straight; a frame entity, a point, is not.
fn planar_straight(curve: Option<&PlanarCurve>) -> bool {
    match curve {
        None => false,
        Some(PlanarCurve::Spatial(curve)) => spatial_straight(Some(curve)),
        Some(curve) => matches!(
            curve,
            PlanarCurve::Line { .. } | PlanarCurve::InfiniteLine { .. } | PlanarCurve::Nurbs(_)
        ),
    }
}

fn spatial_straight(curve: Option<&SpatialCurve>) -> bool {
    match curve {
        None | Some(SpatialCurve::Point(_)) => false,
        Some(SpatialCurve::Bounded { carrier, .. }) => !curved(carrier),
    }
}

/// Whether a solved carrier is a conic, which is never straight.
pub(super) const fn curved(carrier: &SolvedCurveGeometry) -> bool {
    matches!(
        carrier,
        SolvedCurveGeometry::Circle(_)
            | SolvedCurveGeometry::Ellipse(_)
            | SolvedCurveGeometry::Parabola(_)
            | SolvedCurveGeometry::Hyperbola(_)
    )
}

#[cfg(test)]
mod tests;
