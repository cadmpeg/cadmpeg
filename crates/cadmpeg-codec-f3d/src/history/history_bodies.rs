// SPDX-License-Identifier: Apache-2.0
//! Stored feature results and bodies consumed by initial cuts.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Model, Topology};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::{Feature, FeatureContent, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::history_body::{BodyTopology, HistoryBody, HistoryStateRef};
use cadmpeg_ir::ids::HistoryBodyId;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::schema::EntitySchema;

use crate::container::{self, ContainerScan};
use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;

const MAX_HISTORY_BODIES: usize = 64;
const MAX_HISTORY_BODY_WORK: usize = 1 << 21;

/// A body slot of one history state that a feature produces or consumes.
struct Want<'a> {
    /// The feature that first named the slot, for notes and admission.
    origin: &'a Feature,
    producer: Option<&'a Feature>,
    consumer: Option<&'a Feature>,
    state: i64,
    slot: i64,
    capture_current: bool,
    consumed: bool,
    /// A slot that the state names only by the persistent design identity of
    /// its body. `slot` is not read while this is present.
    identity: Option<BodyIdentity>,
}

/// The persistent design identity that a Split body recipe stores for its
/// target, and the body slots that the Split transition revises or deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BodyIdentity {
    design_id: String,
    selector: i64,
    revised: Vec<i64>,
}

/// A consumed history body found through its design identity. A producing
/// feature whose nest has equal records joins this nest.
struct Joinable {
    slot: i64,
    index: usize,
    topology: BodyTopology,
}

#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub(crate) features: &'a [Feature],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) bindings: &'a HashMap<String, String>,
    pub(crate) primary_brep: Option<&'a str>,
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignBodyRecipeOperand],
    pub(crate) recipes: &'a [ConstructionRecipe],
}

pub(crate) fn derived_base_snapshots(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    scopes: &mut [DesignParameterScope],
    histories: &[AsmHistory],
    bindings: &[crate::records::bodies::DesignBodyBinding],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<HistoryBody>, CodecError> {
    use crate::records::feature::derived_base::DesignDerivedBaseSnapshot;
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureContent, FeatureDefinition, FeatureOperation};

    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    let mut bodies = Vec::new();
    for scope in scopes {
        let Some(source) = scope.derived_base_source.as_ref() else {
            continue;
        };
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        let unique_binding = |suffix| {
            let mut matches = bindings.iter().filter(|binding| {
                crate::ids::native_stream(binding.id()) == Some(stream)
                    && binding.entity_suffix == suffix
            });
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        };
        let (Some(_), Some(result)) = (
            unique_binding(source.source_body.value),
            unique_binding(source.result_body.value),
        ) else {
            continue;
        };
        if !result.history_only || result.pair_count() != 1 {
            continue;
        }
        let Some(blob) = result.blob_name().strip_prefix("BREP.") else {
            continue;
        };
        let mut matching = histories.iter().filter(|history| {
            super::face_resolution::historical_brep_source(&history.id) == Some(blob)
        });
        let Some(history) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            continue;
        }
        let (Some(current), Some(previous)) =
            (scope.history_state_id(), scope.previous_history_state_id())
        else {
            continue;
        };
        let Some((_, state, _)) = super::history_states::unique_history_state_pair(
            std::slice::from_ref(history),
            current,
            previous,
        ) else {
            continue;
        };
        let Some(topology) = history.topology(state) else {
            continue;
        };
        let [slot] = topology.bodies.as_slice() else {
            continue;
        };
        let slot = *slot;
        let Some(facts) = container::history_breps(scan).find(|facts| {
            history
                .id
                .starts_with(&format!("f3d:{}:asm-history#", facts.name))
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&facts.name)?;
        let feature = Feature {
            product_definition: None,
            id: crate::ids::neutral_feature_id(scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::BaseFeature {
                    bodies: BodySelection::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let before = bodies.len();
        fill_history(
            ctx,
            bytes,
            history,
            &[Want {
                origin: &feature,
                producer: Some(&feature),
                consumer: None,
                identity: None,
                state: current,
                slot,
                capture_current: true,
                consumed: false,
            }],
            &mut bodies,
            &work,
            losses,
        )?;
        if bodies.len() > before {
            let snapshot = DesignDerivedBaseSnapshot {
                history_body: bodies[before].id.clone(),
                body_slot: slot,
            };
            if let Some(source) = &mut scope.derived_base_source {
                source.snapshot = Some(snapshot);
            }
        }
    }
    Ok(bodies)
}

#[derive(Debug, Default)]
pub(crate) struct ConsumerContext {
    pub(crate) bindings: HashMap<String, String>,
    pub(crate) remaining_work: usize,
    pub(crate) tails: super::body_history_tail::BodyHistoryTails,
}

pub(crate) fn fill(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    inputs: Inputs<'_>,
    mut bodies_out: Vec<HistoryBody>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(Vec<HistoryBody>, usize), CodecError> {
    let Inputs {
        features,
        scopes,
        histories,
        bindings,
        ..
    } = inputs;
    let mut wants = BTreeMap::<usize, Vec<Want<'_>>>::new();
    for feature in features {
        let Some(bodies) = super::stored_bodies::base_feature_result_bodies(feature, scopes) else {
            continue;
        };
        if let Some((history_index, feature_wants)) =
            result_state_wants(feature, bodies.iter(), inputs, false)
                .filter(|(_, feature_wants)| !feature_wants.is_empty())
        {
            wants
                .entry(history_index)
                .or_default()
                .extend(feature_wants);
        }
    }
    for feature in features {
        let Some((history_index, state, slots)) =
            unbound_base_slots(feature, scopes, histories, bindings)
        else {
            continue;
        };
        let wanted = wants.entry(history_index).or_default();
        for slot in slots {
            wanted.push(Want {
                origin: feature,
                producer: Some(feature),
                consumer: None,
                state,
                slot,
                identity: None,
                capture_current: false,
                consumed: false,
            });
        }
    }
    for feature in features {
        let Some((history_index, state, slots)) =
            split_targets(feature, scopes, histories, bindings)
        else {
            continue;
        };
        let wanted = wants.entry(history_index).or_default();
        for slot in slots {
            match wanted
                .iter_mut()
                .find(|want| want.state == state && want.slot == slot && want.consumer.is_none())
            {
                Some(want) => want.consumer = Some(feature),
                None => wanted.push(Want {
                    origin: feature,
                    producer: None,
                    consumer: Some(feature),
                    state,
                    slot,
                    identity: None,
                    capture_current: false,
                    consumed: false,
                }),
            }
        }
    }
    for feature in features {
        let Some((history_index, state, identity)) = split_identity_target(feature, inputs) else {
            continue;
        };
        wants.entry(history_index).or_default().push(Want {
            origin: feature,
            producer: None,
            consumer: Some(feature),
            state,
            slot: -1,
            identity: Some(identity),
            capture_current: false,
            consumed: false,
        });
    }
    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    fill_requests(ctx, scan, histories, &wants, &mut bodies_out, &work, losses)?;
    Ok((bodies_out, work.remaining()))
}

/// Wants for the output bodies of `feature` at its own result state.
fn result_state_wants<'a>(
    feature: &'a Feature,
    bodies: impl Iterator<Item = &'a cadmpeg_ir::ids::BodyId> + Clone,
    inputs: Inputs<'a>,
    capture_current: bool,
) -> Option<(usize, Vec<Want<'a>>)> {
    let Inputs {
        scopes,
        histories,
        bindings,
        primary_brep,
        ..
    } = inputs;
    let current = feature
        .native_ref
        .as_deref()
        .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        .and_then(DesignParameterScope::history_state_id)?;
    let bound = feature
        .native_ref
        .as_deref()
        .and_then(|id| bindings.get(id))
        .map(String::as_str);
    let output_bound = output_history(
        bodies.clone().map(cadmpeg_ir::ids::BodyId::as_str),
        histories,
        primary_brep,
    );
    let bound = bound.or(output_bound)?;
    let mut matches = histories
        .iter()
        .enumerate()
        .filter(|(_, history)| history.id == bound);
    let (history_index, history) = matches.next()?;
    if matches.next().is_some()
        || history
            .states()
            .iter()
            .filter(|state| state.state_id == current)
            .count()
            != 1
    {
        return None;
    }
    let wants = bodies
        .filter_map(|body| {
            Some(Want {
                origin: feature,
                producer: Some(feature),
                consumer: None,
                identity: None,
                state: current,
                slot: super::body_closures::stable_ref(body.as_str())?,
                capture_current,
                consumed: false,
            })
        })
        .collect();
    Some((history_index, wants))
}

/// Kinds of the output bodies of each projected Loft, decoded at the Loft's own
/// history state. A Loft is absent when any of its output bodies does not
/// decode at that state.
///
/// The pass runs in a context resumed from a checkpoint and never commits its
/// charges, so a refusal inside it leaves the session budget unchanged.
pub(crate) fn loft_result_kinds(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    inputs: Inputs<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> HashMap<cadmpeg_ir::features::FeatureId, Vec<cadmpeg_ir::topology::BodyKind>> {
    let mut wants = BTreeMap::<usize, Vec<Want<'_>>>::new();
    let mut expected = HashMap::new();
    for feature in inputs.features {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Loft { .. })
        ) {
            continue;
        }
        let outputs = feature.evaluation.outputs();
        if outputs.is_empty() {
            continue;
        }
        let Some((history_index, feature_wants)) =
            result_state_wants(feature, outputs.iter(), inputs, true)
        else {
            continue;
        };
        if feature_wants.len() != outputs.len() {
            continue;
        }
        expected.insert(&feature.id, outputs.len());
        wants
            .entry(history_index)
            .or_default()
            .extend(feature_wants);
    }
    if wants.is_empty() {
        return HashMap::new();
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let local = ctx.checkpoint().resume(&arena);
    local.take_trace();
    let mut bodies = Vec::new();
    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    let mut discarded = Vec::new();
    #[cfg(test)]
    if INJECTED_LOFT_PASS_REFUSAL.with(std::cell::Cell::take) {
        let limit = local.policy().limits.max_entities;
        local
            .charge_entities(limit.saturating_add(1), "injected Loft pass charge")
            .expect_err("the injected charge exceeds the entity limit");
    }
    let result = fill_requests(
        &local,
        scan,
        inputs.histories,
        &wants,
        &mut bodies,
        &work,
        &mut discarded,
    );
    let bounded = F3dLossCode::FeatureHistoryBodyBounded.kind();
    losses.extend(discarded.into_iter().filter(|loss| loss.code == bounded));
    if let Some(limit) = local.resource_refusal() {
        let mut refused = expected.keys().collect::<Vec<_>>();
        refused.sort();
        losses.extend(refused.into_iter().map(|feature| {
            F3dLossCode::FeatureHistoryBodyBounded.note(format!(
                "{feature} output bodies at its own history state exceed the Loft pass budget: {}",
                CodecError::ResourceLimit(limit)
            ))
        }));
        return HashMap::new();
    }
    if result.is_err() {
        return HashMap::new();
    }
    let mut kinds = HashMap::<_, Vec<_>>::new();
    for body in &bodies {
        if let Some(feature) = &body.produced_by {
            kinds
                .entry(feature.clone())
                .or_default()
                .push(body.topology.body.kind);
        }
    }
    kinds.retain(|feature, kinds| expected.get(feature) == Some(&kinds.len()));
    kinds
}

#[cfg(test)]
thread_local! {
    static INJECTED_LOFT_PASS_REFUSAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn inject_loft_pass_refusal() {
    INJECTED_LOFT_PASS_REFUSAL.with(|slot| slot.set(true));
}

#[cfg(test)]
thread_local! {
    static INJECTED_CONSUMED_FAILURE: std::cell::Cell<Option<fn() -> CodecError>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn inject_consumed_failure(failure: fn() -> CodecError) {
    INJECTED_CONSUMED_FAILURE.with(|slot| slot.set(Some(failure)));
}

pub(crate) fn fill_consumed(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    model: &mut Model,
    native: &F3dNative,
    primary_brep: Option<&str>,
    context: &ConsumerContext,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let inputs = Inputs {
        features: &model.features,
        scopes: &native.design_parameter_scopes,
        histories: &native.asm_histories,
        bindings: &context.bindings,
        primary_brep,
        groups: &native.design_construction_operand_groups,
        operands: &native.design_body_recipe_operands,
        recipes: &native.construction_recipes,
    };
    let mut wants = BTreeMap::<usize, Vec<Want<'_>>>::new();
    for feature in &model.features {
        if let Some((history, want)) = initial_cut_body(feature, inputs, losses) {
            wants.entry(history).or_default().push(want);
        }
    }
    #[cfg(test)]
    if let Some(failure) = INJECTED_CONSUMED_FAILURE.with(std::cell::Cell::take) {
        return contain_consumed_failure(ctx, Err(failure()), losses);
    }
    let work = WorkBudget::new(context.remaining_work);
    let result = fill_requests(
        ctx,
        scan,
        inputs.histories,
        &wants,
        &mut model.history_bodies,
        &work,
        losses,
    );
    contain_consumed_failure(ctx, result, losses)?;
    super::body_history_tail::append(model, &context.tails, losses)
}

fn contain_consumed_failure(
    ctx: &DecodeContext<'_>,
    result: Result<(), CodecError>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    match result {
        Err(error) if matches!(error, CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => match ctx.resource_refusal() {
            Some(limit) => Err(CodecError::ResourceLimit(limit)),
            None => {
                losses.push(
                    F3dLossCode::FeatureHistoryBodyUndecoded
                        .note(format!("Consumed history body decoding failed: {error}")),
                );
                Ok(())
            }
        },
        Ok(()) => Ok(()),
    }
}

fn fill_requests(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    histories: &[AsmHistory],
    requests: &BTreeMap<usize, Vec<Want<'_>>>,
    bodies_out: &mut Vec<HistoryBody>,
    work: &WorkBudget,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    for (&index, wants) in requests {
        let history = &histories[index];
        let Some(facts) = container::history_breps(scan).find(|facts| {
            history
                .id
                .starts_with(&format!("f3d:{}:asm-history#", facts.name))
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&facts.name)?;
        fill_history(ctx, bytes, history, wants, bodies_out, work, losses)?;
    }
    bodies_out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(())
}

fn initial_cut_body<'a>(
    feature: &'a Feature,
    inputs: Inputs<'a>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<(usize, Want<'a>)> {
    if feature.suppressed == Some(true)
        || !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Cut,
                solid: Some(true),
                ..
            })
        )
    {
        return None;
    }
    let product = feature.product_definition.as_ref()?;
    let selected = super::face_selections::face_selection_state(
        feature,
        inputs.scopes,
        inputs.bindings,
        inputs.histories,
    )?;
    let history = &inputs.histories[selected.history];
    let previous = &history.states()[selected.previous];
    let [slot] = history.topology(previous)?.bodies.as_slice() else {
        return None;
    };
    if inputs.features.iter().any(|earlier| {
        earlier.product_definition.as_ref() == Some(product)
            && earlier.ordinal < feature.ordinal
            && earlier.suppressed != Some(true)
            && !earlier.evaluation.outputs().is_empty()
    }) {
        losses.push(
            F3dLossCode::FeatureHistoryBodyConsumedWithheld.note(format!(
                "{} previous state {} has an earlier unsuppressed feature with bound outputs",
                feature.id, previous.state_id
            )),
        );
        return None;
    }
    let [output] = feature.evaluation.outputs().as_slice() else {
        losses.push(
            F3dLossCode::FeatureHistoryBodyConsumedWithheld.note(format!(
                "{} previous state {} has {} output bodies",
                feature.id,
                previous.state_id,
                feature.evaluation.outputs().len()
            )),
        );
        return None;
    };
    if super::body_closures::stable_ref(output.as_str()) != Some(*slot)
        || output_history(
            std::iter::once(output.as_str()),
            inputs.histories,
            inputs.primary_brep,
        ) != Some(history.id.as_str())
    {
        return None;
    }
    Some((
        selected.history,
        Want {
            origin: feature,
            producer: None,
            consumer: Some(feature),
            identity: None,
            state: previous.state_id,
            slot: *slot,
            consumed: true,
            capture_current: false,
        },
    ))
}

/// The one history that holds both states of a scope exactly once: the
/// history the scope binds, else the one that joins the two states.
fn scope_history(
    native: &str,
    (current, previous): (i64, i64),
    histories: &[AsmHistory],
    bindings: &HashMap<String, String>,
) -> Option<usize> {
    let holds = |history: &AsmHistory| {
        [current, previous].into_iter().all(|id| {
            history
                .states()
                .iter()
                .filter(|state| state.state_id == id)
                .count()
                == 1
        })
    };
    match bindings.get(native) {
        Some(bound) => {
            let mut matches = histories
                .iter()
                .enumerate()
                .filter(|(_, history)| history.id == *bound);
            let (index, history) = matches.next()?;
            (matches.next().is_none() && holds(history)).then_some(index)
        }
        None => {
            let (history, _, _) =
                super::history_states::unique_history_state_pair(histories, current, previous)?;
            let index = histories
                .iter()
                .position(|candidate| std::ptr::eq(candidate, history))?;
            holds(history).then_some(index)
        }
    }
}

/// The history, state and body slots that a Base Feature without a resolved
/// result stored: the bodies its state inserts, one for each stored body.
fn unbound_base_slots(
    feature: &Feature,
    scopes: &[DesignParameterScope],
    histories: &[AsmHistory],
    bindings: &HashMap<String, String>,
) -> Option<(usize, i64, Vec<i64>)> {
    if !matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Native(_) | BodySelection::Unresolved,
        })
    ) {
        return None;
    }
    let native = feature.native_ref.as_deref()?;
    let scope = scopes.iter().find(|scope| scope.id == native)?;
    let (current, previous) = (
        scope.history_state_id()?,
        scope.previous_history_state_id()?,
    );
    let stored = scope
        .base_feature_construction()?
        .body_entity_suffixes()
        .len();
    let index = scope_history(native, (current, previous), histories, bindings)?;
    let state = histories[index]
        .states()
        .iter()
        .find(|state| state.state_id == current)?;
    let transition = state.transition.as_ref()?;
    if transition.previous_state_id != Some(previous) {
        return None;
    }
    let slots = transition.topology.bodies.inserted.clone();
    (stored != 0 && slots.len() == stored).then_some((index, current, slots))
}

/// The history, input state and body slots a Split selects, when the scope
/// binds one history that holds the input state and the feature's own state
/// exactly once.
fn split_targets(
    feature: &Feature,
    scopes: &[DesignParameterScope],
    histories: &[AsmHistory],
    bindings: &HashMap<String, String>,
) -> Option<(usize, i64, Vec<i64>)> {
    let FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) =
        feature.evaluation.definition()
    else {
        return None;
    };
    let ids: Vec<&str> = match targets {
        BodySelection::Historical { bodies, .. } => bodies
            .as_slice()
            .iter()
            .map(cadmpeg_ir::ids::HistoricalBodyId::as_str)
            .collect(),
        BodySelection::HistoricalSet { members, .. } => members
            .bodies()
            .map(cadmpeg_ir::ids::HistoricalBodyId::as_str)
            .collect(),
        _ => return None,
    };
    let input = super::selection::feature_selected_input_state_id(feature)?;
    let native = feature.native_ref.as_deref()?;
    let current = scopes
        .iter()
        .find(|scope| scope.id == native)?
        .history_state_id()?;
    let index = scope_history(native, (current, input), histories, bindings)?;
    let mut slots = Vec::new();
    for id in ids {
        slots.push(id.rsplit(':').next()?.parse::<i64>().ok()?);
    }
    Some((index, input, slots))
}

/// The history, input state and body identity of a Split whose target
/// selection stays native: one body-recipe operand names the target by the
/// design identifier and selector of the body's persistent design link, and
/// the Split transition directly follows the input state.
fn split_identity_target(
    feature: &Feature,
    inputs: Inputs<'_>,
) -> Option<(usize, i64, BodyIdentity)> {
    let FeatureDefinition::Operation(FeatureOperation::SplitBody {
        targets: BodySelection::Native(group_id),
        ..
    }) = feature.evaluation.definition()
    else {
        return None;
    };
    let native = feature.native_ref.as_deref()?;
    let scope = inputs.scopes.iter().find(|scope| scope.id == native)?;
    let current = scope.history_state_id()?;
    let input =
        super::history_states::effective_scope_previous_history_state_id(scope, inputs.histories)?;
    let index = scope_history(native, (current, input), inputs.histories, inputs.bindings)?;
    let state = inputs.histories[index]
        .states()
        .iter()
        .find(|state| state.state_id == current)?;
    let transition = state.transition.as_ref()?;
    if transition.previous_state_id != Some(input) {
        return None;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut groups = inputs.groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && group.role() == DesignOperandRole::BODIES_A
            && crate::ids::native_stream(&group.id) == stream
    });
    let group = groups.next()?;
    let [member] = group.members() else {
        return None;
    };
    if groups.next().is_some() {
        return None;
    }
    let mut operands = inputs.operands.iter().filter(|operand| {
        operand.owner.group() == Some((group.record_index, 0))
            && operand.record_index() == member.value
            && crate::ids::native_stream(&operand.id) == stream
    });
    let operand = operands.next()?;
    if operands.next().is_some() {
        return None;
    }
    let mut recipes = inputs
        .recipes
        .iter()
        .filter(|recipe| recipe.id == operand.recipe_id);
    let recipe = recipes.next()?;
    if recipes.next().is_some() || recipe.kind != ConstructionRecipeKind::Body {
        return None;
    }
    let design = recipe.design.as_ref()?;
    let selector = i64::from(design.selector?.value);
    let bodies = &transition.topology.bodies;
    let mut revised = bodies.updated.clone();
    revised.extend(&bodies.deleted);
    Some((
        index,
        input,
        BodyIdentity {
            design_id: design.id.value.clone(),
            selector,
            revised,
        },
    ))
}

/// The one body slot of a decoded input state that carries the persistent
/// design link of the identity, when the Split transition revises it.
fn identity_slot(
    brep: &cadmpeg_asm::brep::AsmBrep,
    identity: &BodyIdentity,
) -> Result<Option<i64>, CodecError> {
    let mut carriers = BTreeSet::new();
    for attribute in &brep.attributes {
        let cadmpeg_ir::attributes::AttributeTarget::Body(body) = &attribute.target.entity else {
            continue;
        };
        let Some(slot) = super::body_closures::stable_ref(body.as_str()) else {
            continue;
        };
        for link in crate::brep::persistent_design_links(attribute)? {
            if link.design_id.as_str() == identity.design_id
                && link.design_reference == identity.selector
            {
                carriers.insert(slot);
            }
        }
    }
    let mut carriers = carriers.into_iter();
    let slot = carriers.next();
    Ok(slot.filter(|slot| carriers.next().is_none() && identity.revised.contains(slot)))
}

pub(super) fn output_history<'a>(
    bodies: impl Iterator<Item = &'a str>,
    histories: &'a [AsmHistory],
    primary_brep: Option<&str>,
) -> Option<&'a str> {
    let mut source = None;
    for body in bodies {
        let candidate = if body.starts_with("f3d:brep:entity#") {
            primary_brep?.rsplit('/').next()?.strip_prefix("BREP.")?
        } else {
            body.strip_prefix("f3d:brep/")?.split_once('/')?.0
        };
        if source.is_some_and(|source| source != candidate) {
            return None;
        }
        source = Some(candidate);
    }
    let source = source?;
    let mut matches = histories.iter().filter(|history| {
        super::face_resolution::historical_brep_source(&history.id) == Some(source)
    });
    let history = matches.next()?;
    matches.next().is_none().then_some(history.id.as_str())
}

fn fill_history(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    history: &AsmHistory,
    wants: &[Want<'_>],
    bodies: &mut Vec<HistoryBody>,
    work: &WorkBudget,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let Some(header) = cadmpeg_asm::asm_header::parse(bytes) else {
        return Ok(());
    };
    let states = history.states();
    let mut archive = None;
    let mut archive_attempted = false;
    let Some(active_count) = super::archived_active_record_count(states)?
        .or_else(|| super::insert_only_active_record_count(states))
        .and_then(|count| i64::try_from(count).ok())
    else {
        return Ok(());
    };
    let archived_ids = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let cached = history.cached_record_archive();
    let mut subtypes_charged = false;
    let mut subtypes = None;
    let mut joinable = Vec::<Joinable>::new();
    let mut newer = HashMap::<i64, HashSet<Option<i64>>>::new();
    let mut visited = 0;
    let mut failure = None;
    super::walk_entity_versions_hashed(
        states,
        active_count,
        &archived_ids,
        |state_index, versions| {
            let ordinal = states.len() - visited - 1;
            visited += 1;
            if failure.is_some() {
                return;
            }
            let state_id = states[state_index].state_id;
            for want in wants.iter().filter(|want| want.state == state_id) {
                let revision = versions.get(&want.slot).copied();
                let changed = newer
                    .get(&want.slot)
                    .is_some_and(|seen| seen.iter().any(|new| *new != revision));
                // A later state that deletes or revises the slot leaves the
                // body as this state holds it only in the record archive. The
                // transition of a Split that names its target by identity
                // states the revision.
                let produced = want.producer.filter(|_| changed || want.capture_current);
                let consumed = want
                    .consumer
                    .filter(|_| changed || want.identity.is_some() || want.consumed);
                if produced.is_none() && consumed.is_none() {
                    continue;
                }
                if cached.is_none() && !archive_attempted {
                    archive_attempted = true;
                    match super::decode_record_archive(ctx, states, bytes, header.width) {
                        Ok(records) => archive = records,
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                let archive_ref = cached.map(|c| &c.archive).or(archive.as_ref());
                let Some(archive) = archive_ref else {
                    if want.consumed {
                        losses.push(F3dLossCode::FeatureHistoryBodyUndecoded.note(format!(
                            "{} body slot {} state {state_id} has an incomplete record archive",
                            want.origin.id, want.slot
                        )));
                    }
                    continue;
                };
                if bodies.len() >= MAX_HISTORY_BODIES
                    || !work.charge_by(work_units(versions.len()))
                    || (!subtypes_charged && !work.charge_by(work_units(archive.len())))
                {
                    losses.push(F3dLossCode::FeatureHistoryBodyBounded.note(format!(
                        "{} body slot {} state {state_id} exceeds the history body cap",
                        want.origin.id, want.slot
                    )));
                    continue;
                }
                subtypes_charged = true;
                let subtypes = match cached {
                    Some(cached) => &cached.subtypes,
                    None => subtypes.get_or_insert_with(|| {
                        archive_subtypes(archive, header.metadata.save_format_version)
                    }),
                };
                let resolved;
                let want = match &want.identity {
                    None => want,
                    Some(identity) => {
                        match identity_body_slot(
                            ctx,
                            (archive, versions),
                            bytes,
                            (&header.metadata, subtypes),
                            identity,
                        ) {
                            Ok(Some(slot)) => {
                                resolved = Want {
                                    origin: want.origin,
                                    producer: None,
                                    consumer: want.consumer,
                                    state: want.state,
                                    slot,
                                    identity: None,
                                    capture_current: false,
                                    consumed: false,
                                };
                                &resolved
                            }
                            Ok(None) => {
                                losses.push(F3dLossCode::FeatureHistoryBodyUndecoded.note(
                                    format!(
                                        "{} state {state_id} has an unresolved body identity",
                                        want.origin.id
                                    ),
                                ));
                                continue;
                            }
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                };
                // A producing feature of the same body at an earlier state
                // joins the nest of the consuming feature when the records of
                // the two states are equal.
                let join = if produced.is_some() && consumed.is_none() {
                    joinable.iter().position(|found| found.slot == want.slot)
                } else {
                    None
                };
                match build(
                    ctx,
                    (archive, versions),
                    bytes,
                    (&header.metadata, subtypes),
                    want,
                    (produced, consumed),
                    (ordinal, bodies.len(), consumed.is_some() || join.is_some()),
                ) {
                    Ok(Some(built)) => {
                        place(
                            bodies,
                            &mut joinable,
                            built,
                            want.slot,
                            (consumed.is_some(), join),
                        );
                    }
                    Ok(None) => {
                        losses.push(F3dLossCode::FeatureHistoryBodyUndecoded.note(format!(
                            "{} body slot {} state {state_id} has an incomplete or invalid geometry closure",
                            want.origin.id, want.slot
                        )));
                    }
                    Err(error) => failure = Some(error),
                }
            }
            for want in wants.iter().filter(|want| want.identity.is_none()) {
                newer
                    .entry(want.slot)
                    .or_default()
                    .insert(versions.get(&want.slot).copied());
            }
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}

fn archive_subtypes(
    archive: &super::HistoricalRecordArchive,
    version: Option<u32>,
) -> cadmpeg_asm::nurbs::toks::SubtypeTable {
    let mut records = archive.values().collect::<Vec<_>>();
    records.sort_by_key(|record| record.offset);
    cadmpeg_asm::nurbs::toks::SubtypeTable::from_record_refs(&records)
        .with_save_format_version(version)
}

/// Materialize the sorted full record table from a hashed slot-to-revision map.
fn materialize_hashed_table(
    versions: &super::binding::FastMap<i64, i64>,
    archive: &super::HistoricalRecordArchive,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    if versions.is_empty() {
        return None;
    }
    let mut pairs: Vec<(i64, i64)> = versions.iter().map(|(&slot, &rev)| (slot, rev)).collect();
    pairs.sort_unstable_by_key(|&(slot, _)| slot);
    let mut records = Vec::with_capacity(pairs.len());
    for (slot, rev) in pairs {
        let record = archive.get(&rev)?;
        if i64::try_from(record.index).ok() != Some(slot) {
            return None;
        }
        for token in record.tokens.iter() {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !versions.contains_key(reference) {
                return None;
            }
        }
        records.push(record.clone());
    }
    Some(records)
}

/// Extract the closed subset of live records reachable from `body_slot`
/// (plus `asmheader` at slot 0 when present) so decoding one history body
/// avoids decoding every other body in the state.
fn body_record_closure(
    versions: &super::binding::FastMap<i64, i64>,
    archive: &super::HistoricalRecordArchive,
    body_slot: i64,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    use cadmpeg_asm::brep::attributes::attribute_owner;
    use cadmpeg_asm::sab::Token;

    if versions.is_empty() {
        return None;
    }
    let body_rev = versions.get(&body_slot)?;
    let body_rec = archive.get(body_rev)?;
    if body_rec.head() != "body" {
        return None;
    }

    let mut incoming_containers: super::binding::FastMap<i64, Vec<i64>> =
        super::binding::FastMap::default();
    let mut incoming_attribs: super::binding::FastMap<i64, Vec<i64>> =
        super::binding::FastMap::default();
    for (&slot, rev) in versions {
        let record = archive.get(rev)?;
        if i64::try_from(record.index).ok() != Some(slot) {
            return None;
        }
        for token in record.tokens.iter() {
            let Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !versions.contains_key(reference) {
                return None;
            }
        }
        match record.head() {
            "lump" | "region" => {
                if let Some(owner) = record.ref_at(5) {
                    incoming_containers.entry(owner).or_default().push(slot);
                }
            }
            "shell" => {
                if let Some(owner) = record.ref_at(7) {
                    incoming_containers.entry(owner).or_default().push(slot);
                }
            }
            "subshell" => {
                if let Some(owner) = record.ref_at(3) {
                    incoming_containers.entry(owner).or_default().push(slot);
                }
            }
            _ => {}
        }
        if record.name.ends_with("-attrib") {
            if let Some(owner) = attribute_owner(record) {
                incoming_attribs.entry(owner).or_default().push(slot);
            }
        }
    }

    let mut visited: super::binding::FastSet<i64> = super::binding::FastSet::default();
    let mut queue: Vec<i64> = vec![body_slot];
    if let Some(rev0) = versions.get(&0) {
        if archive
            .get(rev0)
            .is_some_and(|rec| rec.head() == "asmheader")
        {
            visited.insert(0);
        }
    }
    while let Some(slot) = queue.pop() {
        if !visited.insert(slot) {
            continue;
        }
        let Some(record) = versions.get(&slot).and_then(|rev| archive.get(rev)) else {
            continue;
        };
        if slot != body_slot && record.head() == "body" {
            visited.remove(&slot);
            continue;
        }
        for token in record.tokens.iter() {
            if let Token::Ref(target) = *token {
                if target >= 0 && !visited.contains(&target) {
                    queue.push(target);
                }
            }
        }
        if let Some(children) = incoming_containers.get(&slot) {
            for &child in children {
                if !visited.contains(&child) {
                    queue.push(child);
                }
            }
        }
        if let Some(attribs) = incoming_attribs.get(&slot) {
            for &attrib in attribs {
                if !visited.contains(&attrib) {
                    queue.push(attrib);
                }
            }
        }
    }

    let mut slots: Vec<i64> = visited.into_iter().collect();
    slots.sort_unstable();
    let mut records = Vec::with_capacity(slots.len());
    for slot in slots {
        let rev = versions.get(&slot)?;
        let record = archive.get(rev)?;
        if i64::try_from(record.index).ok() != Some(slot) {
            return None;
        }
        records.push(record.clone());
    }
    Some(records)
}

fn build(
    ctx: &DecodeContext<'_>,
    (archive, versions): (
        &super::HistoricalRecordArchive,
        &super::binding::FastMap<i64, i64>,
    ),
    bytes: &[u8],
    tables: (
        &cadmpeg_asm::kernel_header::KernelHeader,
        &cadmpeg_asm::nurbs::toks::SubtypeTable,
    ),
    want: &Want<'_>,
    owners: (Option<&Feature>, Option<&Feature>),
    position: (usize, usize, bool),
) -> Result<Option<Built>, CodecError> {
    let (ordinal, nest_index, keep_records) = position;
    let decode_table =
        |table: &[cadmpeg_asm::sab::Record]| -> Result<Option<Box<BodyTopology>>, CodecError> {
            let brep = match cadmpeg_asm::brep::decode_with_table(
                ctx,
                table,
                bytes,
                (Some(tables.0), tables.1),
                "f3d:history",
                crate::ids::ID_FORMAT,
                cadmpeg_asm::brep::DecodePurpose::Model,
            ) {
                Ok(brep) => brep,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => return Ok(None),
            };
            let Some(body) = brep
                .bodies
                .iter()
                .find(|body| super::body_closures::stable_ref(body.id.as_str()) == Some(want.slot))
                .map(|body| body.id.clone())
            else {
                return Ok(None);
            };
            let mut scratch = CadIr::empty();
            cadmpeg_asm::brep::transfer::transfer_into_ir(
                ctx,
                &mut scratch,
                cadmpeg_ir::codec_id!("f3d"),
                brep,
            )?;
            match extract(ctx, &scratch.geometry, body.as_str())? {
                Extract::Nest(topology) => Ok(Some(topology)),
                Extract::Absent | Extract::NotClosed => Ok(None),
            }
        };
    let topology = if let Some(subset) = body_record_closure(versions, archive, want.slot) {
        match decode_table(&subset)? {
            Some(topology) => topology,
            None => {
                let Some(table) = materialize_hashed_table(versions, archive) else {
                    return Ok(None);
                };
                let Some(topology) = decode_table(&table)? else {
                    return Ok(None);
                };
                topology
            }
        }
    } else {
        let Some(table) = materialize_hashed_table(versions, archive) else {
            return Ok(None);
        };
        let Some(topology) = decode_table(&table)? else {
            return Ok(None);
        };
        topology
    };
    let records = keep_records.then(|| (*topology).clone());
    let mut map = IdentityMap::new(|id: &str| {
        let (_, rest) = id
            .split_once(':')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        let (_, rest) = rest
            .split_once(':')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        let (kind, key) = rest
            .split_once('#')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        Ok(format!("f3d:history-body:{kind}#h{nest_index:04}-{key}"))
    });
    let mut topology = topology.rewrite_identities(ctx, &mut map)?;
    map.finish()?;
    sort_by_identity(&mut topology);
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(topology.record_count()),
        "admit history body records",
    )?;
    let body = admit(
        ctx,
        HistoryBody {
            id: HistoryBodyId::mint(format!("f3d:history-body:nest#h{nest_index:04}"))
                .map_err(|error| CodecError::malformed(format_args!("{error}")))?,
            produced_by: owners.0.map(|feature| feature.id.clone()),
            consumed_by: owners.1.map(|feature| feature.id.clone()),
            state: HistoryStateRef {
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| CodecError::malformed("history ordinal"))?,
                identifier: want.state,
            },
            topology,
        },
        want.origin,
    )?;
    Ok(body.map(|body| Built { body, records }))
}

/// Add a built nest, or join its producing feature onto the equal consumed
/// nest that `join` names.
fn place(
    bodies: &mut Vec<HistoryBody>,
    joinable: &mut Vec<Joinable>,
    built: Built,
    slot: i64,
    (consumed, join): (bool, Option<usize>),
) {
    let equal =
        join.filter(|&position| built.records.as_ref() == Some(&joinable[position].topology));
    if let Some(position) = equal {
        let found = joinable.remove(position);
        let nest = &mut bodies[found.index];
        nest.produced_by = built.body.produced_by;
        nest.state = built.body.state;
        return;
    }
    if consumed {
        if let Some(topology) = built.records {
            joinable.push(Joinable {
                slot,
                index: bodies.len(),
                topology,
            });
        }
    }
    bodies.push(built.body);
}

/// A history body, and its records before the nest identities, when kept.
struct Built {
    body: HistoryBody,
    records: Option<BodyTopology>,
}

/// The body slot of the state that carries the identity. Decoding the state
/// reads the persistent design links on its bodies.
fn identity_body_slot(
    ctx: &DecodeContext<'_>,
    (archive, versions): (
        &super::HistoricalRecordArchive,
        &super::binding::FastMap<i64, i64>,
    ),
    bytes: &[u8],
    tables: (
        &cadmpeg_asm::kernel_header::KernelHeader,
        &cadmpeg_asm::nurbs::toks::SubtypeTable,
    ),
    identity: &BodyIdentity,
) -> Result<Option<i64>, CodecError> {
    let Some(table) = materialize_hashed_table(versions, archive) else {
        return Ok(None);
    };
    match cadmpeg_asm::brep::decode_with_table(
        ctx,
        &table,
        bytes,
        (Some(tables.0), tables.1),
        "f3d:history",
        crate::ids::ID_FORMAT,
        cadmpeg_asm::brep::DecodePurpose::Model,
    ) {
        Ok(brep) => identity_slot(&brep, identity),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

fn admit(
    ctx: &DecodeContext<'_>,
    body: HistoryBody,
    origin: &Feature,
) -> Result<Option<HistoryBody>, CodecError> {
    let mut scratch = CadIr::empty();
    // Admission checks the nest and the link to the origin. The stub carries
    // only the identity, so no operand of the real feature is validated here.
    scratch.model.features.push(Feature {
        product_definition: None,
        id: origin.id.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        )),
        native_ref: None,
    });
    scratch.model.history_bodies.push(body);
    // A nest is unusable where its topology or its link to the origin
    // breaks; a finding of another check family does not drop it.
    let valid =
        cadmpeg_ir::admit(ctx, &scratch, cadmpeg_ir::DRAFT_CORE_CHECKS, Vec::new())?.is_ok();
    let Some(body) = scratch.model.history_bodies.pop() else {
        return Err(CodecError::malformed("history body admission"));
    };
    Ok(valid.then_some(body))
}

enum Extract {
    Nest(Box<BodyTopology>),
    Absent,
    NotClosed,
}
fn sort_by_identity(topology: &mut BodyTopology) {
    fn sort<T: EntitySchema>(records: &mut [T]) {
        records.sort_by(|left, right| left.identity().cmp(right.identity()));
    }
    sort(&mut topology.regions);
    sort(&mut topology.shells);
    sort(&mut topology.faces);
    sort(&mut topology.loops);
    sort(&mut topology.coedges);
    sort(&mut topology.edges);
    sort(&mut topology.vertices);
    sort(&mut topology.surfaces);
    sort(&mut topology.curves);
    sort(&mut topology.pcurves);
    sort(&mut topology.points);
}

#[derive(Clone, Copy)]
enum Record<'a> {
    Body(&'a cadmpeg_ir::topology::Body),
    Region(&'a cadmpeg_ir::topology::Region),
    Shell(&'a cadmpeg_ir::topology::Shell),
    Face(&'a cadmpeg_ir::topology::Face),
    Loop(&'a cadmpeg_ir::topology::Loop),
    Coedge(&'a cadmpeg_ir::topology::Coedge),
    Edge(&'a cadmpeg_ir::topology::Edge),
    Vertex(&'a cadmpeg_ir::topology::Vertex),
    Surface,
    Curve,
    Pcurve(&'a cadmpeg_ir::geometry::pcurve::Pcurve),
    Point(&'a cadmpeg_ir::topology::Point),
}

impl Record<'_> {
    fn references(
        self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            // A carrier names a procedural construction or a source object at
            // most; the nest keeps its solved geometry only.
            Self::Surface | Self::Curve => Ok(()),
            Self::Body(record) => record.visit_references(ctx, visitor),
            Self::Region(record) => record.visit_references(ctx, visitor),
            Self::Shell(record) => record.visit_references(ctx, visitor),
            Self::Face(record) => record.visit_references(ctx, visitor),
            Self::Loop(record) => record.visit_references(ctx, visitor),
            Self::Coedge(record) => record.visit_references(ctx, visitor),
            Self::Edge(record) => record.visit_references(ctx, visitor),
            Self::Vertex(record) => record.visit_references(ctx, visitor),
            Self::Pcurve(record) => record.visit_references(ctx, visitor),
            Self::Point(record) => record.visit_references(ctx, visitor),
        }
    }
}

fn reach<'a>(
    ctx: &DecodeContext<'_>,
    geometry: &'a Topology,
    body: &str,
) -> Result<(HashSet<&'a str>, usize), CodecError> {
    let mut table = HashMap::<&str, Record<'_>>::new();
    macro_rules! index {
        ($arena:ident, $variant:ident) => {
            for record in &geometry.$arena {
                table.insert(record.identity(), Record::$variant(record));
            }
        };
    }
    index!(bodies, Body);
    index!(regions, Region);
    index!(shells, Shell);
    index!(faces, Face);
    index!(loops, Loop);
    index!(coedges, Coedge);
    index!(edges, Edge);
    index!(vertices, Vertex);
    for record in &geometry.surfaces {
        table.insert(record.identity(), Record::Surface);
    }
    for record in &geometry.curves {
        table.insert(record.identity(), Record::Curve);
    }
    index!(pcurves, Pcurve);
    index!(points, Point);
    let Some((&start, _)) = table.get_key_value(body) else {
        return Ok((HashSet::new(), 0));
    };
    let mut children = HashMap::<&str, Vec<&str>>::new();
    for region in &geometry.regions {
        children
            .entry(region.body.as_str())
            .or_default()
            .push(region.id.as_str());
    }
    for shell in &geometry.shells {
        children
            .entry(shell.region.as_str())
            .or_default()
            .push(shell.id.as_str());
    }
    for face in &geometry.faces {
        children
            .entry(face.shell.as_str())
            .or_default()
            .push(face.id.as_str());
    }
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut missing = 0;
    while let Some(id) = queue.pop_front() {
        for &child in children.get(id).into_iter().flatten() {
            if seen.insert(child) {
                queue.push_back(child);
            }
        }
        let Some(record) = table.get(id).copied() else {
            continue;
        };
        let mut targets = Vec::new();
        record.references(ctx, &mut |target| {
            targets.push(target.to_owned());
            Ok(())
        })?;
        for target in targets {
            match table.get_key_value(target.as_str()) {
                Some((&key, _)) => {
                    if seen.insert(key) {
                        queue.push_back(key);
                    }
                }
                None => missing += 1,
            }
        }
    }
    Ok((seen, missing))
}

fn retain<T: EntitySchema + Clone>(records: &[T], reached: &HashSet<&str>) -> Vec<T> {
    records
        .iter()
        .filter(|record| reached.contains(record.identity()))
        .cloned()
        .collect()
}

fn solve(topology: &mut BodyTopology) -> bool {
    for surface in &mut topology.surfaces {
        surface.source_object = None;
        if let SurfaceGeometry::Procedural { cache, .. } = &mut surface.geometry {
            let Some(cache) = cache.take() else {
                return false;
            };
            surface.geometry = SurfaceGeometry::Solved(cache);
        }
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ) {
            return false;
        }
    }
    for curve in &mut topology.curves {
        curve.source_object = None;
        if let CurveGeometry::Procedural { cache, .. } = &mut curve.geometry {
            let Some(cache) = cache.take() else {
                return false;
            };
            curve.geometry = CurveGeometry::Solved(cache);
        }
        if matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        ) {
            return false;
        }
    }
    true
}

fn escapes(ctx: &DecodeContext<'_>, topology: &BodyTopology) -> Result<bool, CodecError> {
    let geometry = topology.to_topology();
    let mut inside = HashSet::<&str>::new();
    macro_rules! collect {
        ($($arena:ident),*) => {$(
            for record in &geometry.$arena {
                inside.insert(record.identity());
            }
        )*};
    }
    collect!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, surfaces, curves, pcurves,
        points
    );
    let mut escaped = false;
    macro_rules! visit {
        ($($arena:ident),*) => {$(
            for record in &geometry.$arena {
                record.visit_references(ctx, &mut |target| {
                    escaped |= !inside.contains(target);
                    Ok(())
                })?;
            }
        )*};
    }
    visit!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, surfaces, curves, pcurves,
        points
    );
    Ok(escaped)
}

fn extract(
    ctx: &DecodeContext<'_>,
    geometry: &Topology,
    body: &str,
) -> Result<Extract, CodecError> {
    let (reached, missing) = reach(ctx, geometry, body)?;
    let Some(root) = geometry
        .bodies
        .iter()
        .find(|candidate| candidate.id.as_str() == body)
    else {
        return Ok(Extract::Absent);
    };
    let mut topology = BodyTopology::new(root.clone());
    topology.regions = retain(&geometry.regions, &reached);
    topology.shells = retain(&geometry.shells, &reached);
    topology.faces = retain(&geometry.faces, &reached);
    topology.loops = retain(&geometry.loops, &reached);
    topology.coedges = retain(&geometry.coedges, &reached);
    topology.edges = retain(&geometry.edges, &reached);
    topology.vertices = retain(&geometry.vertices, &reached);
    topology.surfaces = retain(&geometry.surfaces, &reached);
    topology.curves = retain(&geometry.curves, &reached);
    topology.pcurves = retain(&geometry.pcurves, &reached);
    topology.points = retain(&geometry.points, &reached);
    if !solve(&mut topology) {
        return Ok(Extract::Absent);
    }
    if missing != 0 || escapes(ctx, &topology)? {
        return Ok(Extract::NotClosed);
    }
    Ok(Extract::Nest(Box::new(topology)))
}

#[cfg(test)]
mod tests;
