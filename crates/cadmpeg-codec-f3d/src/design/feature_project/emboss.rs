// SPDX-License-Identifier: Apache-2.0
//! Emboss projection.

use crate::design::face_resolve::resolved_face_group;
use crate::design::feature_project::design_angle_unit;
use crate::design::feature_project::design_length_unit;
use crate::design::feature_project::scope_properties::emboss_length;
use crate::design::feature_project::scope_properties::emboss_parameter;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterKind;
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::CodecError;

pub(super) fn emboss_group<'a>(
    scope: &DesignParameterScope,
    groups: &'a [DesignConstructionOperandGroup],
    record_index: u32,
) -> Option<&'a DesignConstructionOperandGroup> {
    let mut matching = groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.record_index == record_index
    });
    let group = matching.next()?;
    matching.next().is_none().then_some(group)
}

pub(super) fn emboss_scalar_owner_is_proven(
    scope: &DesignParameterScope,
    parameter: &DesignParameter,
    owners: &[DesignParameterOwner],
) -> bool {
    let Some(reference) = scope.emboss_construction().and_then(|construction| {
        construction
            .scalar_owners()
            .into_iter()
            .find_map(|(reference, role)| {
                (role == parameter.source_kind_str()).then_some(reference.value)
            })
    }) else {
        return false;
    };
    let mut matching = owners.iter().filter(|owner| {
        native_stream(owner.id()) == native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && owner.record_index() == reference
    });
    let Some(owner) = matching.next() else {
        return false;
    };
    matching.next().is_none()
        && parameter.owner_record_index() == Some(reference)
        && parameter.record_index == owner.parameter_record_index()
        && native_stream(&parameter.id) == native_stream(&scope.id)
        && parameter.kind() == DesignParameterKind::Feature
        && parameter.evaluated_value() == owner.evaluated_value()
}

pub(super) fn project_emboss(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::profiles::PlanarProfileRef;
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::surface_inputs::WrapMode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let source_scope = scope.id.as_str();
    let construction = scope.emboss_construction();
    let profile_source = construction
        .and_then(|value| emboss_group(scope, groups, value.profile_group.value))
        .map_or_else(|| source_scope.to_owned(), |group| group.id.clone());
    let profile = PlanarProfileRef::Native(profile_source);
    let target = if let Some(group) =
        construction.and_then(|value| emboss_group(scope, groups, value.face_group.value))
    {
        resolved_face_group(group, face_operands)?
            .unwrap_or_else(|| FaceSelection::Native(group.id.clone()))
    } else {
        FaceSelection::Native(source_scope.to_owned())
    };
    for role in [
        "Depth",
        "Horizontal Distance",
        "Vertical Distance",
        "Rotation Angle",
    ] {
        if let Some(parameter) = emboss_parameter(scope, parameters, role) {
            let unit_matches = parameter.unit().is_some_and(|unit| {
                if role == "Rotation Angle" {
                    design_angle_unit(unit.value.as_str())
                } else {
                    design_length_unit(unit.value.as_str())
                }
            });
            if !unit_matches {
                losses.push(crate::loss::F3dLossCode::EmbossScalarUnitAssumed.note(format!(
                    "Emboss scope {} interprets {} in its role's native units despite a missing or conflicting display-unit token (DR-121).",
                    scope.id, role
                )));
            }
            if !emboss_scalar_owner_is_proven(scope, parameter, owners) {
                losses.push(crate::loss::F3dLossCode::EmbossScalarOwnerAssumed.note(format!(
                    "Emboss scope {} retains its {} parameter independently of an unresolved or inconsistent scalar-owner association (DR-121).",
                    scope.id, role
                )));
            }
        }
    }
    losses.push(crate::loss::F3dLossCode::EmbossProfileSelectionNative.note(format!(
        "Emboss scope {} retains its native sketch-profile reference because the selected contour mapping is unresolved.",
        scope.id
    )));
    if matches!(target, FaceSelection::Native(_)) {
        losses.push(crate::loss::F3dLossCode::EmbossTargetSelectionNative.note(format!(
            "Emboss scope {} retains its target-face reference because the selected target mapping is unresolved.",
            scope.id
        )));
    }
    let depth = emboss_parameter(scope, parameters, "Depth").and_then(emboss_length);
    let mode = depth
        .and_then(|signed| {
            cadmpeg_ir::scalar::PositiveLength::new(signed.get().abs()).map(|magnitude| {
                if signed.get() > 0.0 {
                    WrapMode::Emboss { depth: magnitude }
                } else {
                    WrapMode::Deboss { depth: magnitude }
                }
            })
        })
        .unwrap_or(WrapMode::Unresolved { depth });
    if matches!(mode, WrapMode::Emboss { .. } | WrapMode::Deboss { .. }) {
        losses.push(crate::loss::F3dLossCode::EmbossDepthSignAssumed.note(format!(
            "Emboss scope {} interprets the signed Depth parameter as the native tool depth: positive adds material and negative removes material (DR-121).",
            scope.id
        )));
    } else {
        losses.push(
            crate::loss::F3dLossCode::EmbossDepthUnresolved.note(format!(
            "Emboss scope {} has no unique nonzero valid Depth value, so its effect remains unresolved.",
            scope.id
        )),
        );
    }
    if parameters.iter().any(|(_, parameter)| {
        matches!(
            parameter.source_kind_str(),
            "Horizontal Distance" | "Vertical Distance" | "Rotation Angle"
        )
    }) {
        losses.push(crate::loss::F3dLossCode::EmbossPlacementNative.note(format!(
            "Emboss scope {} retains horizontal, vertical, and rotation values and expressions as native placement properties.",
            scope.id
        )));
    }
    Ok(FeatureDefinition::Operation(FeatureOperation::Wrap {
        profile,
        face: target,
        mode,
    }))
}
