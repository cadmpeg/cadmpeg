// SPDX-License-Identifier: Apache-2.0

use crate::records::identity::Located;
use crate::records::topology::construction::{
    DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameDraft, DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole as SourceOperandRole;

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => {
            *slot = Some(DesignOffsetFacesOperation {
                distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
                distance_record_index: 300,
                distance_offset: Some(0),
            });
        }
        DesignScopePayloadMut::Thicken(slot) => {
            *slot = Some(DesignThickenOperation {
                signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: Some(0),
            });
        }
        DesignScopePayloadMut::Shell(slot) => {
            *slot = Some(DesignShellOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: Some(0),
                outward: true,
                outward_offset: Some(0),
            });
        }
        _ => panic!("unsupported direct-face scope fixture"),
    }
    scope
}

fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        scope_record_index: 12,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1000,
        role_offset: 1054,
        paired_byte_offset: 1125,
        id: "f3d:test:group#100".to_owned(),
        class_tag: "332".to_owned().try_into().expect("source class tag"),
        paired_class_tag: "259".to_owned().try_into().expect("source class tag"),
        members: [200]
            .into_iter()
            .zip([1026])
            .map(|(value, offset)| Located { value, offset })
            .collect(),
        lost_edge_references: Vec::new(),
        operand_role: DesignConstructionOperandRole::Other(SourceOperandRole::from_raw(role.raw())),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                closing_flag: false,
            },
        )
        .expect("source construction frame"),
    })
    .unwrap()
}

fn projected_operation(
    projected: Result<Option<FeatureDefinition>, CodecError>,
) -> FeatureOperation {
    let Some(FeatureDefinition::Operation(operation)) = projected.unwrap() else {
        panic!("expected a projected operation");
    };
    operation
}

#[test]
fn offset_faces_projects_native_group() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace { faces, .. } = projected_operation(
        super::super::project_offset_faces(&scope, &[], &[], std::slice::from_ref(&group)),
    ) else {
        panic!("expected a face move");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn thicken_projects_native_group() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    let FeatureOperation::Thicken { faces, .. } = projected_operation(
        super::super::project_thicken(&scope, &[], std::slice::from_ref(&group)),
    ) else {
        panic!("expected a thicken");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn shell_projects_native_face_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(bodies, None);
}

#[test]
fn shell_projects_native_body_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(bodies, Some(BodySelection::Native(group.id.clone())));
    assert_eq!(removed_faces, FaceSelection::Faces(Vec::new()));
}

fn move_face_scope(transform: [[f64; 4]; 4]) -> DesignParameterScope {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::MoveFace, 12);
    let DesignScopePayloadMut::MoveFace(slot) = scope.payload_mut() else {
        panic!("MoveFace payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: transform.try_into().unwrap(),
        transform_offset: Some(448),
        transform_record_index: 300,
        form: DesignMoveForm::Form5,
        form_offset: Some(443),
    });
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200, 300]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn move_face_projects_a_translation_of_its_group() {
    use cadmpeg_ir::features::FaceMotion;
    let scope = move_face_scope([
        [1., 0., 0., 0.],
        [0., 1., 0., -0.25],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace {
        faces,
        motion: FaceMotion::Translate {
            direction,
            distance,
        },
    } = projected_operation(super::super::project_move_face(
        &scope,
        std::slice::from_ref(&group),
        &[],
    ))
    else {
        panic!("expected a face translation");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(direction.y, -1.);
    assert_eq!(distance.literal().unwrap().get(), 2.5);
}

#[test]
fn move_face_stays_native_without_a_translation_or_its_exact_group() {
    let translation = [
        [1., 0., 0., 0.25],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let faces = group(DesignOperandRole::ROLE_0X10);
    let refused = |transform, groups: &[DesignConstructionOperandGroup]| {
        super::super::project_move_face(&move_face_scope(transform), groups, &[])
            .unwrap()
            .is_none()
    };
    assert!(!refused(translation, std::slice::from_ref(&faces)));
    assert!(refused(translation, &[]));
    assert!(refused(translation, &[group(DesignOperandRole::BODIES_A)]));
    assert!(refused(translation, &[faces.clone(), faces.clone()]));
    let mut other_member = faces.clone();
    other_member.record_index = 101;
    assert!(refused(translation, &[other_member]));
    for transform in [
        [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        [
            [0., 1., 0., 0.25],
            [-1., 0., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
    ] {
        assert!(refused(transform, std::slice::from_ref(&faces)));
    }
}

#[test]
fn move_face_stays_native_for_a_reflection() {
    let faces = group(DesignOperandRole::ROLE_0X10);
    for transform in [
        [
            [-1., 0., 0., 0.25],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.25],
            [0., 0., -1., 0.],
            [0., 0., 0., 1.],
        ],
    ] {
        assert!(super::super::project_move_face(
            &move_face_scope(transform),
            std::slice::from_ref(&faces),
            &[],
        )
        .unwrap()
        .is_none());
    }
}

#[test]
fn move_transform_projects_signed_rotation_translation_and_unique_body_group() {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Move, 12);
    let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
        panic!("Move payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: [
            [0., 1., 0., 7.],
            [-1., 0., 0., 11.],
            [0., 0., 1., 13.],
            [0., 0., 0., 1.],
        ]
        .try_into()
        .unwrap(),
        transform_offset: Some(448),
        transform_record_index: 71,
        form: DesignMoveForm::Form1,
        form_offset: Some(443),
    });
    let group = group(DesignOperandRole::BODIES_A);
    let Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation: Some(rotation),
        copies,
    })) = super::super::project_move(&scope, std::slice::from_ref(&group))
    else {
        panic!("MoveBody");
    };
    assert_eq!(bodies, BodySelection::Native(group.id.clone()));
    assert_eq!(
        translation.map(|component| component
            .literal()
            .expect("literal translation")
            .get()),
        [70., 110., 130.]
    );
    assert_eq!(
        rotation.origin.get(),
        cadmpeg_ir::math::Point3::new(0., 0., 0.)
    );
    assert_eq!(rotation.direction.z, -1.);
    assert!((rotation
        .angle
        .literal()
        .expect("literal angle")
        .get()
        - std::f64::consts::FRAC_PI_2)
        .abs()
        <= f64::EPSILON);
    assert_eq!(copies, cadmpeg_ir::Dimension::Literal(0));
    assert!(super::super::project_move(&scope, &[]).is_none());
    assert!(super::super::project_move(&scope, &[group.clone(), group.clone()]).is_none());
    let DesignScopePayloadMut::Move(Some(operation)) = scope.payload_mut() else {
        panic!("Move payload");
    };
    operation.transform = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
    .try_into()
    .unwrap();
    assert!(super::super::project_move(&scope, &[group]).is_none());
}
