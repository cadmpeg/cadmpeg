// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignModifyOperation, DesignMoveForm, DesignMoveOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

fn group(record_index: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:group#{record_index}"),
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "359",
        "members": [record_index + 1],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "261",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn modify_scope(transform: [[f64; 4]; 4], listed: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Modify, 12);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                71, 50, 51, 52, 53, 54, 55, 56, 57,
            ]);
            draft.frame_length = 320;
            draft.paired_byte_offset = 320;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::Modify(slot) = scope.payload_mut() else {
        panic!("Modify payload");
    };
    *slot = Some(DesignModifyOperation {
        motion: DesignMoveOperation {
            transform: transform.try_into().unwrap(),
            transform_offset: 48,
            transform_record_index: 71,
            form: DesignMoveForm::Form0,
            form_offset: 43,
            transform_reference_offset: Some(321),
            prefix: None,
        },
        body_group_record_indices: listed,
        body_count_offset: 332,
    });
    scope
}

const TRANSLATION: [[f64; 4]; 4] = [
    [1., 0., 0., 1.],
    [0., 1., 0., 1.3],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

#[test]
fn modify_projects_a_move_of_its_listed_body_groups() {
    let scope = modify_scope(TRANSLATION, vec![54, 56]);
    let groups = [
        group(50, DesignOperandRole::BODIES_A),
        group(52, DesignOperandRole::BODIES_A),
        group(54, DesignOperandRole::BODIES_A),
        group(56, DesignOperandRole::BODIES_A),
    ];
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation,
        copies,
    })) = super::super::project_modify(&scope, &groups, &mut losses)
    else {
        panic!("MoveBody");
    };
    assert_eq!(
        bodies,
        BodySelection::NativeSet(
            cadmpeg_ir::features::NativeSelections::try_from(vec![
                "f3d:test:group#54".to_owned(),
                "f3d:test:group#56".to_owned(),
            ])
            .unwrap()
        )
    );
    let translation = translation.get();
    assert_eq!(translation.x, 10.);
    assert!((translation.y - 13.).abs() <= 1e-12);
    assert_eq!(translation.z, 0.);
    assert_eq!(rotation, None);
    assert_eq!(copies, 0);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureModifyUnlistedGroupsAssumed.kind()
    );
    assert_eq!(
        losses[0].message,
        "Modify scope f3d:test:scope#12 moves the bodies of its listed body groups [54, 56]; its unlisted body groups [50, 52] are assumed to select the same bodies (DR-374)."
    );
}

#[test]
fn modify_with_one_listed_group_selects_that_group() {
    let half_turn = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., -1., 0.],
        [0., 0., 0., 1.],
    ];
    let scope = modify_scope(half_turn, vec![54]);
    let group = group(54, DesignOperandRole::BODIES_A);
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        rotation: Some(rotation),
        ..
    })) = super::super::project_modify(&scope, std::slice::from_ref(&group), &mut losses)
    else {
        panic!("MoveBody");
    };
    assert_eq!(bodies, BodySelection::Native(group.id.clone()));
    assert!((rotation.angle.get() - std::f64::consts::PI).abs() <= 1e-12);
    assert_eq!(rotation.direction.y.abs(), 1.);
}

#[test]
fn modify_stays_native_without_each_listed_body_group_or_with_a_reflection() {
    let mut losses = Vec::new();
    let scope = modify_scope(TRANSLATION, vec![54, 56]);
    assert!(super::super::project_modify(
        &scope,
        &[group(54, DesignOperandRole::BODIES_A)],
        &mut losses
    )
    .is_none());
    assert!(super::super::project_modify(
        &scope,
        &[
            group(54, DesignOperandRole::BODIES_A),
            group(56, DesignOperandRole::ROLE_0X10),
        ],
        &mut losses
    )
    .is_none());
    let reflection = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let scope = modify_scope(reflection, vec![54]);
    assert!(super::super::project_modify(
        &scope,
        &[group(54, DesignOperandRole::BODIES_A)],
        &mut losses
    )
    .is_none());
    assert!(losses.is_empty());
}

#[test]
fn modify_keeps_its_move_when_a_table_position_breaks_the_body_group_run() {
    let scope = modify_scope(TRANSLATION, vec![54]);
    let mut losses = Vec::new();
    let projected = super::super::project_modify(
        &scope,
        &[
            group(52, DesignOperandRole::BODIES_A),
            group(54, DesignOperandRole::BODIES_A),
        ],
        &mut losses,
    );
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Operation(
            FeatureOperation::MoveBody { .. }
        ))
    ));
    let codes = losses
        .iter()
        .map(|note| note.code.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        vec![
            crate::loss::F3dLossCode::FeatureModifyUnlistedGroupsAssumed.kind(),
            crate::loss::F3dLossCode::FeatureModifyTableAssumed.kind(),
        ]
    );
    assert_eq!(
        losses[1].message,
        "Modify scope f3d:test:scope#12 stores ordered references [50] where its table holds single-member body groups, each followed by its member; the MoveBody assumes that they leave the moved bodies unchanged (DR-374)."
    );
}

#[test]
fn modify_keeps_its_move_when_it_commits_a_history_state() {
    let mut scope = modify_scope(TRANSLATION, vec![54]);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let mut losses = Vec::new();
    assert!(super::super::project_modify(
        &scope,
        &[
            group(50, DesignOperandRole::BODIES_A),
            group(52, DesignOperandRole::BODIES_A),
            group(54, DesignOperandRole::BODIES_A),
            group(56, DesignOperandRole::BODIES_A),
        ],
        &mut losses,
    )
    .is_some());
    assert_eq!(losses.len(), 2);
    assert_eq!(
        losses[1].code,
        crate::loss::F3dLossCode::FeatureModifyStateAssumed.kind()
    );
    assert_eq!(
        losses[1].message,
        "Modify scope f3d:test:scope#12 commits history state 7; the MoveBody assumes that the state leaves the motion of the transform record unchanged (DR-374)."
    );
}
