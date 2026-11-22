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
use std::collections::HashMap;

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

/// Seeds of a pattern scope whose transform does not project. A feature seed
/// is kept only when it precedes the pattern in the modelling history.
pub(super) fn unprojected_pattern_seeds(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Vec<cadmpeg_ir::features::patterns::PatternSeed> {
    pattern_seed_group(scope, groups)
        .map(|group| {
            pattern_group_seeds(
                scope,
                group,
                face_operands,
                entity_selection_operands,
                scopes,
                authored_ordinals,
                losses,
            )
        })
        .filter(|seeds| {
            seeds.iter().all(|seed| match seed {
                cadmpeg_ir::features::patterns::PatternSeed::Feature(id) => scopes
                    .iter()
                    .filter(|candidate| crate::ids::neutral_feature_id(candidate) == *id)
                    .all(|candidate| precedes_in_history(candidate, scope, scopes)),
                _ => true,
            })
        })
        .unwrap_or_default()
}

/// Whether `candidate` precedes `scope` in the modelling history: walking the
/// preceding history states back from `scope` reaches `candidate`'s state.
fn precedes_in_history(
    candidate: &DesignParameterScope,
    scope: &DesignParameterScope,
    scopes: &[DesignParameterScope],
) -> bool {
    let Some(target) = candidate.history_state_id() else {
        return false;
    };
    let stream = native_stream(&scope.id);
    let mut state = scope.previous_history_state_id();
    for _ in 0..=scopes.len() {
        let Some(current) = state else {
            return false;
        };
        if current == target {
            return true;
        }
        let Some(owner) = unique_feature_match(scopes.iter().filter(|owner| {
            native_stream(&owner.id) == stream && owner.history_state_id() == Some(current)
        })) else {
            return false;
        };
        state = owner
            .previous_history_state_id()
            .filter(|previous| *previous != current);
    }
    false
}

/// Seeds of a pattern's seed group. A body-role group selects its bodies. A
/// face-role group selects its historical faces in the scope's preceding
/// state; otherwise, when every member is a compact entity selection that
/// names a feature scope, it selects those features in member order. A face
/// seed group whose members all decode as bounded-face operands selects their
/// historical faces, or the group's native faces; with any other member it
/// selects nothing. `authored_ordinals` holds the scopes that the feature
/// timeline lists.
pub(super) fn pattern_group_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Vec<cadmpeg_ir::features::patterns::PatternSeed> {
    use cadmpeg_ir::features::patterns::PatternSeed;

    if group.role() == DesignOperandRole::ROLE_0X10 {
        let stream = native_stream(&group.id);
        let all_bounded_faces = group.members().iter().enumerate().all(|(ordinal, member)| {
            face_operands.iter().any(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == scope.record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand
                        .group_member_ordinal()
                        .and_then(|member_ordinal| usize::try_from(member_ordinal).ok())
                        == Some(ordinal)
                    && operand.record_index() == member.value
                    && operand.recipe_kind
                        == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            })
        });
        if !all_bounded_faces {
            return Vec::new();
        }
        return vec![PatternSeed::Faces(
            resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            )
            .unwrap_or_else(|| {
                cadmpeg_ir::features::selections::FaceSelection::Native(group.id.clone())
            }),
        )];
    }
    if group.role() != DesignOperandRole::BODIES_A {
        return vec![PatternSeed::Bodies(
            cadmpeg_ir::features::selections::BodySelection::Native(group.id.clone()),
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
    entity_selection_feature_seeds(
        scope,
        group,
        entity_selection_operands,
        scopes,
        authored_ordinals,
        losses,
    )
    .unwrap_or_else(|| {
        vec![PatternSeed::Faces(
            cadmpeg_ir::features::selections::FaceSelection::Native(group.id.clone()),
        )]
    })
}

/// Feature seeds named by a face-role group whose members are all compact
/// entity selections. Each compact identity is the record index of a
/// modelling feature scope in the same stream other than `scope` that the
/// feature timeline lists. A scope that the timeline does not list is an
/// internal operation operand and stays native, so the group selects no
/// feature through it.
pub(super) fn entity_selection_feature_seeds(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    authored_ordinals: &HashMap<(&str, u32), u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
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
            let mut matching_scopes = scopes
                .iter()
                .filter(|candidate| {
                    native_stream(&candidate.id) == Some(stream)
                        && candidate.record_index == record_index
                        && candidate.record_index != scope.record_index
                        && candidate.history_state_id().is_some()
                })
                .peekable();
            matching_scopes.peek()?;
            let seed_scope = unique_feature_match(matching_scopes);
            let Some(seed_scope) = seed_scope.filter(|candidate| {
                authored_ordinals.contains_key(&(stream, candidate.record_index))
            }) else {
                losses.push(pattern_feature_seed_substituted_loss(scope, group));
                return None;
            };
            Some(cadmpeg_ir::features::patterns::PatternSeed::Feature(
                crate::ids::neutral_feature_id(seed_scope),
            ))
        })
        .collect()
}

pub(super) fn pattern_feature_seed_substituted_loss(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::FeaturePatternFeatureSeedSubstituted.note(format!(
        "Pattern scope {} uses seed group {} as the fallback selection because its compact feature seed does not resolve to a unique listed scope.",
        scope.id, group.id,
    ))
}

/// The loss a rectangular-pattern payload charges when its seed kind or its
/// body references break the distribution-block rules, or disagree with the
/// face, feature, or body seeds of the pattern. The pattern keeps its seeds.
pub(super) fn distribution_seed_conflict(
    scope: &DesignParameterScope,
    distribution: &crate::records::feature::patterns::DesignRectangularPatternDistribution,
    seeds: &[cadmpeg_ir::features::patterns::PatternSeed],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::patterns::PatternSeed;
    /// Seed kinds of the distribution block.
    const FACE_SEED: u32 = 0;
    const FEATURE_SEED: u32 = 1;
    const BODY_SEED: u32 = 2;

    let name = |kind: u32| match kind {
        FACE_SEED => "faces",
        FEATURE_SEED => "features",
        _ => "bodies",
    };
    let kinds = seeds
        .iter()
        .map(|seed| match seed {
            PatternSeed::Faces(_) => Some(FACE_SEED),
            PatternSeed::Feature(_) => Some(FEATURE_SEED),
            PatternSeed::Bodies(_) => Some(BODY_SEED),
            PatternSeed::Native(_) | PatternSeed::Occurrences(_) => None,
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    let seed_kind = match kinds.as_slice() {
        [first, rest @ ..] if rest.iter().all(|kind| kind == first) => Some(*first),
        _ => None,
    };
    let stored = distribution.seed_kind.value;
    let mut conflicts = Vec::new();
    if stored > BODY_SEED {
        conflicts.push(format!(
            "stores seed kind {stored} at byte {}, above the last seed kind {BODY_SEED}",
            distribution.seed_kind.offset
        ));
    }
    if let Some(kind) = seed_kind.filter(|kind| *kind != stored) {
        conflicts.push(format!(
            "stores seed kind {stored} at byte {} where its seeds are {}",
            distribution.seed_kind.offset,
            name(kind)
        ));
    }
    let face_or_body = stored != FEATURE_SEED || seed_kind.is_some_and(|kind| kind != FEATURE_SEED);
    if face_or_body && distribution.body_references.value != 0 {
        conflicts.push(format!(
            "stores {} body references at byte {} with a face or body seed",
            distribution.body_references.value, distribution.body_references.offset
        ));
    }
    if distribution.untyped_body_references != 0 {
        conflicts.push(format!(
            "names {} records of a type other than the body browser node type among its {} body references",
            distribution.untyped_body_references, distribution.body_references.value
        ));
    }
    (!conflicts.is_empty()).then(|| {
        crate::loss::F3dLossCode::FeaturePatternPayloadSeedUnconfirmed.note(format!(
            "RectangularPattern scope {}: pattern payload record {} {}; the pattern keeps its seeds.",
            scope.id,
            distribution.record_index,
            conflicts.join(", and ")
        ))
    })
}
