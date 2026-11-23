// SPDX-License-Identifier: Apache-2.0
//! Feature path selections and entity selection paths.

pub(crate) fn bind_feature_path_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::surface_inputs::SurfaceBoundary;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| 'feature_edit: {
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
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary,
                    interior_constraints,
                    ..
                }) => {
                    let boundary = match boundary {
                        SurfaceBoundary::Path(path) => Some(path),
                        SurfaceBoundary::Edges(_) => None,
                    };
                    for path in boundary.into_iter().chain(interior_constraints) {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths,
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    for path in paths {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                    if let Some(guide_rail) = guide_rail {
                        bind_entity_selection_path(
                            &mut guide_rail.path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                _ => {}
            }
        });
    }
}

pub(super) fn bind_entity_selection_path(
    path: &mut cadmpeg_ir::features::profiles::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::profiles::PathRef;

    let PathRef::Native(group_id) = path else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }

    let mut edge_slots = Vec::new();

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
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return;
        };
        edge_slots.push(edge_slot);
    }

    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let mut edge_ids = Vec::new();

    for slot in edge_slots {
        edge_ids.push(crate::ids::history_input_edge_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native) {
        *path = historical;
    }
}
