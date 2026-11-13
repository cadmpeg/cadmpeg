use super::*;
use crate::design::decode::parameters::{parse_design_parameter_record, parse_parameter_owner};
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::design::test_support::{
    design_type, parameter_owner_frame, parameter_record, synthetic_feature_timelines,
};
use crate::records::entity_header::SegmentType;
use crate::records::feature::mesh_face_groups::DesignMeshFaceGroups;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::identity::{Located, ReferenceRun};
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::test_support::with_decode_context;

fn scope(combine: bool) -> DesignParameterScope {
    let construction = Some(DesignMeshFaceGroups {
        method_code: Some(Located {
            value: 19,
            offset: 54,
        }),
        source_mesh: Some(Located {
            value: 89,
            offset: 70,
        }),
        group_count: Some(Located {
            value: 0,
            offset: 80,
        }),
        ..Default::default()
    });
    let payload = if combine {
        DesignScopePayload::CombineFaceGroups(construction)
    } else {
        DesignScopePayload::GenerateFaceGroups(construction)
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:design-parameter-scope#0",
        payload,
        700,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![44, 47, 50, 888, 50]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = 400;
            draft.frame_length = 400;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn registration(scope: &DesignParameterScope) -> SegmentType {
    let combine = scope.kind() == DesignFeatureKind::CombineFaceGroups;
    SegmentType::try_new(
        "f3d:native/MetaStream.dat:design-type#0".into(),
        design_type(
            if combine {
                "8136D47E-3955-4682-8BD7-B353B7B45624"
            } else {
                "CA2AE956-FD81-4EBD-A601-F93A0F335722"
            },
            Some("90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"),
            if combine { 1 } else { 3 },
            "Fusion",
            vec![700],
        ),
    )
    .unwrap()
}

fn parameters() -> (Vec<DesignParameter>, Vec<DesignParameterOwner>) {
    let mut parameters = Vec::new();
    let mut owners = Vec::new();
    for (index, (role, unit, value)) in [
        ("Angle Threshold", Some("rad"), 0.73),
        ("Boundary Tolerance", Some("cm"), 0.0047),
        ("Minimum Face Group Size", None, 0.17),
    ]
    .into_iter()
    .enumerate()
    {
        let ordinal = u32::try_from(index).unwrap();
        let owner_index = 44 + ordinal * 3;
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(owner_index),
            &format!("control_{index}"),
            role,
            unit,
            &format!("arbitrary_{index}"),
            value,
        ))
        .unwrap();
        parameter.record_index = owner_index + 1;
        parameter.id = format!(
            "f3d:native/BulkStream.dat:design-parameter#{}",
            parameter.record_index
        );
        let owner = parse_parameter_owner(&parameter_owner_frame())
            .unwrap()
            .into_record("native/BulkStream.dat", 0)
            .unwrap();
        let mut wire = DesignParameterOwnerWire::from(owner);
        wire.id = format!("f3d:native/BulkStream.dat:design-parameter-owner#{index}");
        wire.byte_offset = u64::try_from(index).unwrap();
        wire.evaluated_value_offset += u64::try_from(index).unwrap();
        wire.record_index = owner_index;
        wire.parameter_record_index = owner_index + 1;
        wire.companion_record_index = owner_index + 2;
        wire.scope_record_index = 700;
        wire.local_ordinal = [9, 3, 7][index];
        wire.evaluated_value = value;
        parameters.push(parameter);
        owners.push(DesignParameterOwner::try_from(wire).unwrap());
    }
    (parameters, owners)
}

fn project_definition(
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
) -> (FeatureDefinition, Vec<LossNote>) {
    let mut losses = Vec::new();
    let parameters = parameters
        .iter()
        .map(|parameter| (99, parameter))
        .collect::<Vec<_>>();
    let definition = project(scope, &parameters, &[registration(scope)], &mut losses);
    (definition, losses)
}

#[test]
fn settings_survive_unknown_method_without_a_mesh_graph() {
    let scope = scope(false);
    let (parameters, _) = parameters();
    let (definition, losses) = project_definition(&scope, &parameters);
    let FeatureOperation::MeshGenerateFaceGroups { source, settings } = definition.operation()
    else {
        panic!("Generate");
    };
    assert!(matches!(
        source,
        MeshSelection::Native { members: None, .. }
    ));
    assert!(settings.method.is_none());
    assert!((settings.angle_threshold.unwrap().get() - 0.73).abs() < 1e-12);
    assert!((settings.boundary_tolerance.unwrap().get() - 0.047).abs() < 1e-12);
    assert!((settings.minimum_area_fraction.unwrap().get() - 0.17).abs() < 1e-12);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureFaceGroupsMethodUnmapped.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureFaceGroupsBoundaryDistanceAssumed.kind()));
    let (partial, _) = project_definition(&scope, &parameters[1..2]);
    let FeatureOperation::MeshGenerateFaceGroups { settings, .. } = partial.operation() else {
        panic!("Generate");
    };
    assert!(settings.angle_threshold.is_none());
    assert!(settings.boundary_tolerance.is_some());
    assert!(settings.minimum_area_fraction.is_none());
    for (code, expected) in [
        (0, MeshFaceGroupMethod::Fast),
        (1, MeshFaceGroupMethod::Accurate),
    ] {
        let mut known = scope.clone();
        if let crate::records::feature::scope::DesignScopePayloadMut::GenerateFaceGroups(Some(
            value,
        )) = known.payload_mut()
        {
            value.method_code.as_mut().unwrap().value = code;
        }
        let (definition, _) = project_definition(&known, &parameters);
        let FeatureOperation::MeshGenerateFaceGroups { settings, .. } = definition.operation()
        else {
            panic!("Generate");
        };
        assert_eq!(settings.method, Some(expected));
        assert!(
            settings.angle_threshold.is_some()
                && settings.boundary_tolerance.is_some()
                && settings.minimum_area_fraction.is_some()
        );
    }
    let mut missing = scope.clone();
    missing.id.clear();
    let (definition, _) = project_definition(&missing, &parameters);
    let FeatureOperation::MeshGenerateFaceGroups { source, settings } = definition.operation()
    else {
        panic!("Generate");
    };
    assert!(matches!(source, MeshSelection::Unresolved { .. }));
    assert!(
        settings.angle_threshold.is_some()
            && settings.boundary_tolerance.is_some()
            && settings.minimum_area_fraction.is_some()
    );
}

#[test]
fn invalid_or_competing_setting_roles_preserve_valid_siblings() {
    let scope = scope(false);
    let (mut parameters, _) = parameters();
    parameters[2].try_set_evaluated_value(1.7).unwrap();
    let (definition, losses) = project_definition(&scope, &parameters);
    let FeatureOperation::MeshGenerateFaceGroups { settings, .. } = definition.operation() else {
        panic!("Generate");
    };
    assert!(settings.angle_threshold.is_some());
    assert!(settings.boundary_tolerance.is_some());
    assert!(settings.minimum_area_fraction.is_none());
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureFaceGroupsSettingUnassigned.kind()));
    for (owner, angle_present) in [(899, true), (888, false)] {
        let mut competing = parse_design_parameter_record(&parameter_record(
            Some(owner),
            "other_angle",
            "Angle Threshold",
            Some("rad"),
            "another_control",
            0.81,
        ))
        .unwrap();
        competing.record_index = owner + 1;
        competing.id = format!("f3d:native/BulkStream.dat:design-parameter#{}", owner + 1);
        let mut candidates = parameters.clone();
        candidates.push(competing);
        let (definition, _) = project_definition(&scope, &candidates);
        let FeatureOperation::MeshGenerateFaceGroups { settings, .. } = definition.operation()
        else {
            panic!("Generate");
        };
        assert_eq!(settings.angle_threshold.is_some(), angle_present);
        assert!(settings.boundary_tolerance.is_some());
    }
}

#[test]
fn typed_projection_preserves_all_parameters_and_scope_reference_slots() {
    let scope = scope(false);
    let (parameters, owners) = parameters();
    let timelines = synthetic_feature_timelines(std::slice::from_ref(&scope));
    let types = [registration(&scope)];
    let projected = with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes: std::slice::from_ref(&scope),
                timelines: &timelines,
                design_types: &types,
                ..Default::default()
            },
        )
    })
    .unwrap();
    let feature = &projected.features[0];
    assert!(matches!(
        feature.evaluation.definition().operation(),
        FeatureOperation::MeshGenerateFaceGroups { .. }
    ));
    assert!(feature.evaluation.outputs().is_empty());
    for (ordinal, target) in [44, 47, 50, 888, 50].into_iter().enumerate() {
        assert_eq!(
            feature
                .source_properties
                .get(format!("reference:{ordinal}").as_str()),
            Some(&target.to_string())
        );
    }
    assert_eq!(projected.parameters.len(), 3);
    assert!(projected
        .parameters
        .iter()
        .all(|parameter| parameter.owner.as_ref() == Some(&feature.id)));
    let mut fallback = scope.clone();
    fallback
        .try_edit(|draft| {
            draft.payload = DesignFeatureKind::native("GenerateFaceGroups")
                .try_into()
                .unwrap()
        })
        .unwrap();
    let baseline = with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes: std::slice::from_ref(&fallback),
                timelines: &timelines,
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(projected.parameters, baseline.parameters);
    assert_eq!(feature.id, baseline.features[0].id);
    assert_eq!(feature.native_ref, baseline.features[0].native_ref);
    assert_eq!(feature.ordinal, baseline.features[0].ordinal);
    assert_eq!(feature.suppressed, baseline.features[0].suppressed);
    assert_eq!(feature.dependencies, baseline.features[0].dependencies);
}

#[test]
fn combine_retains_order_and_reports_unrepresentable_runs() {
    for groups in [
        vec!["second", "first"],
        vec!["repeat", "repeat"],
        vec![""],
        vec![],
    ] {
        let mut scope = scope(true);
        if let crate::records::feature::scope::DesignScopePayloadMut::CombineFaceGroups(Some(
            value,
        )) = scope.payload_mut()
        {
            value.group_count = Some(Located {
                value: u32::try_from(groups.len()).unwrap(),
                offset: 83,
            });
            value.groups = groups
                .iter()
                .map(|value| Located {
                    value: (*value).to_owned(),
                    offset: 90,
                })
                .collect();
        }
        let (definition, losses) = project_definition(&scope, &[]);
        let FeatureOperation::MeshCombineFaceGroups {
            source: MeshSelection::Native { members, .. },
        } = definition.operation()
        else {
            panic!("Combine");
        };
        if groups == ["second", "first"] {
            assert_eq!(members.as_ref().unwrap().as_slice(), groups);
        } else {
            assert!(members.is_none());
            if !groups.is_empty() {
                assert!(losses.iter().any(|loss| loss.code
                    == F3dLossCode::FeatureFaceGroupsMemberRunUnrepresentable.kind()));
            }
        }
    }
    let scope = scope(true);
    let mut losses = Vec::new();
    assert!(matches!(
        project(&scope, &[], &[], &mut losses).operation(),
        FeatureOperation::MeshCombineFaceGroups { .. }
    ));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureFaceGroupsTypeUnverified.kind()));
    let mut missing = scope;
    missing.id.clear();
    let mut losses = Vec::new();
    assert!(matches!(
        project(&missing, &[], &[], &mut losses).operation(),
        FeatureOperation::MeshCombineFaceGroups {
            source: MeshSelection::Unresolved { .. }
        }
    ));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureFaceGroupsSourceUnavailable.kind()));
}
