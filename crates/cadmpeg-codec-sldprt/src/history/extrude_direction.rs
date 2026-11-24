// SPDX-License-Identifier: Apache-2.0
//! Finite B-rep witnesses for implicit extrusion travel.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::literals::parse_bool;

const EPS_CARRIER_DISTANCE: f64 = 1.0e-6;
const EPS_BOUNDARY_DISTANCE: f64 = 1.0e-4;
const EPS_CAP_MESH_DISTANCE: f64 = 1.0e-4;
const EPS_CAP_ALIGNMENT: f64 = 1.0e-8;
const EPS_SOLID_SIDE: f64 = 1.0e-6;
const MIN_TRIANGLE_DETERMINANT: f64 = 1.0e-24;
const CURVE_SAMPLE_INTERVALS: u32 = 16;
const MAX_WITNESS_WORK: usize = 2_000_000;

/// The source property that the decoder sets to `false` where an extrusion
/// record states the reversal flag `1` and the extrusion travels
/// along its profile normal: a cut, a two-direction extrusion whose end caps
/// do not confirm the reversal, or a reversal its solid contradicts. The
/// encoder then keeps the record's flag, and a reversal cleared without it
/// is an edit.
pub(crate) const REVERSE_DECODED: &str = "reverse_decoded";

/// The source property that states an extrusion's reversal flag: the
/// reversal word of its end spec, or a `Reverse` attribute of its history
/// record.
pub(crate) const REVERSE: &str = "Reverse";

/// Whether `properties` state the reversal flag `1`, as the [`REVERSE`]
/// property.
pub(crate) fn reversal_flag(properties: &BTreeMap<NonBlankString, String>) -> bool {
    properties
        .get(REVERSE)
        .map(String::as_str)
        .and_then(parse_bool)
        == Some(true)
}

/// The reversal flag the encoder writes for `feature`: the flag its record
/// states, cleared where an edit cleared the reversal the decoder read.
pub(crate) fn written_reversal_flag(feature: &Feature) -> bool {
    reversal_flag(&feature.source_properties) && !cleared_reversal(feature)
}

/// The JSON pointer of the travel in `definition`, the wire form of a
/// feature definition, where it is an extrusion: with [`REVERSE_DECODED`],
/// what [`retain_contradicted_directions`] and [`settle_reversal`] settle
/// against the stored solid over the reversal flag the record states.
pub(crate) fn settled_travel(definition: &serde_json::Value) -> Option<&'static str> {
    let kind = |pointer: &str| {
        definition
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
    };
    match kind("/kind")? {
        "operation" if kind("/value/kind") == Some("extrude") => Some("/value/value/direction"),
        "post_process" if kind("/value/operation/kind") == Some("extrude") => {
            Some("/value/operation/value/direction")
        }
        _ => None,
    }
}

/// Whether the decoder left the reversal flag of `properties` unread.
pub(crate) fn reversal_unread(properties: &BTreeMap<NonBlankString, String>) -> bool {
    properties
        .get(REVERSE_DECODED)
        .is_some_and(|value| value == "false")
}

/// Whether an edit cleared the reversal that the decoder read: `feature`
/// travels along its profile normal over a reversal flag `1` that the
/// decoder read as its travel.
pub(crate) fn cleared_reversal(feature: &Feature) -> bool {
    reversal_flag(&feature.source_properties)
        && !reversal_unread(&feature.source_properties)
        && matches!(
            feature.evaluation.definition().operation(),
            FeatureOperation::Extrude {
                direction: ExtrudeDirection::ProfileNormal {},
                ..
            }
        )
}

/// Marks a projected extrusion that travels along its profile normal over a
/// record whose reversal flag is `1`.
pub(crate) fn mark_unread_reversal(feature: &mut Feature) {
    if reversal_flag(&feature.source_properties)
        && matches!(
            feature.evaluation.definition().operation(),
            FeatureOperation::Extrude {
                direction: ExtrudeDirection::ProfileNormal {},
                ..
            }
        )
    {
        set_unread(feature, true);
    }
}

/// Settles a decoded reversal once the extrusion's Boolean operation is
/// bound: the flag is the travel only of an extrusion that adds material,
/// so any other operation travels along the profile normal over an unread
/// flag. The step never reverses an extrusion, so it keeps the travel that
/// [`retain_contradicted_directions`] settles.
pub(crate) fn settle_reversal(feature: &mut Feature) {
    let adds = match feature.evaluation.definition().operation() {
        FeatureOperation::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal {},
            op,
            ..
        } => matches!(op, BooleanOp::Join | BooleanOp::NewBody),
        _ => return,
    };
    if adds || !reversal_flag(&feature.source_properties) {
        return;
    }
    feature.evaluation.edit(|definition, _| retain(definition));
    set_unread(feature, true);
}

fn set_unread(feature: &mut Feature, unread: bool) {
    if !unread {
        feature.source_properties.remove(REVERSE_DECODED);
    } else if let Some(marker) = NonBlankString::new(REVERSE_DECODED.to_owned()) {
        feature.source_properties.insert(marker, "false".to_owned());
    }
}

/// The finite B-rep evidence of extrusion travel.
struct Witness {
    /// Planar faces that can cap an extrusion.
    caps: Vec<Cap>,
    /// The extent of the whole solid.
    solid: Solid,
}

/// The B-rep witness, where a feature of the model or one of `probes` can
/// use it.
fn witness(ir: &CadIr, probes: &[Feature], work: &WorkBudget) -> Option<Witness> {
    let placed = |profile| {
        profile_sketch(&ir.model, profile)
            .is_some_and(|sketch| sketch.resolved_placement().is_some())
    };
    let mut usable = false;
    let mut reversed = false;
    for feature in ir.model.features.iter().chain(probes) {
        if reversed_join(feature).is_some_and(placed) {
            usable = true;
            reversed = true;
        } else if candidate(feature).is_some_and(|candidate| placed(candidate.profile))
            || two_sided(feature).is_some_and(|candidate| placed(candidate.profile))
        {
            usable = true;
        }
    }
    if !usable {
        return None;
    }
    evidence(ir, work, reversed)
}

pub(crate) fn contradicted_features(ir: &CadIr) -> HashSet<FeatureId> {
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    witness(ir, &[], &work).map_or_else(HashSet::new, |witness| {
        matching(&ir.model, &work, |feature| {
            verdict(&ir.model, feature, &witness, &work) == Some(false)
        })
    })
}

/// The features of `model` that `test` holds for, while `work` lasts.
fn matching(
    model: &Model,
    work: &WorkBudget,
    mut test: impl FnMut(&Feature) -> bool,
) -> HashSet<FeatureId> {
    let mut matched = HashSet::new();
    for feature in &model.features {
        if !work.charge() {
            break;
        }
        if test(feature) {
            matched.insert(feature.id.clone());
        }
    }
    matched
}

/// What [`retain_contradicted_directions`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Retained {
    /// Whether a feature changed.
    pub(crate) changed: bool,
    /// The decoded reversals that the solid contradicts, each named by its
    /// feature name and id.
    pub(crate) reversals: Vec<String>,
    /// The decoded reversals that rest on the reversal flag alone, as the
    /// solid neither holds nor contradicts them, each named by its feature
    /// name and id.
    pub(crate) assumed: Vec<String>,
    /// The configuration states that travel along the profile normal against
    /// their own reversal flag, as the active solid contradicts that flag for
    /// the same extrusion, by configuration.
    pub(crate) configuration_reversals: Vec<ConfigurationStates>,
    /// The configuration states whose reversal rests on the reversal flag of
    /// their own lane alone, by configuration.
    pub(crate) configuration_assumed: Vec<ConfigurationStates>,
}

/// Extrusion states of one configuration.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConfigurationStates {
    /// The configuration, named by name and id.
    pub(crate) configuration: String,
    /// Whether the configuration is the active one, whose solid the decoded
    /// B-rep is.
    pub(crate) active: bool,
    /// The extrusions, each named by name and id.
    pub(crate) extrusions: Vec<String>,
}

/// Settles each extrusion's travel against the B-rep: a contradicted
/// reversal travels along the profile normal, other contradicted travel
/// stays in the native lane, and a two-direction extrusion whose end caps
/// confirm its reversal flag travels against the normal. Each reversal that
/// the B-rep does not hold rests on the flag alone.
pub(crate) fn retain_contradicted_directions(ir: &mut CadIr) -> Retained {
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let mut retained = Retained::default();
    let mut settlement = Settlement::default();
    let probes = active_reversals(ir);
    if let Some(witness) = witness(ir, &[], &work) {
        let held = &mut settlement.held;
        let contradicted = matching(&ir.model, &work, |feature| {
            match verdict(&ir.model, feature, &witness, &work) {
                Some(true) => {
                    held.insert(feature.id.clone());
                    false
                }
                verdict => verdict == Some(false),
            }
        });
        let confirmed = matching(&ir.model, &work, |feature| {
            confirms(&ir.model, feature, &witness.caps, &work)
        });
        settlement.held.extend(confirmed.iter().cloned());
        settle_travel(
            ir,
            &contradicted,
            &confirmed,
            &mut settlement,
            &mut retained,
        );
    }
    settlement.active_verdicts = active_verdicts(ir, &probes, &work);
    settle_configuration_states(ir, &settlement, &mut retained);
    retained.assumed = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            reversal_flag(&feature.source_properties)
                && !settlement.held.contains(&feature.id)
                && reversed(feature.evaluation.definition())
        })
        .map(label)
        .collect();
    retained
}

/// A step that settles an extrusion's travel: [`retain`] or [`reverse`].
type Step = fn(&mut FeatureDefinition);

/// The decoder's settlement of extrusion travel against the active solid,
/// which settles the configuration states.
#[derive(Default)]
struct Settlement {
    /// Each extrusion whose model travel settled, with its definition before
    /// the settlement and the step that settled it.
    travel: Vec<(FeatureId, FeatureDefinition, Step)>,
    /// The extrusions whose decoded travel the active solid holds.
    held: HashSet<FeatureId>,
    /// The decoded model reversals that the active solid contradicts.
    fallen: HashSet<FeatureId>,
    /// The active solid's verdict on each reversed state of the active
    /// configuration whose model feature does not travel against the normal.
    active_verdicts: HashMap<FeatureId, bool>,
}

/// The active solid's verdict on each of `probes`, while `work` lasts.
fn active_verdicts(ir: &CadIr, probes: &[Feature], work: &WorkBudget) -> HashMap<FeatureId, bool> {
    let mut verdicts = HashMap::new();
    if let Some(witness) = (!probes.is_empty())
        .then(|| witness(ir, probes, work))
        .flatten()
    {
        for probe in probes {
            if !work.charge() {
                break;
            }
            if let Some(holds) = verdict(&ir.model, probe, &witness, work) {
                verdicts.insert(probe.id.clone(), holds);
            }
        }
    }
    verdicts
}

/// Settles configuration states that the writer reads back from the lanes
/// as the decoder settled them on decode, so that a part whose lanes no
/// edit changed reads back to the states it decoded to. The model of `ir`
/// is decoded and so already settled: the settlement is rebuilt from each
/// model extrusion as it read before the decoder settled it, tested against
/// the same solid, and applied by the decoder's own step.
///
/// The decoder's settlement changes a model extrusion's direction only, so
/// each settled reading is recovered from what it left: a reversal the
/// solid contradicts reads along the normal over an unread flag `1`, travel
/// along the normal that the solid contradicts reads unresolved, and a
/// two-direction reversal the end caps confirm reads reversed over a flag
/// the decoder read. The model of `ir` is left as it is.
pub(crate) fn settle_projected_configurations(ir: &mut CadIr) {
    if ir.model.configurations.is_empty() {
        return;
    }
    let work = WorkBudget::new(MAX_WITNESS_WORK);
    let mut settlement = Settlement::default();
    let unsettled = ir
        .model
        .features
        .iter()
        .filter_map(unsettled_reading)
        .collect::<Vec<_>>();
    let readings = unsettled
        .iter()
        .map(|(feature, _)| feature.clone())
        .collect::<Vec<_>>();
    if let Some(witness) = (!readings.is_empty())
        .then(|| witness(ir, &readings, &work))
        .flatten()
    {
        for (feature, settled_by) in &unsettled {
            let settled = match settled_by {
                SettledBy::Contradiction => {
                    verdict(&ir.model, feature, &witness, &work) == Some(false)
                }
                SettledBy::Confirmation => confirms(&ir.model, feature, &witness.caps, &work),
            };
            if !settled {
                continue;
            }
            let original = feature.evaluation.definition().clone();
            if reversed(&original) && *settled_by == SettledBy::Contradiction {
                settlement.fallen.insert(feature.id.clone());
            }
            let step: Step = match settled_by {
                SettledBy::Contradiction => retain,
                SettledBy::Confirmation => reverse,
            };
            settlement.travel.push((feature.id.clone(), original, step));
        }
    }
    // The decoder probes the active states of model extrusions that read
    // along the normal before the settlement, which a fallen reversal did
    // not.
    let probes = active_reversals(ir)
        .into_iter()
        .filter(|probe| !settlement.fallen.contains(&probe.id))
        .collect::<Vec<_>>();
    settlement.active_verdicts = active_verdicts(ir, &probes, &work);
    settle_configuration_states(ir, &settlement, &mut Retained::default());
}

/// How the decoder settled a model extrusion's travel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettledBy {
    /// The solid contradicted the decoded travel.
    Contradiction,
    /// The end caps confirmed a two-direction reversal.
    Confirmation,
}

/// A decoded model extrusion that the decoder may have settled, as it read
/// before the settlement, with the test that settles it.
fn unsettled_reading(feature: &Feature) -> Option<(Feature, SettledBy)> {
    let FeatureOperation::Extrude {
        direction, extent, ..
    } = feature.evaluation.definition().operation()
    else {
        return None;
    };
    let flag = reversal_flag(&feature.source_properties);
    let unread = reversal_unread(&feature.source_properties);
    let (before, settled_by) = match (direction, extent) {
        (ExtrudeDirection::ProfileNormal {}, ExtrudeExtent::OneSided { .. }) if flag && unread => (
            ExtrudeDirection::ReversedProfileNormal {},
            SettledBy::Contradiction,
        ),
        (ExtrudeDirection::Unresolved {}, ExtrudeExtent::OneSided { .. }) => {
            (ExtrudeDirection::ProfileNormal {}, SettledBy::Contradiction)
        }
        (ExtrudeDirection::ReversedProfileNormal {}, ExtrudeExtent::TwoSided { .. })
            if flag && !unread =>
        {
            (ExtrudeDirection::ProfileNormal {}, SettledBy::Confirmation)
        }
        _ => return None,
    };
    let mut reading = feature.clone();
    reading.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) =
            definition
        {
            *direction = before;
        }
    });
    if settled_by == SettledBy::Confirmation {
        set_unread(&mut reading, true);
    }
    Some((reading, settled_by))
}

/// The feature's name and id, or its id where it has no name.
pub(crate) fn label(feature: &Feature) -> String {
    feature.name.as_ref().map_or_else(
        || feature.id.as_str().to_owned(),
        |name| format!("\"{name}\" ({})", feature.id.as_str()),
    )
}

/// Settles the travel of each contradicted or confirmed model extrusion, and
/// records each in `settlement`, with the decoded reversals that fell back
/// to the profile normal.
fn settle_travel(
    ir: &mut CadIr,
    contradicted: &HashSet<FeatureId>,
    confirmed: &HashSet<FeatureId>,
    settlement: &mut Settlement,
    retained: &mut Retained,
) {
    for feature in &mut ir.model.features {
        let settle: Step = if contradicted.contains(&feature.id) {
            retain
        } else if confirmed.contains(&feature.id) {
            reverse
        } else {
            continue;
        };
        let original = feature.evaluation.definition().clone();
        feature.evaluation.edit(|definition, _| settle(definition));
        retained.changed |= original != *feature.evaluation.definition();
        if reversed(&original) {
            retained.reversals.push(label(feature));
            settlement.fallen.insert(feature.id.clone());
            set_unread(feature, true);
            retained.changed = true;
        } else if confirmed.contains(&feature.id) {
            set_unread(feature, false);
            retained.changed = true;
        }
        settlement
            .travel
            .push((feature.id.clone(), original, settle));
    }
}

/// Settles the configuration states by `settlement`: each active state equal
/// to a settled model extrusion's definition before the settlement takes the
/// same step, and then each reversed state as
/// [`settle_configuration_reversals`] says.
fn settle_configuration_states(ir: &mut CadIr, settlement: &Settlement, retained: &mut Retained) {
    for (id, original, settle) in &settlement.travel {
        for configuration in ir
            .model
            .configurations
            .iter_mut()
            .filter(|configuration| configuration.active)
        {
            if let Some(state) = configuration.feature_states.get_mut(id) {
                if state.definition == *original {
                    settle(&mut state.definition);
                }
            }
        }
    }
    settle_configuration_reversals(
        ir,
        &settlement.held,
        &settlement.fallen,
        &settlement.active_verdicts,
        retained,
    );
}

/// Each model feature that does not travel against the profile normal, with
/// the definition of its unsuppressed state in the active configuration
/// where that state does: the probes that test that state against the
/// active solid, which is the solid of the active configuration.
fn active_reversals(ir: &CadIr) -> Vec<Feature> {
    ir.model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .flat_map(|configuration| &configuration.feature_states)
        .filter(|(_, state)| reversed(&state.definition) && !state.evaluation.is_suppressed())
        .filter_map(|(id, state)| {
            let feature = ir.model.features.iter().find(|feature| &feature.id == id)?;
            if reversed(feature.evaluation.definition()) {
                return None;
            }
            let mut probe = feature.clone();
            probe.evaluation.set_definition(state.definition.clone());
            Some(probe)
        })
        .collect()
}

/// Settles each configuration state that travels against the profile normal
/// on the reversal flag of its own lane. The active solid is the solid of
/// the active configuration: the model's reading stands for that
/// configuration's state wherever the model feature travels against the
/// normal, and `active_verdicts` holds the active solid's verdict on each
/// other reversed state of that configuration. An inactive state shares the
/// extrusion's geometry where its definition equals the model feature's or
/// the active configuration's state, and the active solid holds it where it
/// holds the model's reversal or that active state. Where the active solid
/// contradicts the reversal of an extrusion, the reversal travels along the
/// normal in every configuration, as the decoder takes the extrusion's
/// sketch normal as the same in every configuration. Each other reversal
/// rests on its flag alone.
fn settle_configuration_reversals(
    ir: &mut CadIr,
    held: &HashSet<FeatureId>,
    fallen: &HashSet<FeatureId>,
    active_verdicts: &HashMap<FeatureId, bool>,
    retained: &mut Retained,
) {
    let features: HashMap<&FeatureId, &Feature> = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect();
    let active: HashMap<FeatureId, FeatureDefinition> = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .flat_map(|configuration| &configuration.feature_states)
        .filter(|(_, state)| reversed(&state.definition))
        .map(|(id, state)| (id.clone(), state.definition.clone()))
        .collect();
    for configuration in &mut ir.model.configurations {
        let name = configuration_label(configuration);
        let is_active = configuration.active;
        let states = |extrusions| ConfigurationStates {
            configuration: name.clone(),
            active: is_active,
            extrusions,
        };
        let mut contradicted = Vec::new();
        let mut assumed = Vec::new();
        for (id, state) in &mut configuration.feature_states {
            if !reversed(&state.definition) {
                continue;
            }
            let feature = features.get(id).copied();
            let extrusion = feature.map_or_else(|| id.as_str().to_owned(), label);
            let verdict = active_verdicts.get(id).copied();
            if fallen.contains(id) || verdict == Some(false) {
                retain(&mut state.definition);
                if !(is_active && fallen.contains(id)) {
                    contradicted.push(extrusion);
                }
                continue;
            }
            let covered = if is_active {
                verdict == Some(true)
                    || feature.is_some_and(|feature| reversed(feature.evaluation.definition()))
            } else {
                (held.contains(id) || verdict == Some(true))
                    && (feature.is_some_and(|feature| {
                        *feature.evaluation.definition() == state.definition
                    }) || active.get(id) == Some(&state.definition))
            };
            if !covered {
                assumed.push(extrusion);
            }
        }
        if !contradicted.is_empty() {
            retained.configuration_reversals.push(states(contradicted));
        }
        if !assumed.is_empty() {
            retained.configuration_assumed.push(states(assumed));
        }
    }
}

/// Whether the definition is an extrusion that travels against the profile
/// normal.
fn reversed(definition: &FeatureDefinition) -> bool {
    matches!(
        definition.operation(),
        FeatureOperation::Extrude {
            direction: ExtrudeDirection::ReversedProfileNormal {},
            ..
        }
    )
}

/// The configuration's name and id, or its id where it has no name.
pub(crate) fn configuration_label(
    configuration: &cadmpeg_ir::features::parameters::DesignConfiguration,
) -> String {
    configuration.name.as_ref().map_or_else(
        || configuration.id.as_str().to_owned(),
        |name| format!("\"{name}\" ({})", configuration.id.as_str()),
    )
}

/// A contradicted reversal travels along the profile normal, as its solid
/// shows; other contradicted travel stays in the native lane.
fn retain(definition: &mut FeatureDefinition) {
    if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) = definition {
        *direction = match direction {
            ExtrudeDirection::ReversedProfileNormal {} => ExtrudeDirection::ProfileNormal {},
            _ => ExtrudeDirection::Unresolved {},
        };
    }
}

/// A confirmed reversal travels against the profile normal.
fn reverse(definition: &mut FeatureDefinition) {
    if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) = definition {
        *direction = ExtrudeDirection::ReversedProfileNormal {};
    }
}

/// A one-sided blind extrusion from its planar profile's plane.
struct OneSided<'a> {
    profile: &'a PlanarProfileRef,
    /// The depth.
    length: f64,
    /// `-1` for a cut, `1` for an extrusion that adds material.
    operation_sign: f64,
    /// `-1` for travel against the profile normal, `1` along it.
    travel: f64,
}

/// The planar profile of a one-sided extrusion from its profile's plane
/// that adds material against the profile normal, whatever its
/// termination: the travel that the whole solid can contradict.
fn reversed_join(feature: &Feature) -> Option<&PlanarProfileRef> {
    if feature.suppressed != Some(false) {
        return None;
    }
    match feature.evaluation.definition().operation() {
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(profile),
            start: ExtrudeStart::ProfilePlane {},
            direction: ExtrudeDirection::ReversedProfileNormal {},
            extent: ExtrudeExtent::OneSided { .. },
            op: BooleanOp::Join | BooleanOp::NewBody,
            ..
        } => Some(profile),
        _ => None,
    }
}

fn candidate(feature: &Feature) -> Option<OneSided<'_>> {
    if feature.suppressed != Some(false) {
        return None;
    }
    match feature.evaluation.definition().operation() {
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(profile),
            start: ExtrudeStart::ProfilePlane {},
            direction,
            extent: ExtrudeExtent::OneSided { side },
            op,
            ..
        } => {
            let travel = match direction {
                ExtrudeDirection::ProfileNormal {} | ExtrudeDirection::Unresolved {} => 1.0,
                ExtrudeDirection::ReversedProfileNormal {} => -1.0,
                ExtrudeDirection::Explicit { .. } => return None,
            };
            let LinearTermination::Blind { length } = side.termination else {
                return None;
            };
            let operation_sign = match op {
                BooleanOp::Cut => -1.0,
                BooleanOp::NewBody | BooleanOp::Join => 1.0,
                _ => return None,
            };
            Some(OneSided {
                profile,
                length: length.get(),
                operation_sign,
                travel,
            })
        }
        _ => None,
    }
}

/// A two-direction blind extrusion that adds material, from its planar
/// profile's plane along the profile normal over a record whose reversal
/// flag the decoder left unread.
struct TwoSided<'a> {
    profile: &'a PlanarProfileRef,
    /// The first direction's depth.
    first: f64,
    /// The second direction's depth.
    second: f64,
}

fn two_sided(feature: &Feature) -> Option<TwoSided<'_>> {
    if feature.suppressed != Some(false)
        || !reversal_flag(&feature.source_properties)
        || !reversal_unread(&feature.source_properties)
    {
        return None;
    }
    match feature.evaluation.definition().operation() {
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(profile),
            start: ExtrudeStart::ProfilePlane {},
            direction: ExtrudeDirection::ProfileNormal {},
            extent: ExtrudeExtent::TwoSided { first, second },
            op: BooleanOp::Join | BooleanOp::NewBody,
            ..
        } => {
            let (
                LinearTermination::Blind { length: first },
                LinearTermination::Blind { length: second },
            ) = (&first.termination, &second.termination)
            else {
                return None;
            };
            Some(TwoSided {
                profile,
                first: first.get(),
                second: second.get(),
            })
        }
        _ => None,
    }
}

fn profile_sketch<'a>(model: &'a Model, profile: &'a PlanarProfileRef) -> Option<&'a Sketch> {
    let id = match profile {
        PlanarProfileRef::Sketch(id)
        | PlanarProfileRef::SketchProfiles { sketch: id, .. }
        | PlanarProfileRef::SketchEntities { sketch: id, .. } => id,
        PlanarProfileRef::Feature(id) => {
            let feature = model.features.iter().find(|feature| &feature.id == id)?;
            let FeatureOperation::Sketch { sketch } = feature.evaluation.definition().operation()
            else {
                return None;
            };
            sketch.id()?
        }
        _ => return None,
    };
    model.sketches.iter().find(|sketch| &sketch.id == id)
}

fn profile_points(
    model: &Model,
    profile: &PlanarProfileRef,
    sketch: &Sketch,
) -> Option<Vec<Point3>> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = normal.get();
    let u_axis = u_axis.get();
    let v_axis = normal.cross(u_axis);
    let entities: HashMap<&SketchEntityId, &SketchEntity> = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .map(|entity| (entity.id(), entity))
        .collect();
    let mut ids = Vec::new();
    match profile {
        PlanarProfileRef::SketchEntities { entities, .. } => ids.extend(entities.iter()),
        PlanarProfileRef::SketchProfiles { profiles, .. } => {
            for index in profiles {
                ids.extend(
                    sketch
                        .profiles
                        .as_slice()
                        .get(usize::try_from(*index).ok()?)?
                        .iter()
                        .map(|use_| &use_.entity),
                );
            }
        }
        _ if !sketch.profiles.is_empty() => ids.extend(
            sketch
                .profiles
                .as_slice()
                .iter()
                .flatten()
                .map(|use_| &use_.entity),
        ),
        _ => ids.extend(
            model
                .sketch_entities
                .iter()
                .filter(|entity| {
                    entity.sketch == sketch.id
                        && !entity.construction
                        && !matches!(
                            entity.geometry.definition(),
                            SketchGeometryDefinition::Point { .. }
                        )
                })
                .map(SketchEntity::id),
        ),
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    let mut points = Vec::new();
    for id in ids {
        let entity = entities.get(id)?;
        for point in samples(entity)? {
            let point = origin
                .get()
                .translated(u_axis, point.u)
                .translated(v_axis, point.v);
            if !point.is_finite() {
                return None;
            }
            points.push(point);
        }
    }
    (!points.is_empty()).then_some(points)
}

fn samples(entity: &SketchEntity) -> Option<Vec<Point2>> {
    let (center, major_radius, minor_radius, angle, start, sweep) =
        match entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                let start = start.get();
                let end = end.get();
                return Some(vec![
                    start,
                    end,
                    Point2::new(start.u.midpoint(end.u), start.v.midpoint(end.v)),
                ]);
            }
            SketchGeometryDefinition::Point { position } => return Some(vec![position.get()]),
            SketchGeometryDefinition::Circle { center, radius } => (
                center.get(),
                radius.get(),
                radius.get(),
                0.0,
                0.0,
                std::f64::consts::TAU,
            ),
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => (
                center.get(),
                radius.get(),
                radius.get(),
                0.0,
                start_angle.get(),
                (end_angle.get() - start_angle.get()).rem_euclid(std::f64::consts::TAU),
            ),
            SketchGeometryDefinition::Ellipse {
                center,
                radii,
                major_angle,
                bounds,
            } => {
                let [start, end] = bounds.map_or([0.0, std::f64::consts::TAU], |bounds| {
                    bounds.map(cadmpeg_ir::scalar::Angle::get)
                });
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                (
                    center.get(),
                    radii.major().get(),
                    radii.minor().get(),
                    major_angle.get(),
                    start,
                    if sweep == 0.0 && end != start {
                        std::f64::consts::TAU
                    } else {
                        sweep
                    },
                )
            }
            _ => return None,
        };
    let (axis_sine, axis_cosine) = angle.sin_cos();
    Some(
        (0..=CURVE_SAMPLE_INTERVALS)
            .map(|i| {
                let (sine, cosine) =
                    (start + sweep * f64::from(i) / f64::from(CURVE_SAMPLE_INTERVALS)).sin_cos();
                Point2::new(
                    center.u + major_radius * cosine * axis_cosine
                        - minor_radius * sine * axis_sine,
                    center.v
                        + major_radius * cosine * axis_sine
                        + minor_radius * sine * axis_cosine,
                )
            })
            .collect(),
    )
}

/// What the B-rep says of `feature`'s decoded travel. It contradicts it
/// (`Some(false)`) for a reversal that adds material whose whole solid lies
/// on the profile normal's side of the profile plane, or for a start cap
/// holding the profile with an end cap only on the opposite side. It holds
/// it (`Some(true)`) for a reversal whose whole solid lies on the other
/// side, or for a start cap with an end cap only on the decoded side.
fn verdict(model: &Model, feature: &Feature, witness: &Witness, work: &WorkBudget) -> Option<bool> {
    let side = reversed_join(feature)
        .and_then(|profile| profile_sketch(model, profile))
        .and_then(Sketch::resolved_placement)
        .and_then(|(origin, normal, _)| witness.solid.span(origin.get(), normal.get()))
        .and_then(|(lowest, highest)| {
            if lowest >= -EPS_SOLID_SIDE && highest > EPS_SOLID_SIDE {
                Some(false)
            } else if highest <= EPS_SOLID_SIDE && lowest < -EPS_SOLID_SIDE {
                Some(true)
            } else {
                None
            }
        });
    if let Some(holds) = side {
        return work.charge().then_some(holds);
    }
    let OneSided {
        profile,
        length,
        operation_sign,
        travel,
    } = candidate(feature)?;
    let sketch = profile_sketch(model, profile)?;
    let (_, normal, _) = sketch.resolved_placement()?;
    let normal = normal.get();
    let points = profile_points(model, profile, sketch)?;
    let start = witness
        .caps
        .iter()
        .filter(|cap| cap.normal.dot(normal).abs() > 1.0 - EPS_CAP_ALIGNMENT)
        .any(|cap| points.iter().all(|point| cap.supports(*point, work)));
    if !start {
        return None;
    }
    let end = |travel: f64| {
        end_cap(
            &witness.caps,
            &points,
            normal,
            length * travel,
            travel * operation_sign,
            work,
        )
    };
    let decoded = end(travel);
    let opposite = end(-travel);
    (decoded != opposite && work.charge()).then_some(decoded)
}

/// Whether the end caps confirm the reversal of a two-direction extrusion:
/// they lie at the first depth against the profile normal and at the
/// second along it, the pair of the unreversed reading is incomplete, and
/// the depths differ.
fn confirms(model: &Model, feature: &Feature, caps: &[Cap], work: &WorkBudget) -> bool {
    let Some(TwoSided {
        profile,
        first,
        second,
    }) = two_sided(feature)
    else {
        return false;
    };
    if (first - second).abs() <= EPS_CARRIER_DISTANCE {
        return false;
    }
    let Some(sketch) = profile_sketch(model, profile) else {
        return false;
    };
    let Some((_, normal, _)) = sketch.resolved_placement() else {
        return false;
    };
    let Some(points) = profile_points(model, profile, sketch) else {
        return false;
    };
    let normal = normal.get();
    let end = |offset: f64| end_cap(caps, &points, normal, offset, offset.signum(), work);
    let reversed = end(-first) && end(second);
    let unreversed = end(first) && end(-second);
    reversed && !unreversed && work.charge()
}

/// Whether a cap facing `outward` (`1` along `normal`, `-1` against it)
/// holds one of the profile `points` moved `offset` along `normal`.
fn end_cap(
    caps: &[Cap],
    points: &[Point3],
    normal: Vector3,
    offset: f64,
    outward: f64,
    work: &WorkBudget,
) -> bool {
    caps.iter().any(|cap| {
        cap.normal.dot(normal) * cap.sense * outward > 1.0 - EPS_CAP_ALIGNMENT
            && points
                .iter()
                .any(|point| cap.supports(point.translated(normal, offset), work))
    })
}

/// The points and circular edges that bound the solid, in model space: the
/// vertices, and each closed edge without vertices by its circle, its
/// ellipse (by its largest radius) or its NURBS control points; a closed
/// edge of another form leaves the extent unknown. An open edge counts by
/// its vertices only: the edge has no parameter range or sense against its
/// curve, and an arc's full circle reaches past the solid. A reversal the
/// solid holds still shows, since its end cap has vertices or closed edges
/// at its depth.
#[derive(Default)]
struct Solid {
    points: Vec<Point3>,
    /// Each circle's center, unit axis and radius.
    circles: Vec<(Point3, Vector3, f64)>,
    unknown: bool,
}

impl Solid {
    /// The lowest and highest offset of the solid along `normal` from
    /// `origin`, where its extent is known.
    fn span(&self, origin: Point3, normal: Vector3) -> Option<(f64, f64)> {
        if self.unknown {
            return None;
        }
        let points = self
            .points
            .iter()
            .map(|point| point.vector_from(origin).dot(normal));
        let circles = self.circles.iter().flat_map(|(center, axis, radius)| {
            let offset = center.vector_from(origin).dot(normal);
            let reach = radius * (1.0 - axis.dot(normal).powi(2)).max(0.0).sqrt();
            [offset - reach, offset + reach]
        });
        points.chain(circles).fold(None, |span, offset| {
            Some(
                span.map_or((offset, offset), |(lowest, highest): (f64, f64)| {
                    (lowest.min(offset), highest.max(offset))
                }),
            )
        })
    }
}

struct Cap {
    origin: Point3,
    normal: Vector3,
    sense: f64,
    vertices: Vec<Point3>,
    triangles: Vec<[Point3; 3]>,
    polygons: Option<Vec<Vec<Point3>>>,
    transform: Transform,
}

/// The planar caps of `ir`'s B-rep in model space, and its solid where
/// `with_solid`. The solid has its own work budget, so that collecting it
/// never costs a cap witness.
fn evidence(ir: &CadIr, work: &WorkBudget, with_solid: bool) -> Option<Witness> {
    let indexing_work = [
        ir.geometry.surfaces.len(),
        ir.geometry.shells.len(),
        ir.geometry.regions.len(),
        ir.geometry.bodies.len(),
        ir.geometry.faces.len(),
        ir.geometry.loops.len(),
        ir.geometry.coedges.len(),
        ir.geometry.edges.len(),
        ir.geometry.vertices.len(),
        ir.geometry.points.len(),
        ir.geometry.curves.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)?;
    if !work.charge_by(indexing_work) {
        return None;
    }
    let surfaces: HashMap<_, _> = ir
        .geometry
        .surfaces
        .iter()
        .map(|s| (&s.id, &s.geometry))
        .collect();
    let shells: HashMap<_, _> = ir
        .geometry
        .shells
        .iter()
        .map(|s| (&s.id, &s.region))
        .collect();
    let body_transforms: HashMap<_, _> = ir
        .geometry
        .bodies
        .iter()
        .map(|b| (&b.id, b.transform.unwrap_or_else(Transform::identity)))
        .collect();
    let regions: HashMap<_, _> = ir
        .geometry
        .regions
        .iter()
        .filter_map(|r| Some((&r.id, *body_transforms.get(&r.body)?)))
        .collect();
    let loops: HashMap<_, _> = ir.geometry.loops.iter().map(|l| (&l.id, l)).collect();
    let coedges: HashMap<_, _> = ir.geometry.coedges.iter().map(|c| (&c.id, c)).collect();
    let edges: HashMap<_, _> = ir.geometry.edges.iter().map(|e| (&e.id, e)).collect();
    let vertices: HashMap<_, _> = ir
        .geometry
        .vertices
        .iter()
        .map(|v| (&v.id, &v.point))
        .collect();
    let points: HashMap<_, _> = ir
        .geometry
        .points
        .iter()
        .map(|p| (&p.id, p.position().get()))
        .collect();
    let curves: HashMap<_, _> = ir
        .geometry
        .curves
        .iter()
        .map(|c| (&c.id, &c.geometry))
        .collect();
    let mut result = Vec::new();
    let solid_work = WorkBudget::new(MAX_WITNESS_WORK);
    let mut solid = Solid {
        unknown: !with_solid,
        ..Solid::default()
    };
    let mut indices = HashMap::new();
    for face in &ir.geometry.faces {
        let Some(transform) = shells
            .get(&face.shell)
            .and_then(|r| regions.get(r))
            .copied()
        else {
            solid.unknown = true;
            continue;
        };
        let Some(face_loops) = face.loops.boundary() else {
            solid.unknown = true;
            continue;
        };
        for coedge in face_loops
            .iter()
            .filter_map(|lid| loops.get(lid))
            .flat_map(|loop_| loop_.coedges())
        {
            if solid.unknown {
                break;
            }
            if !solid_work.charge() {
                solid.unknown = true;
                break;
            }
            let Some(edge) = coedges.get(coedge).and_then(|c| edges.get(&c.edge)) else {
                solid.unknown = true;
                continue;
            };
            if let Some(ends) = edge.ends.as_ref() {
                for vertex in [&ends.start, &ends.end] {
                    match vertices
                        .get(vertex)
                        .and_then(|point| points.get(point))
                        .and_then(|point| transform.apply_point(*point))
                    {
                        Some(point) => solid.points.push(point.get()),
                        None => solid.unknown = true,
                    }
                }
                continue;
            }
            match edge
                .curve()
                .and_then(|c| curves.get(c))
                .and_then(|c| c.solved())
            {
                Some(SolvedCurveGeometry::Circle(circle)) => solid.circle(
                    &transform,
                    circle.center().get(),
                    *circle.frame().axis().as_raw(),
                    circle.radius().get(),
                ),
                Some(SolvedCurveGeometry::Ellipse(ellipse)) => solid.circle(
                    &transform,
                    ellipse.center().get(),
                    *ellipse.frame().axis().as_raw(),
                    ellipse.major_radius().get(),
                ),
                Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    if !solid_work.charge_by(nurbs.pole_count()) {
                        solid.unknown = true;
                        break;
                    }
                    for control in nurbs.control_points() {
                        match transform.apply_point(control.get()) {
                            Some(point) => solid.points.push(point.get()),
                            None => solid.unknown = true,
                        }
                    }
                }
                _ => solid.unknown = true,
            }
        }
        let Some(SolvedSurfaceGeometry::Plane(plane)) =
            surfaces.get(&face.surface).and_then(|s| s.solved())
        else {
            continue;
        };
        let Some(origin) = transform.apply_point(plane.origin().get()) else {
            continue;
        };
        let Some(normal) = transform.apply_normal(*plane.frame().axis().as_raw()) else {
            continue;
        };
        let mut cap = Cap {
            origin: origin.get(),
            normal: *normal.as_raw(),
            sense: if face.sense == Sense::Reversed {
                -1.0
            } else {
                1.0
            },
            vertices: Vec::new(),
            triangles: Vec::new(),
            polygons: Some(Vec::new()),
            transform,
        };
        for lid in face_loops {
            let polygon = (|| {
                let loop_ = loops.get(lid)?;
                let mut polygon = Vec::new();
                let mut first = None;
                let mut previous = None;
                for cid in loop_.coedges() {
                    if !work.charge() {
                        return None;
                    }
                    let coedge = coedges.get(cid)?;
                    let edge = edges.get(&coedge.edge)?;
                    let ends = edge.ends.as_ref()?;
                    let (a, b) = if coedge.sense == Sense::Reversed {
                        (&ends.end, &ends.start)
                    } else {
                        (&ends.start, &ends.end)
                    };
                    let ap = transform.apply_point(*points.get(vertices.get(a)?)?)?.get();
                    let bp = transform.apply_point(*points.get(vertices.get(b)?)?)?.get();
                    cap.vertices.extend([ap, bp]);
                    if previous.is_some_and(|p| p != a) {
                        return None;
                    }
                    first.get_or_insert(a);
                    previous = Some(b);
                    if !matches!(
                        curves.get(edge.curve()?)?.solved(),
                        Some(SolvedCurveGeometry::Line(_))
                    ) {
                        return None;
                    }
                    polygon.push(ap);
                }
                (first.is_some() && first == previous).then_some(polygon)
            })();
            match (cap.polygons.as_mut(), polygon) {
                (Some(polygons), Some(polygon)) => polygons.push(polygon),
                _ => cap.polygons = None,
            }
        }
        indices.insert(&face.id, result.len());
        result.push(cap);
    }
    for mesh in &ir.model.tessellations {
        let face_count = mesh
            .faces
            .iter()
            .filter(|face| indices.contains_key(*face))
            .count();
        if face_count == 0 {
            continue;
        }
        let work_count = mesh
            .triangle_count()
            .checked_mul(face_count)?
            .checked_mul(3)?
            .checked_add(mesh.vertex_count())?;
        if !work.charge_by(work_count) {
            return None;
        }
        let vertices = mesh.vertices();
        let triangles = mesh.triangles();
        for face in &mesh.faces {
            let Some(index) = indices.get(face) else {
                continue;
            };
            let cap = &mut result[*index];
            for triangle in &triangles {
                let points = triangle.map(|i| {
                    vertices
                        .get(cadmpeg_core::decode::index_from_u32(i))
                        .and_then(|p| cap.transform.apply_point(p.get()))
                        .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
                });
                if let [Some(a), Some(b), Some(c)] = points {
                    if [a, b, c].iter().all(|p| {
                        p.vector_from(cap.origin).dot(cap.normal).abs() <= EPS_CAP_MESH_DISTANCE
                    }) {
                        cap.triangles.push([a, b, c].map(|p| {
                            p.translated(cap.normal, -p.vector_from(cap.origin).dot(cap.normal))
                        }));
                    }
                }
            }
        }
    }
    Some(Witness {
        caps: result,
        solid,
    })
}

impl Solid {
    /// Adds the circle about `center` with `axis` and `radius`, placed by
    /// `transform`.
    fn circle(&mut self, transform: &Transform, center: Point3, axis: Vector3, radius: f64) {
        match (transform.apply_point(center), transform.apply_normal(axis)) {
            (Some(center), Some(axis)) if radius.is_finite() => {
                self.circles.push((center.get(), *axis.as_raw(), radius));
            }
            _ => self.unknown = true,
        }
    }
}

impl Cap {
    fn supports(&self, p: Point3, work: &WorkBudget) -> bool {
        if !work.charge()
            || p.vector_from(self.origin).dot(self.normal).abs() > EPS_CARRIER_DISTANCE
        {
            return false;
        }
        if !work.charge_by(self.vertices.len()) {
            return false;
        }
        if self
            .vertices
            .iter()
            .any(|v| p.distance(*v) <= EPS_CARRIER_DISTANCE)
        {
            return true;
        }
        if !self.triangles.is_empty() {
            return self
                .triangles
                .iter()
                .any(|triangle| work.charge() && triangle_contains(p, *triangle));
        }
        let Some(polygons) = &self.polygons else {
            return false;
        };
        let n = self.normal;
        let project = |p: Point3| {
            if n.x.abs() >= n.y.abs().max(n.z.abs()) {
                Point2::new(p.y, p.z)
            } else if n.y.abs() >= n.z.abs() {
                Point2::new(p.x, p.z)
            } else {
                Point2::new(p.x, p.y)
            }
        };
        let p = project(p);
        let mut inside = false;
        for polygon in polygons {
            for (a, b) in polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
            {
                if !work.charge() {
                    return false;
                }
                let a = project(*a);
                let b = project(*b);
                let dx = b.u - a.u;
                let dy = b.v - a.v;
                let length = dx.hypot(dy);
                let along = (p.u - a.u) * dx + (p.v - a.v) * dy;
                if length > EPS_BOUNDARY_DISTANCE
                    && ((p.u - a.u) * dy - (p.v - a.v) * dx).abs() <= EPS_BOUNDARY_DISTANCE * length
                    && along >= -EPS_BOUNDARY_DISTANCE
                    && along <= length * length + EPS_BOUNDARY_DISTANCE * length
                {
                    return true;
                }
                if (a.v > p.v) != (b.v > p.v) && p.u < (b.u - a.u) * (p.v - a.v) / (b.v - a.v) + a.u
                {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

fn triangle_contains(point: Point3, [origin, first, second]: [Point3; 3]) -> bool {
    let first_edge = first.vector_from(origin);
    let second_edge = second.vector_from(origin);
    let offset = point.vector_from(origin);
    let first_squared = first_edge.dot(first_edge);
    let second_squared = second_edge.dot(second_edge);
    let mixed = first_edge.dot(second_edge);
    let determinant = first_squared * second_squared - mixed * mixed;
    if determinant.abs() < MIN_TRIANGLE_DETERMINANT {
        return false;
    }
    let first_weight =
        (offset.dot(first_edge) * second_squared - offset.dot(second_edge) * mixed) / determinant;
    let second_weight =
        (offset.dot(second_edge) * first_squared - offset.dot(first_edge) * mixed) / determinant;
    let slack = EPS_BOUNDARY_DISTANCE / first_edge.norm().max(second_edge.norm()).max(1.0);
    first_weight >= -slack && second_weight >= -slack && first_weight + second_weight <= 1.0 + slack
}

#[cfg(test)]
mod tests;
