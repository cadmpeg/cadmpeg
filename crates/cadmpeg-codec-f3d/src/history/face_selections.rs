// SPDX-License-Identifier: Apache-2.0
//! Feature face selections, entity face groups, and hole face selections.

use super::binding_histories::{history_state_indexes, visit_states_oldest_first};
use super::face_resolution::historical_brep_source;
use super::history_states::{
    bound_history_state_pair, effective_scope_previous_history_state_id, history_state_pair,
    unique_history_state_pair,
};
use crate::history_records::AsmHistory;
use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{HashMap, HashSet};

pub(crate) struct FeatureFaceSelectionInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) operands: &'a [crate::records::topology::face::DesignFaceOperand],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    pub(crate) histories: &'a mut [AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
}

pub(crate) fn bind_feature_face_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::evaluation::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    let FeatureFaceSelectionInputs {
        scopes,
        groups,
        operands,
        entity_operands,
        body_recipe_operands,
        histories,
        scope_histories,
    } = input;
    let selection_states = features
        .iter()
        .map(|feature| face_selection_state(feature, scopes, scope_histories, histories))
        .collect::<Vec<_>>();
    let gates = features
        .iter()
        .zip(&selection_states)
        .enumerate()
        .filter(|(_, (feature, _))| face_selection_reads_state(feature.evaluation.definition()))
        .filter_map(|(position, (_, state))| {
            state.map(|state| (state.history, state.previous, position))
        })
        .collect::<Vec<_>>();
    let mut available = HashSet::new();
    visit_states_oldest_first(histories, gates, |history, previous, position| {
        if history.topology(previous).is_some() {
            available.insert(position);
        }
    });
    let histories = &*histories;
    for ((feature_position, feature), selection_state) in
        features.iter_mut().enumerate().zip(selection_states)
    {
        let selection_state = selection_state.filter(|_| available.contains(&feature_position));
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(FaceSelectionState {
                scope,
                previous_state_id,
                history,
                state,
                previous: selection_previous,
                ..
            }) = selection_state
            else {
                break 'feature_edit;
            };
            let Some(history) = histories.get(history) else {
                break 'feature_edit;
            };
            let (Some(selection_state_row), Some(selection_previous_row)) = (
                history.states().get(state),
                history.states().get(selection_previous),
            ) else {
                break 'feature_edit;
            };
            let Some(transition) = selection_state_row.transition.as_ref() else {
                break 'feature_edit;
            };
            match definition {
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Extrude { start, extent, .. },
                ) => {
                    if let cadmpeg_ir::features::extents::ExtrudeStart::FromFace { face, .. } =
                        start
                    {
                        super::selection::bind_face_selection(
                            face,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        );
                        bind_entity_face_selection(
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            groups,
                            entity_operands,
                        );
                    }
                    let sides = match extent {
                        cadmpeg_ir::features::extents::ExtrudeExtent::OneSided { side }
                        | cadmpeg_ir::features::extents::ExtrudeExtent::Symmetric { side } => {
                            vec![side]
                        }
                        cadmpeg_ir::features::extents::ExtrudeExtent::TwoSided {
                            first,
                            second,
                        } => {
                            vec![first, second]
                        }
                    };
                    for side in sides {
                        if let cadmpeg_ir::features::extents::LinearTermination::ToFace {
                            face,
                            ..
                        } = &mut side.termination
                        {
                            super::selection::bind_face_selection(
                                face,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            );
                        }
                    }
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                ) => {
                    for seed in seeds {
                        let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed else {
                            continue;
                        };
                        super::selection::bind_face_selection(
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        );
                        bind_entity_face_selection(
                            faces,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            groups,
                            entity_operands,
                        );
                    }
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::MoveFace { faces, motion },
                ) => {
                    super::move_face::bind_move_face_selection(
                        faces,
                        motion,
                        feature_id,
                        scope,
                        groups,
                        operands,
                        &super::move_face::MoveFaceStates {
                            histories,
                            history,
                            state: selection_state_row,
                            previous: selection_previous_row,
                        },
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                ) => {
                    super::selection::bind_face_selection(
                        faces,
                        scope,
                        groups,
                        operands,
                        &transition.topology.faces.updated,
                    );
                    crate::design::feature_project::thicken::list_thicken_whole_bodies(
                        faces, scope, groups,
                    );
                    super::selection::bind_body_recipe_face_selection(
                        faces,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::UntrimSurface { faces, .. },
                ) => {
                    super::selection::bind_face_selection(
                        faces,
                        scope,
                        groups,
                        operands,
                        &transition.topology.faces.updated,
                    );
                    super::selection::bind_body_recipe_face_selection(
                        faces,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Shell {
                        removed_faces,
                        thickness: Some(thickness),
                        mode,
                        ..
                    },
                ) => {
                    losses.extend(super::shell_walls::bind_shell_removed_faces(
                        removed_faces,
                        feature_id,
                        scope,
                        groups,
                        &super::shell_walls::ShellWalls {
                            thickness: thickness.get(),
                            mode: *mode,
                            previous_state_id,
                            transition,
                            input: history.topology(selection_previous_row),
                            output: history.topology(selection_state_row),
                        },
                    ));
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                ) => {
                    bind_surface_stitch_face_selection(
                        faces,
                        FaceSelectionBinding {
                            feature_id,
                            previous_state_id,
                            operation_history_id: &history.id,
                            scope,
                            input_topologies: &mut *input_topologies,
                        },
                        groups,
                        entity_operands,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::SplitFace { targets, tool, .. },
                ) => {
                    super::selection::bind_face_selection(
                        targets,
                        scope,
                        groups,
                        operands,
                        &transition.topology.faces.updated,
                    );
                    if let cadmpeg_ir::features::direct_edit::SplitFaceTool::Faces(faces) = tool {
                        super::selection::bind_face_selection(
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        );
                    }
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::DeleteFace { faces, .. },
                ) => {
                    super::delete_face::bind_delete_face_selection(
                        faces,
                        feature_id,
                        scope,
                        groups,
                        &super::delete_face::DeleteFaceStates {
                            histories,
                            history,
                            state: selection_state_row,
                            previous: selection_previous_row,
                        },
                        &mut losses,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Hole {
                        face: hole_face @ Some(_),
                        ..
                    },
                ) => {
                    let preceding = history
                        .states()
                        .get(selection_previous)
                        .and_then(|state| history.topology(state));
                    if let Some(face) = hole_face.as_mut() {
                        bind_hole_face_selection(
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            transition,
                            preceding,
                        );
                    }
                    if matches!(
                        hole_face,
                        Some(cadmpeg_ir::features::selections::FaceSelection::Native(_))
                    ) && scope.hole_construction().zip(preceding).is_some_and(
                        |(construction, preceding)| {
                            super::hole_support::entries_lie_on_no_face(construction, preceding)
                        },
                    ) {
                        *hole_face = None;
                    }
                }
                _ => {}
            }
        });
    }
    losses
}

#[derive(Clone, Copy)]
pub(super) struct FaceSelectionState<'a> {
    pub(super) scope: &'a crate::records::feature::scope::DesignParameterScope,
    pub(super) previous_state_id: i64,
    pub(super) history: usize,
    pub(super) state: usize,
    pub(super) previous: usize,
}

/// The state pairs of every history that holds an unbound fillet scope's
/// state pair, when more than one does. A scope bound to a history has its
/// state from `face_selection_state` instead.
pub(super) fn unbound_face_selection_candidates<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> Vec<FaceSelectionState<'a>> {
    let Some(native_ref) = feature.native_ref.as_deref() else {
        return Vec::new();
    };
    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
    let (Some(scope), None) = (matching_scopes.next(), matching_scopes.next()) else {
        return Vec::new();
    };
    if scope_histories.contains_key(&scope.id) {
        return Vec::new();
    }
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, histories),
    ) else {
        return Vec::new();
    };
    let candidates = histories
        .iter()
        .filter_map(|history| history_state_pair(history, state_id, previous_state_id, false))
        .filter(|(_, state, _)| {
            state
                .transition
                .as_ref()
                .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
        })
        .filter_map(|(history, state, previous)| {
            let (history_index, state_index) = history_state_indexes(histories, history, state)?;
            let (_, previous_index) = history_state_indexes(histories, history, previous)?;
            Some(FaceSelectionState {
                scope,
                previous_state_id,
                history: history_index,
                state: state_index,
                previous: previous_index,
            })
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        candidates
    } else {
        Vec::new()
    }
}

pub(super) fn face_selection_state<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> Option<FaceSelectionState<'a>> {
    let native_ref = feature.native_ref.as_deref()?;
    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    // A scope bound to a history reads that history; other scopes need a
    // state pair that only one history holds.
    let (history, state, previous) = if scope_histories.contains_key(&scope.id) {
        bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )
    } else {
        unique_history_state_pair(histories, state_id, previous_state_id)
    }?;
    let spans_transitions = matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::MoveFace { .. }
                | cadmpeg_ir::features::FeatureOperation::DeleteFace { heal: true, .. }
        )
    );
    if !spans_transitions
        && state.transition.as_ref()?.previous_state_id != Some(previous_state_id)
        && !extrude_spans_state_chain(feature, histories, history, state_id, previous_state_id)
    {
        return None;
    }
    let (history_index, state_index) = history_state_indexes(histories, history, state)?;
    let (_, previous_index) = history_state_indexes(histories, history, previous)?;
    Some(FaceSelectionState {
        scope,
        previous_state_id,
        history: history_index,
        state: state_index,
        previous: previous_index,
    })
}

/// Whether an `Extrude` reaches its result state from its preceding state
/// through one complete chain of uniquely bound transitions.
pub(super) fn extrude_spans_state_chain(
    feature: &cadmpeg_ir::features::Feature,
    histories: &[AsmHistory],
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
) -> bool {
    matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude { .. }
        )
    ) && super::output_chain::OutputChain::resolve(
        histories,
        state_id,
        previous_state_id,
        Some(&history.id),
    )
    .is_some()
}

pub(super) fn face_selection_reads_state(
    definition: &cadmpeg_ir::features::FeatureDefinition,
) -> bool {
    matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                | cadmpeg_ir::features::FeatureOperation::Pattern { .. }
                | cadmpeg_ir::features::FeatureOperation::MoveFace { .. }
                | cadmpeg_ir::features::FeatureOperation::Thicken { .. }
                | cadmpeg_ir::features::FeatureOperation::Shell { .. }
                | cadmpeg_ir::features::FeatureOperation::KnitSurface { .. }
                | cadmpeg_ir::features::FeatureOperation::SplitFace { .. }
                | cadmpeg_ir::features::FeatureOperation::Hole { face: Some(_), .. }
                | cadmpeg_ir::features::FeatureOperation::DeleteFace { heal: true, .. }
        )
    )
}

pub(super) struct FaceSelectionBinding<'a> {
    pub(super) feature_id: &'a cadmpeg_ir::features::FeatureId,
    pub(super) previous_state_id: i64,
    pub(super) operation_history_id: &'a str,
    pub(super) scope: &'a crate::records::feature::scope::DesignParameterScope,
    pub(super) input_topologies: &'a mut [cadmpeg_ir::features::evaluation::FeatureInputTopology],
}

pub(super) fn bind_entity_face_selection(
    selection: &mut cadmpeg_ir::features::selections::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::selections::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return,
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    bind_entity_face_groups(
        selection,
        &group.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        std::slice::from_ref(&group),
        operands,
    );
}

// Keep the serialized-selection context explicit at this boundary so every
// admission input remains visible to the strict all-members proof.
pub(super) fn bind_surface_stitch_face_selection(
    selection: &mut cadmpeg_ir::features::selections::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;
    let native_id = match selection {
        cadmpeg_ir::features::selections::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return,
    };
    if native_id != scope.id {
        return;
    }
    let Some(input_end) = scope.reference_members().len().checked_sub(2) else {
        return;
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups
        .iter()
        .filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
        })
        .collect::<Vec<_>>();
    matching_groups.sort_by_key(|a| a.scope_reference_ordinal);
    if matching_groups.len().checked_mul(2) != Some(input_end)
        || matching_groups
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
            })
    {
        return;
    }
    bind_entity_face_groups(
        selection,
        &scope.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        &matching_groups,
        operands,
    );
}

// Keep the shared selection-binding inputs explicit; this helper is the
// single admission point for both one-group and SurfaceStitch selections.
pub(super) fn bind_entity_face_groups(
    selection: &mut cadmpeg_ir::features::selections::FaceSelection,
    native_id: &str,
    input: FaceSelectionBinding<'_>,
    groups: &[&crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    if groups.is_empty() {
        return;
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let stream = crate::ids::native_stream(&scope.id);
    for group in groups {
        if group.members().is_empty() {
            return;
        }
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return;
            };
            let mut matches = operands.iter().filter(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
                    && crate::ids::native_stream(&operand.id) == stream
            });
            let Some(operand) = matches.next() else {
                return;
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return;
            };
            if matches.next().is_some() {
                return;
            }
            let local = candidate.history_id == operation_history_id
                && candidate.historical.state_ids.contains(&previous_state_id);
            let Some(source) = historical_brep_source(&candidate.history_id) else {
                return;
            };
            if !selected.contains(&(source, candidate.face_slot, local)) {
                selected.push((source, candidate.face_slot, local));
            }
        }
    }
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let mut faces = Vec::new();
    for (source, face, local) in selected {
        let slot = if local {
            cadmpeg_ir::ids::IdentityKey::from(face)
        } else {
            let Ok(slot) =
                cadmpeg_ir::ids::IdentityKey::try_new(format!("{}:{source}:{face}", source.len()))
            else {
                return;
            };
            slot
        };
        let id = crate::ids::history_input_face_id(&prefix, slot);

        faces.push(id);
    }
    for face in &faces {
        if !topology.faces.contains(face) {
            let retained = face.clone();
            topology.faces.insert(retained);
        }
    }
    let native = native_id.to_owned();
    if let Ok(historical) =
        cadmpeg_ir::features::selections::FaceSelection::historical(state_id, faces, native)
    {
        *selection = historical;
    }
}

pub(super) fn bind_hole_face_selection(
    selection: &mut cadmpeg_ir::features::selections::FaceSelection,
    input: FaceSelectionBinding<'_>,
    transition: &crate::history_records::AsmHistoricalTransition,
    preceding: Option<&crate::history_records::AsmHistoricalTopology>,
) {
    use cadmpeg_ir::features::selections::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let FaceSelection::Native(native_id) = selection else {
        return;
    };
    let Some(construction) = scope.hole_construction() else {
        return;
    };
    if let Some(faces) = preceding.and_then(|preceding| {
        super::hole_support::hole_support_faces(construction, transition, preceding)
    }) {
        let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
        let state_id = crate::ids::history_input_state_id(&prefix);
        let mut topologies = input_topologies
            .iter_mut()
            .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
        let Some(topology) = topologies.next() else {
            return;
        };
        if topologies.next().is_some() {
            return;
        }
        let selected: Vec<_> = faces
            .into_iter()
            .map(|slot| {
                crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
            })
            .collect();
        for face in &selected {
            if !topology.faces.contains(face) {
                topology.faces.insert(face.clone());
            }
        }
        if let Ok(historical) = cadmpeg_ir::features::selections::FaceSelection::historical(
            state_id,
            selected,
            native_id.to_owned(),
        ) {
            *selection = historical;
        }
        return;
    }
    let Some(face_selection) = &construction.face_selection else {
        return;
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return;
    };
    let local = candidate.history_id == operation_history_id;
    if local && !candidate.historical.state_ids.contains(&previous_state_id) {
        return;
    }
    let Some(source) = historical_brep_source(&candidate.history_id) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let slot = if local {
        cadmpeg_ir::ids::IdentityKey::from(candidate.face_slot)
    } else {
        let Ok(slot) = cadmpeg_ir::ids::IdentityKey::try_new(format!(
            "{}:{source}:{}",
            source.len(),
            candidate.face_slot
        )) else {
            return;
        };
        slot
    };
    let face = crate::ids::history_input_face_id(&prefix, slot);
    if !topology.faces.contains(&face) {
        let retained = face.clone();
        topology.faces.insert(retained);
    }
    let native = native_id.to_owned();
    let selected = vec![face];
    if let Ok(historical) =
        cadmpeg_ir::features::selections::FaceSelection::historical(state_id, selected, native)
    {
        *selection = historical;
    }
}
