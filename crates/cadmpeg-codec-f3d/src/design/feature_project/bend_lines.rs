use super::{
    design_angle, design_positive_length, native_stream, project_face_selection, ProjectInputs,
};
use crate::records::{feature::scope::DesignParameterScope, parameters::DesignParameter};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        sheet_metal::SheetMetalBendLinesConstruction, sheet_metal::SheetMetalLineBend,
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
    },
    report::loss::LossNote,
};

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    use crate::loss::F3dLossCode;

    let references: Vec<_> = scope.reference_members().values().copied().collect();
    let groups: Vec<_> = inputs
        .construction_groups
        .iter()
        .filter(|group| {
            group.scope_record_index == scope.record_index
                && native_stream(&group.id) == native_stream(&scope.id)
        })
        .collect();
    let mut construction = SheetMetalBendLinesConstruction::default();
    let mut angles: Vec<_> = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind() == "BendAngle")
        .collect();
    angles.sort_by_key(|(_, parameter)| {
        let position = references
            .iter()
            .position(|index| Some(*index) == parameter.owner_record_index());
        (position.is_none(), position)
    });
    for (_, parameter) in angles {
        let group = references
            .iter()
            .position(|index| Some(*index) == parameter.owner_record_index())
            .and_then(|at| at.checked_sub(2))
            .and_then(|at| {
                let index = references.get(at)?;
                let member = references.get(at + 1)?;
                groups.iter().find(|group| {
                    group.record_index == *index
                        && group
                            .members()
                            .iter()
                            .map(|entry| entry.value)
                            .eq([*member])
                })
            });
        construction.bends.push(SheetMetalLineBend {
            line: Some(PathRef::Native(
                group.map_or_else(|| scope.id.clone(), |group| group.id.clone()),
            )),
            angle: design_angle(parameter),
        });
    }
    losses.push(F3dLossCode::BendLineOwnerOrderAssumed.note(format!(
        "Bend scope {} associates each BendAngle owner with the preceding ordered group/member pair; unresolved line selections retain the scope reference.",
        scope.id
    )));
    let mut radii = parameters.iter().filter_map(|(_, parameter)| {
        (parameter.source_kind() == "BendRadius")
            .then(|| design_positive_length(parameter))
            .flatten()
    });
    construction.inside_radius = radii.next();
    if radii.next().is_some() {
        losses.push(F3dLossCode::BendRadiusFirstAssumed.note(format!(
            "Bend scope {} uses its first decoded BendRadius override.",
            scope.id
        )));
    }
    if construction.inside_radius.is_none() {
        losses.push(F3dLossCode::BendRuleRadiusUnresolved.note(format!(
            "Bend scope {} retains its sheet-metal rule radius as unresolved.",
            scope.id
        )));
    }
    if construction.bends.is_empty() || construction.bends.iter().any(|bend| bend.angle.is_none()) {
        losses.push(F3dLossCode::BendAngleUnresolved.note(format!(
            "Bend scope {} has an absent or undecoded angular definition.",
            scope.id
        )));
    }
    losses.push(F3dLossCode::BendStationaryRoleAssumed.note(format!(
        "Bend scope {} reads its role-0x10 construction group as the stationary face selection.",
        scope.id
    )));
    let mut stationary = groups.iter().filter(|group| {
        group.role() == crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X10
    });
    let first = stationary.next();
    construction.stationary = Some(if stationary.next().is_none() {
        match first {
            Some(group) => {
                project_face_selection(scope, group, inputs.face_operands, inputs.histories)?
            }
            None => FaceSelection::Native(scope.id.clone()),
        }
    } else {
        FaceSelection::Native(scope.id.clone())
    });
    if matches!(construction.stationary, Some(FaceSelection::Native(_))) {
        losses.push(F3dLossCode::BendStationarySelectionNative.note(format!(
            "Bend scope {} retains its stationary face selection through its native reference.",
            scope.id
        )));
    }
    Ok(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalBendLines { construction },
    ))
}

#[cfg(test)]
mod tests {
    use super::project;
    use crate::design::{
        decode::parameters::parse_design_parameter_record, feature_project::ProjectInputs,
        test_support::parameter_record,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    #[test]
    fn bend_preserves_signed_angles_and_radius_from_synthetic_parameters() {
        let negative = parse_design_parameter_record(&parameter_record(
            Some(100),
            "-30 deg",
            "BendAngle",
            Some("deg"),
            "d1",
            -0.5,
        ))
        .unwrap();
        let positive = parse_design_parameter_record(&parameter_record(
            Some(200),
            "60 deg",
            "BendAngle",
            Some("deg"),
            "d2",
            1.0,
        ))
        .unwrap();
        let radius = parse_design_parameter_record(&parameter_record(
            Some(300),
            "2.5 mm",
            "BendRadius",
            Some("mm"),
            "d3",
            0.25,
        ))
        .unwrap();
        let scope = DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0",
            DesignFeatureKind::Bend,
            10,
        );
        let mut losses = Vec::new();
        let definition = project(
            &scope,
            &[(0, &negative), (1, &positive), (2, &radius)],
            &ProjectInputs::default(),
            &mut losses,
        )
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalBendLines { construction }) =
            definition
        else {
            panic!("expected sheet-metal bend lines");
        };
        assert_eq!(construction.bends.len(), 2);
        assert_eq!(construction.bends[0].angle.unwrap().get(), -0.5);
        assert_eq!(construction.bends[1].angle.unwrap().get(), 1.0);
        assert_eq!(construction.inside_radius.unwrap().get(), 2.5);
        assert!(construction.bends.iter().all(|bend| bend.line.is_some()));
    }

    #[test]
    fn bend_without_parameter_definitions_still_projects() {
        let scope = DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0",
            DesignFeatureKind::Bend,
            10,
        );
        let mut losses = Vec::new();
        let definition = project(&scope, &[], &ProjectInputs::default(), &mut losses).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalBendLines { construction }) =
            definition
        else {
            panic!("expected partial sheet-metal bend lines");
        };
        assert!(construction.bends.is_empty());
        assert!(construction.inside_radius.is_none());
        assert!(construction.stationary.is_some());
        assert_eq!(losses.len(), 5);
    }
}
