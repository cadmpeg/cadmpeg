// SPDX-License-Identifier: Apache-2.0
//! Seed selections of rectangular, circular, and Mirror pattern scopes.

use super::{resolved_historical_face_group, unique_feature_match};
use crate::ids::native_stream;
use crate::records::{
    feature::scope::DesignParameterScope,
    topology::{
        construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand,
    },
};

/// The seed group of a rectangular, circular, or Mirror pattern scope: its
/// unique non-empty body- or face-role group, or, when it has none, its unique
/// face seed group.
pub(super) fn pattern_seed_group<'a>(
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> Option<&'a DesignConstructionOperandGroup> {
    let stream = native_stream(&scope.id)?;
    let scope_groups = || {
        groups.iter().filter(move |group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && !group.members().is_empty()
        })
    };
    let mut role_groups = scope_groups()
        .filter(|group| {
            matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            )
        })
        .peekable();
    if role_groups.peek().is_some() {
        return unique_feature_match(role_groups);
    }
    unique_feature_match(
        scope_groups()
            .filter(|group| crate::design::is_pattern_face_seed_group(&scope.kind(), group)),
    )
}

/// Seeds of a pattern scope whose transform does not project.
pub(super) fn unprojected_pattern_seeds(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Vec<cadmpeg_ir::features::patterns::PatternSeed> {
    pattern_seed_group(scope, groups)
        .map(|group| {
            pattern_group_seeds(
                scope,
                group,
                face_operands,
                entity_selection_operands,
                scopes,
            )
        })
        .unwrap_or_default()
}

/// Seeds of a pattern's seed group. A body-role group selects its bodies. A
/// face-role group selects its historical faces in the scope's preceding
/// state; otherwise, when every member is a compact entity selection that
/// names a feature scope, it selects those features in member order.
pub(super) fn pattern_group_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Vec<cadmpeg_ir::features::patterns::PatternSeed> {
    use cadmpeg_ir::features::patterns::PatternSeed;

    if group.role() == DesignOperandRole::ROLE_0X10 {
        return vec![PatternSeed::Faces(
            resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            )
            .unwrap_or_else(|| cadmpeg_ir::features::FaceSelection::Native(group.id.clone())),
        )];
    }
    if group.role() != DesignOperandRole::BODIES_A {
        return vec![PatternSeed::Bodies(
            cadmpeg_ir::features::BodySelection::Native(group.id.clone()),
        )];
    }
    if let Some(selection) = resolved_historical_face_group(
        scope,
        scope.previous_history_state_id(),
        group,
        face_operands,
    ) {
        return vec![PatternSeed::Faces(selection)];
    }
    entity_selection_feature_seeds(scope, group, entity_selection_operands, scopes).unwrap_or_else(
        || {
            vec![PatternSeed::Faces(
                cadmpeg_ir::features::FaceSelection::Native(group.id.clone()),
            )]
        },
    )
}

/// Feature seeds named by a face-role group whose members are all compact
/// entity selections. Each compact identity is the record index of a
/// modelling feature scope in the same stream other than `scope`.
fn entity_selection_feature_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
) -> Option<Vec<cadmpeg_ir::features::patterns::PatternSeed>> {
    let stream = native_stream(&scope.id)?;
    if group.members().is_empty() {
        return None;
    }
    group
        .members()
        .iter()
        .enumerate()
        .map(|(ordinal, member)| {
            let ordinal = u32::try_from(ordinal).ok()?;
            let selection =
                unique_feature_match(entity_selection_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == ordinal
                        && operand.record_index() == member.value
                }))?;
            let record_index = u32::try_from(selection.compact_identity()?).ok()?;
            let seed_scope = unique_feature_match(scopes.iter().filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == record_index
                    && candidate.record_index != scope.record_index
                    && candidate.history_state_id().is_some()
            }))?;
            Some(cadmpeg_ir::features::patterns::PatternSeed::Feature(
                crate::ids::neutral_feature_id(seed_scope),
            ))
        })
        .collect()
}
