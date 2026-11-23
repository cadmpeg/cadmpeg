// SPDX-License-Identifier: Apache-2.0
//! The kinds a construction input's reference holds (§5.9, What a relation
//! reads; part 10 "Constructions"). §5.9 tabulates what each relation reads
//! and the kinds of reference that cannot supply it, and what each `Built`
//! kind and each pick target supplies. A `Built` construction fits as a pick
//! of its kind would. A pick whose target does not state its kind (a body, a
//! feature, a feature result, a sketch) and a `Native` pick fit every
//! relation. The measuring origin, the second pick of an `AtParameter` or
//! `Coordinates` reference, is a point, and a `Fold` input picks four
//! straight edges.
//!
//! This module checks each `Built` reference and each pick whose target
//! states its kind against the §5.9 table (except the first pick of
//! `Coordinates`, which `constructions.rs` checks), that the measuring
//! origin may be a point, and that each pick of a `Fold` reference may be a
//! straight edge.

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
            match relation {
                Relation::AtParameter => {
                    if let Some(first) = picks.first() {
                        if !pick_fits(index, relation, first) {
                            error(format_args!(
                                "an AtParameter reference picks a target that cannot supply what it reads"
                            ));
                        }
                    }
                    if let [_, origin] = picks {
                        if !may_be_point(index, origin) {
                            error(format_args!(
                                "the measuring origin, the second pick of a {relation:?} reference, is not a point"
                            ));
                        }
                    }
                }
                Relation::Coordinates => {
                    if let [_, origin] = picks {
                        if !may_be_point(index, origin) {
                            error(format_args!(
                                "the measuring origin, the second pick of a {relation:?} reference, is not a point"
                            ));
                        }
                    }
                }
                Relation::Fold => {
                    if picks.iter().any(|pick| !may_be_straight_edge(index, pick)) {
                        error(format_args!(
                            "a Fold reference picks something other than a straight edge"
                        ));
                    }
                }
                _ => {
                    if picks.iter().any(|pick| !pick_fits(index, relation, pick)) {
                        error(format_args!(
                            "a {relation:?} reference picks a target that cannot supply what it reads"
                        ));
                    }
                }
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

/// Whether a nested construction of `kind` fits `relation` under the table
/// of §5.9. `Fold` is rejected separately by the single-pick rule on `Built`.
const fn built_fits(relation: &Relation, kind: Kind) -> bool {
    match relation {
        Relation::Coordinates => matches!(kind, Kind::Frame),
        Relation::Center | Relation::AxisOf | Relation::PrincipalNormal | Relation::Binormal => {
            matches!(kind, Kind::Curve)
        }
        Relation::Parallel | Relation::Perpendicular | Relation::Angle => {
            !matches!(kind, Kind::Point)
        }
        Relation::Tangent | Relation::AtParameter => !matches!(kind, Kind::Point | Kind::Direction),
        Relation::Coincident
        | Relation::Offset
        | Relation::Centroid
        | Relation::Radius
        | Relation::Limit => !matches!(kind, Kind::Direction),
        Relation::Midway | Relation::Projection | Relation::Fold | Relation::Native(_) => true,
    }
}

/// The geometric kind a pick target states for the §5.9 relation table.
#[derive(Clone, Copy)]
enum TargetKind {
    Point,
    Axis,
    Plane,
    Frame,
    Curve,
    Surface,
}

const fn principal_kind(entity: PrincipalEntity) -> TargetKind {
    match entity {
        PrincipalEntity::Origin => TargetKind::Point,
        PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::ZAxis => {
            TargetKind::Axis
        }
        PrincipalEntity::XyPlane | PrincipalEntity::XzPlane | PrincipalEntity::YzPlane => {
            TargetKind::Plane
        }
        PrincipalEntity::Frame => TargetKind::Frame,
    }
}

const fn target_kind_fits(relation: &Relation, kind: TargetKind) -> bool {
    match relation {
        Relation::Coincident
        | Relation::Offset
        | Relation::Centroid
        | Relation::Radius
        | Relation::Limit
        | Relation::Midway
        | Relation::Projection
        | Relation::Native(_) => true,
        Relation::Parallel
        | Relation::Perpendicular
        | Relation::Angle
        | Relation::Tangent
        | Relation::AtParameter => !matches!(kind, TargetKind::Point),
        Relation::Center | Relation::AxisOf | Relation::PrincipalNormal | Relation::Binormal => {
            !matches!(
                kind,
                TargetKind::Point | TargetKind::Axis | TargetKind::Plane | TargetKind::Frame
            )
        }
        Relation::Coordinates => matches!(kind, TargetKind::Frame),
        Relation::Fold => matches!(kind, TargetKind::Curve | TargetKind::Axis),
    }
}

/// Whether a pick fits `relation` under the table of §5.9. Targets whose
/// kind is not stated (a body, a feature, a feature result, a sketch, a
/// `Native` pick) fit every relation.
fn pick_fits(index: &DesignIndex<'_>, relation: &Relation, pick: &Picked) -> bool {
    if matches!(
        relation,
        Relation::Midway | Relation::Projection | Relation::Native(_)
    ) {
        return true;
    }
    match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.is_empty()
                || kinds.iter().any(|kind| match kind {
                    NameKind::Body => true,
                    NameKind::Vertex => target_kind_fits(relation, TargetKind::Point),
                    NameKind::Edge | NameKind::Curve => {
                        target_kind_fits(relation, TargetKind::Curve)
                    }
                    NameKind::Face | NameKind::Surface => {
                        target_kind_fits(relation, TargetKind::Surface)
                    }
                })
        }
        PickTarget::SketchEntity(entity) => {
            index
                .sketch_entities
                .get(entity.as_str())
                .is_none_or(|entity| {
                    sketch_entity_kind(&entity.definition, planar_point)
                        .is_none_or(|kind| target_kind_fits(relation, kind))
                })
        }
        PickTarget::SpatialSketchEntity(entity) => index
            .spatial_sketch_entities
            .get(entity.as_str())
            .is_none_or(|entity| {
                sketch_entity_kind(&entity.definition, spatial_point)
                    .is_none_or(|kind| target_kind_fits(relation, kind))
            }),
        PickTarget::SketchRegion { .. } => target_kind_fits(relation, TargetKind::Surface),
        PickTarget::Datum { feature, part } => match part {
            Some(DatumPart::Point(_)) => target_kind_fits(relation, TargetKind::Point),
            Some(DatumPart::Principal(entity)) => {
                target_kind_fits(relation, principal_kind(*entity))
            }
            None => match datum_element(index, feature.as_str()) {
                DatumFeature::Datum(DatumElement::Points(_)) => {
                    target_kind_fits(relation, TargetKind::Point)
                }
                DatumFeature::Datum(DatumElement::Axis(_)) => {
                    target_kind_fits(relation, TargetKind::Axis)
                }
                DatumFeature::Datum(DatumElement::Plane(_)) => {
                    target_kind_fits(relation, TargetKind::Plane)
                }
                DatumFeature::Datum(DatumElement::Frame(_)) => {
                    target_kind_fits(relation, TargetKind::Frame)
                }
                DatumFeature::Unstated | DatumFeature::Other => true,
            },
        },
        PickTarget::Principal { entity, .. } | PickTarget::SketchFrame { entity, .. } => {
            target_kind_fits(relation, principal_kind(*entity))
        }
        PickTarget::Native { .. }
        | PickTarget::Sketch(_)
        | PickTarget::Feature(_)
        | PickTarget::FeatureResult(_)
        | PickTarget::Publication(_)
        | PickTarget::TemplateInput { .. }
        | PickTarget::Component => true,
    }
}

/// The kind a sketch entity states for the §5.9 table, or `None` where it
/// states none.
fn sketch_entity_kind<S: SketchSpace>(
    definition: &EntityDefinition<S>,
    is_point: fn(Option<&S::Curve>) -> bool,
) -> Option<TargetKind> {
    let curve_kind = |curve: Option<&S::Curve>| {
        if is_point(curve) {
            TargetKind::Point
        } else {
            TargetKind::Curve
        }
    };
    match definition {
        EntityDefinition::Free(curve) => Some(curve_kind(Some(curve))),
        EntityDefinition::Defined { form, solved } => match form.as_ref() {
            EntityForm::Frame => Some(TargetKind::Point),
            EntityForm::Conic { .. } => Some(TargetKind::Curve),
            EntityForm::Native(_) | EntityForm::Text(_) | EntityForm::BlockInstance(_) => None,
            EntityForm::Interpolated(_) | EntityForm::Derived(_) | EntityForm::Instanced { .. } => {
                solved.as_ref().map(|curve| curve_kind(Some(curve)))
            }
        },
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
            kinds.is_empty() || kinds.contains(&NameKind::Vertex) || kinds.contains(&NameKind::Body)
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
        | PickTarget::Sketch(_)
        | PickTarget::Feature(_)
        | PickTarget::FeatureResult(_)
        | PickTarget::Publication(_)
        | PickTarget::TemplateInput { .. } => true,
        PickTarget::SketchRegion { .. } | PickTarget::Component => false,
    };
    target
        && resolved_admits(pick, |entity| {
            matches!(entity, EntityRef::Vertex(_) | EntityRef::Body(_))
        })
}

/// Whether a pick may pick a straight edge: an edge or curve, a straight
/// sketch entity, an axis, or a target whose kind is not stated.
pub(super) fn may_be_straight_edge(index: &DesignIndex<'_>, pick: &Picked) -> bool {
    let axis = |entity: &PrincipalEntity| {
        matches!(
            entity,
            PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::ZAxis
        )
    };
    let target = match &pick.target {
        PickTarget::Entity(name) => {
            let kinds = derived_kinds(name);
            kinds.is_empty()
                || kinds.contains(&NameKind::Edge)
                || kinds.contains(&NameKind::Curve)
                || kinds.contains(&NameKind::Body)
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
        | PickTarget::Sketch(_)
        | PickTarget::Feature(_)
        | PickTarget::FeatureResult(_)
        | PickTarget::Publication(_)
        | PickTarget::TemplateInput { .. } => true,
        PickTarget::SketchRegion { .. } | PickTarget::Component => false,
    };
    target
        && resolved_admits(pick, |entity| {
            matches!(
                entity,
                EntityRef::Edge(_) | EntityRef::Curve(_) | EntityRef::Body(_)
            )
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
