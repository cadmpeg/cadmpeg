// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn assembly_target_header_lookup_preserves_work_refusal() {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D assembly target header",
        0,
        |decode| {
            super::super::design_header_matches(
                decode,
                &std::collections::HashMap::new(),
                "design-stream",
                1,
                "307",
                0,
            )
        },
    );
}

#[test]
fn bounded_class_tag_matches_skip_comparison_admission() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |decode| {
        let header = crate::records::decal::DesignRecordHeader {
            id: "f3d:test:design-record-header#1".into(),
            record_index: 1,
            class_tag: "264".to_owned().try_into().unwrap(),
            byte_offset: 17,
        };
        let headers = std::collections::HashMap::from([(("f3d:test", 1), &header)]);
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "compare F3D assembly target class tag",
            None,
        );
        assert!(
            super::super::design_header_matches(decode, &headers, "f3d:test", 1, "264", 17)
                .unwrap()
        );
        assert!(!super::super::design_header_matches(
            decode,
            &headers,
            "f3d:test",
            1,
            &"264".repeat(100),
            17
        )
        .unwrap());
        assert!(decode.resource_refusal().is_none());
        let error = decode
            .equal(
                "positive control",
                "positive control",
                "compare F3D assembly target class tag",
            )
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "compare F3D assembly target class tag")
        );
    });
}

#[test]
fn axial_guid_offset_overflow_skips_work_admission() {
    use crate::records::feature::{
        assembly::DesignAssemblyAxialSelectorIdentity,
        scope::{DesignFeatureKind, DesignParameterScope},
    };
    let guid = "11111111-2222-3333-4444-555555555555";
    let selector: DesignAssemblyAxialSelectorIdentity = serde_json::from_value(serde_json::json!({
        "axis_record_index": 0, "axis_class_tag": "316", "axis_byte_offset": 0,
        "axis_paired_class_tag": "261", "axis_paired_byte_offset": 0,
        "selector_record_index": 3, "selector_class_tag": "277", "selector_byte_offset": 0,
        "selector_paired_class_tag": "261", "selector_paired_byte_offset": 0,
        "nested_record_index": 6, "nested_record_index_offset": 0,
        "selector_asset_id": guid, "selector_asset_id_offset": u64::MAX,
        "selector_context_id": guid, "selector_context_id_offset": 0,
        "occurrence_reference": 0, "occurrence_reference_offset": 0,
        "external_object_reference": 0, "external_object_reference_offset": 0,
        "external_segment": 0, "external_segment_offset": 0,
        "external_asset_id": guid, "external_asset_id_offset": 0,
        "external_link_name": "link", "external_link_name_offset": 0,
        "role_record_index": 8, "role_class_tag": "298", "role_byte_offset": 0,
        "occurrence_role": guid, "occurrence_role_offset": 0
    }))
    .unwrap();
    let scope = DesignParameterScope::empty("f3d:test:scope#1", DesignFeatureKind::Hole, 1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!super::super::valid_axial_selector_identity(
        &decode,
        &std::collections::HashMap::new(),
        "f3d:test",
        &scope,
        &selector,
        u64::MAX
    )
    .unwrap());
}

#[test]
fn joint_origin_builds_only_the_assembly_index_it_reads() {
    use crate::records::feature::scope::{
        DesignJointOriginReference, DesignJointOriginTransform, DesignParameterScope,
        DesignScopePayload,
    };
    for referenced in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |decode| {
            let ir = cadmpeg_ir::CadIr::empty();
            let origin = DesignJointOriginTransform {
                joint_origin_transform:
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                joint_origin_transform_offset: 60,
                reference: referenced.then_some(DesignJointOriginReference {
                    joint_origin_reference: 2,
                    joint_origin_reference_offset: 46,
                }),
            };
            let native = crate::native::F3dNative {
                design_parameter_scopes: vec![DesignParameterScope::empty(
                    "f3d:test:scope#1",
                    DesignScopePayload::JointOrigin(Some(origin)),
                    1,
                )],
                ..Default::default()
            };
            let ctx = crate::validate::Ctx::new(&ir, &native, decode).unwrap();
            let operation = if referenced {
                "group F3D assembly scopes by frame"
            } else {
                "group F3D assembly scopes by offset"
            };
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                None,
            );
            let mut findings = Vec::new();
            super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            assert!(decode.resource_refusal().is_none());
            let mut storage = decode
                .reserve_scoped(0, "hold test assembly index")
                .unwrap();
            let error = if referenced {
                super::super::assembly_scopes_by_frame(&ctx, &mut storage).unwrap_err()
            } else {
                super::super::assembly_scopes_by_offset(&ctx, &mut storage).unwrap_err()
            };
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == operation)
            );
        });
    }
}
