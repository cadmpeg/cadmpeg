// SPDX-License-Identifier: Apache-2.0
//! Feature body selections, combine tool slots, and body recipe selections.

use super::body_candidates::BodyCandidates;
use super::body_closures::stable_ref;
use super::face_resolution::body_member;
use super::history_states::{effective_scope_previous_history_state_id, unique_history_state_pair};
use super::pattern_bodies::bind_pattern_body_selections;
use crate::history_records::{AsmDeltaState, AsmHistory};
use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub(crate) construction_recipes: &'a [crate::records::recipes::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub(crate) persistent_design_links: &'a [crate::records::sketch_links::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub(crate) histories: &'a [AsmHistory],
    /// The ASM history id that each bound Design scope reads.
    pub(crate) scope_histories: &'a HashMap<String, String>,
    /// Neutral top-level bodies.
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub(crate) shells: &'a [cadmpeg_ir::topology::Shell],
    /// Neutral shell faces.
    pub(crate) faces: &'a [cadmpeg_ir::topology::Face],
}

/// Bind the body selections of `features`, and return the browser-node
/// refusals of `Combine` operands that stay native, for a later binding pass
/// to settle.
pub(crate) fn bind_feature_body_selections_with_refusals(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> (
    Vec<cadmpeg_ir::report::loss::LossNote>,
    Vec<CombineOperandRefusal>,
) {
    use cadmpeg_ir::features::boundary_fill::BoundaryFillCell;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut losses = Vec::new();
    let mut operand_refusals = Vec::new();
    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;
    let histories = inputs.histories;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    let mut body_candidates = BodyCandidates::new(
        bodies,
        regions,
        shells,
        inputs.faces,
        inputs.construction_recipes,
        inputs.persistent_design_links,
    );
    bind_pattern_body_selections(features, inputs, &mut body_candidates);
    let mut pattern_body_slots = HashMap::new();
    'pattern: for feature in features.iter() {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::patterns::PatternTransform::Circular { count, .. } =
            pattern.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Bodies(BodySelection::Historical {
            bodies: seed_bodies,
            ..
        })] = seeds.as_slice()
        else {
            continue;
        };
        let [seed_body] = seed_bodies.as_slice() else {
            continue;
        };
        let Ok(expected_count) = usize::try_from(*count) else {
            continue;
        };
        if feature.evaluation.outputs().len().checked_add(1) != Some(expected_count) {
            continue;
        }
        let mut slots = BTreeSet::new();
        for slot in std::iter::once(historical_body_slot(seed_body.as_str())).chain(
            feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| stable_ref(body.as_str())),
        ) {
            let Some(slot) = slot else {
                continue 'pattern;
            };
            slots.insert(slot);
        }
        if slots.len() != expected_count {
            continue;
        }
        let feature_id = feature.id.clone();
        pattern_body_slots.insert(feature_id, slots);
    }

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        feature.evaluation.edit(|definition, _| {
            'feature_edit: {
                let Some(native_ref) = native_ref else {
                    break 'feature_edit;
                };
                let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
                let Some(scope) = matching_scopes.next() else {
                    break 'feature_edit;
                };
                if matching_scopes.next().is_some() {
                    break 'feature_edit;
                }
                if matches!(
                    definition,
                    FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
                ) {
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::SilhouetteSplit {
                    target,
                    ..
                }) = definition
                {
                    if let Some(previous_state_id) =
                        effective_scope_previous_history_state_id(scope, histories)
                    {
                        bind_body_recipe_body_selection(
                            target,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
                    tools,
                    cells,
                    ..
                }) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        bind_body_recipe_body_selection(
                            tools,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                        for cell in cells {
                            let BoundaryFillCell::BodyRegion(cell) = cell else {
                                continue;
                            };
                            bind_body_recipe_body_selection(
                                cell,
                                feature_id,
                                previous_state_id,
                                scope,
                                groups,
                                body_recipe_operands,
                            );
                        }
                    } else {
                        bind_direct_body_recipe_body_selection(
                            tools,
                            scope,
                            inputs,
                            &mut body_candidates,
                        );
                        for cell in cells {
                            let BoundaryFillCell::BodyRegion(cell) = cell else {
                                continue;
                            };
                            bind_direct_body_recipe_body_selection(
                                cell,
                                scope,
                                inputs,
                                &mut body_candidates,
                            );
                        }
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands, ..
                }) = definition
                {
                    let pattern_bodies = {
                        let mut sets = dependencies
                            .iter()
                            .filter_map(|dependency| pattern_body_slots.get(dependency));
                        match (sets.next(), sets.next()) {
                            (Some(bodies), None) => Some(bodies),
                            _ => None,
                        }
                    };
                    let refusal =
                        super::combine_selection::refine(operands, feature_id, |target, tools| {
                            super::combine_binding::bind_combine_operands(
                                (target, tools),
                                scope,
                                feature_id,
                                pattern_bodies,
                                inputs,
                                &mut body_candidates,
                                (&mut operand_refusals, &mut losses),
                            );
                        });
                    if let Some(loss) = refusal {
                        losses.push(loss);
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(
                    FeatureOperation::Coil {
                        result: cadmpeg_ir::features::operation_settings::SolidFeatureResult::Boolean { targets, .. },
                        ..
                    }
                    | FeatureOperation::Thicken {
                        result:
                            Some(cadmpeg_ir::features::operation_settings::SolidFeatureResult::Boolean { targets, .. }),
                        ..
                    },
                ) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        bind_body_recipe_body_selection(
                            targets,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    } else {
                        bind_direct_body_recipe_body_selection(
                            targets,
                            scope,
                            inputs,
                            &mut body_candidates,
                        );
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                    bodies, ..
                }) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        bind_body_recipe_body_selection(
                            bodies,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    } else {
                        let native = bodies.clone();
                        bind_direct_body_recipe_body_selection(
                            bodies,
                            scope,
                            inputs,
                            &mut body_candidates,
                        );
                        // The recipe identity of the operands outranks a face match.
                        super::generated_bodies::keep_recipe_body_slots(bodies, native, scope, inputs);
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::ReverseShape {
                    source: bodies,
                }) = definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        bind_body_recipe_body_selection(
                            bodies,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    } else {
                        bind_direct_body_recipe_body_selection(
                            bodies,
                            scope,
                            inputs,
                            &mut body_candidates,
                        );
                    }
                    break 'feature_edit;
                }
                if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) =
                    definition
                {
                    if let Some(previous_state_id) = scope.previous_history_state_id() {
                        bind_body_recipe_body_selection(
                            bodies,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                        if matches!(bodies, BodySelection::Native(_)) {
                            bind_direct_body_recipe_body_selection(
                                bodies,
                                scope,
                                inputs,
                                &mut body_candidates,
                            );
                        }
                    } else {
                        bind_direct_body_recipe_body_selection(
                            bodies,
                            scope,
                            inputs,
                            &mut body_candidates,
                        );
                    }
                    break 'feature_edit;
                }
                let (bodies, proof) = match definition {
                    FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                        (bodies, BodySelectionProof::TopologyStableRevision)
                    }
                    FeatureDefinition::Operation(FeatureOperation::Shell {
                        bodies: Some(bodies),
                        ..
                    }) => (bodies, BodySelectionProof::RevisedInput),
                    FeatureDefinition::Operation(FeatureOperation::SplitBody {
                        targets, ..
                    }) => (targets, BodySelectionProof::RevisedInput),
                    _ => break 'feature_edit,
                };
                let BodySelection::Native(group_id) = bodies else {
                    break 'feature_edit;
                };
                let mut matching_groups = groups.iter().filter(|group| {
                    group.id == *group_id
                        && group.scope_record_index == scope.record_index
                        && group.role() == DesignOperandRole::BODIES_A
                        && crate::ids::native_stream(&group.id)
                            == crate::ids::native_stream(&scope.id)
                });
                let Some(group) = matching_groups.next() else {
                    break 'feature_edit;
                };
                if matching_groups.next().is_some() || group.members().len() != 1 {
                    break 'feature_edit;
                }
                let (Some(state_id), Some(previous_state_id)) =
                    (scope.history_state_id(), scope.previous_history_state_id())
                else {
                    bind_direct_body_recipe_body_selection(
                        bodies,
                        scope,
                        inputs,
                        &mut body_candidates,
                    );
                    break 'feature_edit;
                };
                let Some((history, state, _previous)) =
                    unique_history_state_pair(histories, state_id, previous_state_id)
                else {
                    bind_direct_body_recipe_body_selection(
                        bodies,
                        scope,
                        inputs,
                        &mut body_candidates,
                    );
                    break 'feature_edit;
                };
                let history_states = unique_feature_history_states(history);
                let body = match proof {
                    BodySelectionProof::TopologyStableRevision => {
                        singleton_body_revision_across_state_chain(
                            history,
                            state,
                            previous_state_id,
                            &history_states,
                        )
                    }
                    BodySelectionProof::RevisedInput => {
                        singleton_revised_input_body_across_state_chain(
                            history,
                            state,
                            previous_state_id,
                            &history_states,
                        )
                    }
                };
                let Some(body) = body else {
                    break 'feature_edit;
                };
                let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
                let state_id = crate::ids::history_input_state_id(&prefix);
                let body_id = crate::ids::history_input_body_id(&prefix, body);
                let native = group_id.to_owned();
                let selected = vec![body_id];
                if let Ok(historical) =
                    cadmpeg_ir::features::selections::BodySelection::historical(state_id, selected, native)
                {
                    *bodies = historical;
                }
            }
        });
    }

    (losses, operand_refusals)
}

pub(super) fn pattern_combine_tool_slots(
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Option<Vec<i64>> {
    if !pattern_bodies.contains(&target_body)
        || pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
    {
        return None;
    }
    Some(
        pattern_bodies
            .iter()
            .copied()
            .filter(|body| *body != target_body)
            .collect::<Vec<_>>(),
    )
}

pub(super) fn combine_recipe_family_tool_slots(
    scope_key: (&str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> Option<Vec<i64>> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    type FamilyKey<'a> = (
        &'a crate::records::mesh::DesignRelaxedGuidText,
        &'a crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u32,
        &'a str,
    );
    type FamilyMember<'a> = (u32, Option<i64>, &'a [i64]);
    let (stream, scope_record_index) = scope_key;

    if tool_record_indices.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    for index in tool_record_indices {
        if !seen.insert(*index) {
            return None;
        }
    }
    let mut recipes_by_id =
        HashMap::<&str, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes.iter().filter(|recipe| {
        recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    }) {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(recipe));
    }
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for record_index in tool_record_indices {
        let mut matching = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope_record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == *record_index
        });
        let operand = required!(matching.next());
        if matching.next().is_some() || operand.references().len() != 1 {
            return None;
        }
        let recipe = required!(required!(recipes_by_id.get(operand.recipe_id.as_str())).as_ref());
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return None;
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return None,
        };
        let reference = &operand.references()[0];
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        families.entry(key).or_default().push((
            selector,
            resolved,
            reference.preceding_body_slots.as_slice(),
        ));
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let mut exact = BTreeSet::new();
        for body in family.iter().filter_map(|(_, body, _)| *body) {
            exact.insert(body);
        }
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return None;
            }
            for body in exact {
                selected.insert(body);
            }
            continue;
        }
        let mut selectors = BTreeSet::new();
        for (selector, _, _) in &family {
            selectors.insert(*selector);
        }
        let count = required!(u32::try_from(family.len()).ok());
        if selectors.len() != family.len()
            || !(1..=count).all(|selector| selectors.contains(&selector))
        {
            return None;
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidate_slice = required!(candidate_sets.next());
        let mut candidates = BTreeSet::new();
        for candidate in *candidate_slice {
            candidates.insert(*candidate);
        }
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| {
                other.iter().any(|body| !candidates.contains(body))
                    || candidates.iter().any(|body| !other.contains(body))
            })
        {
            return None;
        }
        for body in candidates {
            selected.insert(body);
        }
    }
    if selected.len() != tool_record_indices.len() || selected.contains(&target_body) {
        return None;
    }
    Some(selected.into_iter().collect::<Vec<_>>())
}

/// The operand of a `Combine` that a binding refusal concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CombineOperandSide {
    Target,
    Tools,
}

/// A browser-node slot refusal of one `Combine` operand. A refusal becomes a
/// loss note only when its operand stays native after every binding pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CombineOperandRefusal {
    pub(crate) feature: cadmpeg_ir::features::FeatureId,
    pub(crate) side: CombineOperandSide,
    /// The operand word of the note: `target`, `tool` or `tools`.
    pub(crate) operand: &'static str,
    /// The native record of the refused selection.
    pub(crate) record: String,
    pub(crate) reason: &'static str,
}

impl CombineOperandRefusal {
    pub(super) fn note(&self) -> cadmpeg_ir::report::loss::LossNote {
        crate::loss::F3dLossCode::FeatureCombineOperandBodyUnbound.note(format!(
            "feature {} does not bind Combine {} {} through its browser node: {}",
            self.feature, self.operand, self.record, self.reason
        ))
    }
}

/// The loss notes of the refusals whose operand stays native in `features`.
pub(crate) fn native_combine_operand_refusal_notes(
    features: &[cadmpeg_ir::features::Feature],
    refusals: Vec<CombineOperandRefusal>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let native = |selection: &BodySelection| {
        matches!(
            selection,
            BodySelection::Native(_) | BodySelection::NativeSet(_)
        )
    };
    refusals
        .into_iter()
        .filter(|refusal| {
            features
                .iter()
                .find(|feature| feature.id == refusal.feature)
                .is_none_or(|feature| match feature.evaluation.definition() {
                    FeatureDefinition::Operation(FeatureOperation::Combine {
                        operands, ..
                    }) => native(match refusal.side {
                        CombineOperandSide::Target => operands.target(),
                        CombineOperandSide::Tools => operands.tools(),
                    }),
                    _ => true,
                })
        })
        .map(|refusal| refusal.note())
        .collect()
}

/// Bind an operand-list `Combine` through the browser node of each operand.
///
/// The target binds when its node names a body of the preceding history
/// state. The tools bind when every tool node names a distinct body of that
/// state. A node whose creating scope is bound to another history names no
/// body of that state. A body that both sides name leaves both selections native, and the
/// result is `Some(true)`. Each refusal names the operand side that it
/// concerns.
pub(super) fn bind_operand_list_combine_bodies(
    (target, tools): (
        &mut cadmpeg_ir::features::selections::BodySelection,
        &mut cadmpeg_ir::features::selections::BodySelection,
    ),
    scope: &crate::records::feature::scope::DesignParameterScope,
    operation: &crate::records::feature::combine::DesignCombineOperation,
    (feature_id, previous_state_id): (&cadmpeg_ir::features::FeatureId, i64),
    (histories, scopes, scope_histories): (
        &[AsmHistory],
        &[crate::records::feature::scope::DesignParameterScope],
        &HashMap<String, String>,
    ),
    refusals: &mut Vec<CombineOperandRefusal>,
) -> Option<bool> {
    use crate::records::feature::combine::DesignCombineBodyNode;
    use cadmpeg_ir::features::selections::{BodyMembers, BodySelection};

    let stream = crate::ids::native_stream(&scope.id)?;
    let (history, _, previous) =
        unique_history_state_pair(histories, scope.history_state_id()?, previous_state_id)?;
    let input = history.topology(previous)?;
    let body_slot = |node: Option<DesignCombineBodyNode>| -> Result<i64, &'static str> {
        let node = node.ok_or("it has no browser body node")?;
        let creator = scopes
            .iter()
            .find(|candidate| {
                candidate.record_index == node.creating_scope_record_index
                    && crate::ids::native_stream(&candidate.id) == Some(stream)
            })
            .ok_or("its creating scope is not decoded")?;
        if scope_histories
            .get(&creator.id)
            .is_some_and(|bound| *bound != history.id)
        {
            return Err("its creating scope reads another history");
        }
        let creator_state = creator
            .history_state_id()
            .ok_or("its creating scope has no history state")?;
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == creator_state);
        let (Some(state), None) = (states.next(), states.next()) else {
            return Err("its creating history state is not unique");
        };
        let [slot] = state.transition.as_ref().map_or(&[][..], |transition| {
            &transition.topology.bodies.inserted[..]
        }) else {
            return Err("its creating state does not insert exactly one body");
        };
        if input.bodies.contains(slot) {
            Ok(*slot)
        } else {
            Err("its body is absent from the preceding history state")
        }
    };
    let mut refuse = |operand: &'static str, record_index: u32, reason: &'static str| {
        let side = if operand == "target" {
            CombineOperandSide::Target
        } else {
            CombineOperandSide::Tools
        };
        refusals.push(CombineOperandRefusal {
            feature: feature_id.clone(),
            side,
            operand,
            record: format!("{stream}:design-record#{record_index}"),
            reason,
        });
    };
    let target_slot = match body_slot(operation.target_body_node) {
        Ok(slot) => Some(slot),
        Err(reason) => {
            refuse("target", operation.target_record_index, reason);
            None
        }
    };
    let mut tool_slots = Some(Vec::new());
    let mut tool_nodes = std::collections::HashSet::new();
    let mut distinct_tools = Vec::new();
    for tool in operation.tools.iter() {
        if tool
            .body_node
            .is_some_and(|node| !tool_nodes.insert(node.record_index))
        {
            continue;
        }
        distinct_tools.push(tool);
        match body_slot(tool.body_node) {
            Ok(slot) if Some(slot) == target_slot => {
                refuse(
                    "tool",
                    tool.record_index,
                    "its body is also the target body",
                );
                return Some(true);
            }
            Ok(slot) => {
                if let Some(slots) = tool_slots.as_mut() {
                    slots.push(slot);
                }
            }
            Err(reason) => {
                refuse("tool", tool.record_index, reason);
                tool_slots = None;
            }
        }
    }
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let design_record = |record_index| format!("{stream}:design-record#{record_index}");
    if let Some(target_slot) = target_slot {
        *target = BodySelection::historical(
            crate::ids::history_input_state_id(&prefix),
            vec![crate::ids::history_input_body_id(&prefix, target_slot)],
            design_record(operation.target_record_index),
        )
        .ok()?;
    }
    let mut rows = Vec::new();
    for (slot, tool) in tool_slots?.into_iter().zip(distinct_tools) {
        rows.push(body_member(
            crate::ids::history_input_body_id(&prefix, slot),
            design_record(tool.record_index),
        )?);
    }
    let Ok(members) = BodyMembers::try_from_rows(rows) else {
        refuse(
            "tools",
            operation.tools.first.record_index,
            "two tools name one body",
        );
        return None;
    };
    *tools = BodySelection::HistoricalSet {
        state: crate::ids::history_input_state_id(&prefix),
        members,
    };
    Some(false)
}

pub(super) fn combine_external_local_tools(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<cadmpeg_ir::features::selections::BodySelection> {
    let operation = scope.combine_operation()?;
    let mut bodies = Vec::new();
    for tool in operation.tools.iter() {
        let id = match (&tool.external_identity, &tool.legacy_external_identity) {
            (Some(identity), None) => crate::ids::neutral_combine_external_body_id(identity),
            (None, Some(identity)) => crate::ids::neutral_combine_legacy_external_body_id(identity),
            _ => return None,
        };

        if bodies.iter().any(|body| body == &id) {
            return None;
        }
        bodies.push(id);
    }
    let native = scope.id.clone();
    cadmpeg_ir::features::selections::BodySelection::local(bodies, native).ok()
}

/// The loss for a `Combine` whose tools hold a legacy cross-document
/// selector and that [`combine_external_local_tools`] refuses, because the
/// selectors name one body twice or mix with other tool selectors.
pub(super) fn combine_external_tools_refusal(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let operation = scope.combine_operation()?;
    if !operation
        .tools
        .iter()
        .any(|tool| tool.legacy_external_identity.is_some())
        || combine_external_local_tools(scope).is_some()
    {
        return None;
    }
    Some(
        crate::loss::F3dLossCode::FeatureCombineToolExternalBodyUnbound.note(format!(
            "Combine scope {}: the legacy cross-document tool selectors name one body twice or mix with other tool selectors; the tools stay native",
            scope.id
        )),
    )
}

pub(super) fn historical_body_slot(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:body#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

pub(super) fn bind_body_recipe_body_selection(
    selection: &mut cadmpeg_ir::features::selections::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) {
    use cadmpeg_ir::features::selections::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A
                    | DesignOperandRole::ROLE_0X5
                    | DesignOperandRole::BODIES_B
            )
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            return;
        };
        if !body_slots.contains(&body_slot) {
            body_slots.push(body_slot);
        }
    }

    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let mut body_ids = Vec::new();

    for slot in body_slots {
        body_ids.push(crate::ids::history_input_body_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) =
        cadmpeg_ir::features::selections::BodySelection::historical(state, body_ids, native)
    {
        *selection = historical;
    }
}

pub(super) fn direct_design_record_body<'a>(
    native: &str,
    scope: &crate::records::feature::scope::DesignParameterScope,
    stream: Option<&str>,
    operands: &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    body_candidates: &mut BodyCandidates<'a>,
) -> Option<cadmpeg_ir::features::selections::BodyMember<cadmpeg_ir::ids::BodyId>> {
    let (native_stream_name, record_index) = native.rsplit_once(":design-record#")?;
    let record_index = record_index.parse::<u32>().ok()?;
    if Some(native_stream_name) != stream {
        return None;
    }
    let mut matching_operands = operands.iter().filter(|operand| {
        crate::ids::native_stream(&operand.id) == Some(native_stream_name)
            && operand.scope_record_index == scope.record_index
            && matches!(
                operand.owner,
                crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
            )
            && operand.record_index() == record_index
    });
    let operand = matching_operands.next()?;
    if matching_operands.next().is_some() {
        return None;
    }
    let body = body_candidates.direct(operand)?;
    body_member(body, native.to_owned())
}

pub(super) fn bind_direct_body_recipe_body_selection<'a>(
    selection: &mut cadmpeg_ir::features::selections::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
    use cadmpeg_ir::features::selections::{BodyMember, BodySelection};

    let groups = inputs.groups;
    let operands = inputs.body_recipe_operands;

    let stream = crate::ids::native_stream(&scope.id);
    let native_members = match selection {
        BodySelection::Native(group_id) => {
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    )
                    && crate::ids::native_stream(&group.id) == stream
            });
            match (matching_groups.next(), matching_groups.next()) {
                (Some(group), None) if !group.members().is_empty() => {
                    let mut selected = Vec::new();
                    for (ordinal, record_index) in group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .enumerate()
                    {
                        let Ok(ordinal) = u32::try_from(ordinal) else {
                            return;
                        };
                        let mut matching_operands = operands.iter().filter(|operand| {
                            operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == record_index
                                && crate::ids::native_stream(&operand.id) == stream
                        });
                        let Some(operand) = matching_operands.next() else {
                            return;
                        };
                        if matching_operands.next().is_some() {
                            return;
                        }
                        let Some(body) = body_candidates.direct(operand) else {
                            return;
                        };
                        if selected.contains(&body) {
                            return;
                        }

                        selected.push(body);
                    }
                    let Ok(bodies) =
                        cadmpeg_ir::features::selections::DistinctMembers::try_from(selected)
                    else {
                        return;
                    };
                    let native = group.id.clone();
                    *selection = BodySelection::Resolved { bodies, native };
                    return;
                }
                _ => {
                    // A single design-record identity names no operand group.
                    // Resolve it through the same per-record lookup the
                    // multi-member form uses.
                    let Some(row) = direct_design_record_body(
                        group_id,
                        scope,
                        stream,
                        operands,
                        body_candidates,
                    ) else {
                        return;
                    };
                    let Ok(members) =
                        cadmpeg_ir::features::selections::BodyMembers::try_from_rows(vec![row])
                    else {
                        return;
                    };
                    *selection = BodySelection::ResolvedSet { members };
                    return;
                }
            }
        }
        BodySelection::NativeSet(native) => native.iter().map(String::as_str).collect::<Vec<_>>(),
        _ => return,
    };
    if native_members.is_empty() {
        return;
    }
    for (index, native) in native_members.iter().enumerate() {
        for previous in &native_members[..index] {
            if native == previous {
                return;
            }
        }
    }
    let mut rows = Vec::new();
    for native in &native_members {
        let Some(row) = direct_design_record_body(native, scope, stream, operands, body_candidates)
        else {
            return;
        };
        if rows
            .iter()
            .any(|existing: &BodyMember<_>| existing.body() == row.body())
        {
            return;
        }
        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::selections::BodyMembers::try_from_rows(rows) {
        *selection = BodySelection::ResolvedSet { members };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

pub(super) fn unique_feature_history_states(
    history: &AsmHistory,
) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in history.states() {
        states
            .entry(state.state_id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(state));
    }
    states
}

pub(super) fn singleton_revised_input_body_across_state_chain<'a>(
    history: &AsmHistory,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for &body in transition
            .topology
            .bodies
            .updated
            .iter()
            .chain(&transition.topology.bodies.deleted)
        {
            revised.insert(body);
        }
        let previous_id = transition.previous_state_id?;
        let Some(Some(previous)) = states.get(&previous_id) else {
            return None;
        };
        current = previous;
    }
    let input = history.topology(current)?;
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let body = candidates.next().copied()?;
    candidates.next().is_none().then_some(body)
}

pub(super) fn singleton_body_revision_across_state_chain<'a>(
    history: &AsmHistory,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let result_topology = history.topology(state)?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let revision = body_revision_without_topology_change(current)?;
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return None,
            }
        }
        let previous = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)?;
        let Some(Some(previous)) = states.get(&previous) else {
            return None;
        };
        current = previous;
    }
    let body = selected?;
    let input_topology = history.topology(current)?;
    (result_topology.bodies.contains(&body) && input_topology.bodies.contains(&body))
        .then_some(body)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TopologyStableBodyRevision {
    Unchanged,
    Revised(i64),
}

pub(super) fn body_revision_without_topology_change(
    current: &AsmDeltaState,
) -> Option<TopologyStableBodyRevision> {
    let transition = current.transition.as_ref()?;
    let delta = &transition.topology;
    let body = match delta.bodies.updated.as_slice() {
        [] => TopologyStableBodyRevision::Unchanged,
        [body] => TopologyStableBodyRevision::Revised(*body),
        _ => return None,
    };
    if !delta.bodies.inserted.is_empty()
        || !delta.bodies.deleted.is_empty()
        || [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
        ]
        .into_iter()
        .any(|family| {
            !family.inserted.is_empty() || !family.deleted.is_empty() || !family.updated.is_empty()
        })
    {
        return None;
    }
    Some(body)
}
