// SPDX-License-Identifier: Apache-2.0
//! Chamfer projection: the edge groups the source states and the parameter
//! lanes they pair with.
use super::{
    localized_fillet_group, localized_fillet_group_with_role, localized_fillet_owner,
    localized_fillet_parameter,
};
use crate::loss::F3dLossCode::{
    self, ChamferAngleReferenceAssumed, ChamferCornerTreatmentUnresolved,
    ChamferDefinitionUnresolved, ChamferParameterKindAssumed, ChamferParameterOwnerAssumed,
    ChamferParameterUnitAssumed, ChamferParameterValueAssumed, ChamferReferenceFaceAssumed,
    ChamferSelectionUnresolved, ChamferTangentChainAssumed,
};
use crate::records::feature::chamfer_set::{DesignChamferCornerType, DesignChamferSetList};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::parameters::{
    DesignParameter, DesignParameterOwner, DesignParameterOwnerWire, DesignParameterSource,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::edge_treatments::{ChamferForm, ChamferGroup, ChamferSpec};
use cadmpeg_ir::features::EdgeSelection;
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
    let parameters: Vec<DesignParameter> =
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
    let assumed = F3dLossCode::ChamferEdgeRoleAssumed.kind();
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
    let specs = [
        serde_json::json!({"kind":"equal_distance", "distance_owner_record_index":10}),
        serde_json::json!({"kind":"two_distances", "first_owner_record_index":20,
            "second_owner_record_index":30}),
        serde_json::json!({"kind":"distance_angle", "distance_owner_record_index":40,
            "angle_owner_record_index":50}),
    ];
    let sets = (0_u32..)
        .zip(specs)
        .map(|(ordinal, spec)| {
            serde_json::json!({
                "record_index":500+ordinal, "byte_offset":2000+100*u64::from(ordinal),
                "frame_length":if ordinal==0 {83} else {93}, "group_record_index":100+ordinal,
                "spec":spec, "auxiliary_record_index":600+ordinal, "entry_count":1,
                "tangent_chain":false
            })
        })
        .collect::<Vec<_>>();
    let mut scope = localized_chamfer_scope();
    if let DesignScopePayloadMut::Chamfer(payload) = scope.payload_mut() {
        payload.sets = Some(
            serde_json::from_value(serde_json::json!({
                "form":"current", "corner_type":"miter", "entry_count":3, "sets":sets
            }))
            .unwrap(),
        );
    }
    scope
}

fn mixed_parameters() -> (Vec<DesignParameter>, Vec<DesignParameterOwner>) {
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
        let mut wire =
            DesignParameterOwnerWire::from(localized_fillet_owner(owner, owner + 1, ordinal));
        wire.evaluated_value = value;
        owners.push(DesignParameterOwner::try_from(wire).unwrap());
    }
    (parameters, owners)
}

fn mixed_groups() -> Vec<DesignConstructionOperandGroup> {
    (0..3)
        .map(|ordinal| localized_fillet_group(100 + ordinal, ordinal, vec![200 + ordinal]))
        .collect()
}

fn project_mixed(
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
) -> crate::design::feature_project::ProjectedParameterDesign {
    project_scope(&mixed_scope(), parameters, owners, groups)
}

fn project_scope(
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
) -> crate::design::feature_project::ProjectedParameterDesign {
    crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(scope);
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
    let (mut parameters, mut owners) = mixed_parameters();
    parameters.reverse();
    owners.reverse();
    let groups = mixed_groups();
    let projected = project_mixed(&parameters, &owners, &groups);
    let actual = projected_groups(&projected);
    let [first, second, third] = actual else {
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
    for (actual, native) in actual.iter().zip(&groups) {
        assert!(matches!(&actual.edges, EdgeSelection::Native(id) if id == &native.id));
    }
    assert_eq!(loss_count(&projected, ChamferDefinitionUnresolved), 0);
    for (code, count) in [
        (ChamferReferenceFaceAssumed, 2),
        (ChamferAngleReferenceAssumed, 1),
    ] {
        assert_eq!(loss_count(&projected, code), count);
    }
}

#[test]
fn mixed_chamfer_recovers_missing_owners_and_isolates_duplicated_owners() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for duplicated in [false, true] {
        for absent in 0..owners.len() {
            let mut changed = owners.clone();
            if duplicated {
                changed.push(owners[absent].clone());
            } else {
                changed.remove(absent);
            }
            let projected = project_mixed(&parameters, &changed, &groups);
            let groups = projected_groups(&projected);
            let affected = match absent {
                0 => 0,
                1 | 2 => 1,
                _ => 2,
            };
            for (ordinal, group) in groups.iter().enumerate() {
                assert_eq!(
                    group.spec.is_unresolved(),
                    duplicated && ordinal == affected
                );
            }
            let code = if duplicated {
                ChamferDefinitionUnresolved
            } else {
                ChamferParameterOwnerAssumed
            };
            assert_eq!(loss_count(&projected, code), 1);
            let retained: Vec<DesignParameter> = serde_json::from_str(
                &projected.features[0].source_properties["chamfer:parameters"],
            )
            .unwrap();
            assert_eq!(retained.len(), parameters.len());
        }
    }
}

#[test]
fn mixed_chamfer_missing_or_duplicated_groups_keep_dimensions() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for duplicated in [false, true] {
        let mut changed = groups.clone();
        if duplicated {
            changed.push(groups[1].clone());
        } else {
            changed.remove(1);
        }
        let projected = project_mixed(&parameters, &owners, &changed);
        let groups = projected_groups(&projected);
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|group| !group.spec.is_unresolved()));
        assert!(
            matches!(&groups[1].edges, EdgeSelection::Native(id) if id == "f3d:native/BulkStream.dat:design-record-header#2100")
        );
        assert_eq!(loss_count(&projected, ChamferSelectionUnresolved), 1);
    }
}

#[test]
fn mixed_chamfer_invalid_scalars_keep_the_form_and_other_dimensions() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for (index, kind, unit, value) in [
        (0, "Distance", "mm", 0.0),
        (2, "Distance 2", "mm", -1.0),
        (3, "Distance", "mm", 1e308),
        (4, "Angle", "rad", 0.0),
        (4, "Angle", "rad", std::f64::consts::PI),
    ] {
        let mut changed = parameters.clone();
        let owner = owners[index].record_index();
        changed[index] = localized_fillet_parameter(owner, owner + 1, kind, Some(unit), value);
        let projected = project_mixed(&changed, &owners, &groups);
        let groups = projected_groups(&projected);
        let affected = match index {
            0 => 0,
            1 | 2 => 1,
            _ => 2,
        };
        for (ordinal, group) in groups.iter().enumerate() {
            assert_eq!(group.spec.is_unresolved(), ordinal == affected);
        }
    }
}

#[test]
fn mixed_chamfer_keeps_forms_when_owner_and_parameter_definitions_are_absent() {
    let (parameters, owners) = mixed_parameters();
    let groups = [localized_fillet_group(100, 0, vec![200])];
    let projected = project_mixed(&parameters[..1], &owners[..1], &groups);
    let groups = projected_groups(&projected);
    assert!(matches!(groups, [
        first,
        ChamferGroup { spec: ChamferSpec::Unresolved { form: Some(ChamferForm::TwoDistances) }, .. },
        ChamferGroup { spec: ChamferSpec::Unresolved { form: Some(ChamferForm::DistanceAngle) }, .. },
    ] if matches!(first.spec, ChamferSpec::Distance { distance } if (distance.get() - 1.5).abs() < 1e-12)));
    let retained: Vec<DesignParameter> =
        serde_json::from_str(&projected.features[0].source_properties["chamfer:parameters"])
            .unwrap();
    assert_eq!(retained.len(), 1);
}

fn projected_groups(
    projected: &crate::design::feature_project::ProjectedParameterDesign,
) -> &[ChamferGroup] {
    match projected.features[0].evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => groups,
        _ => panic!("chamfer"),
    }
}

fn loss_count(
    projected: &crate::design::feature_project::ProjectedParameterDesign,
    code: F3dLossCode,
) -> usize {
    projected
        .projection_losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

#[test]
fn mixed_chamfer_missing_parameters_use_owner_scalars_without_borrowing_foreign_values() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for foreign in [false, true] {
        let mut changed = parameters.clone();
        if foreign {
            changed[2].id = "f3d:other/BulkStream.dat:parameter#31".into();
        } else {
            changed.remove(2);
        }
        let projected = project_mixed(&changed, &owners, &groups);
        assert!(
            matches!(projected_groups(&projected)[1].spec, ChamferSpec::TwoDistances { first, second }
            if (first.get() - 2.0).abs() < 1e-12 && (second.get() - 3.0).abs() < 1e-12)
        );
        assert_eq!(loss_count(&projected, ChamferParameterValueAssumed), 1);
    }
}

#[test]
fn mixed_chamfer_isolates_foreign_scopes_duplicate_parameters_and_broken_backlinks() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for case in 0..3 {
        let mut parameters = parameters.clone();
        let mut owners = owners.clone();
        if case == 0 {
            parameters.push(parameters[2].clone());
        } else if case == 1 {
            let mut wire = DesignParameterOwnerWire::from(owners[2].clone());
            wire.scope_record_index = 99;
            owners[2] = DesignParameterOwner::try_from(wire).unwrap();
        } else {
            parameters[2]
                .try_set_source(
                    DesignParameterSource::new("Distance 2".to_owned(), Some(999), None).unwrap(),
                )
                .unwrap();
        }
        let projected = project_mixed(&parameters, &owners, &groups);
        assert!(projected_groups(&projected)[1].spec.is_unresolved());
        assert!(!projected_groups(&projected)[0].spec.is_unresolved());
        assert!(!projected_groups(&projected)[2].spec.is_unresolved());
        assert_eq!(loss_count(&projected, ChamferDefinitionUnresolved), 1);
    }
}

#[test]
fn mixed_chamfer_explicit_slots_name_kind_and_unit_assumptions() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for (kind, unit, code) in [
        ("Other", Some("rad"), ChamferParameterKindAssumed),
        ("Angle", Some("mm"), ChamferParameterUnitAssumed),
        ("Angle", None, ChamferParameterUnitAssumed),
    ] {
        let mut changed = parameters.clone();
        changed[4] = localized_fillet_parameter(50, 51, kind, unit, std::f64::consts::FRAC_PI_6);
        let projected = project_mixed(&changed, &owners, &groups);
        assert!(
            matches!(projected_groups(&projected)[2].spec, ChamferSpec::DistanceAngle { distance, angle }
            if (distance.get() - 4.0).abs() < 1e-12 && (angle.get() - std::f64::consts::FRAC_PI_6).abs() < 1e-12)
        );
        assert_eq!(loss_count(&projected, code), 1);
    }
}

#[test]
fn mixed_chamfer_set_flags_and_corners_keep_metadata_and_specific_losses() {
    let (parameters, owners) = mixed_parameters();
    let groups = mixed_groups();
    for corner in [
        DesignChamferCornerType::Chamfer,
        DesignChamferCornerType::Miter,
        DesignChamferCornerType::Blend,
    ] {
        for tangent in [false, true] {
            let mut scope = mixed_scope();
            if let DesignScopePayloadMut::Chamfer(payload) = scope.payload_mut() {
                let sets = payload.sets.as_mut().unwrap();
                sets.corner_type = corner;
                sets.sets[0].tangent_chain = tangent;
            }
            let projected = project_scope(&scope, &parameters, &owners, &groups);
            assert_eq!(
                loss_count(&projected, ChamferCornerTreatmentUnresolved),
                usize::from(corner != DesignChamferCornerType::Chamfer)
            );
            assert_eq!(
                loss_count(&projected, ChamferTangentChainAssumed),
                usize::from(tangent)
            );
            let retained: DesignChamferSetList =
                serde_json::from_str(&projected.features[0].source_properties["chamfer:sets"])
                    .unwrap();
            assert_eq!(retained.corner_type, corner);
            assert_eq!(retained.sets[0].tangent_chain, tangent);
        }
    }
}

#[test]
fn ownerless_chamfer_fixed_scalar_fallback_preserves_the_referenced_dimensions() {
    let mut scope = mixed_scope();
    if let DesignScopePayloadMut::Chamfer(payload) = scope.payload_mut() {
        let sets = payload.sets.as_mut().unwrap();
        sets.sets = vec![sets.sets[1].clone()];
        payload.fixed = Some(
            serde_json::from_value(serde_json::json!({
                "kind":"two_distances", "first":{"value":0.2,"record_index":20,"value_offset":2020},
                "second":{"value":0.3,"record_index":30,"value_offset":2030}
            }))
            .unwrap(),
        );
    }
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![101, 20, 30]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups = mixed_groups();
    let projected = project_scope(&scope, &[], &[], &groups[1..2]);
    assert!(
        matches!(projected_groups(&projected)[0].spec, ChamferSpec::TwoDistances { first, second }
        if (first.get() - 2.0).abs() < 1e-12 && (second.get() - 3.0).abs() < 1e-12)
    );
    assert_eq!(loss_count(&projected, ChamferParameterValueAssumed), 2);
}

#[test]
fn mixed_chamfer_additional_parameters_remain_available_with_a_named_loss() {
    let (mut parameters, mut owners) = mixed_parameters();
    let extra = localized_fillet_parameter(60, 61, "Extra", None, 0.125);
    parameters.push(extra);
    owners.push(localized_fillet_owner(60, 61, 5));
    let projected = project_mixed(&parameters, &owners, &mixed_groups());
    assert!(projected_groups(&projected)
        .iter()
        .all(|group| !group.spec.is_unresolved()));
    assert_eq!(loss_count(&projected, ChamferDefinitionUnresolved), 1);
    let retained: Vec<DesignParameter> =
        serde_json::from_str(&projected.features[0].source_properties["chamfer:parameters"])
            .unwrap();
    assert_eq!(retained.len(), 6);
    assert!((retained[5].evaluated_value().get() - 0.125).abs() < 1e-12);
}

#[test]
fn explicit_equal_chamfer_does_not_consume_an_unclaimed_angle() {
    let mut scope = mixed_scope();
    if let DesignScopePayloadMut::Chamfer(payload) = scope.payload_mut() {
        payload.sets.as_mut().unwrap().sets.truncate(1);
    }
    let (all_parameters, all_owners) = mixed_parameters();
    let parameters = [
        all_parameters[0].clone(),
        localized_fillet_parameter(60, 61, "Angle", Some("rad"), std::f64::consts::FRAC_PI_6),
    ];
    let owners = [all_owners[0].clone(), localized_fillet_owner(60, 61, 1)];
    let projected = project_scope(&scope, &parameters, &owners, &mixed_groups()[..1]);
    assert!(matches!(projected_groups(&projected)[0].spec,
        ChamferSpec::Distance { distance } if (distance.get() - 1.5).abs() < 1e-12));
    assert_eq!(loss_count(&projected, ChamferDefinitionUnresolved), 1);
    let retained: DesignChamferSetList =
        serde_json::from_str(&projected.features[0].source_properties["chamfer:sets"]).unwrap();
    assert_eq!(retained.sets.len(), 1);
}

#[test]
fn ungrouped_chamfers_name_reference_face_and_angle_assumptions() {
    let scope = localized_chamfer_scope();
    let (parameters, owners) = mixed_parameters();
    for (range, angle_count) in [(1..3, 0), (3..5, 1)] {
        let projected = project_scope(&scope, &parameters[range.clone()], &owners[range], &[]);
        assert!(!projected_groups(&projected)[0].spec.is_unresolved());
        assert_eq!(loss_count(&projected, ChamferSelectionUnresolved), 1);
        assert_eq!(loss_count(&projected, ChamferReferenceFaceAssumed), 1);
        assert_eq!(
            loss_count(&projected, ChamferAngleReferenceAssumed),
            angle_count
        );
    }
}

#[test]
fn fixed_two_distance_chamfer_names_reference_face_assumption() {
    let mut scope = localized_chamfer_scope();
    if let DesignScopePayloadMut::Chamfer(payload) = scope.payload_mut() {
        payload.fixed = Some(
            serde_json::from_value(serde_json::json!({
                "kind":"two_distances", "first":{"value":0.2,"record_index":20,"value_offset":2020},
                "second":{"value":0.3,"record_index":30,"value_offset":2030}
            }))
            .unwrap(),
        );
    }
    let projected = project_scope(&scope, &[], &[], &[]);
    assert!(matches!(projected_groups(&projected)[0].spec,
        ChamferSpec::TwoDistances { first, second }
        if (first.get() - 2.0).abs() < 1e-12 && (second.get() - 3.0).abs() < 1e-12));
    assert_eq!(loss_count(&projected, ChamferReferenceFaceAssumed), 1);
    assert_eq!(loss_count(&projected, ChamferSelectionUnresolved), 1);
}

#[test]
fn unresolved_chamfer_does_not_charge_resolved_orientation() {
    let scope = localized_chamfer_scope();
    let parameters = [
        localized_fillet_parameter(10, 11, "Distance 1", Some("mm"), 0.2),
        localized_fillet_parameter(20, 21, "Distance 1", Some("mm"), -0.1),
        localized_fillet_parameter(30, 31, "Distance 2", Some("mm"), 0.3),
        localized_fillet_parameter(40, 41, "Distance 2", Some("mm"), 0.4),
    ];
    let owners = [
        localized_fillet_owner(10, 11, 0),
        localized_fillet_owner(20, 21, 1),
        localized_fillet_owner(30, 31, 2),
        localized_fillet_owner(40, 41, 3),
    ];
    let projected = project_scope(&scope, &parameters, &owners, &mixed_groups()[..2]);
    assert!(projected_groups(&projected)
        .iter()
        .all(|group| group.spec.is_unresolved()));
    assert_eq!(loss_count(&projected, ChamferReferenceFaceAssumed), 0);
    assert_eq!(loss_count(&projected, ChamferAngleReferenceAssumed), 0);
}
