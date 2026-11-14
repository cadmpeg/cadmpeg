// SPDX-License-Identifier: Apache-2.0
//! Chamfer projection: the edge groups the source states and the parameter
//! lanes they pair with.
use super::{
    localized_fillet_group, localized_fillet_group_with_role, localized_fillet_owner,
    localized_fillet_parameter,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

fn localized_chamfer_scope() -> DesignParameterScope {
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            label: None,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101],
                vec![185, 196],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Chamfer
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}
#[test]
fn a_chamfer_without_an_edge_group_keeps_its_distance_and_scope_selector() {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec},
        EdgeSelection,
    };

    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];

    let crate::design::feature_project::ProjectedParameterDesign {
        features: refused, ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        refused[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::Distance { distance },
            }] if selection == &scope.id && (distance.get() - 1.0).abs() < 1e-12)
    ));

    let group = localized_fillet_group(100, 0, vec![200]);
    let crate::design::feature_project::ProjectedParameterDesign {
        features: accepted, ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes,
                construction_groups: std::slice::from_ref(&group),
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        accepted[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::Distance { distance },
            }] if selection == &group.id && distance.get() == 1.0)
    ));
}

#[test]
fn a_face_group_takes_the_one_specification_of_the_edge_group() {
    use cadmpeg_ir::features::edge_treatments::{ChamferGroup, ChamferSpec};

    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];
    let groups = [
        localized_fillet_group(100, 0, vec![200]),
        localized_fillet_group_with_role(101, 2, vec![201], DesignOperandRole::ROLE_0X10),
    ];
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &groups,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: projected, ..
    }) = features[0].evaluation.definition()
    else {
        panic!("expected a chamfer");
    };
    assert_eq!(projected.as_slice().len(), 2);
    assert!(projected.as_slice().iter().all(|group| matches!(
        group,
        ChamferGroup {
            spec: ChamferSpec::Distance { distance },
            ..
        } if distance.get() == 1.0
    )));
}

#[test]
fn an_incomplete_group_pairing_keeps_chamfer_selectors_and_parameters() {
    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];
    let groups = [
        localized_fillet_group(100, 0, vec![200]),
        localized_fillet_group(101, 2, vec![201]),
        localized_fillet_group_with_role(102, 4, vec![202], DesignOperandRole::ROLE_0X10),
    ];
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &groups,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if groups.as_slice().len() == 3
                && groups.as_slice().iter().all(|group| group.spec.is_unresolved())
    ));
    let parameters: Vec<crate::records::parameters::DesignParameter> =
        serde_json::from_str(&features[0].source_properties["chamfer:parameters"]).unwrap();
    assert_eq!(parameters.len(), 1);
}

#[test]
fn another_role_beside_a_face_group_keeps_the_reading_and_names_the_assumption() {
    let scope = localized_chamfer_scope();
    let parameters = [localized_fillet_parameter(
        10,
        11,
        "Distance",
        Some("mm"),
        0.1,
    )];
    let owners = [localized_fillet_owner(10, 11, 0)];
    let project = |role: DesignOperandRole| {
        let groups = [
            localized_fillet_group_with_role(100, 0, vec![200], role),
            localized_fillet_group_with_role(101, 2, vec![201], DesignOperandRole::ROLE_0X10),
        ];
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &parameters,
                    owners: &owners,
                    scopes,
                    construction_groups: &groups,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        })
    };
    let assumed = crate::loss::F3dLossCode::ChamferEdgeRoleAssumed.kind();
    let other = project(DesignOperandRole::BODIES_A);
    assert!(matches!(
        other.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { .. })
    ));
    assert!(other
        .projection_losses
        .iter()
        .any(|loss| loss.code == assumed));
    let edge = project(DesignOperandRole::BODIES_B);
    assert!(!edge
        .projection_losses
        .iter()
        .any(|loss| loss.code == assumed));
}

fn mixed_scope() -> DesignParameterScope {
    use crate::records::feature::chamfer_set::{
        DesignChamferCornerType, DesignChamferSet, DesignChamferSetList, DesignChamferSetListForm,
        DesignChamferSetSpec,
    };
    let mut scope = localized_chamfer_scope();
    let specs = [
        DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index: 10,
        },
        DesignChamferSetSpec::TwoDistances {
            first_owner_record_index: 20,
            second_owner_record_index: 30,
        },
        DesignChamferSetSpec::DistanceAngle {
            distance_owner_record_index: 40,
            angle_owner_record_index: 50,
        },
    ];
    if let crate::records::feature::scope::DesignScopePayloadMut::Chamfer(payload) =
        scope.payload_mut()
    {
        payload.sets = Some(DesignChamferSetList {
            form: DesignChamferSetListForm::Current,
            corner_type: DesignChamferCornerType::Miter,
            entry_count: 3,
            sets: (0_u32..)
                .zip(specs)
                .map(|(ordinal, spec)| DesignChamferSet {
                    record_index: 500 + ordinal,
                    byte_offset: 2000 + 100 * u64::from(ordinal),
                    frame_length: if ordinal == 0 { 83 } else { 93 },
                    group_record_index: 100 + ordinal,
                    spec,
                    auxiliary_record_index: 600 + ordinal,
                    entry_count: 1,
                    tangent_chain: false,
                })
                .collect(),
        });
    }
    scope
}

fn mixed_parameters() -> (
    Vec<crate::records::parameters::DesignParameter>,
    Vec<crate::records::parameters::DesignParameterOwner>,
) {
    let mut parameters = Vec::new();
    let mut owners = Vec::new();
    for (ordinal, (owner, kind, unit, value)) in (0_u32..).zip([
        (10, "Distance", "mm", 0.15),
        (20, "Distance 1", "mm", 0.2),
        (30, "Distance 2", "mm", 0.3),
        (40, "Distance", "mm", 0.4),
        (50, "Angle", "rad", std::f64::consts::FRAC_PI_6),
    ]) {
        parameters.push(localized_fillet_parameter(
            owner,
            owner + 1,
            kind,
            Some(unit),
            value,
        ));
        owners.push(localized_fillet_owner(owner, owner + 1, ordinal));
    }
    (parameters, owners)
}

fn project_mixed(
    parameters: &[crate::records::parameters::DesignParameter],
    owners: &[crate::records::parameters::DesignParameterOwner],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> crate::design::feature_project::ProjectedParameterDesign {
    let scope = mixed_scope();
    crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: parameters,
                owners,
                scopes,
                construction_groups: groups,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
    })
}

#[test]
fn mixed_chamfer_sets_preserve_each_form_dimension_and_selector() {
    use cadmpeg_ir::features::{edge_treatments::ChamferSpec, EdgeSelection};
    let (mut parameters, mut owners) = mixed_parameters();
    parameters.reverse();
    owners.reverse();
    let groups = (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect::<Vec<_>>();
    let projected = project_mixed(&parameters, &owners, &groups);
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups: actual, .. }) =
        projected.features[0].evaluation.definition()
    else {
        panic!("chamfer");
    };
    let [first, second, third] = actual.as_slice() else {
        panic!("three sets");
    };
    assert!(
        matches!(first.spec, ChamferSpec::Distance { distance } if (distance.get() - 1.5).abs() < 1e-12)
    );
    assert!(
        matches!(second.spec, ChamferSpec::TwoDistances { first, second } if (first.get() - 2.0).abs() < 1e-12 && (second.get() - 3.0).abs() < 1e-12)
    );
    assert!(
        matches!(third.spec, ChamferSpec::DistanceAngle { distance, angle } if (distance.get() - 4.0).abs() < 1e-12 && (angle.get() - std::f64::consts::FRAC_PI_6).abs() < 1e-12)
    );
    for (actual, native) in actual.as_slice().iter().zip(&groups) {
        assert!(matches!(&actual.edges, EdgeSelection::Native(id) if id == &native.id));
    }
    assert!(!projected
        .projection_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ChamferDefinitionUnresolved.kind()));
    let saved = &projected.features[0].source_properties;
    let saved_parameters: Vec<crate::records::parameters::DesignParameter> =
        serde_json::from_str(&saved["chamfer:parameters"]).unwrap();
    assert_eq!(saved_parameters.len(), 5);
    let saved_sets: crate::records::feature::chamfer_set::DesignChamferSetList =
        serde_json::from_str(&saved["chamfer:sets"]).unwrap();
    assert_eq!(
        saved_sets.corner_type,
        crate::records::feature::chamfer_set::DesignChamferCornerType::Miter
    );
}

#[test]
fn mixed_chamfer_missing_or_duplicated_owners_preserve_other_sets() {
    let (parameters, owners) = mixed_parameters();
    let groups = (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect::<Vec<_>>();
    for duplicated in [false, true] {
        for absent in 0..owners.len() {
            let mut changed = owners.clone();
            if duplicated {
                changed.push(owners[absent].clone());
            } else {
                changed.remove(absent);
            }
            let projected = project_mixed(&parameters, &changed, &groups);
            let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
                projected.features[0].evaluation.definition()
            else {
                panic!("chamfer");
            };
            let affected = match absent {
                0 => 0,
                1 | 2 => 1,
                _ => 2,
            };
            for (ordinal, group) in groups.as_slice().iter().enumerate() {
                assert_eq!(group.spec.is_unresolved(), ordinal == affected);
            }
            assert_eq!(
                projected
                    .projection_losses
                    .iter()
                    .filter(|loss| loss.code
                        == crate::loss::F3dLossCode::ChamferDefinitionUnresolved.kind())
                    .count(),
                1
            );
        }
    }
}

#[test]
fn mixed_chamfer_missing_or_duplicated_groups_keep_dimensions() {
    use cadmpeg_ir::features::EdgeSelection;
    let (parameters, owners) = mixed_parameters();
    let groups = (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect::<Vec<_>>();
    for duplicated in [false, true] {
        let mut changed = groups.clone();
        if duplicated {
            changed.push(groups[1].clone());
        } else {
            changed.remove(1);
        }
        let projected = project_mixed(&parameters, &owners, &changed);
        let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
            projected.features[0].evaluation.definition()
        else {
            panic!("chamfer");
        };
        assert_eq!(groups.as_slice().len(), 3);
        assert!(groups
            .as_slice()
            .iter()
            .all(|group| !group.spec.is_unresolved()));
        assert!(
            matches!(&groups.as_slice()[1].edges, EdgeSelection::Native(id) if id == &mixed_scope().id)
        );
        assert_eq!(
            projected
                .projection_losses
                .iter()
                .filter(
                    |loss| loss.code == crate::loss::F3dLossCode::ChamferSelectionUnresolved.kind()
                )
                .count(),
            1
        );
    }
}

#[test]
fn mixed_chamfer_foreign_owner_and_broken_backlink_are_unresolved() {
    let (parameters, owners) = mixed_parameters();
    let groups = (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect::<Vec<_>>();
    for foreign in [false, true] {
        let mut changed = parameters.clone();
        if foreign {
            changed[2].id = "f3d:other/BulkStream.dat:parameter#31".into();
        } else {
            changed[2]
                .try_set_source(
                    crate::records::parameters::DesignParameterSource::new(
                        "Distance 2".to_owned(),
                        Some(999),
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let projected = project_mixed(&changed, &owners, &groups);
        let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
            projected.features[0].evaluation.definition()
        else {
            panic!("chamfer");
        };
        assert!(groups.as_slice()[1].spec.is_unresolved());
        assert!(!groups.as_slice()[0].spec.is_unresolved());
        assert!(!groups.as_slice()[2].spec.is_unresolved());
    }
}

#[test]
fn mixed_chamfer_invalid_scalars_keep_the_form_and_other_dimensions() {
    let (parameters, owners) = mixed_parameters();
    let groups = (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect::<Vec<_>>();
    for (index, kind, unit, value) in [
        (0, "Distance", "mm", 0.0),
        (2, "Distance 2", "mm", -1.0),
        (3, "Distance", "mm", 1e308),
        (4, "Angle", "rad", 0.0),
        (4, "Angle", "rad", std::f64::consts::PI),
        (4, "Angle", "mm", 0.5),
    ] {
        let mut changed = parameters.clone();
        let owner = owners[index].record_index();
        changed[index] = localized_fillet_parameter(owner, owner + 1, kind, Some(unit), value);
        let projected = project_mixed(&changed, &owners, &groups);
        let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
            projected.features[0].evaluation.definition()
        else {
            panic!("chamfer");
        };
        let affected = match index {
            0 => 0,
            1 | 2 => 1,
            _ => 2,
        };
        for (ordinal, group) in groups.as_slice().iter().enumerate() {
            assert_eq!(group.spec.is_unresolved(), ordinal == affected);
        }
    }
}
