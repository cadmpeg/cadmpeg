// SPDX-License-Identifier: Apache-2.0
//! The sketches and extrudes of the timeline, typed from the decoder's own
//! records, and what their dimensions state of the parameters they bind.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::design::construction::InputReference;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation};
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::parameter::DimensionDisplay;
use cadmpeg_ir::design::selection::{PickTarget, Picks, PrincipalEntity};
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchEntityId;

use super::parameters::Entries;
use super::profile_check::Difference;
use super::sketch::{Sketches, Unstored};
use super::tree::Timeline;
use super::{extrude, sketch};
use crate::loss::{SldprtLossCode, NAMED_INSTANCES_PER_NOTE};
use crate::records::{FeatureHistory, FeatureInputLane};

/// The sketch records, the quantity of each parameter a dimension binds, and
/// the note of the features left `Unresolved`.
pub(super) struct Typed {
    pub(super) sketches: Sketches,
    pub(super) bound: HashMap<ParameterId, Quantity>,
    pub(super) measured: HashMap<ParameterId, DimensionDisplay>,
    /// The tree parameters a reference dimension binds.
    pub(super) references: HashSet<ParameterId>,
    pub(super) loss: Option<LossNote>,
    /// The note of the extrudes whose depth is their record's only
    /// dimension, whatever its name.
    pub(super) assumed: Option<LossNote>,
    /// The note of the sketch curves stated construction for the state of
    /// their curve record.
    pub(super) state_construction: Option<LossNote>,
    /// The note of the circles and arcs built from a dimension record that
    /// are stated construction.
    pub(super) dimension_construction: Option<LossNote>,
    /// The note of the typed extrudes whose sketch's closed regions differ
    /// from the stored profile.
    pub(super) profile_differs: Option<LossNote>,
}

/// The scope the typed records belong to, and what the decoder read of
/// their sketches' profiles.
pub(super) struct Scope<'a> {
    pub(super) definition: &'a ProductDefinitionId,
    /// The sketch entities read from a curve record of state `0`.
    pub(super) outside_profile: &'a HashSet<SketchEntityId>,
    /// The surfaces of the current geometry.
    pub(super) surfaces: &'a [cadmpeg_ir::geometry::Surface],
}

/// Type the sketches, then the extrudes, of `timeline`.
pub(super) fn project(
    model: &Model,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    timeline: &mut Timeline,
    entries: &Entries<'_>,
    scope: &Scope<'_>,
) -> Typed {
    let supports = supports(histories, lanes, timeline);
    let mut sketches = sketch::project(
        &mut timeline.features,
        &sketch::Context {
            model,
            entries,
            scope: scope.definition,
            supports: &supports,
            outside_profile: scope.outside_profile,
            surfaces: scope.surfaces,
        },
    );
    let stated = sketches
        .records
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let extrudes = extrude::project(
        &mut timeline.features,
        &extrude::Context {
            model,
            histories,
            entries,
            sketches: &stated,
            entities: &sketches.entities,
        },
    );
    let mut bound = HashMap::<ParameterId, Option<Quantity>>::new();
    for (parameter, quantity) in sketches.bound.iter().chain(&extrudes.bound) {
        bound
            .entry(parameter.clone())
            .and_modify(|known| {
                if *known != Some(*quantity) {
                    *known = None;
                }
            })
            .or_insert(Some(*quantity));
    }
    let loss = loss(&sketches.unprojected, &extrudes.unprojected);
    let assumed = assumed(&extrudes.assumed);
    let state_construction = state_construction(&sketches.outside_profile);
    let dimension_construction = dimension_construction(&sketches.dimension_curves);
    let profile_differs = profile_differs(&extrudes.differs);
    let measured = std::mem::take(&mut sketches.measured);
    let references = std::mem::take(&mut sketches.references);
    Typed {
        measured,
        references,
        bound: bound
            .into_iter()
            .filter_map(|(parameter, quantity)| Some((parameter, quantity?)))
            .collect(),
        sketches,
        loss,
        assumed,
        state_construction,
        dimension_construction,
        profile_differs,
    }
}

/// The text naming the first [`NAMED_INSTANCES_PER_NOTE`] of `named`, and
/// how many more there are.
fn listed(named: impl Iterator<Item = String>) -> String {
    let mut shown = Vec::new();
    let mut rest = 0_usize;
    for name in named {
        if shown.len() < NAMED_INSTANCES_PER_NOTE {
            shown.push(name);
        } else {
            rest += 1;
        }
    }
    let shown = shown.join(", ");
    match rest {
        0 => shown,
        rest => format!("{shown}, and {rest} more"),
    }
}

/// One note naming each sketch curve the design states construction because
/// its curve record has state `0`, which the decoder reads as a curve outside
/// the profile (open item EV-32), with its record's construction bit clear.
fn state_construction(entities: &[SketchEntityId]) -> Option<LossNote> {
    if entities.is_empty() {
        return None;
    }
    Some(
        SldprtLossCode::DesignSketchConstructionAssumed.note(format!(
            "{} sketch curves whose curve record has state 0 and no construction bit are \
             construction, as curves outside their sketch's profile (EV-32): {}",
            entities.len(),
            listed(entities.iter().map(|entity| entity.as_str().to_owned())),
        )),
    )
}

/// One note naming each circle and arc built from a dimension record that
/// the design states construction, with why: no curve record stores it, and
/// it repeats another curve of its sketch or no face of the solid lies on it
/// (open item DI-83).
fn dimension_construction(entities: &[(SketchEntityId, Unstored)]) -> Option<LossNote> {
    if entities.is_empty() {
        return None;
    }
    Some(
        SldprtLossCode::DesignSketchDimensionCurveConstruction.note(format!(
            "{} sketch circles and arcs built from a dimension record, which no curve record \
             stores, are construction; their dimensions keep them as references (DI-83): {}",
            entities.len(),
            listed(entities.iter().map(|(entity, reason)| format!(
                "{} ({})",
                entity.as_str(),
                reason.text()
            ))),
        )),
    )
}

/// One note naming each typed extrude whose `Sketch` pick's closed regions
/// differ from the profile loops the decoder resolved from the stored
/// records, with how they differ (§6 "Sources").
fn profile_differs(extrudes: &[(FeatureId, Option<String>, Vec<Difference>)]) -> Option<LossNote> {
    if extrudes.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignExtrudeProfileDiffers.note(format!(
        "{} extrudes pick their whole sketch, whose closed regions differ from the profile \
         the decoder read from the stored records: {}",
        extrudes.len(),
        listed(extrudes.iter().map(|(feature, name, differences)| format!(
            "{}{} ({})",
            feature.as_str(),
            name.as_ref()
                .map(|name| format!(" \"{name}\""))
                .unwrap_or_default(),
            differences
                .iter()
                .copied()
                .map(Difference::text)
                .collect::<Vec<_>>()
                .join("; ")
        ))),
    )))
}

/// One note naming each typed extrude whose blind depth is bound to its
/// record's only dimension, named neither `Depth` nor `D1` (sldprt.md §6:
/// the sole dimension is the depth whatever its name).
fn assumed(extrudes: &[(FeatureId, ParameterId)]) -> Option<LossNote> {
    if extrudes.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignExtrudeDepthAssumed.note(format!(
        "{} extrudes bind their blind depth to their record's only dimension, which is named \
         neither Depth nor D1: {}",
        extrudes.len(),
        listed(extrudes.iter().map(|(feature, parameter)| format!(
            "{} ({})",
            feature.as_str(),
            parameter.as_str()
        ))),
    )))
}

/// The datum feature of the built-in plane each sketch feature's profile
/// names, and that plane's principal entity.
fn supports(
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    timeline: &Timeline,
) -> HashMap<FeatureId, (FeatureId, PrincipalEntity)> {
    let principal = timeline
        .features
        .iter()
        .filter_map(|feature| Some((&feature.id, principal_plane(feature)?)))
        .collect::<HashMap<_, _>>();
    crate::resolved_features::sketch_supports::sketch_supports(histories, lanes)
        .into_iter()
        .filter_map(|(sketch, plane)| {
            let sketch = timeline.features_by_record.get(sketch)?;
            let plane = timeline.features_by_record.get(plane)?;
            let entity = *principal.get(plane)?;
            Some((sketch.clone(), (plane.clone(), entity)))
        })
        .collect()
}

/// The principal plane a datum feature is coincident with.
fn principal_plane(feature: &Feature) -> Option<PrincipalEntity> {
    let FeatureOperation::Datum(datum) = &feature.definition.operation else {
        return None;
    };
    let DatumElement::Plane(plane) = &datum.element else {
        return None;
    };
    let [input] = plane.inputs.as_slice() else {
        return None;
    };
    let Some(InputReference::Picks(selection)) = &input.reference else {
        return None;
    };
    let Picks::Named(picks) = &selection.picks else {
        return None;
    };
    match picks.as_slice() {
        [pick] => match pick.target {
            PickTarget::Principal {
                entity,
                container: None,
            } => Some(entity),
            _ => None,
        },
        _ => None,
    }
}

/// One note naming the sketch and extrude features the decoder typed in its
/// own records that the design states `Unresolved`, with the reason of each.
fn loss(
    sketches: &[(FeatureId, &'static str)],
    extrudes: &[(FeatureId, &'static str)],
) -> Option<LossNote> {
    if sketches.is_empty() && extrudes.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignFeatureUnresolved.note(format!(
        "{} sketch and {} extrude features state their family without operands: {}",
        sketches.len(),
        extrudes.len(),
        listed(
            sketches
                .iter()
                .chain(extrudes)
                .map(|(feature, reason)| format!("{} ({reason})", feature.as_str()))
        ),
    )))
}
