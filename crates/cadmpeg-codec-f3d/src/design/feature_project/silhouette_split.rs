// SPDX-License-Identifier: Apache-2.0

use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::surfaces::SilhouetteSplitMode;
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureDirection3, FeatureOperation};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;

pub(super) fn project(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<LossNote>,
) -> Option<FeatureDefinition> {
    let DesignScopePayload::SilhouetteSplit(Some(operation)) = scope.payload() else {
        return None;
    };
    let mode = match operation.mode_word {
        0 => SilhouetteSplitMode::FacesOnly,
        1 => SilhouetteSplitMode::ShelledBody,
        2 => SilhouetteSplitMode::SolidBody,
        word => {
            losses.push(
                F3dLossCode::FeatureSilhouetteSplitModeUnresolved.note(format!(
                    "Silhouette Split {} has an unassigned result mode {word}.",
                    scope.id
                )),
            );
            SilhouetteSplitMode::Unresolved
        }
    };
    let direction = operation.carrier.as_ref().and_then(|carrier| {
        let [x, y, z] = carrier.direction.map(FiniteReal::get);
        FeatureDirection3::new(Vector3::new(x, y, z))
    });
    if direction.is_none() {
        losses.push(
            F3dLossCode::FeatureSilhouetteSplitDirectionUnresolved.note(format!(
                "Silhouette Split {} has no representable model-space direction.",
                scope.id
            )),
        );
    }
    let target = match target_group(scope, groups) {
        Some(group) => BodySelection::Native(group.id.clone()),
        None => {
            losses.push(
                F3dLossCode::FeatureSilhouetteSplitTargetUnresolved.note(format!(
                    "Silhouette Split {} has no unique complete target body group.",
                    scope.id
                )),
            );
            BodySelection::Unresolved
        }
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::SilhouetteSplit {
            target,
            direction,
            mode,
        },
    ))
}

fn target_group<'a>(
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
) -> Option<&'a DesignConstructionOperandGroup> {
    let stream = native_stream(&scope.id)?;
    let mut members = scope.reference_members().values().copied().skip(2);
    let group_index = members.next()?;
    let body_index = members.next()?;
    if members.next().is_some() {
        return None;
    }
    let mut groups = groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.scope_reference_ordinal == 2
            && group.record_index == group_index
    });
    let group = groups.next()?;
    (groups.next().is_none()
        && group.role() == DesignOperandRole::BODIES_A
        && group
            .members()
            .iter()
            .map(|member| member.value)
            .eq([body_index]))
    .then_some(group)
}
