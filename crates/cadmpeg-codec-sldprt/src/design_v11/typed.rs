// SPDX-License-Identifier: Apache-2.0
//! The sketches, extrudes and revolves of the timeline, typed from the decoder's own
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
use super::profile_check::{Difference, Unchecked};
use super::revolve::AxisPick;
use super::sketch::{DimensionCircle, Sketches};
use super::tree::Timeline;
use super::{datum, extrude, pattern, revolve, sketch, swept};
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
    /// The note of the circles built from a dimension record that the
    /// design leaves out or states construction.
    pub(super) dimension_circles: Option<LossNote>,
    /// The note of the typed extrudes whose sketch's closed regions differ
    /// from the stored profile.
    pub(super) profile_differs: Option<LossNote>,
    /// The note of the typed extrudes whose sketch holds no profile curve.
    pub(super) sketch_empty: Option<LossNote>,
    /// The note of the typed extrudes whose sketch's closed regions the
    /// check did not compare with the stored profile.
    pub(super) profile_unchecked: Option<LossNote>,
    /// The note of the typed revolves whose stored axis picks a line of
    /// their profile sketch.
    pub(super) revolve_axis_lines: Option<LossNote>,
    /// The note of the typed revolves whose axis no record stores and which
    /// lies on a line of their profile sketch.
    pub(super) revolve_axis_derived: Option<LossNote>,
    /// The note of the typed revolves whose boolean the design leaves
    /// unstated.
    pub(super) revolve_boolean_unstated: Option<LossNote>,
    /// The note of the typed revolves whose sketch's closed regions differ
    /// from the stored profile.
    pub(super) revolve_profile_differs: Option<LossNote>,
    /// The note of the typed revolves whose sketch's closed regions the
    /// check did not compare with the stored profile.
    pub(super) revolve_profile_unchecked: Option<LossNote>,
    /// The note of the revolves whose decoded axis carries the turn sense
    /// their record stores and whose solid depends on it.
    pub(super) revolve_sense_assumed: Option<LossNote>,
    /// The notes of the unresolved user datums and of the typed datums that
    /// state an assumed value.
    pub(super) datums: Vec<LossNote>,
}

/// The scope the typed records belong to, and what the decoder read of
/// their sketches' profiles.
pub(super) struct Scope<'a> {
    pub(super) definition: &'a ProductDefinitionId,
    /// The sketch entities read from a curve record of state `0`.
    pub(super) outside_profile: &'a HashSet<SketchEntityId>,
    /// The surfaces of the current geometry.
    pub(super) surfaces: &'a [cadmpeg_ir::geometry::Surface],
    /// The document's angular tolerance.
    pub(super) angular: f64,
}

/// Type the sketches, then the extrudes and the revolves, of `timeline`.
pub(super) fn project(
    model: &Model,
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    timeline: &mut Timeline,
    entries: &Entries<'_>,
    scope: &Scope<'_>,
) -> Typed {
    let supports = supports(&model.features, histories, lanes, timeline);
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
    let context = swept::Context {
        model,
        histories,
        entries,
        sketches: &stated,
        entities: &sketches.entities,
    };
    let datums = datum::project(&mut timeline.features, &context, lanes, scope.angular);
    let extrudes = extrude::project(&mut timeline.features, &context);
    let revolves = revolve::project(&mut timeline.features, &context);
    pattern::project(&mut timeline.features, &context);
    let mut bound = HashMap::<ParameterId, Option<Quantity>>::new();
    for (parameter, quantity) in sketches
        .bound
        .iter()
        .chain(&extrudes.bound)
        .chain(&revolves.bound)
    {
        bound
            .entry(parameter.clone())
            .and_modify(|known| {
                if *known != Some(*quantity) {
                    *known = None;
                }
            })
            .or_insert(Some(*quantity));
    }
    let loss = loss(
        &sketches.unprojected,
        &extrudes.unprojected,
        &revolves.unprojected,
    );
    let assumed = assumed(&extrudes.assumed);
    let state_construction = state_construction(&sketches.outside_profile);
    let dimension_circles = dimension_circles(&sketches.dimension_circles);
    let revolve_axis_lines = axis_lines(
        SldprtLossCode::DesignRevolveAxisLineInferred,
        "pick, as their axis, the line of their profile sketch that lies on the axis their \
         record stores; the decoder does not read which entity the record's axis reference names",
        &revolves.inferred,
    );
    let revolve_axis_derived = axis_lines(
        SldprtLossCode::DesignRevolveAxisDerived,
        "state, as their axis, a line of their profile sketch that lies on the axis the decoder \
         derived from the sketch's records or the solid's surfaces; no record stores the axis",
        &revolves.derived,
    );
    let revolve_boolean_unstated = boolean_unstated(&revolves.boolean_unstated);
    let revolve_sense_assumed = sense_assumed(&revolves.sense_assumed);
    let datums = datum_notes(&datums);
    let revolve_profile_differs = profile_differs(
        SldprtLossCode::DesignRevolveProfileDiffers,
        "revolves",
        &revolves.differs,
    );
    let revolve_profile_unchecked = profile_unchecked(
        SldprtLossCode::DesignRevolveProfileUnchecked,
        "revolves",
        &revolves.unchecked.iter().collect::<Vec<_>>(),
    );
    let profile_differs = profile_differs(
        SldprtLossCode::DesignExtrudeProfileDiffers,
        "extrudes",
        &extrudes.differs,
    );
    let (empty, unchecked): (Vec<_>, Vec<_>) = extrudes
        .unchecked
        .iter()
        .partition(|(_, _, why)| *why == Unchecked::NoProfileCurve);
    let sketch_empty = sketch_empty(&empty);
    let profile_unchecked = profile_unchecked(
        SldprtLossCode::DesignExtrudeProfileUnchecked,
        "extrudes",
        &unchecked,
    );
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
        dimension_circles,
        profile_differs,
        sketch_empty,
        profile_unchecked,
        revolve_axis_lines,
        revolve_axis_derived,
        revolve_boolean_unstated,
        revolve_profile_differs,
        revolve_profile_unchecked,
        revolve_sense_assumed,
        datums,
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

/// One note naming each circle built from a dimension record that the
/// design leaves out or states construction, with what its sketch and the
/// solid say of it and, for a circle left out on a stored curve, the
/// dimensions that name that curve (open item DI-83).
fn dimension_circles(circles: &[DimensionCircle]) -> Option<LossNote> {
    if circles.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignSketchDimensionCircle.note(format!(
        "{} sketch circles the decoder built from a dimension record are left out of the \
         design or construction (DI-83): {}",
        circles.len(),
        listed(circles.iter().map(DimensionCircle::text)),
    )))
}

/// One note naming each typed feature of kind `kind` whose
/// `Sketch` pick's closed regions differ from the profile loops the decoder
/// resolved from the stored records, with how they differ (§6 "Sources").
fn profile_differs(
    code: SldprtLossCode,
    kind: &str,
    features: &[(FeatureId, Option<String>, Vec<Difference>)],
) -> Option<LossNote> {
    if features.is_empty() {
        return None;
    }
    Some(code.note(format!(
        "{} {kind} pick their whole sketch, whose closed regions differ from the profile \
         the decoder read from the stored records: {}",
        features.len(),
        listed(features.iter().map(|(feature, name, differences)| format!(
            "{} ({})",
            named(feature, name.as_deref()),
            differences
                .iter()
                .copied()
                .map(Difference::text)
                .collect::<Vec<_>>()
                .join("; ")
        ))),
    )))
}

/// One note naming each typed extrude whose `Sketch` pick takes no region,
/// because its sketch holds no curve other than points and construction
/// curves.
fn sketch_empty(extrudes: &[&(FeatureId, Option<String>, Unchecked)]) -> Option<LossNote> {
    if extrudes.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignExtrudeSketchEmpty.note(format!(
        "{} extrudes pick their whole sketch, which holds no profile curve, so the pick takes \
         no region: {}",
        extrudes.len(),
        listed(
            extrudes
                .iter()
                .map(|(feature, name, _)| named(feature, name.as_deref()))
        ),
    )))
}

/// One note naming each typed feature of kind `kind` whose
/// `Sketch` pick's closed regions the check did not compare with the stored
/// profile, with why.
fn profile_unchecked(
    code: SldprtLossCode,
    kind: &str,
    features: &[&(FeatureId, Option<String>, Unchecked)],
) -> Option<LossNote> {
    if features.is_empty() {
        return None;
    }
    Some(code.note(format!(
        "{} {kind} pick their whole sketch, whose closed regions the decoder did not compare \
         with the stored profile: {}",
        features.len(),
        listed(features.iter().map(|(feature, name, why)| format!(
            "{} ({})",
            named(feature, name.as_deref()),
            why.text()
        ))),
    )))
}

/// One note of `code` naming each typed revolve whose axis picks a line of
/// its profile sketch, with that line and, where several lines lie on the
/// axis, how many. `what` says what the revolves state.
fn axis_lines(code: SldprtLossCode, what: &str, revolves: &[AxisPick]) -> Option<LossNote> {
    if revolves.is_empty() {
        return None;
    }
    Some(code.note(format!(
        "{} revolves {what}: {}",
        revolves.len(),
        listed(revolves.iter().map(|(feature, name, line, among)| {
            let among = if *among > 1 {
                format!(", one of {among} lines on the axis")
            } else {
                String::new()
            };
            format!(
                "{} ({}{among})",
                named(feature, name.as_deref()),
                line.as_str()
            )
        })),
    )))
}

/// One note naming each typed revolve whose boolean the design leaves
/// unstated, with why.
fn boolean_unstated(revolves: &[(FeatureId, Option<String>, &'static str)]) -> Option<LossNote> {
    if revolves.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignRevolveBooleanUnstated.note(format!(
        "{} revolves state no boolean: {}",
        revolves.len(),
        listed(
            revolves
                .iter()
                .map(|(feature, name, why)| format!("{} ({why})", named(feature, name.as_deref())))
        ),
    )))
}

/// One note naming each revolve whose solid depends on the turn sense, whose
/// decoded axis turns about the axis direction its record stores, reversed
/// where its reverse byte is `1` (open item EV-25).
fn sense_assumed(revolves: &[(FeatureId, Option<String>)]) -> Option<LossNote> {
    if revolves.is_empty() {
        return None;
    }
    Some(SldprtLossCode::DesignRevolveSenseAssumed.note(format!(
        "{} partial or two-sided revolves turn about the axis direction their record \
         stores, the other way where the record's reverse byte is 1 (EV-25): {}",
        revolves.len(),
        listed(
            revolves
                .iter()
                .map(|(feature, name)| named(feature, name.as_deref()))
        ),
    )))
}

/// The notes naming the user datums [`datum::project`] leaves unresolved and
/// the typed datums that state an assumed value.
fn datum_notes(projected: &datum::Projected) -> Vec<LossNote> {
    [
        (
            SldprtLossCode::DesignDatumUnresolved,
            "user datums stay unresolved: the design types no construction for their decoded \
             definition",
            &projected.unresolved,
        ),
        (
            SldprtLossCode::DesignDatumPlaneAxisConstructed,
            "reference planes state an origin and x axis the decoder constructs from their \
             mid-plane constraint",
            &projected.constructed_axis,
        ),
        (
            SldprtLossCode::DesignDatumPlaneAxisAngled,
            "reference planes with an angular D1 take the u-axis of their angular frame as their \
             x axis; offset planes on them can store another u-axis for it",
            &projected.angled_axis,
        ),
        (
            SldprtLossCode::DesignDatumConfigurationDropped,
            "datums state the active configuration's geometry; their geometry decodes \
             differently in another configuration",
            &projected.configured,
        ),
    ]
    .into_iter()
    .filter(|(_, _, datums)| !datums.is_empty())
    .map(|(code, text, datums)| {
        code.note(format!(
            "{} {text}: {}",
            datums.len(),
            listed(
                datums
                    .iter()
                    .map(|(feature, name)| named(feature, name.as_deref()))
            ),
        ))
    })
    .collect()
}

/// A feature's id, and its name where it has one.
fn named(feature: &FeatureId, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{} \"{name}\"", feature.as_str()),
        None => feature.as_str().to_owned(),
    }
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
    features: &[cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    timeline: &Timeline,
) -> HashMap<FeatureId, (FeatureId, PrincipalEntity)> {
    let principal = timeline
        .features
        .iter()
        .filter_map(|feature| Some((&feature.id, principal_plane(feature)?)))
        .collect::<HashMap<_, _>>();
    crate::resolved_features::sketch_supports::sketch_supports(features, histories, lanes)
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

/// One note naming the sketch, extrude and revolve features the decoder
/// typed in its own records that the design states `Unresolved`, under each
/// reason, so that every reason is named however many features the note
/// lists.
fn loss(
    sketches: &[(FeatureId, &'static str)],
    extrudes: &[(FeatureId, &'static str)],
    revolves: &[(FeatureId, &'static str)],
) -> Option<LossNote> {
    if sketches.is_empty() && extrudes.is_empty() && revolves.is_empty() {
        return None;
    }
    let mut reasons = Vec::<(&str, Vec<&FeatureId>)>::new();
    for (feature, reason) in sketches.iter().chain(extrudes).chain(revolves) {
        match reasons.iter_mut().find(|(known, _)| known == reason) {
            Some((_, features)) => features.push(feature),
            None => reasons.push((reason, vec![feature])),
        }
    }
    Some(SldprtLossCode::DesignFeatureUnresolved.note(format!(
        "{} sketch, {} extrude and {} revolve features state their family without operands: {}",
        sketches.len(),
        extrudes.len(),
        revolves.len(),
        reasons
            .iter()
            .map(|(reason, features)| format!(
                "{reason} ({}: {})",
                features.len(),
                listed(features.iter().map(|feature| feature.as_str().to_owned()))
            ))
            .collect::<Vec<_>>()
            .join("; "),
    )))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::features::FeatureId;

    #[test]
    fn the_unresolved_note_names_every_reason_however_many_features_it_lists() {
        let id = |index: usize| -> FeatureId {
            json::from_value(cadmpeg_core::json!(format!("sldprt:model:feature#{index}"))).unwrap()
        };
        let extrudes = (0..12)
            .map(|index| {
                (
                    id(index),
                    "the extrude's limit is neither blind nor through all",
                )
            })
            .collect::<Vec<_>>();
        let revolves = [
            (
                id(20),
                "the revolve is two-sided, and its sense is not read",
            ),
            (id(21), "the revolve's sketch is not stated"),
        ];
        let message = super::loss(&[], &extrudes, &revolves).unwrap().message;
        assert!(
            message.starts_with("0 sketch, 12 extrude and 2 revolve features")
                && message.contains("neither blind nor through all (12: ")
                && message.contains("and 4 more")
                && message.contains(&format!(
                    "the revolve is two-sided, and its sense is not read (1: {})",
                    id(20).as_str()
                ))
                && message.contains(&format!(
                    "the revolve's sketch is not stated (1: {})",
                    id(21).as_str()
                )),
            "{message}"
        );
    }
}
