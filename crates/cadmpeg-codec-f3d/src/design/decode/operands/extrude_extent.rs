// SPDX-License-Identifier: Apache-2.0
//! Admission of Extrude extents after their operand groups have been decoded.

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::{DesignExtrudeExtent, DesignExtrudeStart};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::{DesignExtrudeFaceRole, DesignOperandRole};
use cadmpeg_ir::report::loss::LossNote;

pub(crate) fn report_missing_terminations(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let Some(prologue) = scope.extrude_prologue() else {
            continue;
        };
        if prologue.extent() != Some(DesignExtrudeExtent::OneSidedToFace) {
            continue;
        }
        let required = 1;
        let scope_groups = || {
            groups.iter().filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
        };
        let start_count = scope_groups()
            .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Start))
            .count();
        let termination_count = scope_groups()
            .filter(|group| group.extrude_face_role() == Some(DesignExtrudeFaceRole::Termination))
            .count();
        let shape_target = scope_groups().any(|group| {
            group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
        });
        if start_count != usize::from(prologue.start() == DesignExtrudeStart::FromFace)
            || termination_count >= required
            || shape_target
        {
            continue;
        }
        losses.push(F3dLossCode::FeatureExtrudeTerminationMissing.note(format!(
            "Extrude scope {} requires {required} termination group(s) and stores {termination_count}; the feature stays native.",
            scope.id
        )));
    }
    losses
}

#[cfg(test)]
mod tests;
