// SPDX-License-Identifier: Apache-2.0
//! The construction spellings of §5.9.
//!
//! A plane or frame whose whole is `Coincident` with one planar reference and
//! whose steps begin with a translation along Z states that translation as an
//! `Offset` input on the reference. A `ParameterRatio` value on a `Built`
//! reference, at 0 or 1, or on a curve the document states as a line, a
//! circle, or a circular arc is the arc-length `Ratio`, which agrees with it
//! there. The document states a picked curve by a sketch entity's curve, by
//! the pick's witness, or by the carrier of each edge or curve the pick
//! resolves to, in every state that holds that record.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::construction::AnyConstruction;
use crate::design::construction::{
    Construction, ConstructionInput, ConstructionKind, FrameAxis, InputReference, InputRole,
    PostStep, Relation, ScalarDimension, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::datum::DatumElement;
use crate::design::ops::sketch::{EntityDefinition, PlanarCurve, SpatialCurve};
use crate::design::refs::EntityRef;
use crate::design::selection::{DatumPart, PickTarget, Picked, Picks, PrincipalEntity};
use crate::document::CadIr;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::topology::EdgeCarrier;

/// The kind of element a construction builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Point,
    Direction,
    Axis,
    Plane,
    Frame,
    Curve,
}

/// What the construction rules read of the records a pick names, gathered
/// before the walk changes any record.
#[derive(Debug, Default)]
pub(super) struct Facts {
    /// The datum features whose element is a plane.
    planar_datums: BTreeSet<String>,
    /// The planar and spatial sketch entities whose curve the document
    /// states as a line segment, a circle, or a circular arc.
    straight_or_circular: BTreeSet<String>,
    /// Whether every version of a curve carrier, in the current geometry
    /// and in each stored state, is stated as a line or a circle, by the
    /// carrier's id.
    curves: BTreeMap<String, bool>,
    /// The carrier curve of every version of an edge, by the edge's id;
    /// `None` for a version that states no carrier.
    edges: BTreeMap<String, Vec<Option<String>>>,
}

impl Facts {
    pub(super) fn new(ir: &CadIr) -> Self {
        let design = &ir.design;
        let planar_datums = design
            .features
            .iter()
            .filter(|feature| {
                matches!(
                    &feature.definition.operation,
                    FeatureOperation::Datum(datum) if matches!(datum.element, DatumElement::Plane(_))
                )
            })
            .map(|feature| feature.id.as_str().to_owned())
            .collect();
        let planar = design
            .sketch_entities
            .iter()
            .filter(|entity| {
                stated_curve(&entity.definition).is_some_and(planar_straight_or_circular)
            })
            .map(|entity| entity.id.as_str().to_owned());
        let spatial = design
            .spatial_sketch_entities
            .iter()
            .filter(|entity| {
                stated_curve(&entity.definition).is_some_and(spatial_straight_or_circular)
            })
            .map(|entity| entity.id.as_str().to_owned());
        let topologies = std::iter::once(&ir.geometry)
            .chain(
                design
                    .evaluations
                    .iter()
                    .filter_map(|evaluation| evaluation.content.as_ref())
                    .map(|content| &content.delta.records),
            )
            .chain(
                ir.process
                    .states
                    .iter()
                    .filter_map(|state| state.content.as_ref())
                    .map(|content| &content.delta.records),
            );
        let mut curves: BTreeMap<String, bool> = BTreeMap::new();
        let mut edges: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
        for topology in topologies {
            for curve in &topology.curves {
                let straight = matches!(&curve.geometry, CurveGeometry::Solved(geometry)
                    if solved_straight_or_circular(geometry));
                *curves.entry(curve.id.as_str().to_owned()).or_insert(true) &= straight;
            }
            for edge in &topology.edges {
                let carrier = match &edge.carrier {
                    EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _) => {
                        Some(curve.as_str().to_owned())
                    }
                    EdgeCarrier::Free | EdgeCarrier::Endpoints(_) => None,
                };
                edges
                    .entry(edge.id.as_str().to_owned())
                    .or_default()
                    .push(carrier);
            }
        }
        Self {
            planar_datums,
            straight_or_circular: planar.chain(spatial).collect(),
            curves,
            edges,
        }
    }

    /// Whether a pick is known to pick a plane.
    fn picks_plane(&self, pick: &Picked) -> bool {
        match &pick.target {
            PickTarget::Datum {
                feature,
                part: None,
            } => self.planar_datums.contains(feature.as_str()),
            PickTarget::Datum {
                part: Some(DatumPart::Principal(entity)),
                ..
            }
            | PickTarget::Principal { entity, .. }
            | PickTarget::SketchFrame { entity, .. } => matches!(
                entity,
                PrincipalEntity::XyPlane | PrincipalEntity::XzPlane | PrincipalEntity::YzPlane
            ),
            _ => false,
        }
    }

    /// Whether the document states the one curve a pick picks as a line
    /// segment, a circle, or a circular arc: a sketch entity's curve, or the
    /// curves its witness and its resolved entities state, at least one of
    /// them and each a line or a circle.
    fn picks_straight_or_circular(&self, pick: &Picked) -> bool {
        match &pick.target {
            PickTarget::SketchEntity(entity) => {
                return self.straight_or_circular.contains(entity.as_str())
            }
            PickTarget::SpatialSketchEntity(entity) => {
                return self.straight_or_circular.contains(entity.as_str())
            }
            _ => {}
        }
        let witnessed = pick
            .witness
            .as_ref()
            .map_or(&[][..], |witness| witness.curves.as_slice());
        let resolved = pick.resolved.as_deref().unwrap_or_default();
        if witnessed.len() > 1 || resolved.len() > 1 {
            return false;
        }
        let mut stated: Vec<bool> = witnessed.iter().map(solved_straight_or_circular).collect();
        for entity in resolved {
            let carriers: Vec<&str> = match entity {
                EntityRef::Edge(edge) => self
                    .edges
                    .get(edge.as_str())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(String::as_str)
                    .collect(),
                EntityRef::Curve(curve) => vec![curve.as_str()],
                EntityRef::Body(_)
                | EntityRef::Face(_)
                | EntityRef::Vertex(_)
                | EntityRef::Surface(_)
                | EntityRef::Subd(_) => return false,
            };
            stated.extend(
                carriers
                    .into_iter()
                    .filter_map(|curve| self.curves.get(curve).copied()),
            );
        }
        !stated.is_empty() && stated.into_iter().all(|straight| straight)
    }
}

const fn solved_straight_or_circular(curve: &SolvedCurveGeometry) -> bool {
    matches!(
        curve,
        SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Circle(_)
    )
}

/// The curve an entity's definition states: a free curve, or a defining
/// form's solved curve.
fn stated_curve<S: crate::design::ops::sketch::SketchSpace>(
    definition: &EntityDefinition<S>,
) -> Option<&S::Curve> {
    match definition {
        EntityDefinition::Free(curve) => Some(curve),
        EntityDefinition::Defined { solved, .. } => solved.as_ref(),
    }
}

const fn planar_straight_or_circular(curve: &PlanarCurve) -> bool {
    matches!(
        curve,
        PlanarCurve::Line { .. } | PlanarCurve::Circle { .. } | PlanarCurve::Arc { .. }
    )
}

const fn spatial_straight_or_circular(curve: &SpatialCurve) -> bool {
    matches!(
        curve,
        SpatialCurve::Bounded {
            carrier: SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Circle(_),
            ..
        }
    )
}

/// Puts one construction's inputs and steps into their one spelling.
pub(super) fn normalize<K: ConstructionKind>(
    construction: &mut Construction<K>,
    kind: Kind,
    facts: &Facts,
) {
    for input in &mut construction.inputs {
        arc_length_ratio(input, facts);
    }
    if matches!(kind, Kind::Plane | Kind::Frame) {
        leading_offset(construction, facts);
    }
}

/// States a `ParameterRatio` value as the `Ratio` it agrees with.
fn arc_length_ratio(input: &mut ConstructionInput, facts: &Facts) {
    let at_parameter = input
        .role
        .as_ref()
        .is_some_and(|role| matches!(role.relation, Relation::AtParameter));
    let Some(ScalarDimension::ParameterRatio(ratio)) = &input.value else {
        return;
    };
    let at_end =
        matches!(ratio, Dimension::Literal(value) if value.get() == 0.0 || value.get() == 1.0);
    let agrees = at_end
        || match &input.reference {
            Some(InputReference::Built(_)) => true,
            Some(InputReference::Picks(selection)) => matches!(
                &selection.picks,
                Picks::Named(picks)
                    if matches!(picks.as_slice(), [one] if facts.picks_straight_or_circular(one))
            ),
            None => false,
        };
    if at_parameter && agrees {
        input.value = Some(ScalarDimension::Ratio(ratio.clone()));
    }
}

/// Moves a leading translation along Z into the one `Coincident` input of
/// the whole on a planar reference, which becomes an `Offset` input.
fn leading_offset<K: ConstructionKind>(construction: &mut Construction<K>, facts: &Facts) {
    let Some(PostStep::Translate {
        axis: FrameAxis::Z, ..
    }) = construction.after.first()
    else {
        return;
    };
    let mut whole_coincident = construction.inputs.iter().enumerate().filter(|(_, input)| {
        matches!(
            input.role,
            Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            })
        )
    });
    let (Some((at, input)), None) = (whole_coincident.next(), whole_coincident.next()) else {
        return;
    };
    let planar = match &input.reference {
        Some(InputReference::Picks(selection)) => matches!(
            &selection.picks,
            Picks::Named(picks) if matches!(picks.as_slice(), [one] if facts.picks_plane(one))
        ),
        Some(InputReference::Built(built)) => matches!(built.as_ref(), AnyConstruction::Plane(_)),
        None => false,
    };
    if !planar || input.value.is_some() || input.reversed.is_some() {
        return;
    }
    let PostStep::Translate { distance, .. } = construction.after.remove(0) else {
        return;
    };
    let input = &mut construction.inputs[at];
    input.role = Some(InputRole {
        subject: Subject::Whole,
        relation: Relation::Offset,
    });
    input.value = Some(ScalarDimension::Length(distance));
}
