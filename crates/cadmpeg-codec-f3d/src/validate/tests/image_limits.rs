// SPDX-License-Identifier: Apache-2.0

fn design_type(module: &str) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#1".into(),
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 1,
            type_guid: "11111111-2222-3333-4444-555555555555"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 4,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 1,
            version_offset: 44,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 17,
                    offset: 100,
                },
            ]),
        },
    )
    .unwrap()
}

fn image_index_error(module: &str, canvas: bool, cap: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_types.push(design_type(module));
        if canvas {
            native.design_canvas_images.push(canvas_image());
        } else {
            native.design_decal_images.push(decal_image());
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        if canvas {
            super::super::validate_canvas_images(&ctx, &mut Vec::new()).unwrap_err()
        } else {
            super::super::validate_decal_images(&ctx, &mut Vec::new()).unwrap_err()
        }
    })
}

fn canvas_image() -> crate::records::canvas::DesignCanvasImage {
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75_f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0_f64),
        (13, 2.0),
        (21, 3.0),
        (29, 1.0),
        (37, 0.0),
        (45, 0.0),
        (53, 0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut prologue = [0; 15];
    prologue[14] = 1;
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:canvas#1", "scope_record_index": 103,
        "scope_reference_offset": 247, "geometry_class_tag": "256",
        "geometry_record_index": 101, "geometry_reference_offset": 424,
        "geometry_byte_offset": 100, "geometry_prologue": prologue,
        "visible": true, "visibility_offset": 125,
        "geometry_frame_length": 229, "paired_geometry_class_tag": "257",
        "paired_geometry_byte_offset": 329, "paired_component_reference_offset": 349,
        "boundary_segments": [[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],
                              [{"u":-2.0,"v":4.0},{"u":3.0,"v":4.0}]],
        "boundary_coordinate_offsets": [126,134,142,150,281,289,297,305],
        "second_boundary_present_offset": 280, "plane_entity_suffix": 200,
        "plane_reference_offset": 159, "component_entity_suffix": 201,
        "component_reference_offset": 258, "asset_class_tag": "258",
        "asset_record_index": 102, "asset_reference_offset": 270,
        "asset_byte_offset": 359, "asset_name": "image.png",
        "asset_name_offset": 384, "label": "Canvas", "label_offset": 317,
        "opacity": 0.75, "origin": {"x":10.0,"y":20.0,"z":30.0},
        "u_axis": {"x":1.0,"y":0.0,"z":0.0},
        "v_axis": {"x":0.0,"y":0.0,"z":1.0},
        "geometry_payload": payload.as_slice()
    }))
    .unwrap()
}

fn decal_image() -> crate::records::decal::DesignDecalImage {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:decal#1",
        "scope_record_index": 23, "asset_reference_offset": 222,
        "mapping_mode": 96, "mapping_mode_offset": 232,
        "target_group_record_index": 24, "target_group_reference_offset": 234,
        "asset_class_tag": "258", "asset_record_index": 17,
        "asset_byte_offset": 100, "asset_frame_length": 30,
        "asset_entity_suffix": 50, "asset_entity_reference_offset": 120,
        "name_class_tag": "279", "name_record_index": 18,
        "name_byte_offset": 130, "name_frame_length": 41,
        "asset_name": "mark.png", "asset_name_offset": 155
    }))
    .unwrap()
}

fn scope(
    kind: crate::records::feature::scope::DesignScopePayload,
    record_index: u32,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignParameterScope, DesignParameterScopeDraft};
    DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
            byte_offset: 100,
            class_tag: "301".to_owned().try_into().unwrap(),
            record_index,
            frame_length: 200,
            kind_offset: 0,
            payload: kind,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 150,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![17]),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "261".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        }
        .with_fixture_layout(),
    )
    .unwrap()
}

fn image_record_error(
    canvas: bool,
    with_scope: bool,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        if canvas {
            native.design_canvas_images.push(canvas_image());
            if with_scope {
                native.design_parameter_scopes.push(scope(
                    crate::records::feature::scope::DesignScopePayload::Canvas,
                    103,
                ));
            }
        } else {
            native.design_decal_images.push(decal_image());
            if with_scope {
                native.design_parameter_scopes.push(scope(
                    crate::records::feature::scope::DesignScopePayload::Decal,
                    23,
                ));
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        if canvas {
            super::super::validate_canvas_images(&ctx, &mut Vec::new()).unwrap_err()
        } else {
            super::super::validate_decal_images(&ctx, &mut Vec::new()).unwrap_err()
        }
    })
}

#[test]
fn canvas_geometry_entity_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Canvas geometry entities",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(image_index_error(
                crate::records::entity_header::DESIGN_MODULE_BODY,
                true,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Canvas geometry entities")
    );
}

#[test]
fn canvas_component_entity_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Canvas component entities",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(image_index_error(
                crate::records::entity_header::DESIGN_MODULE_COMPONENT,
                true,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Canvas component entities")
    );
}

#[test]
fn decal_fusion_entity_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Decal fusion entities",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(image_index_error(
                crate::records::entity_header::DESIGN_MODULE_FUSION,
                false,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Decal fusion entities")
    );
}

#[test]
fn canvas_scope_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Canvas scopes",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(true, true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Canvas scopes")
    );
}

#[test]
fn canvas_geometry_record_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Canvas geometry records",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(true, true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Canvas geometry records")
    );
}

#[test]
fn canvas_invalid_finding_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(true, false, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn canvas_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(true, false, u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn decal_scope_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Decal scopes",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(false, true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Decal scopes")
    );
}

#[test]
fn decal_asset_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D Decal assets",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(false, true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D Decal assets")
    );
}

#[test]
fn decal_invalid_finding_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(false, false, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn decal_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(image_record_error(false, false, u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn image_work_error(canvas: bool, operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        if canvas {
            native.design_canvas_images.push(canvas_image());
            native.design_parameter_scopes.push(scope(
                crate::records::feature::scope::DesignScopePayload::Canvas,
                103,
            ));
        } else {
            native.design_decal_images.push(decal_image());
            native.design_parameter_scopes.push(scope(
                crate::records::feature::scope::DesignScopePayload::Decal,
                23,
            ));
        }
        crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |decode| {
                let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
                ctx.decode = decode;
                if canvas {
                    super::super::validate_canvas_images(&ctx, &mut Vec::new())
                } else {
                    super::super::validate_decal_images(&ctx, &mut Vec::new())
                }
            },
        )
    })
}

#[test]
fn canvas_scope_lookup_preserves_work_refusal() {
    let error = image_work_error(true, "find F3D Canvas scope");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D Canvas scope")
    );
}

#[test]
fn decal_scope_lookup_preserves_work_refusal() {
    let error = image_work_error(false, "find F3D Decal scope");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D Decal scope")
    );
}

#[test]
fn decal_unnamed_projected_asset_skips_name_comparison() {
    use cadmpeg_ir::features::{
        DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };
    crate::test_support::with_decode_context(|decode| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let face = ir.model.faces[0].id.clone();
        let asset = cadmpeg_ir::assets::Asset::try_new(
            decode,
            cadmpeg_ir::assets::AssetId::mint("test:model:asset#decal").unwrap(),
            None,
            None,
            cadmpeg_ir::assets::AssetContent::External {
                uri: cadmpeg_core::text::NonBlankString::try_from("mark.png").unwrap(),
            },
            None,
        )
        .unwrap();
        let mut native = super::construction_group_limits::native(false, false);
        let scope = scope(
            crate::records::feature::scope::DesignScopePayload::Decal,
            10,
        );
        let scope_id = scope.id.clone();
        native.design_parameter_scopes.push(scope);
        let mut image_wire = serde_json::to_value(decal_image()).unwrap();
        image_wire["scope_record_index"] = serde_json::json!(10);
        image_wire["target_group_record_index"] = serde_json::json!(100);
        native
            .design_decal_images
            .push(serde_json::from_value(image_wire).unwrap());
        let mut draft = super::body_recipe_limits::operand().into_draft();
        draft.scope_record_index = 10;
        draft.record_index = 101;
        draft.id = "f3d:Design/BulkStream.dat:design-body-recipe-operand#101".into();
        draft.nested_record_index = 104;
        draft.next_record_index = 105;
        draft.owner = crate::records::topology::body_recipe::DesignOperandOwner::Group {
            group_record_index: 100,
            group_member_ordinal: 0,
        };
        draft.references[0].candidate_faces.push(face.clone());
        let operand =
            crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(draft).unwrap();
        let operand_id = operand.id.clone();
        native.design_body_recipe_operands.push(operand);
        ir.model.features.push(Feature {
            id: FeatureId::mint("test:model:feature#decal").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            native_ref: Some(scope_id),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Decal {
                    asset: asset.id.clone(),
                    faces: FaceSelection::Resolved {
                        faces: vec![face],
                        native: operand_id,
                    },
                    mapping: DecalMapping::FitToFaces,
                    opacity: None,
                },
            )),
        });
        ir.model.assets.push(asset);
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "compare F3D Decal asset name",
            None,
        );
        let mut findings = Vec::new();
        super::super::validate_decal_images(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Decal image has an invalid frame or Design object join"
        );
        assert!(decode.resource_refusal().is_none());
    });
}
