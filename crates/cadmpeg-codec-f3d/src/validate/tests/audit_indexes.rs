// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn unlimited_work() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy
}

fn parameter() -> crate::records::parameters::DesignParameter {
    use crate::records::parameters::{
        DesignParameter, DesignParameterDraft, DesignParameterSource,
    };
    DesignParameter::try_from(DesignParameterDraft::<String> {
        id: "f3d:test:parameter#7".into(),
        byte_offset: 20,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index: 7,
        source_ordinal: 0,
        source: DesignParameterSource::new::<String>("Dimension".into(), Some(6), None).unwrap(),
        expression: "1".into(),
        expression_offset: 32,
        source_kind_offset: 52,
        unit: None,
        name: "Dimension".into(),
        name_offset: 82,
        evaluated_value: 1.0,
        evaluated_value_offset: 92,
    })
    .unwrap()
}

#[test]
fn design_stream_search_preserves_unqualified_and_last_separator_semantics() {
    crate::test_support::with_decode_context(|decode| {
        for (id, expected) in [
            ("unqualified", "f3d:design"),
            ("", "f3d:design"),
            (":record", ""),
            ("f3d:stream:record:tail", "f3d:stream:record"),
            ("f3d:流:record", "f3d:流"),
        ] {
            assert_eq!(super::super::design_stream(decode, id).unwrap(), expected);
        }
    });
}

#[test]
fn arbitrary_record_id_refuses_before_reverse_stream_search() {
    let id = "unqualified".repeat(4096);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "resolve F3D validation design stream",
        0,
        |decode| super::super::design_stream(decode, &id).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "resolve F3D validation design stream"));
}

#[test]
fn conditional_indexes_are_absent_until_first_use() {
    crate::test_support::with_decode_policy(&unlimited_work(), |decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let native = crate::native::F3dNative {
            design_parameters: vec![parameter()],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        assert!(ctx.parameter_groups.get().is_none());
        assert!(ctx.occurrence_guid_groups.get().is_none());
        assert!(ctx.states_by_id.get().is_none());
        assert!(ctx.neutral_features.get().is_none());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "group F3D design parameters",
            None,
        );
        let error = ctx
            .parameter_group("f3d:test", 7, "find test parameter")
            .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "group F3D design parameters"));
        assert!(ctx.parameter_groups.get().is_none());
    });
}

#[test]
fn parameter_groups_preserve_arena_order_last_record_and_reuse() {
    crate::test_support::with_decode_policy(&unlimited_work(), |decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let native = crate::native::F3dNative {
            design_parameters: vec![parameter(), parameter()],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let group = ctx
            .parameter_group("f3d:test", 7, "find test parameter")
            .unwrap();
        assert_eq!(group.len(), 2);
        assert!(std::ptr::eq(
            group[0],
            &raw const native.design_parameters[0]
        ));
        assert!(std::ptr::eq(
            *group.last().unwrap(),
            &raw const native.design_parameters[1]
        ));
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "group F3D design parameters",
            None,
        );
        assert_eq!(
            ctx.parameter_group("f3d:test", 7, "find test parameter")
                .unwrap()
                .len(),
            2
        );
        assert!(decode.resource_refusal().is_none());
        assert!(decode
            .admit_iter(&native.design_parameters, "group F3D design parameters")
            .is_err());
    });
}

#[test]
fn shared_scope_and_operand_groups_preserve_first_and_last_records() {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let mut native = super::construction_group_limits::native(true, false);
        native
            .design_parameter_scopes
            .push(native.design_parameter_scopes[0].clone());
        native
            .design_construction_operand_groups
            .push(native.design_construction_operand_groups[0].clone());
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let scopes = ctx
            .scope_group("f3d:Design/BulkStream.dat", 10, "find test scope")
            .unwrap();
        assert_eq!(scopes.len(), 2);
        assert!(std::ptr::eq(
            scopes[0],
            &raw const native.design_parameter_scopes[0]
        ));
        assert!(std::ptr::eq(
            *scopes.last().unwrap(),
            &raw const native.design_parameter_scopes[1]
        ));
        let record_index = native.design_construction_operand_groups[0].record_index;
        let groups = ctx
            .operand_group_records("f3d:Design/BulkStream.dat", record_index, "find test group")
            .unwrap();
        assert_eq!(groups.len(), 2);
        assert!(std::ptr::eq(
            groups[0],
            &raw const native.design_construction_operand_groups[0]
        ));
        assert!(std::ptr::eq(
            *groups.last().unwrap(),
            &raw const native.design_construction_operand_groups[1]
        ));
    });
}

#[test]
fn occurrence_guid_groups_build_once_and_preserve_ascii_folded_order() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    crate::test_support::with_decode_policy(&unlimited_work(), |decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let occurrence = |guid: &str| {
            DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
                id: "f3d:test:occurrence#1".into(),
                class_tag: "256".to_owned().try_into().unwrap(),
                record_index: 1,
                byte_offset: 0,
                component_record_index: 2,
                component_guid: "11111111-2222-4333-8444-555555555555"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                occurrence_guid: guid.to_owned().try_into().unwrap(),
                placement: DesignComponentOccurrencePlacement::Base,
            })
            .unwrap()
        };
        let lower = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let upper = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        let native = crate::native::F3dNative {
            design_component_occurrences: vec![occurrence(lower), occurrence(upper)],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        assert!(ctx.occurrence_guid_groups.get().is_none());
        assert!(ctx
            .occurrences_with_guid("f3d:test", &"x".repeat(39), "find test GUID")
            .unwrap()
            .is_empty());
        assert!(ctx.occurrence_guid_groups.get().is_none());
        let group = ctx
            .occurrences_with_guid("f3d:test", lower, "find test GUID")
            .unwrap();
        assert_eq!(group.len(), 2);
        assert!(std::ptr::eq(
            group[0],
            &raw const native.design_component_occurrences[0]
        ));
        assert!(std::ptr::eq(
            group[1],
            &raw const native.design_component_occurrences[1]
        ));
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "group F3D component occurrences by GUID",
            None,
        );
        assert_eq!(
            ctx.occurrences_with_guid("f3d:test", upper, "find test GUID")
                .unwrap()
                .len(),
            2
        );
        assert!(decode.resource_refusal().is_none());
        assert!(decode
            .admit_iter(
                &native.design_component_occurrences,
                "group F3D component occurrences by GUID"
            )
            .is_err());
    });
}

#[test]
fn state_index_builds_once_and_keeps_repeated_id_tombstones() {
    use crate::history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache};
    crate::test_support::with_decode_policy(&unlimited_work(), |decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let state = AsmDeltaState {
            id: "f3d:test:state#1".into(),
            parent: "f3d:test:history#1".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Released,
            transition: None,
        };
        let native = crate::native::F3dNative {
            asm_histories: vec![AsmHistory {
                id: "f3d:test:history#1".into(),
                byte_offset: 0,
                preamble: None,
                record_table_binding_budget_exceeded: false,
                states: vec![state.clone(), state.clone(), state],
            }],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        assert!(ctx.states_by_id.get().is_none());
        assert!(matches!(ctx.states().unwrap().get(&1), Some(None)));
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "index F3D validation states",
            None,
        );
        assert!(matches!(ctx.states().unwrap().get(&1), Some(None)));
        assert!(decode.resource_refusal().is_none());
        assert!(decode
            .admit_iter(
                &native.asm_histories[0].states,
                "index F3D validation states"
            )
            .is_err());
    });
}

#[test]
fn neutral_feature_group_is_shared_preserves_order_and_reuses_storage() {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    crate::test_support::with_decode_policy(&unlimited_work(), |decode| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        for ordinal in 0..2 {
            ir.model.features.push(Feature {
                id: FeatureId::mint(format!("test:model:feature#{ordinal}")).unwrap(),
                ordinal,
                name: None,
                suppressed: None,
                dependencies: Default::default(),
                source_properties: Default::default(),
                source_tag: None,
                source_text: None,
                source_content: Default::default(),
                native_ref: Some("f3d:test:scope#1".into()),
                evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                    FeatureOperation::MeshImport {
                        tessellations: vec!["test:model:tessellation#1".into()].try_into().unwrap(),
                    },
                )),
            });
        }
        let native = crate::native::F3dNative::default();
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        assert!(ctx.neutral_features.get().is_none());
        let group =
            super::super::neutral_features_with_native_ref(&ctx, "f3d:test:scope#1").unwrap();
        assert_eq!(group.len(), 2);
        assert!(std::ptr::eq(group[0], &raw const ir.model.features[0]));
        assert!(std::ptr::eq(group[1], &raw const ir.model.features[1]));
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "group F3D neutral features by native reference",
            None,
        );
        assert_eq!(
            super::super::neutral_features_with_native_ref(&ctx, "f3d:test:scope#1")
                .unwrap()
                .len(),
            2
        );
        assert!(decode.resource_refusal().is_none());
        assert!(decode
            .admit_iter(
                &ir.model.features,
                "group F3D neutral features by native reference"
            )
            .is_err());
    });
}

#[test]
fn design_segment_uses_the_parsed_stream_and_admits_its_reverse_search() {
    crate::test_support::with_decode_context(|decode| {
        for (stream, expected) in [
            ("f3d:design", None),
            ("f3d:Design/MetaStream.dat", Some("f3d:Design")),
            ("f3d:Design/BulkStream.dat", Some("f3d:Design")),
            ("f3d:Design/Other.dat", None),
        ] {
            assert_eq!(
                super::super::design_segment(decode, stream).unwrap(),
                expected
            );
        }
    });
    let stream = "unqualified".repeat(4096);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "resolve F3D validation design segment",
        0,
        |decode| super::super::design_segment(decode, &stream).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "resolve F3D validation design segment"));
}
