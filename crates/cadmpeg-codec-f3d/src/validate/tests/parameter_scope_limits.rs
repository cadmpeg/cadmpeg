// SPDX-License-Identifier: Apache-2.0

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

fn scope_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_parameter_scopes(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn parameter_scope_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D parameter scope records",
        |cap| Err::<(), cadmpeg_core::CodecError>(scope_error(cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D parameter scope records")
    );
}

#[test]
fn parameter_scope_finding_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
        |cap| Err::<(), cadmpeg_core::CodecError>(scope_error(cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn parameter_scope_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(scope_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design parameter scope has an invalid paired frame"
        );
    })
}

#[test]
fn component_pattern_occurrence_checks_preserve_work_refusal() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement,
    };
    use crate::records::feature::patterns::{
        DesignPatternComponentInstance, DesignPatternInstance, DesignRectangularPatternInstances,
    };
    use crate::records::identity::Located;
    use crate::records::sketch_placement::SketchPlacementMatrix;
    let component = "11111111-2222-4333-8444-555555555555";
    let seed_guid = "22222222-3333-4444-8555-666666666666";
    let generated_guid = "33333333-4444-4555-8666-777777777777";
    let occurrence = |index: u32, guid: &str, placement| {
        DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
            id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{index}"),
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index: index,
            byte_offset: u64::from(index) * 10,
            component_record_index: 700,
            component_guid: component.to_owned().try_into().unwrap(),
            occurrence_guid: guid.to_owned().try_into().unwrap(),
            placement,
        })
        .unwrap()
    };
    let native = crate::native::F3dNative {
        design_component_occurrences: vec![
            occurrence(1, seed_guid, DesignComponentOccurrencePlacement::Base),
            occurrence(
                2,
                generated_guid,
                DesignComponentOccurrencePlacement::Explicit {
                    ordinal: std::num::NonZeroU32::new(2).unwrap(),
                    transform: SketchPlacementMatrix::IDENTITY,
                },
            ),
        ],
        ..Default::default()
    };
    let row = |index, guid: &str| DesignPatternComponentInstance {
        instance: DesignPatternInstance {
            record_index: index,
            transform: Located {
                value: SketchPlacementMatrix::IDENTITY,
                offset: u64::from(index) * 10 + 209,
            },
        },
        occurrence_guid: guid.to_owned().try_into().unwrap(),
    };
    let instances = DesignRectangularPatternInstances::Components {
        component_guid: component.to_owned().try_into().unwrap(),
        seed: row(1, seed_guid),
        generated: vec![row(2, generated_guid)],
    };
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    crate::test_support::with_decode_context(|decode| {
        let ctx = super::super::Ctx::new(&ir, &native, decode).unwrap();
        assert!(super::super::valid_component_pattern_occurrences(
            &ctx,
            "f3d:Design/BulkStream.dat",
            &instances
        )
        .unwrap());
        assert!(!super::super::valid_component_pattern_occurrences(
            &ctx,
            "f3d:Other/BulkStream.dat",
            &instances
        )
        .unwrap());
    });
    for operation in [
        "find F3D pattern seed occurrence",
        "find F3D pattern seed occurrence group",
        "validate F3D generated component occurrences",
        "find F3D generated component occurrence",
        "find F3D generated component occurrence group",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |decode| {
                let ctx = super::super::Ctx::new(&ir, &native, decode)?;
                super::super::valid_component_pattern_occurrences(
                    &ctx,
                    "f3d:Design/BulkStream.dat",
                    &instances,
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
        );
    }
}
