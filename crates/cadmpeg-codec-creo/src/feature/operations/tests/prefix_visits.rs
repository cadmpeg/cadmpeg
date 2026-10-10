// SPDX-License-Identifier: Apache-2.0

use super::super::{recipe_bindings, reference_names, FeatureRecipe};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const REFERENCE_HEADER: &[u8] = b"\xf7\x71\x01\x05\x02";
const RECIPE_HEADER: &[u8] = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73";

fn check_result(
    total: u64,
    allocating: bool,
    operations: &[&str],
    parse: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, operations, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        if !allocating {
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        match parse(&ctx) {
            Ok(()) => {
                assert_eq!(ctx.resource_refusal(), None);
                let original = ctx
                    .charge_work_limit(1, "measure bounded prefix work")
                    .expect_err("measurement");
                assert_eq!((original.used, original.additional), (total, 1));
                assert!(
                    matches!(parse(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                Ok(())
            }
            Err(CodecError::ResourceLimit(original)) => {
                assert_eq!(ctx.resource_refusal(), Some(original));
                assert!(
                    matches!(parse(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                Err(CodecError::ResourceLimit(original))
            }
            Err(error) => Err(error),
        }
    });
}

fn check_empty_result(
    payload: &[u8],
    outer: usize,
    parse: impl Fn(&DecodeContext<'_>, &[u8]) -> Result<bool, CodecError>,
    outer_operation: &'static str,
) {
    check_result(outer as u64, false, &[outer_operation], |ctx| {
        assert!(parse(ctx, payload)?);
        Ok(())
    });
}

#[test]
fn reference_prefix_is_free_and_keeps_terminator_and_control_grammar() {
    for length in [0, 1, 2, 17, 255, 256, 300] {
        let mut payload = REFERENCE_HEADER.to_vec();
        payload.extend(std::iter::repeat_n(b'N', length));
        check_empty_result(
            &payload,
            payload.len() - 2,
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan",
        );
        payload.extend_from_slice(b"\0\x01\x02");
        // Mismatched closing IDs prevent output.
        check_empty_result(
            &payload,
            payload.len() - 2,
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan",
        );
    }
    for name in [b"\x01ignored\0".as_slice(), b"NN\x7fignored\0".as_slice()] {
        let mut payload = REFERENCE_HEADER.to_vec();
        payload.extend_from_slice(name);
        payload.extend_from_slice(b"\x01\x01");
        check_empty_result(
            &payload,
            payload.len() - 2,
            |ctx, bytes| reference_names(ctx, bytes).map(|rows| rows.is_empty()),
            "creo reference name scan",
        );
    }
}

#[test]
fn recipe_prefix_is_free_with_the_original_96_byte_bound() {
    for length in [0, 1, 2, 17, 95, 96, 120] {
        let mut payload = RECIPE_HEADER.to_vec();
        payload.extend(std::iter::repeat_n(b'D', length));
        check_empty_result(
            &payload,
            payload.len(),
            |ctx, bytes| recipe_bindings(ctx, bytes).map(|rows| rows.is_empty()),
            "creo recipe binding scan",
        );
        payload.extend_from_slice(b"\0\xf6\0unknown\0");
        check_empty_result(
            &payload,
            payload.len(),
            |ctx, bytes| recipe_bindings(ctx, bytes).map(|rows| rows.is_empty()),
            "creo recipe binding scan",
        );
    }
}

#[test]
fn reference_prefix_preserves_closed_native_identity_and_byte_copy_work() {
    for length in [1, 17, 255] {
        let mut payload = REFERENCE_HEADER.to_vec();
        let name = vec![0xff; length];
        payload.extend_from_slice(&name);
        payload.extend_from_slice(b"\0\x01\x01");
        // One file-sized source scan and one retained byte copy.
        let total = (payload.len() - 2 + length) as u64;
        check_result(
            total,
            true,
            &["creo reference name scan", "creo reference name bytes"],
            |ctx| {
                let rows = reference_names(ctx, &payload)?;
                assert_eq!(rows.len(), 1);
                let row = &rows[0];
                assert_eq!(
                    (
                        row.feature_id,
                        row.own_reference_id,
                        row.reference_type,
                        row.offset
                    ),
                    (2, 1, 5, 0)
                );
                assert_eq!(row.name_bytes, name);
                Ok(())
            },
        );
    }
}

#[test]
fn recipe_prefix_preserves_binding_identity_and_nonempty_display_grammar() {
    for length in [1, 17, 95] {
        let mut payload = RECIPE_HEADER.to_vec();
        // Display bytes are arbitrary non-NUL bytes, including ASCII controls.
        payload.extend(std::iter::repeat_n(1, length));
        payload.extend_from_slice(b"\0\xf6\0protextrude\0");
        check_result(
            payload.len() as u64,
            true,
            &["creo recipe binding scan"],
            |ctx| {
                let rows = recipe_bindings(ctx, &payload)?;
                assert_eq!(rows.len(), 1);
                let (id, row) = &rows[0];
                assert_eq!(
                    (
                        *id,
                        row.root_schema_class.code(),
                        row.parent_feature_id,
                        row.offset
                    ),
                    (8053, 917, 8051, 0)
                );
                assert_eq!(row.recipe, FeatureRecipe::ProtrudeExtrude);
                Ok(())
            },
        );
    }
}
