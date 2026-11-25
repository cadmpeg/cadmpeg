// SPDX-License-Identifier: Apache-2.0

use crate::records::{
    decal::DesignRecordHeader,
    feature::{
        deleted_members::{DesignDeletedMembers, DesignRecordTimestamp},
        scope::{DesignFeatureKind, DesignParameterScope},
    },
    identity::{Located, ReferenceRun},
};

fn native() -> crate::native::F3dNative {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    crate::native::F3dNative {
        design_parameter_scopes: vec![DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        )],
        ..Default::default()
    }
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design parameter scope has an invalid paired frame"
        );
    })
}

#[test]
fn assembly_datum_envelope_accepts_a_registered_joint_origin_carrier() {
    use crate::native::F3dNative;
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::feature::assembly::{DesignAssemblyAlignment, DesignAssemblyAlignmentForm};
    use crate::records::feature::scope::{DesignJointOriginReference, DesignScopePayload};
    use crate::records::feature::work_geometry::joint_origin::DesignJointOriginCarrierEvidence;
    use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
    use crate::records::sketch_placement::SketchPlacementMatrix;

    fn carrier_evidence(scope: &mut DesignParameterScope) -> &mut DesignJointOriginCarrierEvidence {
        scope
            .joint_origin_frame_mut()
            .unwrap()
            .carrier
            .as_mut()
            .unwrap()
    }

    let stream = "f3d:Design/BulkStream.dat";
    let mut assembly = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#3"),
        DesignFeatureKind::Assemble,
        3,
    );
    let owners: Vec<_> = (0..8)
        .map(|ordinal| {
            let record_index = 50 + ordinal * 3;
            let byte_offset = 2048 + 104 * u64::from(ordinal);
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("{stream}:design-parameter-owner#{record_index}"),
                byte_offset,
                frame_length: 104,
                class_tag: "457".to_owned().try_into().unwrap(),
                record_index,
                scope_record_index: assembly.record_index,
                local_ordinal: ordinal,
                evaluated_value: 0.0,
                evaluated_value_offset: byte_offset + 40,
                parameter_record_index: record_index + 1,
                owned_ordinal: ordinal,
                variant: Some(0),
                companion_record_index: record_index + 2,
            })
            .unwrap()
        })
        .collect();
    let mut origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#91"),
        DesignFeatureKind::JointOrigin,
        91,
    );
    let origin_offset = 1024;
    let carrier_index = origin.record_index + 1;
    let construction = 90;
    assembly
        .try_edit(|draft| {
            draft.class_tag = "276".to_owned().try_into().unwrap();
            draft.paired_class_tag = "258".to_owned().try_into().unwrap();
            draft.frame_length = 604;
            draft.paired_byte_offset = 604;
            draft.reference_count_offset = 360;
            draft.reference_members = ReferenceRun::unlocated(
                std::iter::once(construction)
                    .chain(owners.iter().map(DesignParameterOwner::record_index))
                    .collect(),
            );
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = DesignScopePayload::Assemble(Some(
                DesignAssemblyAlignment::try_new(
                    0.0,
                    [0.0; 3],
                    owners[4..]
                        .iter()
                        .map(|owner| Located {
                            value: owner.record_index(),
                            offset: owner.evaluated_value_offset(),
                        })
                        .collect(),
                    Some(DesignAssemblyAlignmentForm::DatumEnvelope {
                        joint_origin_scope_record_index: origin.record_index,
                    }),
                )
                .unwrap(),
            ));
        })
        .unwrap();
    origin
        .try_edit(|draft| {
            draft.byte_offset = origin_offset;
            draft.frame_length = 315;
            draft.paired_byte_offset = origin_offset + draft.frame_length;
            draft.reference_count_offset = origin_offset + 132;
            draft.reference_members = ReferenceRun::unlocated(vec![carrier_index, construction]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    origin.with_joint_origin_transform(SketchPlacementMatrix::IDENTITY);
    let carrier_start = origin.paired_byte_offset() + 30;

    let type_row = |byte_offset, guid: &str, version, index| {
        SegmentType::try_new(
            format!("f3d:Design/MetaStream.dat:design-type#{byte_offset}"),
            SegmentTypeData {
                byte_offset,
                type_guid: guid.to_owned().try_into().unwrap(),
                type_guid_offset: byte_offset + 4,
                base_type_guid: BaseTypeGuid::Absent,
                version,
                version_offset: byte_offset + 44,
                module: "Fusion".into(),
                entities: ReferenceRun::unlocated(vec![index]),
            },
        )
        .unwrap()
    };
    let headers = [
        (
            assembly.record_index,
            assembly.byte_offset(),
            assembly.class_tag.clone(),
        ),
        (
            origin.record_index,
            origin.byte_offset(),
            origin.class_tag.clone(),
        ),
        (construction, 1800, "301".to_owned().try_into().unwrap()),
    ]
    .into_iter()
    .chain(owners.iter().map(|owner| {
        (
            owner.record_index(),
            owner.byte_offset(),
            owner.class_tag().clone(),
        )
    }))
    .map(
        |(record_index, byte_offset, class_tag)| DesignRecordHeader {
            id: format!("{stream}:design-record-header#{record_index}"),
            record_index,
            byte_offset,
            class_tag,
        },
    )
    .collect();
    let assembly_id = assembly.id.clone();
    let mut native = F3dNative {
        design_types: vec![
            type_row(
                0,
                "02E8AA49-08B8-439C-A8C7-221BE20143D8",
                2,
                u64::from(origin.record_index),
            ),
            type_row(
                100,
                "7B6C3C2D-4096-4E6D-B225-699E8C2D9355",
                6,
                u64::from(carrier_index),
            ),
        ],
        design_parameter_scopes: vec![assembly, origin],
        design_parameter_owners: owners,
        design_record_headers: headers,
        ..Default::default()
    };
    native.design_record_headers.push(DesignRecordHeader {
        id: format!("{stream}:design-record-header#{carrier_index}"),
        record_index: carrier_index,
        byte_offset: carrier_start,
        class_tag: "153".to_owned().try_into().unwrap(),
    });
    let set_frame = |native: &mut F3dNative, transform_offset, reference_offset| {
        let frame = native.design_parameter_scopes[1]
            .joint_origin_frame_mut()
            .unwrap();
        frame.joint_origin_transform_offset = transform_offset;
        frame.reference = Some(DesignJointOriginReference {
            joint_origin_reference: construction,
            joint_origin_reference_offset: reference_offset,
        });
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let findings = |native: &F3dNative| {
            let ctx = super::super::Ctx::new(&ir, native, decode);
            let mut findings = Vec::new();
            super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            findings
        };
        set_frame(&mut native, 36, 25);
        assert!(findings(&native).is_empty());
        set_frame(&mut native, carrier_start + 60, carrier_start + 46);
        let mut prefix = vec![0; 34];
        crate::test_support::push_marked_reference(&mut prefix, construction);
        prefix.extend_from_slice(&[0; 4]);
        let mut scope_backlink = Vec::new();
        crate::test_support::push_marked_reference(&mut scope_backlink, 91);
        native.design_parameter_scopes[1]
            .joint_origin_frame_mut()
            .unwrap()
            .carrier = Some(DesignJointOriginCarrierEvidence {
            frame_length: 369,
            prefix: prefix.into(),
            scope_backlink: scope_backlink.as_slice().try_into().unwrap(),
        });
        let encoded =
            cadmpeg_core::serde_erase::json::to_value(&native.design_parameter_scopes[1]).unwrap();
        native.design_parameter_scopes[1] =
            cadmpeg_core::serde_erase::json::from_value(encoded).unwrap();
        let valid = findings(&native);
        assert!(valid.is_empty(), "{valid:?}");

        let assert_invalid = |native: &F3dNative| {
            let invalid = findings(native);
            assert!(invalid.iter().any(|finding| {
                finding.check == cadmpeg_ir::report::check::Check::NativeLinks
                    && finding.entity.as_deref() == Some(assembly_id.as_str())
            }));
        };
        for prefix_index in [0, 34, 35, 47] {
            let mut invalid = native.clone();
            carrier_evidence(&mut invalid.design_parameter_scopes[1]).prefix[prefix_index] ^= 1;
            assert_invalid(&invalid);
        }
        let mut invalid = native.clone();
        carrier_evidence(&mut invalid.design_parameter_scopes[1]).scope_backlink[1] ^= 1;
        assert_invalid(&invalid);
        let mut invalid = native.clone();
        carrier_evidence(&mut invalid.design_parameter_scopes[1]).frame_length += 1;
        assert_invalid(&invalid);
        let mut invalid = native.clone();
        set_frame(&mut invalid, 36, 25);
        assert_invalid(&invalid);
        let mut invalid = native.clone();
        invalid
            .design_record_headers
            .retain(|header| header.record_index != construction);
        assert_invalid(&invalid);
        native.design_record_headers.last_mut().unwrap().byte_offset += 1;
        assert_invalid(&native);
    });
}

/// A role-`0x10` construction group with record index `record_index`, owned
/// by `OffsetFaces` scope record 17.
fn offset_faces_group(
    record_index: u32,
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    use crate::records::topology::construction::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
        DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        DesignConstructionOperandRole,
    };
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"),
        scope_record_index: 17,
        scope_reference_ordinal: 1,
        record_index,
        byte_offset: 1_000,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: 303,
            offset: 1_026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1_058,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1_062,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X10,
        ),
        role_offset: 1_040,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_080,
    })
    .unwrap()
}

#[test]
fn offset_faces_counted_member_runs_validate() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::ReferenceRun,
    };
    for count in [2, 8, 10] {
        for count_offset in [126, 130] {
            let mut scope = DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#17",
                DesignFeatureKind::OffsetFaces,
                17,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members = ReferenceRun::unlocated((301..301 + count).collect());
                    draft.reference_count_offset = count_offset;
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.kind_offset + 22 + 78;
                    draft.frame_length = draft.paired_byte_offset;
                    draft.feature_ordinal_offset = draft.kind_offset + 22;
                })
                .unwrap();
            if let crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot) =
                scope.payload_mut()
            {
                *slot = Some(
                    crate::records::feature::direct_face::DesignOffsetFacesOperation {
                        distance: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                        distance_record_index: 300 + count,
                        distance_offset: u64::from(340 + count),
                    },
                );
            }
            let mut native = crate::native::F3dNative::default();
            for record_index in std::iter::once(17).chain(301..301 + count) {
                native.design_record_headers.push(DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                    record_index,
                    class_tag: "256".to_owned().try_into().unwrap(),
                    byte_offset: u64::from(record_index),
                });
            }
            native.design_parameter_scopes.push(scope);
            let findings = |native: &crate::native::F3dNative| {
                crate::test_support::with_decode_context(|service_ctx| {
                    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
                    let ctx = super::super::Ctx::new(&ir, native, service_ctx);
                    let mut findings = Vec::new();
                    super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings)
                        .unwrap();
                    findings
                })
            };
            if count < 3 {
                assert_eq!(findings(&native).len(), 1);
                continue;
            }
            assert!(findings(&native).is_empty(), "{count} {count_offset}");
            let with_distance = |distance_record_index| {
                let mut changed = native.clone();
                if let crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(Some(
                    operation,
                )) = changed.design_parameter_scopes[0].payload_mut()
                {
                    operation.distance_record_index = distance_record_index;
                }
                changed
            };
            // A first-position distance needs the selected-face group second.
            assert_eq!(findings(&with_distance(301)).len(), 1);
            let mut distance_first = with_distance(301);
            distance_first
                .design_construction_operand_groups
                .push(offset_faces_group(302));
            assert!(
                findings(&distance_first).is_empty(),
                "{count} {count_offset}"
            );
            let mut group_first = with_distance(301);
            group_first
                .design_construction_operand_groups
                .push(offset_faces_group(301));
            assert_eq!(findings(&group_first).len(), 1);
            assert_eq!(findings(&with_distance(302)).len(), 1);
            native.design_record_headers.pop();
            assert_eq!(findings(&native).len(), 1);
        }
    }
}

#[test]
fn operand_list_combine_requires_its_own_offsets() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::feature::{
        combine::{
            DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
            DesignCombineTools,
        },
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut},
    };
    use crate::records::identity::ReferenceRun;

    let findings = |operation_offset: u64, keep_tools_offset: u64, keep_tools: bool| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#17",
            DesignFeatureKind::Combine,
            17,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![301, 302, 303, 304]);
                draft.reference_count_offset = 240;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.kind_offset + 22 + 78;
                draft.frame_length = draft.paired_byte_offset;
                draft.feature_ordinal_offset = draft.kind_offset + 22;
            })
            .unwrap();
        if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            slot.operation = Some(DesignCombineOperation {
                form: DesignCombineForm::OperandLists,
                operation: cadmpeg_ir::features::operation_settings::BooleanKind::Join,
                operation_offset,
                keep_tools,
                keep_tools_offset,
                target_record_index: 304,
                target_body_node: None,
                tools: DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: 302,
                        external_identity: None,
                        body_node: None,
                    },
                    additional: Vec::new(),
                },

                destination: Default::default(),
                created_body_nodes: Vec::new(),
                consumed_body_nodes: Vec::new(),
            });
        }
        let mut native = crate::native::F3dNative::default();
        for record_index in [17, 301, 302, 303, 304] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                record_index,
                class_tag: "256".to_owned().try_into().unwrap(),
                byte_offset: u64::from(record_index),
            });
        }
        native.design_parameter_scopes.push(scope);
        crate::test_support::with_decode_context(|service_ctx| {
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            findings.len()
        })
    };
    assert_eq!(findings(21, 29, false), 0);
    assert_eq!(findings(20, 29, false), 1);
    assert_eq!(findings(21, 25, false), 1);
    for (operation_offset, keep_tools_offset) in [
        (25, 33),
        (21, 40),
        (25, 44),
        (21, 51),
        (21, 80),
        (25, 84),
        (21, 91),
        (21, 131),
    ] {
        assert_eq!(
            findings(operation_offset, keep_tools_offset, false),
            0,
            "{operation_offset} {keep_tools_offset}"
        );
    }
    for (operation_offset, keep_tools_offset) in [(23, 31), (29, 37), (21, 35), (25, 39)] {
        assert_eq!(
            findings(operation_offset, keep_tools_offset, false),
            1,
            "{operation_offset} {keep_tools_offset}"
        );
    }
    assert_eq!(findings(21, 29, true), 0);
    assert_eq!(findings(25, 33, true), 0);
    assert_eq!(findings(21, 40, true), 1);
    assert_eq!(findings(25, 44, true), 1);
    assert_eq!(findings(21, 80, true), 1);
    assert_eq!(findings(21, 79, false), 1);
}

#[test]
fn validation_accepts_malformed_rest_with_retained_limits() {
    use super::{
        f3d_native_mut, f3d_with_smbh_and_protein, synthetic_geometry_smbh, Codec, Cursor,
        DecodeOptions, F3dCodec,
    };
    use crate::records::feature::joint_motion::{DesignJointLimitLanes, DesignJointMotion};
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    let before = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    f3d_native_mut(&mut ir)
        .design_joint_motions
        .push(DesignJointMotion {
            id: "f3d:Design/BulkStream.dat:design-joint-motion#12".into(),
            joint_record_index: 10,
            joint_byte_offset: 0,
            motion_record_index: 12,
            motion_byte_offset: 100,
            freedoms: vec![crate::records::identity::Located {
                value: 8,
                offset: 150,
            }],
            entry_words: Vec::new(),
            limits: Some(DesignJointLimitLanes {
                flags_offset: 42,
                first_flag: 1,
                second_flag: 1,
                maximum: 1.5,
                minimum: -1.5,
                values_offset: 54,
                rest: None,
                rest_malformed: true,
            }),
        });
    let after = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    assert_eq!(before, after);
}

#[test]
fn corner_flange_scope_round_trip_and_layout_validation() {
    use crate::layout::edge_flange_corner_operation as layout;
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::EdgeFlange,
        40,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.paired_byte_offset =
                draft.byte_offset + cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                51, 52, 55, 61, 62, 65, 81, 91, 101, 104, 108, 141,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let operation = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "edge_wrapper_record_indices":[51,61], "edge_group_record_indices":[52,62],
        "edge_operand_record_indices":[55,65], "aggregate_group_record_index":101,
        "aggregate_operand_record_indices":[104,108], "height_owner_record_index":81,
        "angle_owner_record_index":91, "width_mode":"full_edge",
        "width_distance_owner_record_indices":[], "settings_record_index":141,
        "bend_radius":0.35, "bend_radius_offset":scope.byte_offset() + cadmpeg_core::decode::u64_from_index(layout::INSIDE_BEND_RADIUS),
        "reference_side_code":4, "height_datum":"outer_faces", "bend_position":"inside"
    })).unwrap();
    if let DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
        *slot = Some(operation).into();
    }
    let wire = cadmpeg_core::serde_erase::json::to_value(&scope).unwrap();
    let decoded: DesignParameterScope =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(&decoded).unwrap(),
        wire
    );
    let findings = |scope: DesignParameterScope| {
        crate::test_support::with_decode_context(|service_ctx| {
            let ir = cadmpeg_ir::document::CadIr::empty();
            let headers = std::iter::once(scope.record_index)
                .chain(scope.reference_members().values().copied())
                .map(|record_index| crate::records::decal::DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                    record_index,
                    class_tag: if record_index == scope.record_index {
                        scope.class_tag.clone()
                    } else {
                        crate::records::references::DesignClassTag::try_from("256".to_owned())
                            .unwrap()
                    },
                    byte_offset: if record_index == scope.record_index {
                        scope.byte_offset()
                    } else {
                        scope.paired_byte_offset() + u64::from(record_index) * 21
                    },
                })
                .collect();
            let native = crate::native::F3dNative {
                design_parameter_scopes: vec![scope],
                design_record_headers: headers,
                ..Default::default()
            };
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            findings
        })
    };
    assert!(findings(decoded).is_empty());
    let mut invalid = scope.clone();
    if let DesignScopePayloadMut::EdgeFlange(
        crate::records::feature::sheet_metal::edge::DesignEdgeFlangeScope::Legacy(operation),
    ) = invalid.payload_mut()
    {
        operation.bend_radius_offset += 1;
    }
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    invalid.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    invalid
        .try_edit(|draft| {
            draft.frame_length -= 1;
            draft.paired_byte_offset -= 1;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    if let DesignScopePayloadMut::EdgeFlange(
        crate::records::feature::sheet_metal::edge::DesignEdgeFlangeScope::Legacy(operation),
    ) = invalid.payload_mut()
    {
        operation.height_datum =
            crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::InnerFaces;
    }
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope;
    if let DesignScopePayloadMut::EdgeFlange(
        crate::records::feature::sheet_metal::edge::DesignEdgeFlangeScope::Legacy(operation),
    ) = invalid.payload_mut()
    {
        operation.bend_position = crate::records::feature::sheet_metal::DesignBendPosition::Outside;
    }
    assert!(!findings(invalid).is_empty());
}

#[test]
fn scope_admits_only_its_deleted_absent_members() {
    for kind in [
        DesignFeatureKind::Sketch,
        DesignFeatureKind::try_from("Skizze".to_owned()).unwrap(),
    ] {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#17",
            kind,
            17,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![301, 302]);
                draft.reference_count_offset = 126;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.kind_offset + 22 + 78;
                draft.frame_length = draft.paired_byte_offset;
                draft.feature_ordinal_offset = draft.kind_offset + 22;
            })
            .unwrap();
        let mut native = crate::native::F3dNative::default();
        for record_index in [17, 301] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                record_index,
                class_tag: "256".to_owned().try_into().unwrap(),
                byte_offset: u64::from(record_index),
            });
        }
        native.design_parameter_scopes.push(scope);
        let findings = |native: &crate::native::F3dNative| {
            crate::test_support::with_decode_context(|service_ctx| {
                let ir = cadmpeg_ir::examples::unit_cube().unwrap();
                let ctx = super::super::Ctx::new(&ir, native, service_ctx);
                let mut findings = Vec::new();
                super::super::parameter_scopes::validate_parameter_scopes(&ctx, &mut findings)
                    .unwrap();
                findings
            })
        };
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#500".into(),
            record_index: 18,
            class_tag: "343".to_owned().try_into().unwrap(),
            byte_offset: 500,
        });
        let entry = |ordinal: u64, record_index: u32| DesignRecordTimestamp {
            record_index: Located {
                value: record_index,
                offset: 500 + 31 + 19 * ordinal + 1,
            },
            timestamp: 1_646_998_530_073_417,
        };
        let mut table_bytes = vec![3, 0, 0, 0];
        table_bytes.extend_from_slice(b"343");
        table_bytes.extend_from_slice(&18_u32.to_le_bytes());
        table_bytes.extend_from_slice(&[0; 14]);
        table_bytes.extend_from_slice(&[1, 1]);
        table_bytes.extend_from_slice(&2_u32.to_le_bytes());
        for record_index in [301_u32, 302, 17] {
            table_bytes.push(1);
            table_bytes.extend_from_slice(&record_index.to_le_bytes());
            table_bytes.extend_from_slice(&[0; 6]);
            table_bytes.extend_from_slice(&1_646_998_530_073_417_u64.to_le_bytes());
        }
        let evidence = DesignDeletedMembers {
            table_record_index: 18,
            table_byte_offset: 500,
            table_class_tag: "343".to_owned().try_into().unwrap(),
            table_bytes: table_bytes.into(),
            entries: vec![entry(0, 301), entry(1, 302)],
            owner: entry(2, 17),
            members: vec![302],
        };
        let claim = |change: &dyn Fn(&mut crate::native::F3dNative)| {
            let mut native = native.clone();
            native.design_parameter_scopes[0].deleted_members = Some(evidence.clone());
            change(&mut native);
            findings(&native).len()
        };
        assert_eq!(findings(&native).len(), 1);
        assert_eq!(claim(&|_| {}), 0);
        let evidence_change = |change: &dyn Fn(&mut DesignDeletedMembers)| {
            claim(&|native| {
                change(
                    native.design_parameter_scopes[0]
                        .deleted_members
                        .as_mut()
                        .unwrap(),
                )
            })
        };
        assert_eq!(
            evidence_change(&|evidence| evidence.entries[1].record_index.value = 303),
            1
        );
        assert_eq!(
            evidence_change(&|evidence| evidence.entries[1].record_index.offset += 1),
            1
        );
        assert_eq!(
            evidence_change(&|evidence| evidence.owner.record_index.value = 18),
            1
        );
        assert_eq!(evidence_change(&|evidence| evidence.members = vec![303]), 1);
        assert_eq!(
            evidence_change(&|evidence| evidence.members = vec![302, 302]),
            1
        );
        assert_eq!(evidence_change(&|evidence| evidence.members.clear()), 1);
        assert_eq!(
            evidence_change(&|evidence| evidence.table_bytes[31 + 19 + 1] = 0x2f),
            1
        );
        assert_eq!(evidence_change(&|evidence| evidence.table_bytes[31] = 0), 1);
        assert_eq!(
            evidence_change(&|evidence| evidence.table_bytes[6] = b'4'),
            1
        );
        assert_eq!(evidence_change(&|evidence| evidence.table_bytes.push(0)), 1);
        assert_eq!(
            evidence_change(&|evidence| {
                evidence.table_bytes.pop();
            }),
            1
        );
        assert_eq!(
            evidence_change(&|evidence| {
                evidence.table_byte_offset = u64::MAX;
                evidence.entries.clear();
            }),
            1
        );
        assert_eq!(
            claim(&|native| {
                native.design_record_headers.pop();
            }),
            1
        );
        assert_eq!(
            claim(&|native| {
                native.design_record_headers.push(DesignRecordHeader {
                    id: "f3d:Design/BulkStream.dat:design-record-header#302".into(),
                    record_index: 302,
                    class_tag: "256".to_owned().try_into().unwrap(),
                    byte_offset: 302,
                });
            }),
            1
        );
    }
}
