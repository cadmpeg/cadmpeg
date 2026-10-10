// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    bind_work_plane_constructions, bind_work_point_input_carriers, decode_edge_identity_operands,
    decode_edge_operands, decode_edge_treatment_vertex_operands, decode_face_operands,
};
use crate::records::decal::DesignRecordHeader;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture_scope(
    kind: crate::records::feature::scope::DesignFeatureKind,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignParameterScope, DesignParameterScopeDraft};
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".to_owned(),
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 1100,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 1080,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![1085],
                "reference_members",
            )
            .unwrap(),
            payload: kind.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1200,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn fixture_work_plane_scope() -> crate::records::feature::scope::DesignParameterScope {
    let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::WorkPlane);
    scope.id = crate::ids::native_scoped_id(
        "FusionAssetName[Active]/Design1/BulkStream.dat",
        "scope",
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![100, 101, 102, 103, 104],
                vec![1085, 1096, 1107, 1118, 1129],
                "reference_members",
            )
            .unwrap();
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    scope.with_work_plane_reference(104);
    scope
}

fn fixture_work_plane_owner() -> crate::records::parameters::DesignParameterOwner {
    let mut owner = crate::design::decode::parameters::parse_parameter_owner(
        &cadmpeg_test_support::service_decode_context(),
        &crate::design::test_support::parameter_owner_frame(),
    )
    .expect("service decode context")
    .unwrap()
    .into_record("Design/BulkStream.dat", 0)
    .unwrap();
    let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(owner.clone());
    wire.id = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:owner#104".to_owned();
    wire.record_index = 104;
    wire.scope_record_index = 12;
    wire.parameter_record_index = 105;
    wire.companion_record_index = 106;
    wire.local_ordinal = 0;
    wire.evaluated_value = 0.0;
    owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    owner
}

fn fixture_work_plane_parameter() -> crate::records::parameters::DesignParameter {
    let mut parameter = crate::design::decode::parameters::parse_design_parameter_record(
        &crate::design::test_support::parameter_record(
            Some(104),
            "value",
            "ExtraOffset",
            None,
            "d1",
            0.0,
        ),
    )
    .expect("canonical WorkPlane ExtraOffset parameter");
    parameter.id = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:parameter#105".to_owned();
    parameter.record_index = 105;
    parameter
}

fn fixture_edge_identity_group(
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    fixture_group(
        &[],
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
    )
}

/// A construction group of scope 12 in the synthetic Design stream with
/// `members` and `role`.
fn fixture_group(
    members: &[u32],
    role: crate::records::topology::extrude_selection::DesignOperandRole,
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    use crate::records::topology::construction::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
        DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        DesignConstructionOperandRole,
    };
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:operand-group#100".to_owned(),
        scope_record_index: 12,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1000,
        class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned()).unwrap(),
        members: members
            .iter()
            .zip(0_u64..)
            .map(|(&value, ordinal)| crate::records::identity::Located {
                value,
                offset: 1025 + 11 * ordinal,
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
                opaque_index: 180,
                opaque_index_offset: 1071,
                opaque_scalar: 0.125,
                opaque_scalar_offset: 1075,
                variant: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 1053,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 1124,
    })
    .unwrap()
}

#[test]
fn work_plane_parameter_searches_refuse_work_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let owner = fixture_work_plane_owner();
        let parameter = fixture_work_plane_parameter();
        for (operation, owners, parameters) in [
            (
                "find F3D WorkPlane parameter owners",
                std::slice::from_ref(&owner),
                &[][..],
            ),
            (
                "find F3D WorkPlane offset parameters",
                std::slice::from_ref(&owner),
                std::slice::from_ref(&parameter),
            ),
        ] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                operation,
                0,
                |ctx| {
                    let mut scope = fixture_work_plane_scope();
                    bind_work_plane_constructions(
                        ctx,
                        scan,
                        std::slice::from_mut(&mut scope),
                        &[],
                        &[],
                        owners,
                        parameters,
                    )
                },
            );
            assert!(matches!(
                error,
                CodecError::ResourceLimit(failure)
                    if failure.dimension == ResourceDimension::WorkUnits
                        && failure.operation == operation
                        && failure.additional == 1
            ));
        }
    });
}

#[test]
fn edge_identity_scope_search_refuses_work_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::Fillet);
        scope.id = crate::ids::native_scoped_id(
            "FusionAssetName[Active]/Design1/BulkStream.dat",
            "scope",
            12,
        );
        let group = fixture_edge_identity_group();
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "find F3D edge identity scopes",
            0,
            |ctx| {
                decode_edge_identity_operands(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&group),
                    &[],
                )
            },
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "find F3D edge identity scopes"
                    && failure.additional == 1
        ));
    });
}

#[test]
fn edge_operand_header_and_offset_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d operand header index"),
            (1, "f3d edge operand stream offset"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                decode_edge_operands(&ctx, scan, &[], &[], std::slice::from_ref(&header), &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

/// A scope of `kind` in the synthetic Design stream that names `references`.
fn fixture_stream_scope(
    kind: crate::records::feature::scope::DesignFeatureKind,
    references: &[u32],
) -> crate::records::feature::scope::DesignParameterScope {
    let mut scope = fixture_scope(kind);
    scope.id = crate::ids::native_scoped_id(
        "FusionAssetName[Active]/Design1/BulkStream.dat",
        "scope",
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                references.to_vec(),
                (0_u64..)
                    .take(references.len())
                    .map(|ordinal| 1085 + 11 * ordinal)
                    .collect(),
                "reference_members",
            )
            .unwrap();
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn edge_operand_member_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let scope = fixture_stream_scope(
            crate::records::feature::scope::DesignFeatureKind::Fillet,
            &[100],
        );
        let group = fixture_group(
            &[101, 102],
            crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X5,
        );
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "f3d edge operand member index",
            0,
            |ctx| {
                decode_edge_operands(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&group),
                    &[],
                    &[],
                )
            },
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.operation == "f3d edge operand member index"
        ));
    });
}

#[test]
fn vertex_operand_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            decode_edge_treatment_vertex_operands(
                &ctx, scan, &[], &[], std::slice::from_ref(&header), &[],
            ),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d operand header index"
        ));
    });
}

#[test]
fn work_plane_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_work_plane_constructions(&ctx, scan, &mut [], std::slice::from_ref(&header),
                &[], &[], &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d operand header index"
        ));
    });
}

#[test]
fn work_point_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            bind_work_point_input_carriers(&ctx, scan, &mut [], std::slice::from_ref(&header), &[], &[], &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d operand header index"
        ));
    });
}

#[test]
fn work_point_plane_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::WorkPlane);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d WorkPoint work-plane stream index"),
            (1, "f3d WorkPoint work-plane index"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                bind_work_point_input_carriers(&ctx, scan, std::slice::from_mut(&mut scope), &[], &[], &[], &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn edge_identity_header_index_refuses_collection_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            decode_edge_identity_operands(&ctx, scan, &[], &[], std::slice::from_ref(&header)),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d operand header index"
        ));
    });
}

#[test]
fn face_operand_header_and_scope_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
                .unwrap(),
            record_index: 7,
        };
        let scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::Extrude);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        for (limit, operation) in [
            (0, "f3d operand header index"),
            (1, "f3d face operand scope index"),
        ] {
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                decode_face_operands(&ctx, scan, std::slice::from_ref(&scope), &[],
                    std::slice::from_ref(&header), &[]),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn indexed_face_operand_reference_ordinals_refuse_work_limit() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut scope = fixture_scope(crate::records::feature::scope::DesignFeatureKind::Shell);
        scope.id = crate::ids::native_scoped_id(
            "FusionAssetName[Active]/Design1/BulkStream.dat",
            "scope",
            12,
        );
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "scan F3D indexed face operand reference ordinals",
            0,
            |ctx| decode_face_operands(ctx, scan, std::slice::from_ref(&scope), &[], &[], &[]),
        );
        assert!(matches!(
            error,
            CodecError::ResourceLimit(failure)
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "scan F3D indexed face operand reference ordinals"
                    && failure.additional == 1
        ));
    });
}

#[test]
fn face_operand_visits_keep_the_first_visit_of_each_key() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        // The group visits 101, 102 and 101 again; the scope then visits 100,
        // 101 and 100 again. Each batch decodes only the keys it visits first.
        let scope = fixture_stream_scope(
            crate::records::feature::scope::DesignFeatureKind::Shell,
            &[100, 101, 100],
        );
        let group = fixture_group(&[101, 102, 101], DesignOperandRole::ROLE_0X10);
        for (skip, visits) in [(0, 2), (1, 1)] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                "decode F3D face operand visits",
                skip,
                |ctx| {
                    decode_face_operands(
                        ctx,
                        scan,
                        std::slice::from_ref(&scope),
                        std::slice::from_ref(&group),
                        &[],
                        &[],
                    )
                },
            );
            assert!(matches!(
                error,
                CodecError::ResourceLimit(failure)
                    if failure.operation == "decode F3D face operand visits"
                        && failure.additional == visits
            ));
        }
    });
}
