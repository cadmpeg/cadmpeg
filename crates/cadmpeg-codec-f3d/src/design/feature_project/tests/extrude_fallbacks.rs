// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::extrude::project_extrude;
use crate::design::test_support::parameter_record;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignParameterScopeDraft, DesignScopePayload,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

pub(super) fn scope() -> DesignParameterScope {
    scope_with_extent(DesignExtrudeExtent::OneSidedDistance)
}

pub(super) fn scope_with_extent(extent: DesignExtrudeExtent) -> DesignParameterScope {
    scope_with_start(extent, DesignExtrudeStart::ProfilePlane)
}

fn scope_with_start(
    extent: DesignExtrudeExtent,
    start: DesignExtrudeStart,
) -> DesignParameterScope {
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                    reference: None,
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 128,
                    direction_face_extend_values: [1, 2],
                    side_extent_discriminators: [1, 0],
                    side_extent_discriminator_offsets: [177, 190],
                    first_side_target_ordinal: None,
                    distance_tail: None,
                    extent,
                    direction_face_extend_offsets: [132, 136],
                    direction_reversed: false,
                    direction_reversed_offset: 140,
                    solid_operation: true,
                    solid_operation_offset: 141,
                    start,
                    start_offset: 142,
                }),
                ..Default::default()
            })),
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
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

pub(super) fn profile_group(index: u32, ordinal: u32) -> DesignConstructionOperandGroup {
    profile_group_with_members(index, ordinal, 1)
}

pub(super) fn profile_group_with_members(
    index: u32,
    ordinal: u32,
    member_count: u32,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:operand-group#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        record_index: index,
        byte_offset: 1000,
        class_tag: "332".to_owned().try_into().unwrap(),
        members: (0..member_count)
            .map(|member| crate::records::identity::Located {
                value: index + 100 + member,
                offset: 1026 + u64::from(member) * 11,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::ExtrudeProfile,
        role_offset: 1054,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 1125,
    })
    .unwrap()
}

fn parameters() -> [crate::records::parameters::DesignParameter; 2] {
    [
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "AlongDistance",
            Some("mm"),
            "d1",
            0.55,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            "TaperAngle",
            Some("deg"),
            "d2",
            0.2,
        ))
        .unwrap(),
    ]
}

fn assert_profile_fallback(groups: &[DesignConstructionOperandGroup], expected_native: &str) {
    let scope = scope();
    let parameters = parameters();
    let owned = [(0, &parameters[0]), (1, &parameters[1])];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)), ..
        }) if native == expected_native
    ));
}

#[test]
fn extrude_single_profile_falls_back_to_group_id() {
    let first = profile_group(100, 0);
    assert_profile_fallback(std::slice::from_ref(&first), &first.id);
}

#[test]
fn extrude_multiple_profiles_fall_back_to_scope_id() {
    let groups = [profile_group(100, 0), profile_group(101, 1)];
    assert_profile_fallback(&groups, "f3d:Design/BulkStream.dat:scope#12");
}

fn assert_face_projects(role: DesignConstructionOperandRole) {
    let scope = scope_with_extent(DesignExtrudeExtent::OneSidedToFace);
    let profile = profile_group(100, 0);
    let mut selection = profile_group(101, 1);
    selection.operand_role = role;
    let groups = [profile, selection];
    let side_offset = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side1Offset",
        Some("mm"),
        "d3",
        0.0,
    ))
    .unwrap();
    let taper = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "TaperAngle",
        Some("deg"),
        "d2",
        0.2,
    ))
    .unwrap();
    let owned = [(0, &side_offset), (1, &taper)];
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(decode_ctx, &scope, &owned, &groups, &[], &[], &[])
    })
    .unwrap()
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
    ));
}

#[test]
fn extrude_projects_with_selected_face_group() {
    assert_face_projects(DesignConstructionOperandRole::ExtrudeFaces {
        encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
        usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
    });
}

#[test]
fn extrude_projects_with_target_shape_group() {
    assert_face_projects(DesignConstructionOperandRole::Other(
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    ));
}

#[test]
fn distance_and_through_all_retains_auxiliary_selection() {
    use cadmpeg_ir::features::{ExtrudeExtent, LinearTermination};
    let scope = scope_with_extent(DesignExtrudeExtent::TwoSidedDistanceThroughAll);
    let profile = profile_group(100, 0);
    let mut auxiliary = profile_group(101, 1);
    auxiliary.operand_role = DesignConstructionOperandRole::Other(
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    );
    let params = parameters();
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(
            ctx,
            &scope,
            &[(0, &params[0]), (1, &params[1])],
            &[profile, auxiliary],
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .expect("mixed Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    }) = definition
    else {
        panic!("mixed Extrude definition");
    };
    assert!(
        matches!(first.termination, LinearTermination::Blind { length } if length.get() == 5.5)
    );
    assert!(matches!(
        second.termination,
        LinearTermination::ThroughAll {}
    ));
}

#[test]
fn target_first_and_distance_second_travel_the_parameter_magnitude() {
    use cadmpeg_ir::features::{ExtrudeExtent, FaceSelection, LinearTermination};
    let scope = scope_with_extent(DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero);
    let profile = profile_group(100, 0);
    let mut target = profile_group(101, 1);
    target.operand_role = DesignConstructionOperandRole::Other(
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    );
    let offset = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side1Offset",
        Some("mm"),
        "d1",
        0.0,
    ))
    .unwrap();
    let distance = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "AlongDistance",
        Some("mm"),
        "d2",
        -0.8,
    ))
    .unwrap();
    let target_id = target.id.clone();
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(
            ctx,
            &scope,
            &[(0, &offset), (1, &distance)],
            &[profile, target],
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .expect("target and distance Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    }) = definition
    else {
        panic!("target and distance definition");
    };
    assert!(
        matches!(first.termination, LinearTermination::ToShape { target: FaceSelection::Native(id) } if id == target_id)
    );
    assert!(
        matches!(second.termination, LinearTermination::Blind { length } if length.get() == 8.0)
    );
}

#[test]
fn two_native_targets_keep_their_side_order() {
    use cadmpeg_ir::features::{ExtrudeExtent, FaceSelection, LinearTermination};
    let mut draft =
        scope_with_extent(DesignExtrudeExtent::TwoSidedToTargetsOptionZero).into_draft();
    draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
        vec![100, 101, 102],
        vec![185, 196, 207],
        "reference_members",
    )
    .unwrap();
    let scope = DesignParameterScope::try_new(draft.with_fixture_layout()).unwrap();
    let profile = profile_group(100, 0);
    let mut first = profile_group(101, 1);
    let mut second = profile_group(102, 2);
    first.operand_role = DesignConstructionOperandRole::Other(
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    );
    second.operand_role = first.operand_role;
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let offset = |kind: &str| {
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            kind,
            Some("mm"),
            "d1",
            0.0,
        ))
        .unwrap()
    };
    let first_offset = offset("Side1Offset");
    let second_offset = offset("Side2Offset");
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(
            ctx,
            &scope,
            &[(0, &first_offset), (1, &second_offset)],
            &[second, profile, first],
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .expect("two-target Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    }) = definition
    else {
        panic!("two-target definition");
    };
    assert!(
        matches!(first.termination, LinearTermination::ToShape { target: FaceSelection::Native(id) } if id == first_id)
    );
    assert!(
        matches!(second.termination, LinearTermination::ToShape { target: FaceSelection::Native(id) } if id == second_id)
    );
}

#[test]
fn through_all_and_face_use_opposite_sides() {
    use cadmpeg_ir::features::{ExtrudeExtent, LinearTermination};
    let mut draft = scope_with_extent(DesignExtrudeExtent::TwoSidedThroughAllToFace).into_draft();
    draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
        vec![100, 101, 102],
        vec![185, 196, 207],
        "reference_members",
    )
    .unwrap();
    let scope = DesignParameterScope::try_new(draft.with_fixture_layout()).unwrap();
    let profile = profile_group(100, 0);
    let mut auxiliary = profile_group(102, 2);
    auxiliary.operand_role = DesignConstructionOperandRole::Other(
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    );
    let mut face = profile_group(101, 1);
    face.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
        encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
        usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
    };
    let offset = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side2Offset",
        Some("mm"),
        "d1",
        0.0,
    ))
    .unwrap();
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(
            ctx,
            &scope,
            &[(0, &offset)],
            &[profile, face, auxiliary],
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .expect("through-all and face Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, second },
        ..
    }) = definition
    else {
        panic!("through-all and face definition");
    };
    assert!(matches!(
        first.termination,
        LinearTermination::ThroughAll {}
    ));
    assert!(matches!(
        second.termination,
        LinearTermination::ToFace { offset: None, .. }
    ));
}

#[test]
fn surface_extrude_retains_the_role_43_group_as_its_native_profile() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use cadmpeg_ir::features::{ExtrudeExtent, LinearTermination};
    let mut scope = scope();
    if let Some(DesignExtrudePrologue::ReferenceAware {
        solid_operation, ..
    }) = scope.extrude_prologue_mut()
    {
        *solid_operation = false;
    }
    let mut group = profile_group(100, 0);
    group.operand_role = DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X43);
    let params = parameters();
    let inputs = [(0, &params[0]), (1, &params[1])];
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(ctx, &scope, &inputs, &[group.clone()], &[], &[], &[])
    })
    .unwrap()
    .expect("surface Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Native(profile)),
        extent: ExtrudeExtent::OneSided { side },
        solid: Some(false),
        ..
    }) = definition
    else {
        panic!("surface Extrude definition");
    };
    assert_eq!(profile, group.id);
    assert!(matches!(side.termination, LinearTermination::Blind { length } if length.get() == 5.5));
    assert_eq!(side.draft.unwrap().get(), 0.2);
}

#[test]
fn negative_distance_reverses_the_sweep() {
    use cadmpeg_ir::features::{ExtrudeDirection, ExtrudeExtent, LinearTermination};
    let scope = scope();
    let group = profile_group(100, 0);
    let along = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "AlongDistance",
        Some("mm"),
        "d1",
        -1.5,
    ))
    .unwrap();
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(ctx, &scope, &[(0, &along)], &[group], &[], &[], &[])
    })
    .unwrap()
    .expect("signed-distance Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        direction: ExtrudeDirection::ReversedProfileNormal {},
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        panic!("signed-distance Extrude definition");
    };
    assert!(
        matches!(side.termination, LinearTermination::Blind { length } if length.get() == 15.0)
    );
}

#[test]
fn wall_thickness_stays_native_while_the_extrude_distance_projects() {
    use cadmpeg_ir::features::{ExtrudeExtent, LinearTermination};
    let scope = scope();
    let group = profile_group(100, 0);
    let params = parameters();
    let wall = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Side1WallThickness",
        Some("mm"),
        "wall",
        0.2,
    ))
    .unwrap();
    let inputs = [(0, &params[0]), (1, &params[1]), (2, &wall)];
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(ctx, &scope, &inputs, &[group], &[], &[], &[])
    })
    .unwrap()
    .expect("wall Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        panic!("wall Extrude definition");
    };
    assert!(matches!(side.termination, LinearTermination::Blind { length } if length.get() == 5.5));
}

#[test]
fn selected_face_start_without_a_start_group_reads_the_offset_plane() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeStart, LinearTermination};
    let mut draft = scope_with_start(
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeStart::FromFace,
    )
    .into_draft();
    draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
        vec![100, 101, 102],
        vec![185, 196, 207],
        "reference_members",
    )
    .unwrap();
    let scope = DesignParameterScope::try_new(draft.with_fixture_layout()).unwrap();
    let profile = profile_group(100, 0);
    let mut face = profile_group(101, 1);
    face.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
        encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
        usage: crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
    };
    let parameter = |kind: &str, value: f64| {
        parse_design_parameter_record(&parameter_record(
            Some(44),
            "value",
            kind,
            Some("mm"),
            "d1",
            value,
        ))
        .unwrap()
    };
    let profile_offset = parameter("ProfileOffset", 0.5);
    let side_offset = parameter("Side1Offset", 0.0);
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_extrude(
            ctx,
            &scope,
            &[(0, &profile_offset), (1, &side_offset)],
            &[profile, face],
            &[],
            &[],
            &[],
        )
    })
    .unwrap()
    .expect("selected-face start without a start group");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        start: ExtrudeStart::OffsetProfilePlane { offset },
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        panic!("offset-plane start definition");
    };
    assert_eq!(offset.get(), 5.0);
    assert!(matches!(
        side.termination,
        LinearTermination::ToFace { offset: None, .. }
    ));
}
