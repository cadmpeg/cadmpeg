// SPDX-License-Identifier: Apache-2.0
//! Texture query storage and independently retained appearance output.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_protein::property::{DecodedProperty, PropertyContent, PropertyValue};

const SCRATCH_BYTES: u64 = 64 * 1024;
const OUTPUT_BYTES: u64 = 64 * 1024;
const DIAGNOSTIC_OPERATION: &str = "retain F3D malformed diagnostic";

fn texture_fixture(path_first: char) -> cadmpeg_protein::DecodedRecord {
    let property = |value| DecodedProperty {
        value_offset: 0,
        content: PropertyContent::Value {
            value,
            connections: Vec::new(),
        },
    };
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: "UnifiedBitmapSchema".into(),
        guid: "asset".repeat(256),
        base: "Bitmap".into(),
        asset_lib_id: String::new(),
        properties: BTreeMap::from([
            (
                "texture_Bitmap".into(),
                property(PropertyValue::TextureUri(vec![
                    format!("{path_first}{}", "p".repeat(2047)),
                    "unvisited".repeat(256),
                ])),
            ),
            (
                "texture_Bitmap_urn".into(),
                property(PropertyValue::String("urn".repeat(256))),
            ),
        ]),
    }
}

fn assert_original_refusal(
    ctx: &DecodeContext<'_>,
    error: CodecError,
    dimension: ResourceDimension,
    operation: &str,
) {
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal required");
    };
    assert_eq!(limit.dimension, dimension);
    assert_eq!(limit.operation, operation);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(1, "later texture query"), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
    assert!(
        matches!(super::super::appearances_from_schema_records(ctx, &[]), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn material_texture_lifetime_discarded_single_and_equal_duplicates_retain_nothing() {
    let record = texture_fixture('a');
    for records in [vec![record.clone()], vec![record.clone(), record.clone()]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = SCRATCH_BYTES;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(record.guid.as_bytes(), &arena, &policy).unwrap();
        for _ in 0..7 {
            let storage;
            let (appearances, count, held_storage) =
                super::super::appearances_from_schema_records(&ctx, &records).unwrap();
            storage = held_storage;
            assert!(appearances.is_empty());
            assert_eq!(count, 0);
            drop(appearances);
            drop(storage);
            drop(
                ctx.reserve_scoped(SCRATCH_BYTES, "texture scratch released")
                    .unwrap(),
            );
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn material_texture_lifetime_conflict_keeps_exact_diagnostic_and_releases_query_storage() {
    let records = [texture_fixture('a'), texture_fixture('b')];
    let expected = format!(
        "Protein texture asset {} has conflicting payloads",
        records[0].guid
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = u64::try_from(expected.len()).unwrap();
    let (ctx, _) =
        DecodeContext::from_root_bytes(records[0].guid.as_bytes(), &arena, &policy).unwrap();
    let CodecError::Malformed(message) =
        super::super::appearances_from_schema_records(&ctx, &records).unwrap_err()
    else {
        panic!("conflicting payloads");
    };
    assert_eq!(message, expected);
    assert!(ctx.resource_refusal().is_none());
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "conflict texture scratch released")
            .unwrap(),
    );
    let error = ctx
        .charge_retained(1, "diagnostic remains retained")
        .unwrap_err();
    let CodecError::ResourceLimit(limit) = &error else {
        panic!("retained diagnostic");
    };
    assert_eq!(limit.used, u64::try_from(expected.len()).unwrap());
    assert_eq!(limit.additional, 1);
    assert_original_refusal(
        &ctx,
        error,
        ResourceDimension::RetainedBytes,
        "diagnostic remains retained",
    );
}

#[test]
fn material_texture_lifetime_conflict_one_below_preserves_original_retained_refusal() {
    let records = [texture_fixture('a'), texture_fixture('b')];
    let bytes =
        "Protein texture asset ".len() + records[0].guid.len() + " has conflicting payloads".len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = u64::try_from(bytes - 1).unwrap();
    let (ctx, _) =
        DecodeContext::from_root_bytes(records[0].guid.as_bytes(), &arena, &policy).unwrap();
    let error = super::super::appearances_from_schema_records(&ctx, &records).unwrap_err();
    let CodecError::ResourceLimit(limit) = &error else {
        panic!("diagnostic storage refusal");
    };
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, u64::try_from(bytes).unwrap());
    assert_original_refusal(
        &ctx,
        error,
        ResourceDimension::RetainedBytes,
        DIAGNOSTIC_OPERATION,
    );
}

#[test]
fn material_texture_lifetime_conflict_formatter_preserves_original_work_refusal() {
    let records = [texture_fixture('a'), texture_fixture('b')];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) =
        DecodeContext::from_root_bytes(records[0].guid.as_bytes(), &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        DIAGNOSTIC_OPERATION,
        None,
    );
    let error = super::super::appearances_from_schema_records(&ctx, &records).unwrap_err();
    assert_original_refusal(
        &ctx,
        error,
        ResourceDimension::WorkUnits,
        DIAGNOSTIC_OPERATION,
    );
}

#[test]
fn material_texture_lifetime_final_reference_preserves_payload_and_independent_retained_output() {
    let texture = texture_fixture('a');
    let appearance = super::appearance_connected_to(&texture.guid);
    let paths = [format!("a{}", "p".repeat(2047)), "unvisited".repeat(256)];
    let urn = "urn".repeat(256);
    let records = [appearance, texture.clone(), texture];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = OUTPUT_BYTES;
    let (ctx, _) =
        DecodeContext::from_root_bytes(records[0].guid.as_bytes(), &arena, &policy).unwrap();
    let storage;
    let (appearances, count, held_storage) =
        super::super::appearances_from_schema_records(&ctx, &records).unwrap();
    storage = held_storage;
    assert_eq!(count, 0);
    assert_eq!(appearances.len(), 1);
    assert_eq!(
        appearances[0].id.as_str(),
        format!("f3d:design:appearance#{}", records[0].guid)
    );
    assert_eq!(appearances[0].textures.len(), 1);
    let reference = &appearances[0].textures[0];
    assert_eq!(reference.asset_guid, records[1].guid);
    assert_eq!(reference.schema, "UnifiedBitmapSchema");
    assert_eq!(reference.slot, "generic_diffuse");
    assert_eq!(reference.paths, paths);
    assert_eq!(reference.urn.as_deref(), Some(urn.as_str()));
    drop(appearances);
    drop(storage);
    drop(
        ctx.reserve_scoped(SCRATCH_BYTES, "final texture query scratch released")
            .unwrap(),
    );
    let error = ctx
        .charge_retained(OUTPUT_BYTES, "appearance remains retained")
        .unwrap_err();
    let CodecError::ResourceLimit(limit) = &error else {
        panic!("final appearance output");
    };
    assert!(limit.used > 0);
    assert_original_refusal(
        &ctx,
        error,
        ResourceDimension::RetainedBytes,
        "appearance remains retained",
    );
}
