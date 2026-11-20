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

use super::parameters::Entries;
use super::sketch::Sketches;
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
}

/// Type the sketches, then the extrudes, of `timeline`.
pub(super) fn project(
    model: &Model,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    timeline: &mut Timeline,
    entries: &Entries<'_>,
    scope: &ProductDefinitionId,
) -> Typed {
    let supports = supports(histories, lanes, timeline);
    let mut sketches = sketch::project(
        &mut timeline.features,
        &sketch::Context {
            model,
            entries,
            scope,
            supports: &supports,
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
    }
}

/// One note naming each typed extrude whose blind depth is bound to its
/// record's only dimension, named neither `Depth` nor `D1` (sldprt.md §6:
/// the sole dimension is the depth whatever its name).
fn assumed(extrudes: &[(FeatureId, ParameterId)]) -> Option<LossNote> {
    if extrudes.is_empty() {
        return None;
    }
    let shown = extrudes.len().min(NAMED_INSTANCES_PER_NOTE);
    let named = extrudes[..shown]
        .iter()
        .map(|(feature, parameter)| format!("{} ({})", feature.as_str(), parameter.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let rest = extrudes.len() - shown;
    let more = if rest == 0 {
        String::new()
    } else {
        format!(", and {rest} more")
    };
    Some(SldprtLossCode::DesignExtrudeDepthAssumed.note(format!(
        "{} extrudes bind their blind depth to their record's only dimension, which is named \
         neither Depth nor D1: {named}{more}",
        extrudes.len()
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
    let all = sketches.iter().chain(extrudes).collect::<Vec<_>>();
    if all.is_empty() {
        return None;
    }
    let shown = all.len().min(NAMED_INSTANCES_PER_NOTE);
    let named = all[..shown]
        .iter()
        .map(|(feature, reason)| format!("{} ({reason})", feature.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let rest = all.len() - shown;
    let more = if rest == 0 {
        String::new()
    } else {
        format!(", and {rest} more")
    };
    Some(SldprtLossCode::DesignFeatureUnresolved.note(format!(
        "{} sketch and {} extrude features state their family without operands: {named}{more}",
        sketches.len(),
        extrudes.len()
    )))
}
