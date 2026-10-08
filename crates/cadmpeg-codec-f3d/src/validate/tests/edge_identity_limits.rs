// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        references::DesignClassTag,
    };
    let mut native = super::operand_group_carrier_limits::native_with_identity();
    if valid {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#10",
                DesignFeatureKind::Fillet,
                10,
            ));
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#101".into(),
            record_index: 101,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            byte_offset: 100,
        });
    }
    native
}

fn edge_result(
    valid: bool,
    max_items: u64,
    max_retained: u64,
) -> Result<(), cadmpeg_core::CodecError> {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid);
        if valid {
            native
                .store(
                    &cadmpeg_test_support::service_decode_context(),
                    ir.native.namespace_mut("f3d"),
                )
                .unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_edge_identity_operands(&ctx, &mut Vec::new(), &[]).map(|_| ())
    })
}

/// Refuse `operation` at its boundary in `dimension`, with every earlier charge admitted.
fn edge_refusal(
    valid: bool,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
) -> cadmpeg_core::CodecError {
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| match dimension {
        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
            edge_result(valid, cap, u64::MAX)
        }
        _ => edge_result(valid, u64::MAX, cap),
    })
}

#[test]
fn edge_identity_expected_index_refuses_collection_limit() {
    let error = edge_refusal(
        true,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D expected edge identity operands",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge identity operands")
    );
}

#[test]
fn edge_identity_slot_refuses_collection_limit() {
    let error = edge_refusal(
        true,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D edge identity slots",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge identity slots")
    );
}

#[test]
fn edge_identity_record_refuses_collection_limit() {
    let error = edge_refusal(
        true,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D edge identity records",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge identity records")
    );
}

#[test]
fn edge_identity_invalid_finding_refuses_collection_limit() {
    let error = edge_refusal(
        false,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn edge_identity_invalid_entity_refuses_retained_limit() {
    let error = edge_refusal(
        false,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn edge_identity_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        native
            .store(
                &cadmpeg_test_support::service_decode_context(),
                ir.native.namespace_mut("f3d"),
            )
            .unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        let (records, _records_storage) =
            super::super::validate_edge_identity_operands(&ctx, &mut findings, &[]).unwrap();
        assert!(findings.is_empty());
        assert!(records.contains(&("f3d:Design/BulkStream.dat", 101)));
    })
}

#[test]
fn edge_identity_absent_expected_record_skips_child_comparison() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "compare F3D expected edge identity operand",
            None,
        );
        let mut findings = Vec::new();
        let (records, _storage) =
            super::super::validate_edge_identity_operands(&ctx, &mut findings, &[]).unwrap();
        assert!(records.is_empty());
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design edge identity operand has an invalid fixed frame"
        );
        assert!(decode.resource_refusal().is_none());
        let error = decode
            .equal(
                "positive control",
                "positive control",
                "compare F3D expected edge identity operand",
            )
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "compare F3D expected edge identity operand")
        );
    });
}

#[test]
fn edge_identity_present_expected_record_refuses_before_child_comparison() {
    crate::test_support::with_decode_context(|service| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        native
            .store(service, ir.native.namespace_mut("f3d"))
            .unwrap();
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "compare F3D expected edge identity operand",
            0,
            |decode| {
                let mut ctx = super::super::Ctx::new(&ir, &native, service)?;
                ctx.decode = decode;
                super::super::validate_edge_identity_operands(&ctx, &mut Vec::new(), &[])
                    .map(|_| ())
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compare F3D expected edge identity operand")
        );
    });
}

#[test]
fn edge_identity_reload_and_indexes_release_their_scoped_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    crate::test_support::with_decode_context(|service| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        native
            .store(service, ir.native.namespace_mut("f3d"))
            .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1 << 20;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service).unwrap();
        ctx.decode = &decode;
        let mut findings = Vec::new();
        let (records, storage) =
            super::super::validate_edge_identity_operands(&ctx, &mut findings, &[]).unwrap();
        assert!(findings.is_empty());
        assert!(records.contains(&("f3d:Design/BulkStream.dat", 101)));
        drop((records, storage));
        decode
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "verify released edge identity scratch",
            )
            .unwrap();
    });
}
