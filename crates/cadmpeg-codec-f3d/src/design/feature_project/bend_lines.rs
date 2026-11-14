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
    scalar::Angle,
};

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureDefinition, CodecError> {
    use crate::loss::F3dLossCode;

    let references: Vec<_> = scope.reference_members().values().copied().collect();
    let stream = native_stream(&scope.id);
    let groups: Vec<_> = inputs
        .construction_groups
        .iter()
        .filter(|group| {
            group.scope_record_index == scope.record_index && native_stream(&group.id) == stream
        })
        .collect();
    let line_group_for_owner = |owner_record_index| {
        references
            .iter()
            .position(|index| Some(*index) == owner_record_index)
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
            })
    };
    let mut construction = SheetMetalBendLinesConstruction::default();
    let mut angles: Vec<_> = parameters
        .iter()
        .filter(|(_, parameter)| parameter.source_kind() == "BendAngle")
        .map(|(_, parameter)| (parameter.owner_record_index(), design_angle(parameter)))
        .collect();
    if let Some(stream) = stream {
        for parameter in inputs.native.iter().filter(|parameter| {
            parameter.source_kind() == "BendAngle" && native_stream(&parameter.id) == Some(stream)
        }) {
            let Some(owner_index) = parameter.owner_record_index() else {
                continue;
            };
            if line_group_for_owner(Some(owner_index)).is_none_or(|group| {
                group.role()
                    != crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X21
            }) || parameters.iter().any(|(_, bound)| {
                bound.record_index == parameter.record_index
                    && native_stream(&bound.id) == Some(stream)
            }) || inputs.owners.iter().any(|owner| {
                owner.record_index() == owner_index && native_stream(owner.id()) == Some(stream)
            }) {
                continue;
            }
            angles.push((Some(owner_index), design_angle(parameter)));
            losses.push(F3dLossCode::BendAngleOwnerBacklinkAssumed.note(format!(
                "Bend scope {} binds BendAngle {} through owner backlink {} and its preceding singleton role-0x21 group/member pair while the owner frame is absent.",
                scope.id, parameter.id, owner_index
            )));
        }
        for owner in inputs.owners.iter().filter(|owner| {
            owner.scope_record_index() == scope.record_index
                && native_stream(owner.id()) == Some(stream)
        }) {
            let owner_index = owner.record_index();
            if parameters.iter().any(|(_, parameter)| {
                parameter.owner_record_index() == Some(owner_index)
                    || parameter.record_index == owner.parameter_record_index()
            }) || line_group_for_owner(Some(owner_index)).is_none_or(|group| {
                group.role()
                    != crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X21
            }) || inputs.native.iter().any(|parameter| {
                parameter.record_index == owner.parameter_record_index()
                    && native_stream(&parameter.id) == Some(stream)
            }) {
                continue;
            }
            angles.push((Some(owner_index), Angle::new(owner.evaluated_value().get())));
            losses.push(F3dLossCode::BendCachedOwnerAngleAssumed.note(format!(
                "Bend scope {} reads the cached scalar of owner {} following a singleton role-0x21 group/member pair as its missing BendAngle definition in radians.",
                scope.id, owner_index
            )));
        }
    }
    angles.sort_by_key(|(owner, _)| {
        let position = references.iter().position(|index| Some(*index) == *owner);
        (position.is_none(), position)
    });
    for (owner, angle) in angles {
        let group = line_group_for_owner(owner);
        construction.bends.push(SheetMetalLineBend {
            line: Some(PathRef::Native(
                group.map_or_else(|| scope.id.clone(), |group| group.id.clone()),
            )),
            angle,
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

    fn angular_scope_and_group() -> (
        DesignParameterScope,
        crate::records::topology::construction::DesignConstructionOperandGroup,
    ) {
        use crate::records::{
            identity::{Located, ReferenceRun},
            references::DesignClassTag,
            topology::{
                construction::{
                    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
                    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
                    DesignConstructionOperandRole,
                },
                extrude_selection::DesignOperandRole,
            },
        };
        let mut scope = DesignParameterScope::empty(
            "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0",
            DesignFeatureKind::Bend,
            12,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![10, 13, 44]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
            id: "f3d:test/Design1/BulkStream.dat:design-construction-operand-group#0".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            record_index: 10,
            byte_offset: 0,
            class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
            members: vec![Located {
                value: 13,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X21),
            role_offset: 0,
            paired_class_tag: DesignClassTag::try_from("000".to_owned()).unwrap(),
            paired_byte_offset: 0,
        })
        .unwrap();
        (scope, group)
    }

    fn cached_angular_owner() -> crate::records::parameters::DesignParameterOwner {
        let mut bytes = crate::design::test_support::parameter_owner_frame();
        bytes[40..48].copy_from_slice(&(-0.75_f64).to_le_bytes());
        crate::design::decode::parameters::parse_parameter_owner(&bytes)
            .unwrap()
            .into_record("test/Design1/BulkStream.dat", 0)
            .unwrap()
    }

    fn angular_definition() -> crate::records::parameters::DesignParameter {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(44),
            "-0.75 rad",
            "BendAngle",
            Some("rad"),
            "d1",
            -0.75,
        ))
        .unwrap();
        parameter.record_index = 45;
        parameter.id = "f3d:test/Design1/BulkStream.dat:design-parameter#0".into();
        parameter
    }

    #[test]
    fn bend_reads_cached_angular_owner_when_definition_is_missing() {
        let (scope, group) = angular_scope_and_group();
        let owners = [cached_angular_owner()];
        let inputs = ProjectInputs {
            owners: &owners,
            construction_groups: std::slice::from_ref(&group),
            ..Default::default()
        };
        let mut losses = Vec::new();
        let definition = project(&scope, &[], &inputs, &mut losses).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalBendLines { construction }) =
            definition
        else {
            panic!("expected cached angular input");
        };
        assert_eq!(construction.bends.len(), 1);
        assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
        assert_eq!(
            construction.bends[0].line,
            Some(cadmpeg_ir::features::PathRef::Native(group.id))
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::BendCachedOwnerAngleAssumed.kind()));
    }

    #[test]
    fn bend_reads_angular_definition_when_owner_frame_is_missing() {
        let (scope, group) = angular_scope_and_group();
        let parameters = [angular_definition()];
        let inputs = ProjectInputs {
            native: &parameters,
            construction_groups: std::slice::from_ref(&group),
            ..Default::default()
        };
        let mut losses = Vec::new();
        let definition = project(&scope, &[], &inputs, &mut losses).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalBendLines { construction }) =
            definition
        else {
            panic!("expected backlink angular input");
        };
        assert_eq!(construction.bends.len(), 1);
        assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
        assert_eq!(
            construction.bends[0].line,
            Some(cadmpeg_ir::features::PathRef::Native(group.id))
        );
        assert!(losses.iter().any(
            |loss| loss.code == crate::loss::F3dLossCode::BendAngleOwnerBacklinkAssumed.kind()
        ));
    }

    #[test]
    fn bend_recovery_preserves_one_bound_angular_input() {
        let (scope, group) = angular_scope_and_group();
        let parameters = [angular_definition()];
        let owners = [cached_angular_owner()];
        let inputs = ProjectInputs {
            native: &parameters,
            owners: &owners,
            construction_groups: std::slice::from_ref(&group),
            ..Default::default()
        };
        let mut losses = Vec::new();
        let definition = project(&scope, &[(2, &parameters[0])], &inputs, &mut losses).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::SheetMetalBendLines { construction }) =
            definition
        else {
            panic!("expected one bound angular input");
        };
        assert_eq!(construction.bends.len(), 1);
        assert_eq!(construction.bends[0].angle.unwrap().get(), -0.75);
        assert!(!losses.iter().any(|loss| {
            loss.code == crate::loss::F3dLossCode::BendAngleOwnerBacklinkAssumed.kind()
                || loss.code == crate::loss::F3dLossCode::BendCachedOwnerAngleAssumed.kind()
        }));
    }
}
