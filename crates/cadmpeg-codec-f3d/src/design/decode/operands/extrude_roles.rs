// SPDX-License-Identifier: Apache-2.0
//! Extrude face roles and body target groups.

use super::construction_groups::{extrude_operand_role, PendingExtrudeRole};
use crate::ids::native_stream;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::{
    DesignExtrudeFaceEncoding, DesignExtrudeFaceRole, DesignOperandRole,
};

pub(in crate::design) fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
    seventh_slot_record_index: Option<u32>,
) {
    let face_group_count = groups
        .iter()
        .filter(|group| {
            matches!(
                extrude_operand_role(scope, group.role()),
                Some(PendingExtrudeRole::Faces(_))
            )
        })
        .count();
    let start_group_absent = face_group_count == 1
        && scope
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent)
            == Some(DesignExtrudeExtent::OneSidedToFace)
        && groups.iter().any(|group| {
            matches!(
                extrude_operand_role(scope, group.role()),
                Some(PendingExtrudeRole::Faces(_))
            ) && Some(group.record_index) == seventh_slot_record_index
        });
    let mut face_groups =
        groups
            .iter_mut()
            .filter_map(|group| match extrude_operand_role(scope, group.role()) {
                Some(PendingExtrudeRole::Faces(encoding)) => Some((group, encoding)),
                _ => None,
            });
    if !start_group_absent
        && scope.extrude_prologue().map(DesignExtrudePrologue::start)
            == Some(DesignExtrudeStart::FromFace)
    {
        if let Some((group, encoding)) = face_groups.next() {
            group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
                encoding,
                usage: DesignExtrudeFaceRole::Start,
            };
        }
    }
    for (group, encoding) in face_groups {
        group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
            encoding,
            usage: DesignExtrudeFaceRole::Termination,
        };
    }
}

pub(crate) fn allows_slotted_extrude_face_targets(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.extrude_prologue(),
        Some(DesignExtrudePrologue::ReferenceAware {
            extent: DesignExtrudeExtent::TwoSidedToTargetsOptionZero,
            operation: DesignExtrudeOperation::NewBody,
            start: DesignExtrudeStart::ProfilePlane,
            solid_operation: true,
            first_side_target_ordinal: None,
            ..
        })
    )
}

pub(in crate::design) fn assign_slotted_extrude_face_targets(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
    slots: Option<[Option<u32>; 7]>,
) {
    let Some([None, None, None, Some(_), Some(second), Some(_), Some(first)]) = slots else {
        return;
    };
    if first == second || !allows_slotted_extrude_face_targets(scope) {
        return;
    }
    let mut targets = groups
        .iter_mut()
        .filter(|group| group.role() == DesignOperandRole::ROLE_0X5);
    let (Some(first_group), Some(second_group), None) =
        (targets.next(), targets.next(), targets.next())
    else {
        return;
    };
    if first_group.record_index == first
        && second_group.record_index == second
        && first_group.scope_reference_ordinal < second_group.scope_reference_ordinal
        && first_group.members().len() == 1
        && second_group.members().len() == 1
    {
        for group in [first_group, second_group] {
            group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
                encoding: DesignExtrudeFaceEncoding::SelectedStart,
                usage: DesignExtrudeFaceRole::Termination,
            };
        }
    }
}

/// Return the Extrude groups that select bodies to their source role.
///
/// A start-from-face Extrude reads every `0x5` group after the first as a
/// termination face run. A `0x5` group whose members are all body recipe
/// operands selects bodies, not faces, so it is not a face run.
pub(crate) fn release_extrude_body_target_groups(
    groups: &mut [DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
) {
    for group in groups.iter_mut() {
        let DesignConstructionOperandRole::ExtrudeFaces {
            encoding: DesignExtrudeFaceEncoding::SelectedStart,
            usage: DesignExtrudeFaceRole::Termination,
        } = group.operand_role
        else {
            continue;
        };
        let stream = native_stream(&group.id);
        let selects_bodies = !group.members().is_empty()
            && group.members().iter().enumerate().all(|(ordinal, member)| {
                u32::try_from(ordinal).is_ok_and(|ordinal| {
                    operands.iter().any(|operand| {
                        native_stream(&operand.id) == stream
                            && operand.scope_record_index == group.scope_record_index
                            && operand.owner.group() == Some((group.record_index, ordinal))
                            && operand.record_index() == member.value
                    })
                })
            });
        if selects_bodies {
            group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
        }
    }
}
