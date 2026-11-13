use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::entity_header::SegmentType;
use crate::records::feature::mesh_face_groups::registered_kind;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::DesignParameter;
use cadmpeg_ir::features::mesh::{
    MeshFaceGroupGenerationSettings, MeshFaceGroupMethod, MeshSelection,
};
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, NativeSelections, SelectionReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Fraction, NonNegativeLength};

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    types: &[SegmentType],
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let metadata = native_stream(&scope.id)
        .and_then(|stream| stream.strip_suffix("BulkStream.dat"))
        .map(|prefix| format!("{prefix}MetaStream.dat"));
    let mut registrations = types.iter().filter(|info| {
        native_stream(info.id()) == metadata.as_deref()
            && info
                .entities
                .values()
                .any(|entity| *entity == u64::from(scope.record_index))
    });
    let registered = registrations.next().and_then(|info| registered_kind(info))
        == Some(scope.kind())
        && registrations.next().is_none();
    if !registered {
        losses.push(F3dLossCode::FeatureFaceGroupsTypeUnverified.note(format!(
            "{} retains {} intent with an unverified scope registration.",
            scope.id,
            scope.kind_name()
        )));
    }
    let construction = scope.mesh_face_groups();
    if let Some(source) = construction.and_then(|value| value.source_mesh.as_ref()) {
        let mut targets = types.iter().filter(|info| {
            native_stream(info.id()) == metadata.as_deref()
                && info
                    .entities
                    .values()
                    .any(|entity| *entity == u64::from(source.value))
        });
        let verified = targets.next().is_some_and(|info| {
            info.type_guid
                .as_str()
                .eq_ignore_ascii_case(crate::records::mesh::MESH_GUID_TYPE_GUID)
                && info.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case(crate::records::mesh::MESH_GUID_BASE_TYPE_GUID)
                })
                && info.version == crate::records::mesh::MESH_GUID_TYPE_VERSION
                && info.module == "ParaMesh"
        }) && targets.next().is_none();
        if !verified {
            losses.push(F3dLossCode::FeatureFaceGroupsSourceTargetUnverified.note(format!(
                "{} retains mesh source reference {} with a missing, ambiguous or unsupported target registration.", scope.id, source.value
            )));
        }
    }
    if construction.is_none_or(|value| value.source_mesh.is_none() || value.group_count.is_none()) {
        losses.push(F3dLossCode::FeatureFaceGroupsPrefixUnparsed.note(format!(
            "{} retains partial mesh construction; its source reference or group count is unavailable.", scope.id
        )));
    }
    let mut members = None;
    if let Some(value) = construction {
        if let Some(count) = &value.group_count {
            let values = value
                .groups
                .iter()
                .map(|group| group.value.clone())
                .collect::<Vec<_>>();
            if usize::try_from(count.value).ok() != Some(values.len()) {
                losses.push(
                    F3dLossCode::FeatureFaceGroupsMemberRunUnrepresentable.note(format!(
                        "{} retains {} of {} declared native group selectors.",
                        scope.id,
                        values.len(),
                        count.value
                    )),
                );
            } else if !values.is_empty() {
                match NativeSelections::try_from(values) {
                    Ok(values) => {
                        members = Some(values);
                        losses.push(F3dLossCode::FeatureFaceGroupsGroupRolesAssumed.note(format!(
                            "{} reads its ordered UUID run as selected input face groups; historical triangle membership remains native.", scope.id
                        )));
                    }
                    Err(_) => losses.push(F3dLossCode::FeatureFaceGroupsMemberRunUnrepresentable.note(format!(
                        "{} retains repeated or blank group selectors in its native construction.", scope.id
                    ))),
                }
            }
        }
    }
    let source = match SelectionReference::try_from(scope.id.clone()) {
        Ok(native) => {
            losses.push(F3dLossCode::FeatureFaceGroupsSourceAssumed.note(format!(
                "{} uses its native scope as the mesh selection expression; source mesh reference {:?} retains its native history state.",
                scope.id, construction.and_then(|value| value.source_mesh.as_ref()).map(|reference| reference.value)
            )));
            MeshSelection::Native { native, members }
        }
        Err(_) => {
            losses.push(F3dLossCode::FeatureFaceGroupsSourceUnavailable.note("Mesh face-group construction retains its originating feature because its native source identity is unavailable."));
            MeshSelection::Unresolved {
                feature: crate::ids::neutral_feature_id(scope),
            }
        }
    };
    if scope.kind() == DesignFeatureKind::CombineFaceGroups {
        return FeatureDefinition::Operation(FeatureOperation::MeshCombineFaceGroups { source });
    }
    let method = match construction
        .and_then(|value| value.method_code.as_ref())
        .map(|field| field.value)
    {
        Some(code @ (0 | 1)) => {
            losses.push(F3dLossCode::FeatureFaceGroupsMethodAssumed.note(format!(
                "{} reads generation method {code} as {}.",
                scope.id,
                if code == 0 { "Fast" } else { "Accurate" }
            )));
            Some(if code == 0 {
                MeshFaceGroupMethod::Fast
            } else {
                MeshFaceGroupMethod::Accurate
            })
        }
        code => {
            losses.push(F3dLossCode::FeatureFaceGroupsMethodUnmapped.note(format!(
                "{} retains unassigned generation method {code:?}; independent parameter settings remain available.", scope.id
            )));
            None
        }
    };
    let mut settings = MeshFaceGroupGenerationSettings {
        method,
        ..Default::default()
    };
    let is_referenced = |parameter: &DesignParameter| {
        parameter.owner_record_index().is_some_and(|owner| {
            scope
                .reference_members()
                .values()
                .any(|reference| *reference == owner)
        })
    };
    for (_, parameter) in parameters {
        let role = parameter.source_kind_str();
        let same_role = parameters
            .iter()
            .filter(|(_, candidate)| {
                candidate.source_kind_str() == role && is_referenced(candidate)
            })
            .count();
        let reciprocal = is_referenced(parameter);
        let unit = parameter.unit().map(|field| field.value.as_str());
        let value = parameter.evaluated_value().get();
        let assigned = if same_role == 1 && reciprocal {
            match role {
                "Angle Threshold" if unit.is_some_and(super::design_angle_unit) => {
                    settings.angle_threshold = Angle::new(value);
                    settings.angle_threshold.is_some()
                }
                "Boundary Tolerance" if unit.is_some_and(super::design_length_unit) => {
                    settings.boundary_tolerance =
                        NonNegativeLength::new(value * super::DESIGN_LENGTH_TO_MILLIMETRE);
                    if settings.boundary_tolerance.is_some() {
                        losses.push(F3dLossCode::FeatureFaceGroupsBoundaryDistanceAssumed.note(format!(
                            "{} reads parameter {} as absolute boundary tolerance in millimetres from its stored length unit; relative mesh-extent normalization remains unresolved.", scope.id, parameter.id
                        )));
                    }
                    settings.boundary_tolerance.is_some()
                }
                "Minimum Face Group Size" if unit.is_none() => {
                    settings.minimum_area_fraction = Fraction::new(value);
                    settings.minimum_area_fraction.is_some()
                }
                _ => false,
            }
        } else {
            false
        };
        if !assigned {
            losses.push(F3dLossCode::FeatureFaceGroupsSettingUnassigned.note(format!(
                "{} retains parameter {} ({role}) with its expression, unit and scalar; its role, owner uniqueness or quantity is unassigned.", scope.id, parameter.id
            )));
        }
    }
    FeatureDefinition::Operation(FeatureOperation::MeshGenerateFaceGroups { source, settings })
}

#[cfg(test)]
mod tests;
