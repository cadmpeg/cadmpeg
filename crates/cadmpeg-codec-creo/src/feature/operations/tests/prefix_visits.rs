// SPDX-License-Identifier: Apache-2.0

use super::super::{recipe_bindings, reference_names, FeatureRecipe};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const REFERENCE_HEADER: &[u8] = b"\xf7\x71\x01\x05\x02";
const RECIPE_HEADER: &[u8] = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73";

fn check_empty_result(
    payload: &[u8],
    outer: usize,
    visits: usize,
    parse: impl Fn(&DecodeContext<'_>, &[u8]) -> Result<bool, CodecError>,
    outer_operation: &'static str,
    prefix_operation: &'static str,
) {
    let total = (outer + visits) as u64;
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = parse(&ctx, payload);
        if cap == total {
            assert!(result.expect("exact visits admit the empty result"));
            assert_eq!(ctx.resource_refusal(), None);
            let original = ctx.charge_work_limit(1, "after bounded prefix")
                .expect_err("no unvisited work remains");
            assert_eq!((original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, total, 1));
        } else {
            let original = ctx.resource_refusal().expect("executed work refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            let expected = if cap < outer as u64 {
                (0, outer as u64, outer_operation)
            } else {
                (cap, 1, prefix_operation)
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(parse(&ctx, payload),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn reference_prefix_admits_only_present_bytes_through_terminator_or_control() {
    for length in [0, 1, 2, 17, 255, 256, 300] {
        let mut payload = REFERENCE_HEADER.to_vec();
        payload.extend(std::iter::repeat_n(b'N', length));
        check_empty_result(&payload, payload.len() - 2, length.min(256),
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan", "creo reference name prefix scan");
        payload.extend_from_slice(b"\0\x01\x02");
        // Mismatched closing IDs prevent output without changing prefix visits.
        check_empty_result(&payload, payload.len() - 2, (length + 1).min(256),
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan", "creo reference name prefix scan");
    }
    for (name, visits) in [(b"\x01ignored\0".as_slice(), 1),
        (b"NN\x7fignored\0".as_slice(), 3)] {
        let mut payload = REFERENCE_HEADER.to_vec();
        payload.extend_from_slice(name);
        payload.extend_from_slice(b"\x01\x01");
        check_empty_result(&payload, payload.len() - 2, visits,
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan", "creo reference name prefix scan");
    }
}

#[test]
fn recipe_prefix_admits_present_display_bytes_with_the_original_96_byte_bound() {
    for length in [0, 1, 2, 17, 95, 96, 120] {
        let mut payload = RECIPE_HEADER.to_vec();
        payload.extend(std::iter::repeat_n(b'D', length));
        check_empty_result(&payload, payload.len(), length.min(96),
            |ctx, bytes| recipe_bindings(ctx, bytes).map(|rows| rows.is_empty()),
            "creo recipe binding scan", "creo recipe display prefix scan");
        payload.extend_from_slice(b"\0\xf6\0unknown\0");
        check_empty_result(&payload, payload.len(), (length + 1).min(96),
            |ctx, bytes| recipe_bindings(ctx, bytes).map(|rows| rows.is_empty()),
            "creo recipe binding scan", "creo recipe display prefix scan");
    }
}

#[test]
fn reference_prefix_preserves_closed_native_identity_and_byte_copy_work() {
    for length in [1, 17, 255] {
        let mut payload = REFERENCE_HEADER.to_vec();
        let name = vec![0xff; length];
        payload.extend_from_slice(&name);
        payload.extend_from_slice(b"\0\x01\x01");
        // One source scan, one prefix pass including NUL, one retained byte copy.
        let total = (payload.len() - 2 + length + 1 + length) as u64;
        for cap in [total - 1, total] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = reference_names(&ctx, &payload);
            if cap == total {
                let rows = result.expect("exact scan and copy work");
                assert_eq!(rows.len(), 1);
                let row = &rows[0];
                assert_eq!((row.feature_id, row.own_reference_id, row.reference_type, row.offset),
                    (2, 1, 5, 0));
                assert_eq!(row.name_bytes, name);
                let original = ctx.charge_work_limit(1, "after reference output")
                    .expect_err("exact work bound");
                assert_eq!((original.used, original.additional), (total, 1));
            } else {
                let original = ctx.resource_refusal().expect("byte copy refuses");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, total - length as u64, length as u64,
                        "creo reference name bytes"));
            }
            let original = ctx.resource_refusal().expect("original refusal");
            assert!(matches!(reference_names(&ctx, &payload),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn recipe_prefix_preserves_binding_identity_and_nonempty_display_grammar() {
    for length in [1, 17, 95] {
        let mut payload = RECIPE_HEADER.to_vec();
        // Display bytes are arbitrary non-NUL bytes, including ASCII controls.
        payload.extend(std::iter::repeat_n(1, length));
        payload.extend_from_slice(b"\0\xf6\0protextrude\0");
        let total = (payload.len() + length + 1) as u64;
        for cap in [total - 1, total] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = recipe_bindings(&ctx, &payload);
            if cap == total {
                let rows = result.expect("exact source and display work");
                assert_eq!(rows.len(), 1);
                let (id, row) = &rows[0];
                assert_eq!((*id, row.root_schema_class.code(), row.parent_feature_id, row.offset),
                    (8053, 917, 8051, 0));
                assert_eq!(row.recipe, FeatureRecipe::ProtrudeExtrude);
                let original = ctx.charge_work_limit(1, "after recipe output")
                    .expect_err("exact work bound");
                assert_eq!((original.used, original.additional), (total, 1));
            } else {
                let original = ctx.resource_refusal().expect("display terminator refuses");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, 1, "creo recipe display prefix scan"));
            }
            let original = ctx.resource_refusal().expect("original refusal");
            assert!(matches!(recipe_bindings(&ctx, &payload),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}
