// SPDX-License-Identifier: Apache-2.0

use super::{bind_revolve_shape_target, RevolveTargetInputs};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::body_recipe::{
    DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeReference,
    DesignOperandOwner,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::entity_selection::{
    DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
use cadmpeg_ir::features::{
    AngularTermination, BooleanOp, FaceSelection, FeatureDefinition, FeatureOperation,
    PartialRevolveConstruction, PlanarProfileRef, PlaneTargetSource, RevolveConstruction,
    RevolveExtent,
};
use cadmpeg_ir::math::{Point3, Vector3};

const SCOPE: u32 = 12;
const GROUP: u32 = 100;
const MEMBER: u32 = 200;

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#12",
        DesignFeatureKind::Revolve,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn target_group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:group#{GROUP}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal: 2,
        record_index: GROUP,
        byte_offset: 0,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: MEMBER,
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
                opaque_index: 88,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5),
        role_offset: 0,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn body_operand() -> DesignBodyRecipeOperand {
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:body-recipe#200".into(),
        scope_record_index: SCOPE,
        owner: DesignOperandOwner::Group {
            group_record_index: GROUP,
            group_member_ordinal: 0,
        },
        record_index: MEMBER,
        byte_offset: 0,
        class_tag: "316".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 56,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 25,
            form: 33,
            form_offset: 33,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 203,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#202".into(),
        resolved_face_slot: Some(20),
        resolved_body_state_id: Some(7),
        resolved_body_slot: Some(3),
        resolved_body_face_slots: vec![20, 21],
        next_record_index: 204,
        next_byte_offset: 256,
    })
    .unwrap()
}

fn face_operand(resolved_face_slots: Vec<i64>) -> DesignFaceOperand {
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:face-operand#200".into(),
        scope_record_index: SCOPE,
        scope_reference_ordinal: 3,
        group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
            group_record_index: GROUP,
            group_member_ordinal: 0,
        }),
        record_index: MEMBER,
        byte_offset: 10,
        class_tag: "256".to_owned().try_into().unwrap(),
        paired_byte_offset: 30,
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        recipe_record_index: MEMBER + 3,
        recipe_record_byte_offset: 40,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: crate::records::recipes::ConstructionRecipeKind::Face,
        recipe_program_offset: 60,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots,
        resolved_active_face: None,
        next_record_index: MEMBER + 5,
        next_byte_offset: 70,
    })
    .unwrap()
}

fn revolve(extent: RevolveExtent) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
            profile: PlanarProfileRef::Native("f3d:Design/BulkStream.dat:group#99".into()),
            extent: Some(extent),
            solid: Some(true),
            face_maker: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        }),
        op: BooleanOp::NewBody,
    })
}

fn native_target() -> AngularTermination {
    AngularTermination::ToShape {
        target: FaceSelection::Native(target_group().id.clone()),
    }
}

fn bound(
    extent: RevolveExtent,
    faces: &[DesignFaceOperand],
    bodies: &[DesignBodyRecipeOperand],
) -> RevolveExtent {
    bound_plane(extent, faces, bodies, &[], &[], 1)
}

fn bound_plane(
    extent: RevolveExtent,
    faces: &[DesignFaceOperand],
    bodies: &[DesignBodyRecipeOperand],
    selections: &[DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    components: u64,
) -> RevolveExtent {
    let mut definition = revolve(extent);
    let scope = scope();
    let spaces = (1..=components).map(naming_space).collect::<Vec<_>>();
    bind_revolve_shape_target(
        &mut definition,
        &RevolveTargetInputs {
            scope: &scope,
            scopes,
            construction_groups: &[target_group()],
            entity_selection_operands: selections,
            face_operands: faces,
            body_recipe_operands: bodies,
            histories: &[],
            design_types: &crate::test_support::origin_types::origin_design_types(
                "f3d:Design/MetaStream.dat",
                39,
            ),
            component_naming_spaces: &spaces,
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) = definition
    else {
        panic!("expected a revolve");
    };
    construction.extent().cloned().expect("extent")
}

fn one_sided() -> RevolveExtent {
    RevolveExtent::OneSided {
        termination: native_target(),
    }
}

#[test]
fn a_body_target_selects_every_face_of_the_body() {
    let RevolveExtent::OneSided {
        termination:
            AngularTermination::ToShape {
                target: FaceSelection::Historical { faces, .. },
            },
    } = bound(one_sided(), &[], &[body_operand()])
    else {
        panic!("expected a historical target shape");
    };
    assert_eq!(faces.len(), 2);
}

#[test]
fn a_face_target_ends_at_the_selected_face() {
    let RevolveExtent::OneSided {
        termination:
            AngularTermination::ToFace {
                face: FaceSelection::Historical { faces, .. },
                offset: None,
            },
    } = bound(one_sided(), &[face_operand(vec![14])], &[])
    else {
        panic!("expected a historical target face");
    };
    assert_eq!(faces.len(), 1);
}

#[test]
fn an_unresolved_target_stays_native() {
    assert_eq!(bound(one_sided(), &[], &[]), one_sided());
    assert_eq!(
        bound(one_sided(), &[face_operand(Vec::new())], &[]),
        one_sided()
    );
    assert_eq!(
        bound(one_sided(), &[face_operand(vec![14, 18])], &[]),
        one_sided()
    );
}

#[test]
fn a_two_sided_extent_keeps_its_terminations() {
    let extent = RevolveExtent::TwoSided {
        first: native_target(),
        second: AngularTermination::Unresolved {},
    };
    assert_eq!(
        bound(extent.clone(), &[face_operand(vec![14])], &[body_operand()]),
        extent
    );
}

fn naming_space(
    component_record_index: u64,
) -> crate::records::recipes::DesignComponentNamingSpace {
    crate::records::recipes::DesignComponentNamingSpace {
        id: format!(
            "f3d:Design/BulkStream.dat:design-component-naming-space#{component_record_index}"
        ),
        byte_offset: component_record_index,
        component_record_index,
        context_uuid: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
            .to_owned()
            .try_into()
            .expect("GUID"),
        context_uuid_offset: component_record_index + 12,
        name: None,
        name_offset: None,
    }
}

fn plane_selection(primary_identity: u64, secondary: bool) -> DesignEntitySelectionOperand {
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:design-entity-selection-operand#{MEMBER}"),
        scope_record_index: SCOPE,
        group_record_index: GROUP,
        group_member_ordinal: 0,
        record_index: MEMBER,
        byte_offset: 0,
        class_tag: "000".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 0,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 0,
        serialization_form:
            crate::records::topology::entity_selection::SelectionSerializationForm::Zero,
        identity_record_index: MEMBER + 3,
        identity_record_offset: 0,
        primary_identity,
        primary_identity_offset: 21,
        secondary: secondary.then_some(crate::records::identity::DesignSecondaryIdentity {
            identity: crate::records::identity::Located {
                value: 110,
                offset: 29,
            },
            curve_identity: None,
        }),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: MEMBER + 4,
        next_byte_offset: 29,
    })
    .unwrap()
}

fn work_plane_scope(record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record_index}"),
        DesignFeatureKind::WorkPlane,
        record_index,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPlane(slot) =
        scope.payload_mut()
    {
        *slot = Some(crate::records::feature::scope::DesignWorkPlaneTransform {
            work_plane_transform: [
                [0.0, 0.0, 1.0, -0.5],
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: None,
        });
    }
    scope
}

fn plane_termination(extent: RevolveExtent) -> AngularTermination {
    let RevolveExtent::OneSided { termination } = extent else {
        panic!("expected a one-sided extent");
    };
    termination
}

#[test]
fn a_work_plane_target_ends_at_its_frame() {
    let AngularTermination::ToPlane {
        plane,
        native,
        source,
    } = plane_termination(bound_plane(
        one_sided(),
        &[],
        &[],
        &[plane_selection(282, false)],
        &[work_plane_scope(283)],
        1,
    ))
    else {
        panic!("expected a plane target");
    };
    assert_eq!(plane.origin().get(), Point3::new(-5.0, 0.0, 0.0));
    assert_eq!(plane.normal().get(), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(plane.u_axis().get(), Vector3::new(0.0, 0.0, -1.0));
    assert_eq!(native.as_str(), target_group().id);
    assert_eq!(source, Some(PlaneTargetSource::WorkPlane));
}

#[test]
fn an_origin_plane_target_ends_at_its_canonical_frame() {
    let AngularTermination::ToPlane { plane, source, .. } = plane_termination(bound_plane(
        one_sided(),
        &[],
        &[],
        &[plane_selection(44, false)],
        &[],
        1,
    )) else {
        panic!("expected a plane target");
    };
    assert_eq!(plane.origin().get(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(plane.normal().get(), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(plane.u_axis().get(), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(source, None);
}

#[test]
fn an_unproven_plane_target_stays_native() {
    for (selection, scopes, components) in [
        (plane_selection(282, false), Vec::new(), 1),
        (
            plane_selection(282, false),
            vec![work_plane_scope(283), work_plane_scope(283)],
            1,
        ),
        (plane_selection(40, false), Vec::new(), 1),
        (plane_selection(44, true), Vec::new(), 1),
        (plane_selection(282, true), vec![work_plane_scope(283)], 1),
        (plane_selection(44, false), Vec::new(), 2),
        (plane_selection(44, false), Vec::new(), 0),
    ] {
        assert_eq!(
            bound_plane(one_sided(), &[], &[], &[selection], &scopes, components),
            one_sided()
        );
    }
}
